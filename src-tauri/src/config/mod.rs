use crate::ai_judge::AiJudgeConfig;
use crate::webhook::WebhookConfig;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

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

/// 桌面通知与声音提醒配置（v1.1 感知层）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    /// 总开关
    pub enabled: bool,
    /// 🔴 需要输入/授权时通知
    pub on_needs_input: bool,
    /// 🟢 任务完成时通知
    pub on_completed: bool,
    /// 🟡 触发限流等待时通知
    pub on_rate_limited: bool,
    /// ⚫ 出错时通知
    pub on_error: bool,
    /// 自动续跑成功后通知
    pub on_resumed: bool,
    /// 声音提醒
    pub sound_enabled: bool,
    /// 音量（0-100）
    pub sound_volume: u32,
    /// 同一会话同一状态的最小通知间隔（秒），防止刷屏
    pub throttle_secs: u64,
    /// 托盘角标显示待处理数量
    pub tray_badge: bool,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            on_needs_input: true,
            on_completed: true,
            on_rate_limited: true,
            on_error: true,
            on_resumed: false,
            sound_enabled: true,
            sound_volume: 60,
            throttle_secs: 120,
            tray_badge: true,
        }
    }
}

/// 单个模型的价格覆盖（美元 / 每百万 token）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPriceOverride {
    /// 模型 ID（支持前缀匹配）
    pub model: String,
    pub input: f64,
    pub output: f64,
    /// 缓存写入价（留空则按 input × 1.25 计算）
    #[serde(default)]
    pub cache_write: Option<f64>,
    /// 缓存读取价（留空则按 input × 0.1 计算）
    #[serde(default)]
    pub cache_read: Option<f64>,
}

/// Token / 成本统计与预算告警配置（v1.2 洞察层）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostConfig {
    /// 总开关
    pub enabled: bool,
    /// 每日预算（美元），0 = 不限
    pub daily_budget_usd: f64,
    /// 单会话预算（美元），0 = 不限
    pub session_budget_usd: f64,
    /// 达到预算的百分之多少时告警
    pub alert_at_percent: u32,
    /// 限流窗口长度（小时）——Claude 订阅制为 5 小时滚动窗口
    pub rate_limit_window_hours: u32,
    /// 窗口内的 token 预算，0 = 不做限流预测
    pub rate_limit_token_budget: u64,
    /// 自定义模型价格覆盖
    #[serde(default)]
    pub price_overrides: Vec<ModelPriceOverride>,
}

impl Default for CostConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            daily_budget_usd: 0.0,
            session_budget_usd: 0.0,
            alert_at_percent: 80,
            rate_limit_window_hours: 5,
            rate_limit_token_budget: 0,
            price_overrides: vec![],
        }
    }
}

/// 本地只读看板配置（v1.3 远程层）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteConfig {
    /// 总开关
    pub enabled: bool,
    /// 监听端口
    pub port: u16,
    /// 是否监听 0.0.0.0（暴露到局域网，需自行承担风险）
    pub bind_all: bool,
    /// 访问令牌，为空时启动服务前自动生成
    pub token: String,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 17650,
            bind_all: false,
            token: String::new(),
        }
    }
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
    /// 「需要我输入」类关键词（权限确认 / 等待回答）
    #[serde(default = "default_input_keywords")]
    pub input_keywords: Vec<String>,
    /// 「限流等待」类关键词
    #[serde(default = "default_rate_limit_keywords")]
    pub rate_limit_keywords: Vec<String>,
    /// 「出错」类关键词
    #[serde(default = "default_error_keywords")]
    pub error_keywords: Vec<String>,
    /// 桌面通知配置
    #[serde(default)]
    pub notification: NotificationConfig,
    /// 成本统计配置
    #[serde(default)]
    pub cost: CostConfig,
    /// 本地只读看板配置
    #[serde(default)]
    pub remote: RemoteConfig,
}

fn default_lang() -> String {
    "zh".to_string()
}

fn default_input_keywords() -> Vec<String> {
    vec![
        "Do you want".to_string(),
        "Would you like".to_string(),
        "(y/n)".to_string(),
        "[y/N]".to_string(),
        "Yes, and don't ask again".to_string(),
        "requires approval".to_string(),
        "permission to".to_string(),
        "Allow this".to_string(),
        "waiting for your".to_string(),
        "Press Enter to continue".to_string(),
        "需要确认".to_string(),
        "是否继续".to_string(),
    ]
}

fn default_rate_limit_keywords() -> Vec<String> {
    vec![
        "rate limit".to_string(),
        "rate_limit_error".to_string(),
        "429".to_string(),
        "overloaded".to_string(),
        "usage limit reached".to_string(),
        "quota exceeded".to_string(),
        "retrying in".to_string(),
        "too many requests".to_string(),
    ]
}

fn default_error_keywords() -> Vec<String> {
    vec![
        "connection error".to_string(),
        "timed out".to_string(),
        "ECONNRESET".to_string(),
        "ETIMEDOUT".to_string(),
        "internal server error".to_string(),
        "500".to_string(),
        "authentication_error".to_string(),
        "invalid api key".to_string(),
        "panic".to_string(),
        "fatal error".to_string(),
    ]
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
            input_keywords: default_input_keywords(),
            rate_limit_keywords: default_rate_limit_keywords(),
            error_keywords: default_error_keywords(),
            notification: NotificationConfig::default(),
            cost: CostConfig::default(),
            remote: RemoteConfig::default(),
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
