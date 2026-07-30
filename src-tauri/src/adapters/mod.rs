pub mod claude_code;
pub mod codex;
pub mod opencode;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// 进程快照 — 一次扫描共享，避免每个适配器重复枚举进程
#[derive(Debug, Clone)]
pub struct ProcessSnapshot {
    pub pid: u32,
    /// 小写进程名（Windows 含 .exe 后缀）
    pub name: String,
    /// 完整命令行
    pub cmd: String,
    /// 工作目录
    pub cwd: String,
}

/// 获取当前所有进程的轻量快照（仅 name/cmd/cwd，不刷新 CPU/内存/磁盘）
pub fn take_process_snapshot() -> Vec<ProcessSnapshot> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_cwd(UpdateKind::Always),
    );

    system
        .processes()
        .iter()
        .map(|(pid, process)| ProcessSnapshot {
            pid: pid.as_u32(),
            name: process.name().to_string_lossy().to_lowercase(),
            cmd: process
                .cmd()
                .iter()
                .map(|c| c.to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join(" "),
            cwd: process
                .cwd()
                .map(|c| c.to_string_lossy().to_string())
                .unwrap_or_default(),
        })
        .collect()
}

/// 将路径转为 glob 安全的模式串
///
/// glob crate 将 `\` 视为转义字符，Windows 反斜杠路径会导致匹配永远失败，
/// 因此统一转为正斜杠（Windows API 同样接受正斜杠）。
pub fn to_glob_pattern(path: &std::path::Path, suffix: &str) -> String {
    format!("{}{}", path.to_string_lossy().replace('\\', "/"), suffix)
}

/// Agent 会话信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSession {
    /// 会话唯一 ID
    pub id: String,
    /// Agent 类型标识
    pub adapter_id: String,
    /// Agent 显示名称
    pub agent_name: String,
    /// 关联的进程 PID
    pub pid: u32,
    /// 进程命令行
    pub command: String,
    /// 工作目录
    pub working_dir: String,
    /// 会话文件路径（如有）
    pub session_file: Option<String>,
    /// 会话发现时间
    pub discovered_at: String,
    /// 最后活动时间
    pub last_activity: String,
    /// 会话状态
    pub status: SessionStatus,
    /// 已续跑次数
    pub resume_count: u32,
    /// 最后一次续跑时间
    pub last_resume_at: Option<String>,
}

/// 会话状态
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    /// 活跃运行中
    Active,
    /// 疑似中断（检测到中断信号但未确认）
    Suspended,
    /// 已确认中断，等待续跑
    Interrupted,
    /// 已完成
    Completed,
    /// 进程已退出
    Exited,
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionStatus::Active => write!(f, "运行中"),
            SessionStatus::Suspended => write!(f, "疑似中断"),
            SessionStatus::Interrupted => write!(f, "已中断"),
            SessionStatus::Completed => write!(f, "已完成"),
            SessionStatus::Exited => write!(f, "已退出"),
        }
    }
}

/// Agent 适配器 trait — 定义如何发现和监控特定 AI Agent
pub trait AgentAdapter: Send + Sync {
    /// 适配器唯一标识
    fn id(&self) -> &str;
    /// 显示名称
    fn name(&self) -> &str;
    /// 从进程快照中发现该类型 agent 会话
    fn discover_sessions(&self, processes: &[ProcessSnapshot]) -> Vec<AgentSession>;
    /// 获取会话文件路径（用于文件监听）
    fn session_files(&self) -> Vec<PathBuf>;
    /// 读取会话最近的输出内容（用于关键词匹配）
    fn recent_output(&self, session: &AgentSession) -> Option<String>;
}

/// 获取所有已注册的适配器
pub fn all_adapters() -> Vec<Box<dyn AgentAdapter>> {
    vec![
        Box::new(claude_code::ClaudeCodeAdapter::new()),
        Box::new(codex::CodexAdapter),
        Box::new(opencode::OpenCodeAdapter),
    ]
}
