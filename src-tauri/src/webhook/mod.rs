//! 外部推送：Slack / Discord / ntfy / Bark / 自定义
//!
//! 桌面通知只救得了坐在电脑前的人。痛点 #4「人不在电脑前就等于抓瞎」
//! 得靠手机，而 ntfy 和 Bark 正好是「一个地址就能收推送」的通道——
//! 不用注册开发者账号、不用长连接，装个 App 填个主题就完事，
//! 和 AgentPulse「非侵入、装上就能用」的定位是一路的。
//!
//! 消息文案在后端而不在前端：**谁渲染，谁持有文案**，这条消息是后端发出去的。

use crate::i18n::{I18n, Lang};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

/// 推送配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    /// 是否启用
    pub enabled: bool,
    /// 完整推送地址；ntfy / Bark 填了主题时只取其中的协议 + 主机
    pub url: String,
    /// 渠道: "slack" | "discord" | "ntfy" | "bark" | "custom"
    pub provider: String,
    /// ntfy 的主题名 / Bark 的设备 Key
    #[serde(default)]
    pub topic: String,
    /// 消息模板，占位符 `{agent_name}` `{session_id}` `{verdict}` `{message}`
    pub template: String,
    /// 是否在中断时推送
    pub notify_on_interrupt: bool,
    /// 是否在续跑时推送
    pub notify_on_resume: bool,
    /// 是否在任务完成时推送
    pub notify_on_complete: bool,
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: String::new(),
            provider: "custom".to_string(),
            topic: String::new(),
            // 默认模板只放占位符，不写死语言：{verdict} 由后端按当前语言填，
            // 免得中文用户切成英文之后收到一半中文的推送
            template: "🚨 AgentPulse · {agent_name} {verdict}\n{message}".to_string(),
            notify_on_interrupt: true,
            notify_on_resume: true,
            notify_on_complete: false,
        }
    }
}

impl WebhookConfig {
    /// 够不够发一条
    ///
    /// ntfy / Bark 只要有主题就能发（服务端用官方默认），其余渠道必须有完整地址。
    pub fn is_configured(&self) -> bool {
        match self.provider.as_str() {
            "ntfy" | "bark" => !self.topic.trim().is_empty() || !self.url.trim().is_empty(),
            _ => !self.url.trim().is_empty(),
        }
    }
}

/// 取 URL 的「协议 + 主机」
///
/// 用户往地址栏里粘的常常是 `https://api.day.app/AbCd123` 这种带 Key 的整串，
/// 同时又在主题里填了一遍 Key。只取 origin 才不会拼出 `/AbCd123/push`。
fn origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.trim().split_once("://")?;
    if scheme.is_empty() || rest.is_empty() {
        return None;
    }
    let host = rest.split('/').next().unwrap_or(rest);
    if host.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{host}"))
}

/// 请求体：纯文本还是 JSON 由渠道决定
enum Payload {
    Json(serde_json::Value),
    Text(String),
}

/// 推送发送器
pub struct WebhookNotifier {
    config: WebhookConfig,
    client: reqwest::Client,
    i18n: I18n,
}

impl WebhookNotifier {
    pub fn new(config: WebhookConfig, lang: Lang) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            i18n: I18n::new(lang),
        }
    }

    /// 中断推送（优先级最高，手机上会响）
    pub async fn notify_interrupt(&self, agent_name: &str, session_id: &str, message: &str) {
        if !self.config.enabled || !self.config.notify_on_interrupt {
            return;
        }
        self.push("push.verdict_interrupt", agent_name, session_id, message, true)
            .await;
    }

    /// 续跑推送
    pub async fn notify_resume(&self, agent_name: &str, session_id: &str, message: &str) {
        if !self.config.enabled || !self.config.notify_on_resume {
            return;
        }
        self.push("push.verdict_resume", agent_name, session_id, message, false)
            .await;
    }

    /// 完成推送
    pub async fn notify_complete(&self, agent_name: &str, session_id: &str) {
        if !self.config.enabled || !self.config.notify_on_complete {
            return;
        }
        let body = self.i18n.t("push.complete_body").to_string();
        self.push("push.verdict_complete", agent_name, session_id, &body, false)
            .await;
    }

    async fn push(
        &self,
        verdict_key: &'static str,
        agent_name: &str,
        session_id: &str,
        message: &str,
        high_priority: bool,
    ) {
        if !self.config.is_configured() {
            return;
        }
        let verdict = self.i18n.t(verdict_key);
        let title = self.i18n.tf("push.title", &[("verdict", verdict)]);
        let text = self.format_message(agent_name, session_id, verdict, message);
        let Some((url, payload)) = self.build(&title, &text, high_priority) else {
            return;
        };
        match self.dispatch(&url, payload).await {
            Ok(status) => tracing::info!("[Push] 发送成功 HTTP {status}"),
            Err(detail) => tracing::warn!("[Push] 发送失败: {detail}"),
        }
    }

    fn format_message(
        &self,
        agent_name: &str,
        session_id: &str,
        verdict: &str,
        message: &str,
    ) -> String {
        self.config
            .template
            .replace("{agent_name}", agent_name)
            .replace("{session_id}", session_id)
            .replace("{verdict}", verdict)
            .replace("{message}", message)
    }

    /// 按渠道拼出目标地址和请求体
    ///
    /// ntfy 和 Bark 都优先走 JSON 接口：标题里有中文，而 HTTP 头只保证 ASCII，
    /// 走 `Title:` 头会变成乱码。
    fn build(&self, title: &str, text: &str, high_priority: bool) -> Option<(String, Payload)> {
        let url = self.config.url.trim();
        let topic = self.config.topic.trim();

        let (target, payload) = match self.config.provider.as_str() {
            "slack" => (url.to_string(), Payload::Json(json!({ "text": text }))),
            "discord" => (url.to_string(), Payload::Json(json!({ "content": text }))),
            "ntfy" => {
                if topic.is_empty() {
                    // 地址里已经带了主题，按 ntfy 的约定直接把正文 POST 过去
                    (url.to_string(), Payload::Text(text.to_string()))
                } else {
                    let base = origin(url).unwrap_or_else(|| "https://ntfy.sh".to_string());
                    (
                        base,
                        Payload::Json(json!({
                            "topic": topic,
                            "title": title,
                            "message": text,
                            "priority": if high_priority { 4 } else { 3 },
                            "tags": ["robot"],
                        })),
                    )
                }
            }
            "bark" => {
                if topic.is_empty() {
                    (
                        url.to_string(),
                        Payload::Json(json!({ "title": title, "body": text })),
                    )
                } else {
                    let base = origin(url).unwrap_or_else(|| "https://api.day.app".to_string());
                    (
                        format!("{base}/push"),
                        Payload::Json(json!({
                            "device_key": topic,
                            "title": title,
                            "body": text,
                            "group": "AgentPulse",
                            "level": if high_priority { "timeSensitive" } else { "active" },
                        })),
                    )
                }
            }
            // 自定义 Webhook：几种常见字段名都带上，接收端挑一个用
            _ => (
                url.to_string(),
                Payload::Json(json!({ "title": title, "text": text, "message": text })),
            ),
        };

        if target.is_empty() {
            return None;
        }
        Some((target, payload))
    }

    async fn dispatch(&self, url: &str, payload: Payload) -> Result<u16, String> {
        let request = self.client.post(url).timeout(Duration::from_secs(10));
        let request = match payload {
            Payload::Json(body) => request.json(&body),
            Payload::Text(body) => request
                .header("Content-Type", "text/plain; charset=utf-8")
                .body(body),
        };

        let response = request
            .send()
            .await
            .map_err(|e| self.i18n.tf("err.push_request", &[("detail", &e.to_string())]))?;
        let status = response.status();
        if status.is_success() {
            Ok(status.as_u16())
        } else {
            Err(self
                .i18n
                .tf("err.push_status", &[("status", &status.as_u16().to_string())]))
        }
    }

    /// 「发送测试」按钮：错误直接回给界面，所以这里的文案也走词表
    pub async fn test(&self) -> Result<String, String> {
        let title = self.i18n.t("push.test_title");
        let body = self.i18n.t("push.test_body");
        let (url, payload) = self
            .build(title, body, false)
            .filter(|_| self.config.is_configured())
            .ok_or_else(|| self.i18n.t("err.push_url_missing").to_string())?;
        let status = self.dispatch(&url, payload).await?;
        Ok(self
            .i18n
            .tf("push.test_ok", &[("status", &status.to_string())]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(provider: &str, url: &str, topic: &str) -> WebhookConfig {
        WebhookConfig {
            enabled: true,
            url: url.to_string(),
            provider: provider.to_string(),
            topic: topic.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn origin_keeps_scheme_and_host_only() {
        assert_eq!(
            origin("https://api.day.app/AbCd123"),
            Some("https://api.day.app".to_string())
        );
        assert_eq!(
            origin("http://192.168.1.9:8080/topic/x"),
            Some("http://192.168.1.9:8080".to_string())
        );
        assert_eq!(origin("api.day.app"), None);
        assert_eq!(origin(""), None);
    }

    #[test]
    fn topic_alone_is_enough_for_ntfy_and_bark() {
        assert!(config("ntfy", "", "agent-pulse").is_configured());
        assert!(config("bark", "", "AbCd123").is_configured());
        // 其余渠道没有公共服务端可用，必须有地址
        assert!(!config("slack", "", "whatever").is_configured());
    }

    #[test]
    fn ntfy_topic_goes_into_the_json_body() {
        let notifier = WebhookNotifier::new(config("ntfy", "", "agent-pulse"), Lang::Zh);
        let (url, payload) = notifier.build("标题", "正文", true).expect("应当可发送");
        assert_eq!(url, "https://ntfy.sh");
        match payload {
            // 标题带中文，必须留在 JSON 里而不是 HTTP 头里
            Payload::Json(body) => {
                assert_eq!(body["topic"], "agent-pulse");
                assert_eq!(body["title"], "标题");
                assert_eq!(body["priority"], 4);
            }
            Payload::Text(_) => panic!("填了主题时应当走 JSON 接口"),
        }
    }

    #[test]
    fn ntfy_without_topic_posts_plain_text() {
        let notifier =
            WebhookNotifier::new(config("ntfy", "https://ntfy.sh/agent-pulse", ""), Lang::Zh);
        let (url, payload) = notifier.build("标题", "正文", false).expect("应当可发送");
        assert_eq!(url, "https://ntfy.sh/agent-pulse");
        assert!(matches!(payload, Payload::Text(body) if body == "正文"));
    }

    #[test]
    fn bark_key_uses_the_push_endpoint() {
        let notifier =
            WebhookNotifier::new(config("bark", "https://api.day.app/AbCd123", "AbCd123"), Lang::Zh);
        let (url, payload) = notifier.build("标题", "正文", false).expect("应当可发送");
        // 地址里已经带了 Key 也不该拼成 /AbCd123/push
        assert_eq!(url, "https://api.day.app/push");
        assert!(matches!(payload, Payload::Json(body) if body["device_key"] == "AbCd123"));
    }

    #[test]
    fn unconfigured_channel_sends_nothing() {
        let notifier = WebhookNotifier::new(config("custom", "", ""), Lang::Zh);
        assert!(notifier.build("标题", "正文", false).is_none());
    }
}
