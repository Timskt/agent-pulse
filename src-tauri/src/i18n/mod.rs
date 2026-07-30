//! 后端文案的国际化
//!
//! 边界约定：**谁渲染，谁持有文案**。
//! - 本模块只管后端直接呈现给用户的文字：系统通知、托盘菜单、注意力级别名称。
//! - 界面上的文字由前端 `src/i18n/index.ts` 持有（那边有类型安全的 key 和插值）。
//!
//! 两份字典都由同一个 `config.language` 驱动，所以不会出现「界面中文、通知英文」
//! 那种土不土洋不洋的搭配。
//!
//! 表格用 `(key, zh, en)` 三元组：两种语言写在同一行，漏翻一眼就能看出来。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 支持的语言
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lang {
    Zh,
    En,
}

impl Lang {
    pub fn as_str(&self) -> &'static str {
        match self {
            Lang::Zh => "zh",
            Lang::En => "en",
        }
    }

    pub fn from_code(s: &str) -> Self {
        match s {
            "en" => Lang::En,
            _ => Lang::Zh,
        }
    }
}

/// `(key, 中文, English)`
const TABLE: &[(&str, &str, &str)] = &[
    // ── 通知标题 ──
    ("notify.needs_input.title", "Agent 在等你回应", "Agent is waiting for you"),
    ("notify.completed.title", "任务已完成", "Task completed"),
    ("notify.rate_limited.title", "已触发限流，正在等待", "Rate limited — waiting it out"),
    ("notify.error.title", "会话出错", "Session error"),
    ("notify.resumed.title", "已自动续跑", "Auto-resumed"),
    ("notify.budget.title", "成本预警", "Budget alert"),
    ("notify.rate_forecast.title", "限流预警", "Rate limit warning"),
    ("notify.test.title", "AgentPulse 通知测试", "AgentPulse test notification"),
    ("notify.test.body", "如果你看到这条通知，说明提醒通道已经打通。", "If you can see this, notifications are working."),
    // ── 注意力级别 ──
    ("attention.none", "正常", "Normal"),
    ("attention.needs_input", "等待输入", "Needs input"),
    ("attention.completed", "已完成", "Completed"),
    ("attention.rate_limited", "限流等待", "Rate limited"),
    ("attention.error", "出错", "Error"),
    // ── 托盘菜单 ──
    ("tray.show", "显示主窗口", "Show window"),
    ("tray.start", "开始监控", "Start monitoring"),
    ("tray.stop", "停止监控", "Stop monitoring"),
    ("tray.scan", "立即扫描", "Scan now"),
    ("tray.quit", "退出 AgentPulse", "Quit AgentPulse"),
    ("tray.tooltip", "AgentPulse · AI Agent 守护", "AgentPulse · AI agent guardian"),
    ("tray.pending", "个会话在等你", "session(s) need you"),
    // ── 通知正文（带 {占位符}，经 tf() 插值）──
    ("notify.budget.daily_body", "今日已花费 ${spent}，是预算 ${budget} 的 {percent}%", "Spent ${spent} today — {percent}% of the ${budget} daily budget"),
    ("notify.budget.session_body", "{label} 已花费 ${spent}，超过单会话预算 ${budget}", "{label} has spent ${spent}, past the ${budget} per-session budget"),
    ("notify.rate_forecast.body", "{window} 小时窗口已用 {percent}%，按当前速度约 {minutes} 分钟后触发限流", "{percent}% of the {window}h window used — about {minutes} min to the limit at this rate"),
    // ── 引擎日志（前端 Activity Log 直接展示）──
    ("log.engine_started", "监控引擎已启动，开始守护 AI Agent 会话", "Monitoring started — your agent sessions are being watched"),
    ("log.engine_stopped", "监控引擎已停止", "Monitoring stopped"),
    ("log.interrupt_detected", "[{agent}] 检测到中断信号：{signals}", "[{agent}] Interruption detected: {signals}"),
    ("log.cooldown_skip", "续跑冷却中，跳过本次触发", "Still cooling down — skipping this resume"),
    ("log.suspicious", "疑似中断，继续观察", "Possibly interrupted — keeping watch"),
    ("log.resume_sent", "已触发续跑（{mode}，第 {count} 次）：{detail}", "Resume sent ({mode}, attempt {count}): {detail}"),
    ("log.resume_failed", "续跑失败：{detail}", "Resume failed: {detail}"),
    ("log.resume_manual", "手动续跑：{detail}", "Manual resume: {detail}"),
    ("log.mode_goal", "Goal 恢复", "goal recovery"),
    ("log.mode_generic", "通用", "generic"),
    ("log.alerted", "已提醒：{label}（{level}）", "Alerted: {label} ({level})"),
    ("log.usage_added", "新增 {count} 条用量记录，今日累计 ${cost}", "{count} new usage record(s) — ${cost} spent today"),
    ("log.heartbeat", "心跳：会话 {total} 个，活跃 {active}，中断 {interrupted}", "Heartbeat: {total} session(s), {active} active, {interrupted} interrupted"),
    ("log.focused", "已跳转到会话所在终端：{detail}", "Jumped to the session's terminal: {detail}"),
    // ── 外部推送（Slack / Discord / ntfy / Bark）──
    ("push.title", "AgentPulse · {verdict}", "AgentPulse · {verdict}"),
    ("push.verdict_interrupt", "检测到中断", "Interrupted"),
    ("push.verdict_resume", "已自动续跑", "Auto-resumed"),
    ("push.verdict_complete", "任务完成", "Task completed"),
    ("push.complete_body", "Agent 已经把活儿干完了", "The agent finished its task"),
    ("push.test_title", "AgentPulse 推送测试", "AgentPulse push test"),
    ("push.test_body", "看到这条就说明推送通道是通的。", "If this reached you, the push channel works."),
    ("push.test_ok", "发送成功（HTTP {status}）", "Sent (HTTP {status})"),
    // ── 命令返回的错误（前端直接弹给用户）──
    ("err.already_running", "监控已经在运行了", "Monitoring is already running"),
    ("err.session_not_found", "找不到这个会话，可能已经退出了", "That session is gone — it may have exited"),
    ("err.push_url_missing", "还没填推送地址或主题", "No endpoint or topic configured yet"),
    ("err.push_request", "请求失败：{detail}", "Request failed: {detail}"),
    ("err.push_status", "服务端返回 HTTP {status}", "The server returned HTTP {status}"),
    ("err.notify_failed", "系统通知发不出去：{detail}", "The system notification failed: {detail}"),
    ("err.remote_bind", "看板端口 {port} 打不开：{detail}", "Could not open dashboard port {port}: {detail}"),
    // ── 会话状态（手机看板自己渲染，所以文案在后端）──
    ("status.active", "运行中", "Running"),
    ("status.suspended", "疑似中断", "Maybe stuck"),
    ("status.interrupted", "已中断", "Interrupted"),
    ("status.completed", "已完成", "Completed"),
    ("status.exited", "已退出", "Exited"),
    // ── 手机看板（v1.3 远程层）──
    ("remote.title", "AgentPulse 看板", "AgentPulse dashboard"),
    ("remote.readonly", "只读", "Read-only"),
    ("remote.watching", "守护中", "Watching"),
    ("remote.idle", "未启动", "Idle"),
    ("remote.metric_sessions", "会话", "Sessions"),
    ("remote.metric_pending", "等你回应", "Waiting on you"),
    ("remote.metric_cost", "今日花费", "Spent today"),
    ("remote.empty", "现在没有正在跑的 Agent", "No agents running right now"),
    ("remote.resumed", "已续跑 {count} 次", "resumed {count}×"),
    ("remote.updated", "{time} 更新", "updated {time}"),
    ("remote.offline", "连不上 AgentPulse，电脑上的应用可能已经退出", "Can't reach AgentPulse — the desktop app may have quit"),
    ("log.remote_started", "手机看板已启动：{url}", "Phone dashboard is up: {url}"),
    ("log.remote_lan", "看板已开放到局域网：同一网络里拿到令牌的人都能读你的会话", "The dashboard is open to the LAN — anyone on it with the token can read your sessions"),
    ("log.remote_stopped", "手机看板已停止", "Phone dashboard stopped"),
    // ── 续跑与聚焦的结果文案 ──
    //
    // 这些字符串会被塞进 `log.resume_sent` / `log.resume_failed` 的 `{detail}`，
    // 是实打实要给用户看的，所以不能留在 `resumer` 里硬编码中文。
    (
        "resume.sent",
        "已通过 {terminal} 发送续跑指令（{outcome}，TTY {tty}）",
        "Resume prompt sent via {terminal} ({outcome}, TTY {tty})",
    ),
    ("resume.matched", "已精确匹配到窗口", "matched the exact window"),
    ("resume.followed", "回退到当前窗口", "fell back to the current window"),
    ("resume.outcome_other", "结果 {raw}", "result {raw}"),
    (
        "resume.no_terminal",
        "认不出这个会话在哪个终端里，没有动手",
        "Could not tell which terminal this session lives in — nothing was typed",
    ),
    // 定位不到窗口时宁可不做：往错误的窗口里敲一句中文再回车，比不续跑糟糕得多
    (
        "resume.blind_refused",
        "没能定位到这个会话的窗口，为免打错地方已放弃续跑；确实想让它兜底的话，去设置里打开「跟随最新会话」",
        "Couldn't locate this session's window, so nothing was typed — enable \"Follow the newest session\" in settings if you want a blind fallback",
    ),
    ("resume.script_failed", "终端脚本执行出错：{detail}", "The terminal script failed: {detail}"),
    // Windows 的 SetForegroundWindow 在后台进程里经常被系统拒掉，
    // 而按键打的是「当时的前台窗口」——切不过去就必须放弃，不能赌
    (
        "resume.focus_failed",
        "没能把这个会话的窗口切到前台（系统拒绝了），为免把提示词敲进别的窗口已放弃",
        "Couldn't bring this session's window to the front (the OS refused), so nothing was typed — it would have gone to whatever window was focused",
    ),
    (
        "resume.app_not_running",
        "这个会话所在的应用已经不在运行了",
        "The app hosting this session is no longer running",
    ),
    ("resume.unsupported", "当前平台不支持自动续跑", "Auto-resume isn't supported on this platform"),
    // TTY / 窗口是 macOS 才有的定位手段，Windows 和 Wayland 上只能给个笼统结果
    ("resume.sent_simple", "已发送续跑指令（{outcome}）", "Resume prompt sent ({outcome})"),
    (
        "resume.sent_window",
        "已通过 {tool} 发送续跑指令（窗口 {window}）",
        "Resume prompt sent via {tool} (window {window})",
    ),
    ("resume.frontmost_app", "前台终端", "the frontmost terminal"),
    ("resume.tty_unknown", "未知", "unknown"),
    (
        "resume.tool_missing",
        "没找到 xdotool 或 ydotool，装一个再试：sudo apt install xdotool",
        "Neither xdotool nor ydotool is installed — try: sudo apt install xdotool",
    ),
    ("resume.tool_failed", "{tool} 输入失败", "{tool} could not type the text"),
    (
        "resume.clipboard_missing",
        "中文提示词需要借剪贴板投递，但没找到 wl-copy / xclip / xsel，装一个再试：sudo apt install wl-clipboard",
        "Non-ASCII prompts are delivered via the clipboard, but none of wl-copy / xclip / xsel is installed — try: sudo apt install wl-clipboard",
    ),
    ("resume.no_window", "找不到 PID {pid} 对应的终端窗口", "No terminal window found for PID {pid}"),
    ("focus.done", "已跳到 {terminal}（{outcome}）", "Jumped to {terminal} ({outcome})"),
    ("focus.app_only", "只激活了应用", "brought the app forward only"),
    ("focus.failed", "跳转失败：{detail}", "Couldn't jump: {detail}"),
    (
        "focus.no_window",
        "找不到这个会话对应的窗口（Wayland 下没法聚焦）",
        "No window found for this session (focusing isn't possible on Wayland)",
    ),
    ("focus.unsupported", "当前平台不支持跳转", "Jumping to a terminal isn't supported on this platform"),
    (
        "focus.no_terminal",
        "认不出这个会话在哪个终端里",
        "Could not tell which terminal this session lives in",
    ),
    ("focus.done_simple", "已跳到会话窗口（{outcome}）", "Jumped to the session window ({outcome})"),
    // ── 外部命令（osascript / xdotool / PowerShell）的失败原因 ──
    //
    // 这些字符串会经 Err 一路冒到 Activity Log 里，同样是用户可见文案。
    ("cmd.spawn_failed", "启动 {program} 失败：{detail}", "Could not start {program}: {detail}"),
    ("cmd.failed", "{program} 执行失败：{detail}", "{program} failed: {detail}"),
    (
        "cmd.timeout",
        "{program} 执行超时（{secs}s），已强制终止",
        "{program} timed out after {secs}s and was killed",
    ),
    // ── 检测信号的说明 ──
    //
    // 这些句子会拼进 `log.interrupt_detected` 的 `{signals}`，是用户在
    // Activity Log 里唯一能看到的「凭什么这么判」，必须跟着界面语言走。
    (
        "signal.process_exited",
        "进程 {pid} 已经退出了",
        "Process {pid} has exited",
    ),
    (
        "signal.file_stale",
        "会话记录已经 {elapsed}s 没有更新（阈值 {threshold}s）",
        "The transcript hasn't changed for {elapsed}s (threshold {threshold}s)",
    ),
    (
        "signal.keyword_match",
        "输出里出现了关键词「{keyword}」",
        "The output mentions \"{keyword}\"",
    ),
    (
        "signal.heartbeat_timeout",
        "最后一次活动距今 {elapsed}s（阈值 {threshold}s）",
        "Last activity was {elapsed}s ago (threshold {threshold}s)",
    ),
    // ── 注意力分级的依据（进通知正文）──
    (
        "attention.detail.completed",
        "匹配到完成标记「{marker}」",
        "Matched the completion marker \"{marker}\"",
    ),
    (
        "attention.detail.keyword",
        "输出里出现了「{keyword}」",
        "The output mentions \"{keyword}\"",
    ),
    (
        "attention.detail.awaiting_keyword",
        "输出里出现了「{keyword}」，Agent 正在等你回应",
        "The output mentions \"{keyword}\" — the agent is waiting for you",
    ),
    (
        "attention.detail.process_exited",
        "进程已经退出，也没看到完成标记",
        "The process exited without any completion marker",
    ),
    // 这一条对应「它其实没有干完活，每次都要我去发继续」：回合收尾了、
    // 记录也不动了、又没有完成标记，就是活没干完自己停了。
    (
        "attention.detail.stalled",
        "这一轮已经收尾，但活儿看着没干完",
        "The turn is finished, but the work looks unfinished",
    ),
    (
        "attention.detail.silent",
        "会话长时间没有输出，疑似卡住了",
        "The session has been silent for a long time — it may be stuck",
    ),
];

/// 国际化查表器
pub struct I18n {
    lang: Lang,
}

impl I18n {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    pub fn from_code(code: &str) -> Self {
        Self::new(Lang::from_code(code))
    }

    pub fn lang(&self) -> Lang {
        self.lang
    }

    pub fn set_lang(&mut self, lang: Lang) {
        self.lang = lang;
    }

    /// 取翻译；未收录的 key 原样返回，便于排查而不至于显示 `key_not_found`
    pub fn t(&self, key: &'static str) -> &'static str {
        TABLE
            .iter()
            .find(|(k, _, _)| *k == key)
            .map(|(_, zh, en)| match self.lang {
                Lang::Zh => *zh,
                Lang::En => *en,
            })
            .unwrap_or(key)
    }

    /// 取翻译并按 `{名字}` 占位符插值
    ///
    /// 后端文案只有几十条，引模板引擎不值得；但插值必须集中在这里，
    /// 否则又会退化成到处 `format!` 手拼中文——那正是「土不土洋不洋」的来源。
    pub fn tf(&self, key: &'static str, vars: &[(&str, &str)]) -> String {
        let mut out = self.t(key).to_string();
        for (name, value) in vars {
            out = out.replace(&format!("{{{name}}}"), value);
        }
        out
    }

    /// 导出当前语言的全部条目（前端调试用；界面文案在前端自己的字典里）
    pub fn all(&self) -> HashMap<&'static str, &'static str> {
        TABLE
            .iter()
            .map(|(k, zh, en)| {
                (
                    *k,
                    match self.lang {
                        Lang::Zh => *zh,
                        Lang::En => *en,
                    },
                )
            })
            .collect()
    }
}

impl Default for I18n {
    fn default() -> Self {
        Self::new(Lang::Zh)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_both_languages() {
        assert_eq!(I18n::new(Lang::Zh).t("tray.quit"), "退出 AgentPulse");
        assert_eq!(I18n::new(Lang::En).t("tray.quit"), "Quit AgentPulse");
    }

    #[test]
    fn unknown_key_returns_itself() {
        assert_eq!(I18n::new(Lang::Zh).t("nope.nope"), "nope.nope");
    }

    #[test]
    fn interpolates_placeholders() {
        let text = I18n::new(Lang::En).tf(
            "log.heartbeat",
            &[("total", "3"), ("active", "2"), ("interrupted", "1")],
        );
        assert_eq!(text, "Heartbeat: 3 session(s), 2 active, 1 interrupted");
    }

    #[test]
    fn placeholders_are_all_documented() {
        // 占位符漏填会原样显示 `{name}`，比翻译错更难看，所以两种语言的
        // 占位符集合必须一致——漏一个这里就会红。
        for (key, zh, en) in TABLE {
            let mut zh_vars: Vec<&str> = placeholders(zh);
            let mut en_vars: Vec<&str> = placeholders(en);
            zh_vars.sort_unstable();
            en_vars.sort_unstable();
            assert_eq!(zh_vars, en_vars, "{key} 两种语言的占位符不一致");
        }
    }

    fn placeholders(s: &str) -> Vec<&str> {
        let mut out = Vec::new();
        let mut rest = s;
        while let Some(start) = rest.find('{') {
            let after = &rest[start + 1..];
            match after.find('}') {
                Some(end) => {
                    out.push(&after[..end]);
                    rest = &after[end + 1..];
                }
                None => break,
            }
        }
        out
    }

    #[test]
    fn no_duplicate_keys() {
        let mut keys: Vec<&str> = TABLE.iter().map(|(k, _, _)| *k).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(before, keys.len(), "i18n 表里有重复 key");
    }
}
