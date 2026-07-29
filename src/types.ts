/** 与 Rust 后端对应的类型定义 */

export type SessionStatus =
  | "active"
  | "suspended"
  | "interrupted"
  | "completed"
  | "exited";

export interface AgentSession {
  id: string;
  adapter_id: string;
  agent_name: string;
  pid: number;
  command: string;
  working_dir: string;
  session_file: string | null;
  discovered_at: string;
  last_activity: string;
  status: SessionStatus;
  resume_count: number;
  last_resume_at: string | null;
}

export type LogLevel = "info" | "warn" | "error" | "success";

export interface EngineEvent {
  timestamp: string;
  level: LogLevel;
  session_id: string | null;
  message: string;
}

export interface EngineStatus {
  running: boolean;
  sessions_total: number;
  sessions_active: number;
  sessions_interrupted: number;
  total_resumes: number;
  total_detections: number;
  last_scan_at: string | null;
  uptime_secs: number;
}

export interface MonitorState {
  running: boolean;
  sessions: AgentSession[];
  events: EngineEvent[];
  status: EngineStatus;
}

export interface AppConfig {
  poll_interval_secs: number;
  idle_timeout_secs: number;
  idle_threshold: number;
  max_resume_count: number;
  resume_cooldown_secs: number;
  check_on_startup: boolean;
  auto_follow_latest: boolean;
  heartbeat_log: boolean;
  custom_keywords: string[];
  completion_markers: string[];
  resume_prompt: string;
  goal_resume_prompt: string;
  goal_keywords: string[];
  auto_resume_enabled: boolean;
  enabled_adapters: string[];
}

export const STATUS_LABELS: Record<SessionStatus, string> = {
  active: "运行中",
  suspended: "疑似中断",
  interrupted: "已中断",
  completed: "已完成",
  exited: "已退出",
};

export const STATUS_COLORS: Record<SessionStatus, string> = {
  active: "text-emerald-400 bg-emerald-400/10 border-emerald-400/30",
  suspended: "text-amber-400 bg-amber-400/10 border-amber-400/30",
  interrupted: "text-red-400 bg-red-400/10 border-red-400/30",
  completed: "text-blue-400 bg-blue-400/10 border-blue-400/30",
  exited: "text-gray-400 bg-gray-400/10 border-gray-400/30",
};
