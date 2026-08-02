//! Token / 成本核算（v1.2 洞察层）
//!
//! 数据来源是 Claude Code 自己写下的会话日志 `~/.claude/projects/**/*.jsonl`——
//! 每条 assistant 消息都带 `message.usage`。这是完全非侵入的：
//! 不改 Agent、不代理网络、不注入环境变量，只读它已经写在磁盘上的东西。
//!
//! 两个关键实现细节：
//! 1. **增量读取**：为每个文件记录已读字节偏移，每轮只解析新追加的部分。
//!    否则每 10 秒重新解析几十 MB 日志会把 CPU 烧掉。
//! 2. **跨文件去重**：同一个 API 请求可能因 resume / 分支出现在多个 jsonl 里，
//!    以 `requestId + message.id` 作为去重键（与 ccusage 的做法一致），
//!    否则续跑越多、账单越虚高。

use crate::config::ModelPriceOverride;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::Mutex;

/// 每百万 token 的美元单价
#[derive(Debug, Clone, Copy)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub cache_write: f64,
    pub cache_read: f64,
}

impl Price {
    /// 由基础输入/输出价推导缓存价
    ///
    /// 缓存写入 = 1.25 × 输入价（5 分钟 TTL），缓存读取 = 0.1 × 输入价。
    /// Claude Code 默认用 5 分钟 TTL，因此这里按 1.25× 计。
    const fn new(input: f64, output: f64) -> Self {
        Self {
            input,
            output,
            cache_write: input * 1.25,
            cache_read: input * 0.1,
        }
    }
}

/// 模型价目表（美元 / 每百万 token）
///
/// 按「模式串越长越优先」匹配，因此 `claude-opus-4-6` 不会被 `claude-opus-4` 抢走。
/// 旧模型价格为公开挂牌价，若与实际账单不符可在设置里覆盖。
const PRICE_TABLE: &[(&str, Price)] = &[
    // 当前世代
    ("claude-fable-5", Price::new(10.0, 50.0)),
    ("claude-mythos-5", Price::new(10.0, 50.0)),
    ("claude-opus-5", Price::new(5.0, 25.0)),
    ("claude-opus-4-8", Price::new(5.0, 25.0)),
    ("claude-opus-4-7", Price::new(5.0, 25.0)),
    ("claude-opus-4-6", Price::new(5.0, 25.0)),
    ("claude-sonnet-5", Price::new(3.0, 15.0)),
    ("claude-sonnet-4-6", Price::new(3.0, 15.0)),
    ("claude-haiku-4-5", Price::new(1.0, 5.0)),
    // 上一世代
    ("claude-opus-4-5", Price::new(5.0, 25.0)),
    ("claude-opus-4-1", Price::new(15.0, 75.0)),
    ("claude-sonnet-4-5", Price::new(3.0, 15.0)),
    ("claude-sonnet-4", Price::new(3.0, 15.0)),
    ("claude-opus-4", Price::new(15.0, 75.0)),
    // Claude 3.x
    ("claude-3-7-sonnet", Price::new(3.0, 15.0)),
    ("claude-3-5-sonnet", Price::new(3.0, 15.0)),
    ("claude-3-5-haiku", Price::new(0.8, 4.0)),
    ("claude-3-opus", Price::new(15.0, 75.0)),
    ("claude-3-haiku", Price::new(0.25, 1.25)),
];

/// Sonnet 5 的引入期优惠价（截止 2026-08-31，之后回到 3/15）
const SONNET_5_INTRO_END: &str = "2026-08-31";
const SONNET_5_INTRO: Price = Price::new(2.0, 10.0);

/// 查询某模型在某日期的单价
///
/// `date` 形如 `2026-07-30`，用于处理有时效的引入期定价。
pub fn price_for(model: &str, date: &str, overrides: &[ModelPriceOverride]) -> Option<Price> {
    let model_lower = model.to_lowercase();

    // 用户覆盖优先，同样按最长匹配
    let best_override = overrides
        .iter()
        .filter(|o| !o.model.is_empty() && model_lower.contains(&o.model.to_lowercase()))
        .max_by_key(|o| o.model.len());
    if let Some(o) = best_override {
        return Some(Price {
            input: o.input,
            output: o.output,
            cache_write: o.cache_write.unwrap_or(o.input * 1.25),
            cache_read: o.cache_read.unwrap_or(o.input * 0.1),
        });
    }

    if model_lower.contains("claude-sonnet-5") && date <= SONNET_5_INTRO_END {
        return Some(SONNET_5_INTRO);
    }

    PRICE_TABLE
        .iter()
        .filter(|(pattern, _)| model_lower.contains(pattern))
        .max_by_key(|(pattern, _)| pattern.len())
        .map(|(_, price)| *price)
}

/// 用量汇总（可用于单会话、单项目或单日）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageSnapshot {
    /// 未命中缓存的输入 token
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_write_tokens: u64,
    pub cache_read_tokens: u64,
    /// 总 token（含缓存部分，即真实上下文规模）
    pub total_tokens: u64,
    pub cost_usd: f64,
    pub requests: u32,
}

/// 一次 API 请求的用量记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageEntry {
    /// 去重键：requestId + message.id
    pub dedup_key: String,
    /// 本地时间 `%Y-%m-%d %H:%M:%S`
    pub timestamp: String,
    pub model: String,
    /// 项目路径（取 jsonl 中的 cwd，回退到目录名）
    pub project: String,
    /// 来源会话文件绝对路径
    pub session_file: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_write_tokens: u64,
    pub cache_read_tokens: u64,
    pub cost_usd: f64,
}

/// 按日聚合的成本
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyCost {
    pub date: String,
    pub total_tokens: u64,
    pub cost_usd: f64,
    pub requests: u32,
}

/// 按项目聚合的成本
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectCost {
    pub project: String,
    pub total_tokens: u64,
    pub cost_usd: f64,
    pub requests: u32,
}

/// 限流窗口预测
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitForecast {
    /// 窗口长度（小时）
    pub window_hours: u32,
    /// 窗口内已消耗 token
    pub used_tokens: u64,
    /// 窗口 token 预算（0 表示未配置）
    pub budget_tokens: u64,
    /// 已用百分比
    pub used_percent: u32,
    /// 最近一小时的消耗速率（token/分钟）
    pub tokens_per_min: u64,
    /// 按当前速率预计多少分钟后触发限流（None = 无法预测）
    pub minutes_to_limit: Option<u64>,
}

/// 成本追踪器 —— 持有增量读取游标
pub struct CostTracker {
    /// 文件路径 → 已解析到的字节偏移
    cursors: Mutex<HashMap<String, u64>>,
    /// 上次刷新时间，用于限制磁盘扫描频率
    last_refresh: Mutex<Option<std::time::Instant>>,
}

/// 磁盘扫描的最小间隔——比轮询间隔更宽松，避免每轮都遍历目录
const MIN_REFRESH_INTERVAL_SECS: u64 = 20;

impl CostTracker {
    pub fn new(cursors: HashMap<String, u64>) -> Self {
        Self {
            cursors: Mutex::new(cursors),
            last_refresh: Mutex::new(None),
        }
    }

    /// 距上次刷新是否已超过最小间隔
    pub fn should_refresh(&self) -> bool {
        match *self.last_refresh.lock().unwrap() {
            Some(t) => t.elapsed().as_secs() >= MIN_REFRESH_INTERVAL_SECS,
            None => true,
        }
    }

    /// 增量扫描所有 Claude Code 会话日志，返回新增的用量记录与更新后的游标
    ///
    /// 只解析每个文件上次读取位置之后追加的字节；文件被截断（长度变小）时重置游标。
    pub fn refresh(
        &self,
        overrides: &[ModelPriceOverride],
    ) -> (Vec<UsageEntry>, Vec<(String, u64)>) {
        *self.last_refresh.lock().unwrap() = Some(std::time::Instant::now());

        let files = claude_session_files();
        let mut entries = Vec::new();
        let mut updated_cursors = Vec::new();

        for path in files {
            let path_key = path.to_string_lossy().to_string();
            let file_len = match std::fs::metadata(&path) {
                Ok(m) => m.len(),
                Err(_) => continue,
            };

            let start_offset = {
                let cursors = self.cursors.lock().unwrap();
                match cursors.get(&path_key) {
                    // 文件变短说明被轮转或重写，从头再来
                    Some(&off) if off <= file_len => off,
                    Some(_) => 0,
                    None => 0,
                }
            };

            if start_offset == file_len {
                continue;
            }

            match parse_range(&path, start_offset, overrides) {
                Ok((mut parsed, new_offset)) => {
                    entries.append(&mut parsed);
                    self.cursors
                        .lock()
                        .unwrap()
                        .insert(path_key.clone(), new_offset);
                    updated_cursors.push((path_key, new_offset));
                }
                Err(e) => {
                    tracing::debug!("[Cost] 解析 {} 失败: {e}", path.display());
                }
            }
        }

        (entries, updated_cursors)
    }
}

/// 列出所有 Claude Code 会话日志
fn claude_session_files() -> Vec<PathBuf> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return vec![],
    };
    let projects = home.join(".claude").join("projects");
    if !projects.exists() {
        return vec![];
    }
    let pattern = crate::adapters::to_glob_pattern(&projects, "/**/*.jsonl");
    glob::glob(&pattern)
        .map(|paths| paths.filter_map(|p| p.ok()).collect())
        .unwrap_or_default()
}

/// 解析文件中 `[start, EOF)` 区间的 JSONL，返回记录与新的读取偏移
///
/// 若最后一行不完整（正在写入），则不消费该行，把偏移停在行首，下轮重读。
fn parse_range(
    path: &PathBuf,
    start: u64,
    overrides: &[ModelPriceOverride],
) -> std::io::Result<(Vec<UsageEntry>, u64)> {
    let file = std::fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    reader.seek(SeekFrom::Start(start))?;

    let path_str = path.to_string_lossy().to_string();
    let fallback_project = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut entries = Vec::new();
    let mut offset = start;
    let mut buf = Vec::new();

    loop {
        buf.clear();
        let read = reader.read_until(b'\n', &mut buf)?;
        if read == 0 {
            break;
        }
        // 行未以 \n 结尾 → 正在写入，留给下一轮
        if !buf.ends_with(b"\n") {
            break;
        }
        offset += read as u64;

        let line = match std::str::from_utf8(&buf) {
            Ok(s) => s.trim(),
            Err(_) => continue,
        };
        if line.is_empty() {
            continue;
        }

        if let Some(entry) = parse_usage_line(line, &path_str, &fallback_project, overrides) {
            entries.push(entry);
        }
    }

    Ok((entries, offset))
}

/// 从单行 JSONL 中提取用量
fn parse_usage_line(
    line: &str,
    session_file: &str,
    fallback_project: &str,
    overrides: &[ModelPriceOverride],
) -> Option<UsageEntry> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let message = value.get("message")?;
    let usage = message.get("usage")?;

    let input_tokens = usage
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_write_tokens = usage
        .get("cache_creation_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_read_tokens = usage
        .get("cache_read_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    if input_tokens == 0 && output_tokens == 0 && cache_write_tokens == 0 && cache_read_tokens == 0
    {
        return None;
    }

    let model = message
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    // 合成器/本地模型不计费
    if model.contains("synthetic") {
        return None;
    }

    let request_id = value
        .get("requestId")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let message_id = message.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let dedup_key = if request_id.is_empty() && message_id.is_empty() {
        // 没有任何 ID 时退化为「文件 + 时间戳 + token 数」，仍能挡住重复解析
        format!(
            "{session_file}#{}#{input_tokens}/{output_tokens}",
            value
                .get("timestamp")
                .and_then(|v| v.as_str())
                .unwrap_or("")
        )
    } else {
        format!("{request_id}:{message_id}")
    };

    let timestamp = value
        .get("timestamp")
        .and_then(|v| v.as_str())
        .and_then(to_local_timestamp)
        .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string());

    let project = value
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| fallback_project.to_string());

    let date = &timestamp[..timestamp.len().min(10)];
    let cost_usd = match price_for(&model, date, overrides) {
        Some(p) => {
            (input_tokens as f64 * p.input
                + output_tokens as f64 * p.output
                + cache_write_tokens as f64 * p.cache_write
                + cache_read_tokens as f64 * p.cache_read)
                / 1_000_000.0
        }
        None => 0.0,
    };

    Some(UsageEntry {
        dedup_key,
        timestamp,
        model,
        project,
        session_file: session_file.to_string(),
        input_tokens,
        output_tokens,
        cache_write_tokens,
        cache_read_tokens,
        cost_usd,
    })
}

/// ISO8601（UTC）→ 本地 `%Y-%m-%d %H:%M:%S`
fn to_local_timestamp(iso: &str) -> Option<String> {
    chrono::DateTime::parse_from_rfc3339(iso).ok().map(|dt| {
        dt.with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
    })
}

/// 由已用量与窗口预算推算限流风险
pub fn forecast_rate_limit(
    window_hours: u32,
    budget_tokens: u64,
    used_tokens: u64,
    last_hour_tokens: u64,
) -> RateLimitForecast {
    let tokens_per_min = last_hour_tokens / 60;
    let used_percent = if budget_tokens > 0 {
        ((used_tokens as f64 / budget_tokens as f64) * 100.0).round() as u32
    } else {
        0
    };
    let minutes_to_limit = if budget_tokens > used_tokens && tokens_per_min > 0 {
        Some((budget_tokens - used_tokens) / tokens_per_min)
    } else if budget_tokens > 0 && budget_tokens <= used_tokens {
        Some(0)
    } else {
        None
    };

    RateLimitForecast {
        window_hours,
        used_tokens,
        budget_tokens,
        used_percent,
        tokens_per_min,
        minutes_to_limit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_longest_matching_price_pattern() {
        // claude-opus-4-6 必须命中自己而不是 claude-opus-4
        let p = price_for("claude-opus-4-6", "2026-07-30", &[]).unwrap();
        assert_eq!(p.input, 5.0);
        let p4 = price_for("claude-opus-4-20250514", "2026-07-30", &[]).unwrap();
        assert_eq!(p4.input, 15.0);
    }

    #[test]
    fn applies_sonnet5_intro_window() {
        assert_eq!(
            price_for("claude-sonnet-5", "2026-07-30", &[])
                .unwrap()
                .input,
            2.0
        );
        assert_eq!(
            price_for("claude-sonnet-5", "2026-09-01", &[])
                .unwrap()
                .input,
            3.0
        );
    }

    #[test]
    fn cache_prices_follow_multipliers() {
        let p = price_for("claude-opus-5", "2026-07-30", &[]).unwrap();
        assert_eq!(p.cache_write, 6.25);
        assert_eq!(p.cache_read, 0.5);
    }

    #[test]
    fn user_override_wins() {
        let overrides = vec![ModelPriceOverride {
            model: "claude-opus-5".to_string(),
            input: 1.0,
            output: 2.0,
            cache_write: None,
            cache_read: None,
        }];
        let p = price_for("claude-opus-5", "2026-07-30", &overrides).unwrap();
        assert_eq!(p.input, 1.0);
        assert_eq!(p.cache_write, 1.25);
    }

    #[test]
    fn parses_usage_line_and_costs_it() {
        let line = r#"{"type":"assistant","timestamp":"2026-07-30T07:24:17.123Z","requestId":"req_1","cwd":"/tmp/demo","message":{"id":"msg_1","model":"claude-opus-5","usage":{"input_tokens":1000,"output_tokens":1000,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#;
        let entry = parse_usage_line(line, "/x.jsonl", "fallback", &[]).unwrap();
        assert_eq!(entry.dedup_key, "req_1:msg_1");
        assert_eq!(entry.project, "/tmp/demo");
        // 1000 × $5/M + 1000 × $25/M = $0.03
        assert!((entry.cost_usd - 0.03).abs() < 1e-9);
    }

    #[test]
    fn skips_lines_without_usage() {
        assert!(parse_usage_line(r#"{"type":"user"}"#, "/x", "f", &[]).is_none());
    }

    #[test]
    fn forecast_reports_minutes_to_limit() {
        let f = forecast_rate_limit(5, 1000, 400, 600);
        assert_eq!(f.tokens_per_min, 10);
        assert_eq!(f.minutes_to_limit, Some(60));
        assert_eq!(f.used_percent, 40);
    }
}
