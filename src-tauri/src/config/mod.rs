use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use crate::webhook::WebhookConfig;
use crate::ai_judge::AiJudgeConfig;

/// 自定义适配器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomAdapterConfig {
    /// 适配器名称
    pub name: String,
    /// 进程匹配关键词
    pub process_pattern: String,
    /// 会话文件路径模式（可选）
    pub session_file_pattern: String,
}

/// 应用全局配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// 轮询间隔（秒）
    pub poll_interval_secs: u64,
    /// 无活动超时判定（秒）
    pub idle_timeout_secs: u64,
    /// 连续多少次无活动判定为中断
    pub idle_threshold: u32,
    /// 单个会话最大自动续跑次数
    pub max_resume_count: u32,
    /// 两次续跑之间的冷却时间（秒）
    pub resume_cooldown_secs: u64,
    /// 启动时是否立即扫描
    pub check_on_startup: bool,
    /// 是否自动跟随最新会话
    pub auto_follow_latest: bool,
    /// 是否启用心跳日志
    pub heartbeat_log: bool,
    /// 自定义触发关键词
    pub custom_keywords: Vec<String>,
    /// 完成标记列表
    pub completion_markers: Vec<String>,
    /// 续跑提示词（通用）
    pub resume_prompt: String,
    /// Goal 恢复专用提示词
    pub goal_resume_prompt: String,
    /// Goal 相关关键词
    pub goal_keywords: Vec<String>,
    /// 是否启用自动续跑
    pub auto_resume_enabled: bool,
    /// 监控的 agent 类型
    pub enabled_adapters: Vec<String>,
    /// Webhook 通知配置
    #[serde(default)]
    pub webhook: WebhookConfig,
    /// AI 智能判断配置
    #[serde(default)]
    pub ai_judge: AiJudgeConfig,
    /// 界面语言 ("zh" | "en")
    #[serde(default = "default_lang")]
    pub language: String,
    /// 自定义适配器列表
    #[serde(default)]
    pub custom_adapters: Vec<CustomAdapterConfig>,
}

fn default_lang() -> String {
    "zh".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            poll_interval_secs: 10,
            idle_timeout_secs: 60,
            idle_threshold: 3,
            max_resume_count: 5,
            resume_cooldown_secs: 30,
            check_on_startup: true,
            auto_follow_latest: false,
            heartbeat_log: false,
            custom_keywords: vec![
                "rate limit".to_string(),
                "overloaded".to_string(),
                "connection error".to_string(),
                "timed out".to_string(),
            ],
            completion_markers: vec![
                "Task completed".to_string(),
                "task completed".to_string(),
                "All tasks completed".to_string(),
                "✓ Done".to_string(),
                "completed successfully".to_string(),
            ],
            resume_prompt: "请继续完成刚才未完成的任务，不要重新开始。".to_string(),
            goal_resume_prompt: "你之前有一个活跃的 goal 目标还未完成，请立即恢复并继续执行。不要重新规划，直接从上次中断的地方继续。".to_string(),
            goal_keywords: vec![
                "goal".to_string(),
                "objective".to_string(),
                "Goal completed".to_string(),
                "goal paused".to_string(),
                "goal blocked".to_string(),
                "updateGoal".to_string(),
                "createGoal".to_string(),
                "turn_budget".to_string(),
                "Turns remaining".to_string(),
            ],
            auto_resume_enabled: true,
            enabled_adapters: vec!["claude-code".to_string(), "codex".to_string(), "opencode".to_string()],
            webhook: WebhookConfig::default(),
            ai_judge: AiJudgeConfig::default(),
            language: "zh".to_string(),
            custom_adapters: vec![],
        }
    }
}

/// 配置管理器
pub struct ConfigManager {
    config: Mutex<AppConfig>,
    config_path: PathBuf,
}

impl Default for ConfigManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigManager {
    pub fn new() -> Self {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("agent-pulse");
        let config_path = config_dir.join("config.json");

        let config = if config_path.exists() {
            match fs::read_to_string(&config_path) {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(_) => AppConfig::default(),
            }
        } else {
            let cfg = AppConfig::default();
            // 首次启动时写入默认配置
            if let Some(parent) = config_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string_pretty(&cfg) {
                let _ = fs::write(&config_path, json);
            }
            cfg
        };

        Self {
            config: Mutex::new(config),
            config_path,
        }
    }

    pub fn get(&self) -> AppConfig {
        self.config.lock().unwrap().clone()
    }

    pub fn update(&self, new_config: AppConfig) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&new_config)
            .map_err(|e| format!("序列化配置失败: {e}"))?;
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?;
        }
        fs::write(&self.config_path, json).map_err(|e| format!("写入配置失败: {e}"))?;
        *self.config.lock().unwrap() = new_config;
        Ok(())
    }
}
