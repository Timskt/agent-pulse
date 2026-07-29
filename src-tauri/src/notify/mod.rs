use serde::{Deserialize, Serialize};

/// Webhook 通知配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyConfig {
    /// 是否启用 Webhook 通知
    pub enabled: bool,
    /// Webhook URL（支持 Slack / Discord / 飞书 / 自定义）
    pub webhook_url: String,
    /// 通知事件类型
    pub notify_on_resume: bool,
    pub notify_on_interrupt: bool,
    pub notify_on_complete: bool,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            webhook_url: String::new(),
            notify_on_resume: true,
            notify_on_interrupt: true,
            notify_on_complete: false,
        }
    }
}

/// 发送 Webhook 通知
pub async fn send_webhook(config: &NotifyConfig, title: &str, body: &str) -> Result<(), String> {
    if !config.enabled || config.webhook_url.is_empty() {
        return Ok(());
    }

    let payload = serde_json::json!({
        "text": format!("**[AgentPulse] {}**\n{}", title, body),
        "content": format!("**[AgentPulse] {}**\n{}", title, body),
        "msgtype": "text",
        "content_detail": { "text": format!("[AgentPulse] {}: {}", title, body) }
    });

    // 使用系统 curl 发送（避免引入 HTTP 客户端依赖）
    let output = std::process::Command::new("curl")
        .arg("-s")
        .arg("-X")
        .arg("POST")
        .arg(&config.webhook_url)
        .arg("-H")
        .arg("Content-Type: application/json")
        .arg("-d")
        .arg(payload.to_string())
        .output()
        .map_err(|e| format!("发送通知失败: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        Err("Webhook 请求返回错误状态".to_string())
    }
}
