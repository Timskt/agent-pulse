use crate::detector::Arbitration;
use serde::{Deserialize, Serialize};

/// AI 判断配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiJudgeConfig {
    /// 是否启用 AI 辅助判断
    pub enabled: bool,
    /// API 端点（兼容 OpenAI 格式）
    pub api_url: String,
    /// API Key
    pub api_key: String,
    /// 模型名称
    pub model: String,
    /// 置信度阈值（0-100），AI 判断中断概率超过此值才触发续跑
    pub confidence_threshold: u32,
}

impl Default for AiJudgeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_url: "https://api.openai.com/v1/chat/completions".to_string(),
            api_key: String::new(),
            model: "gpt-4o-mini".to_string(),
            confidence_threshold: 75,
        }
    }
}

/// AI 判断结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiVerdict {
    /// 是否判定为中断
    pub is_interrupted: bool,
    /// 置信度 (0-100)
    pub confidence: u32,
    /// AI 分析说明
    pub reasoning: String,
    /// 建议的续跑提示词（AI 生成）
    pub suggested_prompt: Option<String>,
}

/// AI 智能判断引擎
/// 使用 LLM 分析 Agent 最近输出，判断是否真正中断
pub struct AiJudge {
    config: AiJudgeConfig,
    client: reqwest::Client,
}

impl AiJudge {
    pub fn new(config: AiJudgeConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    /// 分析 Agent 输出，判断是否真正中断
    pub async fn analyze(
        &self,
        agent_name: &str,
        recent_output: &str,
    ) -> Result<AiVerdict, String> {
        if !self.config.enabled {
            return Err("AI 判断未启用".to_string());
        }
        if self.config.api_key.is_empty() {
            return Err("API Key 未配置".to_string());
        }

        // 截取最近输出（避免 token 过多），并保证中文不会被切成半个字符
        let truncated = tail(recent_output, 2000);

        let system_prompt = r#"你是一个 AI 编程助手的监控分析器。你的任务是分析 AI Agent 的最近输出，判断它是否真正中断/卡住了。

请以 JSON 格式回复：
{
  "is_interrupted": true/false,
  "confidence": 0-100,
  "reasoning": "简短分析原因",
  "suggested_prompt": "如果中断了，建议的续跑提示词（否则为null）"
}

判断标准：
- 如果输出以错误信息、rate limit、timeout 结尾 → 大概率中断
- 如果输出以正常的代码/解释结尾但没有完成标记 → 可能中断
- 如果输出包含 "completed"、"done"、"finished" → 未中断
- 如果输出正在等待用户输入（如提问） → 未中断，是正常交互

只返回 JSON，不要其他内容。"#;

        let user_prompt = format!(
            "Agent 类型: {}\n\n最近输出:\n```\n{}\n```",
            agent_name, truncated
        );

        let body = serde_json::json!({
            "model": self.config.model,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": user_prompt }
            ],
            "temperature": 0.1,
            "max_tokens": 300
        });

        let resp = self
            .client
            .post(&self.config.api_url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| format!("AI API 请求失败: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("AI API 返回错误: HTTP {}", resp.status()));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("解析 AI 响应失败: {e}"))?;

        // 提取 content
        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("{}");

        // 解析 JSON（可能被 markdown 包裹）
        let clean = content
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        let verdict: AiVerdict = serde_json::from_str(clean).unwrap_or_else(|e| {
            // 回退：尝试从文本中提取关键信息
            tracing::warn!("[AiJudge] JSON 解析失败: {e}, 原始: {clean}");
            AiVerdict {
                is_interrupted: clean.contains("true"),
                confidence: 50,
                reasoning: format!(
                    "AI 返回非标准格式: {}",
                    clean.chars().take(100).collect::<String>()
                ),
                suggested_prompt: None,
            }
        });

        Ok(verdict)
    }

    /// 判断 AI 结果是否应该触发续跑
    pub fn should_resume(&self, verdict: &AiVerdict) -> bool {
        verdict.is_interrupted && verdict.confidence >= self.config.confidence_threshold
    }

    /// 一道是非题：这一轮活干完了没有
    ///
    /// 跟上面 [`Self::analyze`] 的差别不在模型，在**回复的形状**。`analyze` 要一段
    /// JSON，里面有置信度、有理由、有建议提示词——那是给人看的，所以解析失败时
    /// 只能连蒙带猜（见它的回退分支）。这里要的是一个能直接拿来做决定的信号：
    /// 两个词，选一个，多一个字都算没答上。
    ///
    /// 这样换来三件事：
    ///
    /// 1. **没有解析歧义。** 不是那两个词就是 `Err`，绝不猜。判定层拿到 `Err`
    ///    等于没问过，行为退回原样（见 [`Arbitration`] 的「权限是单向的」）。
    /// 2. **便宜。** 回复被压到几个 token，问题本身也只带记录的尾巴。
    /// 3. **可复现。** `temperature: 0` + 是非题，同一段记录问两次基本同一个答案；
    ///    散文式的回答做不到这一点，而一个每轮都可能翻面的判定比没有更糟。
    ///
    /// 拿不准时让它回「没干完」是故意的，因为两边的代价不对称：多敲一句「继续」
    /// 最坏是浪费一次对话，少敲一句就是用户又得自己去发一遍。何况这道题只在
    /// 已经有一条中断关键词命中之后才问，天平本来就偏向「没干完」。
    ///
    /// 请求体沿用 `analyze` 那套 OpenAI 兼容形状：这个模块对接的是用户自己填的
    /// `api_url`，不绑任何一家厂商。
    pub async fn arbitrate(
        &self,
        agent_name: &str,
        recent_output: &str,
    ) -> Result<Arbitration, String> {
        if !self.config.enabled {
            return Err("AI 判断未启用".to_string());
        }
        if self.config.api_key.is_empty() {
            return Err("API Key 未配置".to_string());
        }

        let system_prompt = "你是一个只回一个词的判定器。\n\
读下面这段 AI 编程助手的会话记录，判断它这一轮的任务是否已经做完。\n\
只允许回复下面两个词之一，不要标点、不要解释、不要换行：\n\
DONE\n\
CONTINUE\n\
DONE = 这一轮任务已经收尾，没有待办。\n\
CONTINUE = 它停在半路上，等一句「继续」就能接着做。\n\
拿不准的时候回 CONTINUE。";

        let body = serde_json::json!({
            "model": self.config.model,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": format!(
                    "Agent 类型: {}\n\n记录尾部:\n```\n{}\n```",
                    agent_name, tail(recent_output, ARBITER_TAIL_BYTES)) }
            ],
            "temperature": 0,
            // 够两个词加一点余量。给大了只会让不听话的模型有地方写解释，
            // 而写了解释的回答一律算没答上
            "max_tokens": 8
        });

        let resp = self
            .client
            .post(&self.config.api_url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            // 比 analyze 短得多：这道题问的是「要不要提前几分钟动手」，
            // 等三十秒就把一轮扫描搭进去了，不值得
            .timeout(std::time::Duration::from_secs(ARBITER_TIMEOUT_SECS))
            .send()
            .await
            .map_err(|e| format!("仲裁请求失败: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("仲裁返回错误: HTTP {}", resp.status()));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("解析仲裁响应失败: {e}"))?;
        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default();

        parse_arbitration(content)
    }
}

/// 问出去的记录尾部长度（字节）
///
/// 比 `analyze` 的 2000 更短：是非题不需要全文，只需要「它最后停在哪儿」。
const ARBITER_TAIL_BYTES: usize = 1200;

/// 仲裁的超时（秒）
const ARBITER_TIMEOUT_SECS: u64 = 10;

/// 取尾部若干字节，且**落在字符边界上**
///
/// 直接切 `&s[s.len() - n..]` 会在中文上 panic——多字节字符被切成两半，
/// Rust 的字符串索引会直接崩。这个函数是踩过之后补的：会话记录里全是中文。
fn tail(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut start = s.len() - max_bytes;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    &s[start..]
}

/// 把回复解析成一个判定，认不出就是认不出
///
/// **刻意没有模糊回退。** 「包含 DONE 就算完成」这种写法会让
/// 「NOT DONE」「I can't say DONE」都被认成完成，而这条路上一个错误答案
/// 会直接变成一次不该发生的动作。认不出来就当没问过，代价只是慢一步。
fn parse_arbitration(content: &str) -> Result<Arbitration, String> {
    let answer = content.trim().to_ascii_uppercase();
    match answer.as_str() {
        "DONE" => Ok(Arbitration::Finished),
        "CONTINUE" => Ok(Arbitration::Unfinished),
        _ => Err(format!(
            "仲裁没有按约定回一个词：{}",
            content.chars().take(60).collect::<String>()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arbitration_accepts_only_the_two_contract_words() {
        assert_eq!(parse_arbitration("DONE\n"), Ok(Arbitration::Finished));
        assert_eq!(parse_arbitration("continue"), Ok(Arbitration::Unfinished));
        assert!(parse_arbitration("NOT DONE").is_err());
        assert!(parse_arbitration("DONE.").is_err());
        assert!(parse_arbitration("```DONE```").is_err());
    }

    #[test]
    fn tail_never_splits_utf8() {
        let text = "开头-这是中文尾巴";
        assert_eq!(tail(text, 12), "中文尾巴");
        assert_eq!(tail(text, text.len()), text);
    }
}
