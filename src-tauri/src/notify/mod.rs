//! 桌面通知 / 声音 / 托盘角标（v1.1 感知层）
//!
//! 这一层回答的问题只有一个：**Agent 停下来等我时，我怎么第一时间知道？**
//!
//! 三条通道各有分工，互补而不重复：
//! - **系统通知**：人不在应用前时唯一能穿透的手段。
//! - **前端提醒事件**：人在应用前时用声音 + 高亮，比弹窗更轻。
//! - **托盘角标**：常驻的「还有几件事等你」计数，不打扰但随时可查。
//!
//! 关于「点通知直接跳到终端」：桌面三平台都没有稳定统一的通知点击回调
//! （tauri-plugin-notification 的 action 回调目前只在移动端可靠），
//! 所以收口动作放在应用内——提醒事件带 `session_id`，
//! 前端把该会话顶到列表最前并给一个「跳到终端」按钮，
//! 走 `focus_terminal` 命令直达那个标签页。

use crate::config::NotificationConfig;
use crate::detector::AttentionLevel;
use crate::i18n::I18n;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

/// 托盘图标的固定 id，便于后续取回句柄刷新角标
pub const TRAY_ID: &str = "agent-pulse-tray";

/// 推送给前端的提醒事件
#[derive(Debug, Clone, Serialize)]
pub struct AttentionAlert {
    pub session_id: String,
    /// 注意力级别的稳定键：needs_input / completed / rate_limited / error
    pub level: String,
    pub title: String,
    pub body: String,
    /// 是否播放声音（由配置决定，前端不再自行判断）
    pub sound: bool,
    /// 音量 0-100
    pub volume: u32,
}

/// 通知管理器
pub struct Notifier {
    app: AppHandle,
    /// `session_id:level` → 上次发送时间，用于节流
    throttle: Mutex<HashMap<String, Instant>>,
    /// 上次写入的角标数量；相同则跳过重绘（合成图标不便宜）
    last_badge: Mutex<Option<u32>>,
}

impl Notifier {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            throttle: Mutex::new(HashMap::new()),
            last_badge: Mutex::new(None),
        }
    }

    /// 节流判定：同一会话的同一状态在窗口内只提醒一次
    ///
    /// 没有这道闸，一个卡住的会话会每轮扫描弹一次通知——
    /// 那比不提醒更让人想卸载。
    fn allow(&self, key: String, throttle_secs: u64) -> bool {
        let mut map = self.throttle.lock().unwrap();
        match map.get(&key) {
            Some(t) if t.elapsed().as_secs() < throttle_secs => false,
            _ => {
                map.insert(key, Instant::now());
                true
            }
        }
    }

    /// 会话状态变化时提醒
    ///
    /// 返回是否真的发出了提醒（用于日志）。
    pub fn notify_attention(
        &self,
        cfg: &NotificationConfig,
        lang: &str,
        session_id: &str,
        session_label: &str,
        level: AttentionLevel,
        detail: Option<&str>,
    ) -> bool {
        if !cfg.enabled || level == AttentionLevel::None {
            return false;
        }

        let wanted = match level {
            AttentionLevel::NeedsInput => cfg.on_needs_input,
            AttentionLevel::Completed => cfg.on_completed,
            AttentionLevel::RateLimited => cfg.on_rate_limited,
            AttentionLevel::Error => cfg.on_error,
            AttentionLevel::None => false,
        };
        if !wanted {
            return false;
        }

        if !self.allow(format!("{session_id}:{}", level.key()), cfg.throttle_secs) {
            return false;
        }

        let i18n = I18n::from_code(lang);
        let title = match level {
            AttentionLevel::NeedsInput => i18n.t("notify.needs_input.title"),
            AttentionLevel::Completed => i18n.t("notify.completed.title"),
            AttentionLevel::RateLimited => i18n.t("notify.rate_limited.title"),
            AttentionLevel::Error => i18n.t("notify.error.title"),
            AttentionLevel::None => return false,
        };

        let body = match detail {
            Some(d) => format!("{session_label} · {d}"),
            None => session_label.to_string(),
        };

        self.show(title, &body);
        self.emit_alert(cfg, session_id, level.key(), title, &body);
        true
    }

    /// 续跑成功后的提醒（默认关闭，属于「知道就好」的信息）
    pub fn notify_resumed(
        &self,
        cfg: &NotificationConfig,
        lang: &str,
        session_id: &str,
        detail: &str,
    ) {
        if !cfg.enabled || !cfg.on_resumed {
            return;
        }
        if !self.allow(format!("{session_id}:resumed"), cfg.throttle_secs) {
            return;
        }
        let title = I18n::from_code(lang).t("notify.resumed.title");
        self.show(title, detail);
        self.emit_alert(cfg, session_id, "resumed", title, detail);
    }

    /// 预算 / 限流类提醒（不绑定具体会话）
    pub fn notify_alert(
        &self,
        cfg: &NotificationConfig,
        throttle_key: &str,
        title: &str,
        body: &str,
    ) {
        if !cfg.enabled {
            return;
        }
        // 预算告警用更长的静默期：金额是缓慢累积的，没必要每分钟喊一次
        if !self.allow(throttle_key.to_string(), cfg.throttle_secs.max(600)) {
            return;
        }
        self.show(title, body);
        self.emit_alert(cfg, "", "budget", title, body);
    }

    /// 手动测试通道
    pub fn notify_test(&self, lang: &str) -> Result<String, String> {
        let i18n = I18n::from_code(lang);
        let title = i18n.t("notify.test.title");
        let body = i18n.t("notify.test.body");
        self.app
            .notification()
            .builder()
            .title(title)
            .body(body)
            .show()
            .map_err(|e| i18n.tf("err.notify_failed", &[("detail", &e.to_string())]))?;
        let _ = self.app.emit(
            "attention-alert",
            AttentionAlert {
                session_id: String::new(),
                level: "completed".to_string(),
                title: title.to_string(),
                body: body.to_string(),
                sound: true,
                volume: 60,
            },
        );
        Ok(body.to_string())
    }

    fn show(&self, title: &str, body: &str) {
        if let Err(e) = self
            .app
            .notification()
            .builder()
            .title(title)
            .body(body)
            .show()
        {
            tracing::warn!("[Notify] 系统通知发送失败: {e}");
        }
    }

    fn emit_alert(
        &self,
        cfg: &NotificationConfig,
        session_id: &str,
        level: &str,
        title: &str,
        body: &str,
    ) {
        let _ = self.app.emit(
            "attention-alert",
            AttentionAlert {
                session_id: session_id.to_string(),
                level: level.to_string(),
                title: title.to_string(),
                body: body.to_string(),
                sound: cfg.sound_enabled,
                volume: cfg.sound_volume.min(100),
            },
        );
    }

    /// 刷新托盘角标
    ///
    /// 直接在窗口图标的 RGBA 上合成一个红点——
    /// `default_window_icon()` 拿到的已经是解码后的像素，
    /// 所以不需要引入任何图片解码 feature。
    pub fn update_tray_badge(&self, cfg: &NotificationConfig, pending: u32, lang: &str) {
        {
            let mut last = self.last_badge.lock().unwrap();
            let current = if cfg.tray_badge { Some(pending) } else { None };
            if *last == current {
                return;
            }
            *last = current;
        }

        let Some(tray) = self.app.tray_by_id(TRAY_ID) else {
            return;
        };
        let Some(base) = self.app.default_window_icon().cloned() else {
            return;
        };

        let show_badge = cfg.tray_badge && pending > 0;
        let icon = if show_badge {
            composite_badge(&base)
        } else {
            base
        };
        let _ = tray.set_icon(Some(icon));

        // macOS 托盘可以直接显示文字，比红点更精确
        let i18n = I18n::from_code(lang);
        if show_badge {
            let _ = tray.set_title(Some(format!("{pending}")));
            let _ = tray.set_tooltip(Some(format!(
                "{} · {} {}",
                i18n.t("tray.tooltip"),
                pending,
                i18n.t("tray.pending")
            )));
        } else {
            let _ = tray.set_title(Some(""));
            let _ = tray.set_tooltip(Some(i18n.t("tray.tooltip")));
        }
    }
}

/// 在图标右下角合成一个带白边的红点
fn composite_badge(base: &tauri::image::Image<'_>) -> tauri::image::Image<'static> {
    let w = base.width() as i64;
    let h = base.height() as i64;
    let mut rgba = base.rgba().to_vec();

    // 半径取短边的 28%：32px 托盘图标上约 9px 直径，肉眼可辨又不糊成一团
    let radius = ((w.min(h) as f64) * 0.28).max(3.0);
    let cx = w as f64 - radius - 1.0;
    let cy = h as f64 - radius - 1.0;

    for y in 0..h {
        for x in 0..w {
            let dx = x as f64 + 0.5 - cx;
            let dy = y as f64 + 0.5 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            // 外圈白边负责在深色/浅色托盘背景上都能看清
            let (r, g, b, coverage) = if dist <= radius - 1.2 {
                (239u8, 68u8, 68u8, (radius - 1.2 - dist).min(1.0))
            } else if dist <= radius {
                (255u8, 255u8, 255u8, (radius - dist).min(1.0))
            } else {
                continue;
            };
            let idx = ((y * w + x) * 4) as usize;
            if idx + 3 >= rgba.len() {
                continue;
            }
            let a = coverage.clamp(0.0, 1.0);
            for (offset, value) in [(0, r), (1, g), (2, b)] {
                let old = rgba[idx + offset] as f64;
                rgba[idx + offset] = (old * (1.0 - a) + value as f64 * a).round() as u8;
            }
            rgba[idx + 3] = 255;
        }
    }

    tauri::image::Image::new_owned(rgba, base.width(), base.height())
}
