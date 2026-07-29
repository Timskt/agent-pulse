use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

/// 应用全局配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// 轮询间隔（秒）
    pub poll_interval_secs: u64,
    /// 无活动超时判定（秒），超过此时间无输出视为疑似中断
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
    /// 自定义触发关键词（多个用逗号分隔）
    pub custom_keywords: Vec<String>,
    /// 完成标记列表（出现则不触发续跑）
    pub completion_markers: Vec<String>,
    /// 续跑提示词
    pub resume_prompt: String,
    /// 是否启用自动续跑（关闭则仅通知）
    pub auto_resume_enabled: bool,
    /// 监控的 agent 类型
    pub enabled_adapters: Vec<String>,
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
            auto_resume_enabled: true,
            enabled_adapters: vec!["claude-code".to_string(), "codex".to_string(), "opencode".to_string()],
        }
    }
}

/// 配置管理器
pub struct ConfigManager {
    config: Mutex<AppConfig>,
    config_path: PathBuf,
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
