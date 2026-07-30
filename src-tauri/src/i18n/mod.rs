use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 支持的语言
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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

/// 国际化翻译管理器
pub struct I18n {
    lang: Lang,
    translations: HashMap<String, HashMap<&'static str, &'static str>>,
}

impl I18n {
    pub fn new(lang: Lang) -> Self {
        let mut translations = HashMap::new();
        translations.insert("zh".to_string(), Self::zh_translations());
        translations.insert("en".to_string(), Self::en_translations());
        Self { lang, translations }
    }

    pub fn set_lang(&mut self, lang: Lang) {
        self.lang = lang;
    }

    pub fn lang(&self) -> Lang {
        self.lang
    }

    /// 获取翻译文本
    pub fn t(&self, key: &str) -> &'static str {
        self.translations
            .get(self.lang.as_str())
            .and_then(|m| m.get(key))
            .copied()
            .unwrap_or("key_not_found")
    }

    /// 获取所有翻译（供前端使用）
    pub fn all(&self) -> HashMap<&'static str, &'static str> {
        self.translations
            .get(self.lang.as_str())
            .cloned()
            .unwrap_or_default()
    }

    fn zh_translations() -> HashMap<&'static str, &'static str> {
        let mut m = HashMap::new();
        m.insert("app.title", "AgentPulse");
        m.insert("app.subtitle", "AI Agent 守护 · Goal 自动恢复 · 跨平台精准续跑");
        m.insert("nav.dashboard", "监控面板");
        m.insert("nav.config", "配置");
        m.insert("nav.stats", "统计");
        m.insert("btn.start", "开始监听");
        m.insert("btn.stop", "停止监听");
        m.insert("btn.scan", "立即分析");
        m.insert("btn.resume", "续跑");
        m.insert("btn.save", "保存配置");
        m.insert("btn.saved", "✓ 已保存");
        m.insert("status.running", "监控运行中");
        m.insert("status.stopped", "监控已停止");
        m.insert("status.sessions", "监控会话");
        m.insert("status.active", "活跃中");
        m.insert("status.interrupted", "已中断");
        m.insert("status.resumes", "自动续跑");
        m.insert("status.detections", "检测次数");
        m.insert("session.empty", "暂未发现 AI Agent 会话");
        m.insert("session.empty_hint", "启动 Claude Code / Codex / OpenCode 后将自动检测");
        m.insert("log.title", "运行日志");
        m.insert("log.empty", "暂无日志，启动监控后将实时输出...");
        m.insert("config.detection", "检测设置");
        m.insert("config.behavior", "行为设置");
        m.insert("config.system", "系统设置");
        m.insert("config.prompts", "续跑提示词");
        m.insert("config.keywords", "关键词触发");
        m.insert("config.webhook", "Webhook 通知");
        m.insert("config.ai", "AI 智能判断");
        m.insert("tray.show", "显示主窗口");
        m.insert("tray.start", "开始监控");
        m.insert("tray.stop", "停止监控");
        m.insert("tray.scan", "立即扫描");
        m.insert("tray.quit", "退出 AgentPulse");
        m
    }

    fn en_translations() -> HashMap<&'static str, &'static str> {
        let mut m = HashMap::new();
        m.insert("app.title", "AgentPulse");
        m.insert("app.subtitle", "AI Agent Guardian · Goal Auto-Recovery · Cross-Platform Resume");
        m.insert("nav.dashboard", "Dashboard");
        m.insert("nav.config", "Settings");
        m.insert("nav.stats", "Statistics");
        m.insert("btn.start", "Start Monitoring");
        m.insert("btn.stop", "Stop Monitoring");
        m.insert("btn.scan", "Scan Now");
        m.insert("btn.resume", "Resume");
        m.insert("btn.save", "Save Config");
        m.insert("btn.saved", "✓ Saved");
        m.insert("status.running", "Monitoring Active");
        m.insert("status.stopped", "Monitoring Stopped");
        m.insert("status.sessions", "Sessions");
        m.insert("status.active", "Active");
        m.insert("status.interrupted", "Interrupted");
        m.insert("status.resumes", "Auto Resumes");
        m.insert("status.detections", "Detections");
        m.insert("session.empty", "No AI Agent sessions found");
        m.insert("session.empty_hint", "Start Claude Code / Codex / OpenCode to auto-detect");
        m.insert("log.title", "Runtime Logs");
        m.insert("log.empty", "No logs yet. Start monitoring to see real-time output...");
        m.insert("config.detection", "Detection Settings");
        m.insert("config.behavior", "Behavior Settings");
        m.insert("config.system", "System Settings");
        m.insert("config.prompts", "Resume Prompts");
        m.insert("config.keywords", "Keyword Triggers");
        m.insert("config.webhook", "Webhook Notifications");
        m.insert("config.ai", "AI Smart Judgment");
        m.insert("tray.show", "Show Window");
        m.insert("tray.start", "Start Monitoring");
        m.insert("tray.stop", "Stop Monitoring");
        m.insert("tray.scan", "Scan Now");
        m.insert("tray.quit", "Quit AgentPulse");
        m
    }
}

impl Default for I18n {
    fn default() -> Self {
        Self::new(Lang::Zh)
    }
}
