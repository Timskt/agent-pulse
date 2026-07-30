use serde::{Deserialize, Serialize};

/// Webhook 通知配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    /// 是否启用
    pub enabled: bool,
    /// Webhook URL（支持 Slack / Discord / 自定义）
    pub url: String,
    /// 通知类型: "slack" | "discord" | "custom"
    pub provider: String,
    /// 自定义消息模板（支持 {agent_name} {session_id} {verdict} {message} 占位符）
    pub template: String,
    /// 是否在中断时通知
    pub notify_on_interrupt: bool,
    /// 是否在续跑时通知
    pub notify_on_resume: bool,
    /// 是否在任务完成时通知
    pub notify_on_complete: bool,
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: String::new(),
            provider: "custom".to_string(),
            template: "🚨 AgentPulse: {agent_name} 检测到中断\n会话: {session_id}\n详情: {message}".to_string(),
            notify_on_interrupt: true,
            notify_on_resume: true,
            notify_on_complete: false,
        }
    }
}

/// Webhook 通知发送器
pub struct WebhookNotifier {
    config: WebhookConfig,
    client: reqwest::Client,
}

impl WebhookNotifier {
    pub fn new(config: WebhookConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    /// 发送中断通知
    pub async fn notify_interrupt(&self, agent_name: &str, session_id: &str, message: &str) {
        if !self.config.enabled || !self.config.notify_on_interrupt {
            return;
        }
        let text = self.format_message(agent_name, session_id, "中断检测", message);
        self.send(&text).await;
    }

    /// 发送续跑通知
    pub async fn notify_resume(&self, agent_name: &str, session_id: &str, message: &str) {
        if !self.config.enabled || !self.config.notify_on_resume {
            return;
        }
        let text = self.format_message(agent_name, session_id, "自动续跑", message);
        self.send(&text).await;
    }

    /// 发送完成通知
    pub async fn notify_complete(&self, agent_name: &str, session_id: &str) {
        if !self.config.enabled || !self.config.notify_on_complete {
            return;
        }
        let text = self.format_message(agent_name, session_id, "任务完成", "Agent 已完成任务");
        self.send(&text).await;
    }

    /// 格式化消息
    fn format_message(&self, agent_name: &str, session_id: &str, verdict: &str, message: &str) -> String {
        self.config
            .template
            .replace("{agent_name}", agent_name)
            .replace("{session_id}", session_id)
            .replace("{verdict}", verdict)
            .replace("{message}", message)
    }

    /// 发送 HTTP 请求
    async fn send(&self, text: &str) {
        if self.config.url.is_empty() {
            return;
        }

        let body = match self.config.provider.as_str() {
            "slack" => serde_json::json!({ "text": text }),
            "discord" => serde_json::json!({ "content": text }),
            _ => serde_json::json!({ "text": text, "message": text }),
        };

        let result = self
            .client
            .post(&self.config.url)
            .json(&body)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await;

        match result {
            Ok(resp) if resp.status().is_success() => {
                tracing::info!("[Webhook] 通知发送成功");
            }
            Ok(resp) => {
                tracing::warn!("[Webhook] 通知返回错误: {}", resp.status());
            }
            Err(e) => {
                tracing::warn!("[Webhook] 通知发送失败: {e}");
            }
        }
    }

    /// 测试 Webhook 连接
    pub async fn test(&self) -> Result<String, String> {
        if self.config.url.is_empty() {
            return Err("Webhook URL 未配置".to_string());
        }

        let text = "✅ AgentPulse Webhook 测试消息 - 连接正常！";
        let body = match self.config.provider.as_str() {
            "slack" => serde_json::json!({ "text": text }),
            "discord" => serde_json::json!({ "content": text }),
            _ => serde_json::json!({ "text": text, "message": text }),
        };

        let resp = self
            .client
            .post(&self.config.url)
            .json(&body)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| format!("请求失败: {e}"))?;

        if resp.status().is_success() {
            Ok(format!("发送成功 (HTTP {})", resp.status()))
        } else {
            Err(format!("服务端返回错误: HTTP {}", resp.status()))
        }
    }
}
