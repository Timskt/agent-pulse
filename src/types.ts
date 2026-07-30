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

// ===== v0.3.0 / v1.0.0 新增类型 =====

export interface WebhookConfig {
  enabled: boolean;
  url: string;
  provider: string;
  template: string;
  notify_on_interrupt: boolean;
  notify_on_resume: boolean;
  notify_on_complete: boolean;
}

export interface AiJudgeConfig {
  enabled: boolean;
  api_url: string;
  api_key: string;
  model: string;
  confidence_threshold: number;
}

export interface CustomAdapterConfig {
  name: string;
  process_pattern: string;
  session_file_pattern: string;
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
  webhook: WebhookConfig;
  ai_judge: AiJudgeConfig;
  language: string;
  custom_adapters: CustomAdapterConfig[];
}

// ===== 统计类型 =====

export interface DailyStats {
  date: string;
  total_scans: number;
  total_detections: number;
  total_resumes: number;
  successful_resumes: number;
  failed_resumes: number;
}

export interface ResumeRecord {
  id: number;
  session_id: string;
  agent_name: string;
  working_dir: string;
  prompt_type: string;
  success: boolean;
  message: string;
  created_at: string;
}

export interface AiVerdict {
  is_interrupted: boolean;
  confidence: number;
  reasoning: string;
  suggested_prompt: string | null;
}

export const STATUS_LABELS: Record<SessionStatus, string> = {
  active: "运行中",
  suspended: "疑似中断",
  interrupted: "已中断",
  completed: "已完成",
  exited: "已退出",
};

export const STATUS_COLORS: Record<SessionStatus, string> = {
  active: "text-emerald-600 bg-emerald-50",
  suspended: "text-amber-600 bg-amber-50",
  interrupted: "text-red-600 bg-red-50",
  completed: "text-blue-600 bg-blue-50",
  exited: "text-neutral-500 bg-neutral-100",
};
