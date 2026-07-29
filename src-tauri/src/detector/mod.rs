use crate::adapters::{AgentSession, SessionStatus};
use crate::config::AppConfig;
use chrono::{Local, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// 检测结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionResult {
    /// 会话 ID
    pub session_id: String,
    /// 是否检测到中断
    pub interrupted: bool,
    /// 检测到的信号列表
    pub signals: Vec<DetectionSignal>,
    /// 是否发现完成标记（有完成标记则不触发续跑）
    pub has_completion_marker: bool,
    /// 匹配到的完成标记
    pub matched_marker: Option<String>,
    /// 是否检测到活跃 Goal（用于智能选择续跑提示词）
    pub has_active_goal: bool,
    /// 判定结论
    pub verdict: Verdict,
    /// 检测时间
    pub detected_at: String,
}

/// 检测信号
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionSignal {
    pub kind: SignalKind,
    pub description: String,
}

/// 信号类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    /// 进程空闲（CPU 无活动）
    ProcessIdle,
    /// 会话文件长时间未更新
    FileStale,
    /// 匹配到中断关键词
    KeywordMatch,
    /// 进程已退出
    ProcessExited,
    /// 心跳超时
    HeartbeatTimeout,
}

/// 判定结论
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// 正常运行
    Running,
    /// 疑似中断，继续观察
    Suspicious,
    /// 确认中断，应触发续跑
    ConfirmInterrupt,
    /// 任务已完成，无需续跑
    TaskCompleted,
}

/// 多策略检测引擎
pub struct Detector {
    config: AppConfig,
}

impl Detector {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }

    /// 对单个会话执行全策略检测
    pub fn detect(&self, session: &AgentSession, recent_output: Option<&str>) -> DetectionResult {
        let now = Local::now();
        let mut signals = Vec::new();
        let mut has_completion_marker = false;
        let mut matched_marker: Option<String> = None;
        let mut has_active_goal = false;

        // 策略 1: 进程存活检测
        let process_alive = self.check_process_alive(session.pid);
        if !process_alive {
            signals.push(DetectionSignal {
                kind: SignalKind::ProcessExited,
                description: format!("进程 {} 已退出", session.pid),
            });
        }

        // 策略 2: 会话文件新鲜度检测
        if let Some(ref file_path) = session.session_file {
            if let Some(stale_secs) = self.check_file_staleness(file_path) {
                if stale_secs > self.config.idle_timeout_secs {
                    signals.push(DetectionSignal {
                        kind: SignalKind::FileStale,
                        description: format!(
                            "会话文件已 {}s 未更新（阈值 {}s）",
                            stale_secs, self.config.idle_timeout_secs
                        ),
                    });
                }
            }
        }

        // 策略 3: 关键词匹配（中断信号 + 完成标记双重校验）
        if let Some(output) = recent_output {
            // 检查完成标记
            for marker in &self.config.completion_markers {
                if output.contains(marker.as_str()) {
                    has_completion_marker = true;
                    matched_marker = Some(marker.clone());
                    break;
                }
            }

            // 检查中断关键词
            if !has_completion_marker {
                for keyword in &self.config.custom_keywords {
                    if output.to_lowercase().contains(&keyword.to_lowercase()) {
                        signals.push(DetectionSignal {
                            kind: SignalKind::KeywordMatch,
                            description: format!("输出中匹配到关键词: \"{keyword}\""),
                        });
                        break; // 一个关键词足够
                    }
                }

                // 检测活跃 Goal 状态（用于智能续跑提示词选择）
                for goal_kw in &self.config.goal_keywords {
                    if output.contains(goal_kw.as_str()) {
                        has_active_goal = true;
                        break;
                    }
                }
            }
        }

        // 策略 4: 心跳超时（基于 last_activity）
        if let Ok(last) = NaiveDateTime::parse_from_str(&session.last_activity, "%Y-%m-%d %H:%M:%S")
        {
            let elapsed = now.naive_local() - last;
            let timeout = self.config.idle_timeout_secs * self.config.idle_threshold as u64;
            if elapsed.num_seconds() as u64 > timeout {
                signals.push(DetectionSignal {
                    kind: SignalKind::HeartbeatTimeout,
                    description: format!(
                        "心跳超时：最后活动距今 {}s（阈值 {}s）",
                        elapsed.num_seconds(),
                        timeout
                    ),
                });
            }
        }

        // 综合判定
        let verdict = self.make_verdict(process_alive, &signals, has_completion_marker, session);

        DetectionResult {
            session_id: session.id.clone(),
            interrupted: verdict == Verdict::ConfirmInterrupt,
            signals,
            has_completion_marker,
            matched_marker,
            has_active_goal,
            verdict,
            detected_at: now.format("%Y-%m-%d %H:%M:%S").to_string(),
        }
    }

    /// 检查进程是否存活
    fn check_process_alive(&self, pid: u32) -> bool {
        let system = sysinfo::System::new_all();
        system.process(sysinfo::Pid::from_u32(pid)).is_some()
    }

    /// 检查文件距上次修改的秒数
    fn check_file_staleness(&self, path: &str) -> Option<u64> {
        let path_buf = PathBuf::from(path);
        let metadata = fs::metadata(&path_buf).ok()?;
        let modified = metadata.modified().ok()?;
        let elapsed = Local::now()
            .naive_local()
            .signed_duration_since(chrono::DateTime::<Local>::from(modified).naive_local());
        Some(elapsed.num_seconds().max(0) as u64)
    }

    /// 综合判定逻辑
    fn make_verdict(
        &self,
        process_alive: bool,
        signals: &[DetectionSignal],
        has_completion_marker: bool,
        session: &AgentSession,
    ) -> Verdict {
        // 已完成 → 不续跑
        if has_completion_marker {
            return Verdict::TaskCompleted;
        }

        // 会话已标记完成或退出
        if session.status == SessionStatus::Completed || session.status == SessionStatus::Exited {
            return Verdict::Running;
        }

        // 进程已退出且无完成标记 → 确认中断
        if !process_alive {
            return Verdict::ConfirmInterrupt;
        }

        // 达到续跑上限 → 不再续跑
        if session.resume_count >= self.config.max_resume_count {
            return Verdict::Suspicious;
        }

        // 有中断信号 → 确认中断
        let has_interrupt_signal = signals.iter().any(|s| {
            matches!(
                s.kind,
                SignalKind::FileStale | SignalKind::KeywordMatch | SignalKind::HeartbeatTimeout
            )
        });

        if has_interrupt_signal {
            // 至少两个信号或一个强信号才确认
            let strong_signal = signals.iter().any(|s| {
                matches!(s.kind, SignalKind::KeywordMatch | SignalKind::HeartbeatTimeout)
            });
            if signals.len() >= 2 || strong_signal {
                Verdict::ConfirmInterrupt
            } else {
                Verdict::Suspicious
            }
        } else {
            Verdict::Running
        }
    }
}
