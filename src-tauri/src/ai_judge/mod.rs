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
    pub async fn analyze(&self, agent_name: &str, recent_output: &str) -> Result<AiVerdict, String> {
        if !self.config.enabled {
            return Err("AI 判断未启用".to_string());
        }
        if self.config.api_key.is_empty() {
            return Err("API Key 未配置".to_string());
        }

        // 截取最近输出（避免 token 过多）
        let truncated = if recent_output.len() > 2000 {
            &recent_output[recent_output.len() - 2000..]
        } else {
            recent_output
        };

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
                reasoning: format!("AI 返回非标准格式: {}", &clean[..clean.len().min(100)]),
                suggested_prompt: None,
            }
        });

        Ok(verdict)
    }

    /// 判断 AI 结果是否应该触发续跑
    pub fn should_resume(&self, verdict: &AiVerdict) -> bool {
        verdict.is_interrupted && verdict.confidence >= self.config.confidence_threshold
    }
}
