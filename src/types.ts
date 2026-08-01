/**
 * 与 Rust 后端一一对应的类型
 *
 * 字段名就是 serde 序列化后的名字（snake_case），所以这里刻意不做
 * camelCase 转换——多一层映射就多一处能对不上的地方。
 *
 * 显示用的文案一律不放在这里：状态名走 i18n 的 `status.*`，
 * 注意力级别走 `attention.*`。以前 `STATUS_LABELS` 把中文写死在类型文件里，
 * 切英文时就露馅了。
 */

export type SessionStatus =
  "active" | "suspended" | "interrupted" | "completed" | "exited";

/** 注意力分级（v1.1）：这个会话现在要不要叫人 */
export type AttentionLevel =
  "none" | "needs_input" | "completed" | "rate_limited" | "error";

/** 用量汇总，可用于单会话、单项目或单日 */
export interface UsageSnapshot {
  input_tokens: number;
  output_tokens: number;
  cache_write_tokens: number;
  cache_read_tokens: number;
  /** 含缓存部分，即真实上下文规模 */
  total_tokens: number;
  cost_usd: number;
  requests: number;
}

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
  /** 累计帮它按过多少次「继续」，只用于显示 */
  resume_count: number;
  last_resume_at: string | null;
  /** 连着催了几次还不见动静；对着 `max_resume_count` 那道上限，会话一动就清零 */
  resume_streak: number;
  /** 连着几次根本没送达（权限掉了 / 敲进了别的窗口），驱动退避与告警 */
  resume_failures: number;
  attention: AttentionLevel;
  attention_detail: string | null;
  /** 所在终端的 TTY，如 `/dev/ttys003`——多标签页时靠它认人 */
  tty: string | null;
  terminal_app: string | null;
  usage: UsageSnapshot | null;
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
  /** 需要人介入的会话数，也就是托盘角标上的数字 */
  pending_attention: number;
  total_resumes: number;
  total_detections: number;
  last_scan_at: string | null;
  uptime_secs: number;
  cost_today: number;
}

export interface MonitorState {
  running: boolean;
  sessions: AgentSession[];
  events: EngineEvent[];
  status: EngineStatus;
}

/** 后端推过来的提醒事件（`attention-alert`） */
export interface AttentionAlert {
  session_id: string;
  /** 注意力级别，另有预算告警用的 `budget` */
  level: Exclude<AttentionLevel, "none"> | "budget";
  title: string;
  body: string;
  /** 要不要响一声，由后端读配置决定，前端不再自己判断 */
  sound: boolean;
  volume: number;
}

// ===== 配置 =====

export type WebhookProvider = "slack" | "discord" | "ntfy" | "bark" | "custom";

export interface WebhookConfig {
  enabled: boolean;
  url: string;
  provider: WebhookProvider;
  /** ntfy 的主题名 / Bark 的设备 Key；留空则直接 POST `url` */
  topic: string;
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

export interface NotificationConfig {
  enabled: boolean;
  on_needs_input: boolean;
  on_completed: boolean;
  on_rate_limited: boolean;
  on_error: boolean;
  on_resumed: boolean;
  sound_enabled: boolean;
  sound_volume: number;
  /** 同一会话同一状态的最小通知间隔，防刷屏 */
  throttle_secs: number;
  tray_badge: boolean;
}

/** 单个模型的价格覆盖（美元 / 每百万 token） */
export interface ModelPriceOverride {
  model: string;
  input: number;
  output: number;
  cache_write: number | null;
  cache_read: number | null;
}

export interface CostConfig {
  enabled: boolean;
  daily_budget_usd: number;
  session_budget_usd: number;
  alert_at_percent: number;
  rate_limit_window_hours: number;
  rate_limit_token_budget: number;
  price_overrides: ModelPriceOverride[];
}

export interface RemoteConfig {
  enabled: boolean;
  port: number;
  /** true 时监听 0.0.0.0，同网段拿到令牌即可查看——只在可信网络里开 */
  bind_all: boolean;
  token: string;
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
  input_keywords: string[];
  rate_limit_keywords: string[];
  error_keywords: string[];
  notification: NotificationConfig;
  cost: CostConfig;
  remote: RemoteConfig;
}

// ===== 统计与花费 =====

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
  /** `generic` | `goal` */
  prompt_type: string;
  success: boolean;
  message: string;
  created_at: string;
}

export interface DailyCost {
  date: string;
  total_tokens: number;
  cost_usd: number;
  requests: number;
}

export interface ProjectCost {
  project: string;
  total_tokens: number;
  cost_usd: number;
  requests: number;
}

export interface RateLimitForecast {
  window_hours: number;
  used_tokens: number;
  /** 0 表示没配额度，无法预测 */
  budget_tokens: number;
  used_percent: number;
  tokens_per_min: number;
  minutes_to_limit: number | null;
}

export interface SessionHistoryEntry {
  session_key: string;
  session_id: string;
  agent_name: string;
  working_dir: string;
  session_file: string;
  tty: string;
  terminal_app: string;
  first_seen: string;
  last_seen: string;
  last_status: string;
  resume_count: number;
  total_tokens: number;
  cost_usd: number;
}

export interface AiVerdict {
  is_interrupted: boolean;
  confidence: number;
  reasoning: string;
  suggested_prompt: string | null;
}

/** 续跑这条路要用到的一个外部依赖 */
export interface ToolStatus {
  name: string;
  available: boolean;
  /** 后端已本地化的一句话 */
  purpose: string;
}

/**
 * 一次续跑演练的结果
 *
 * 演练走完全部定位流程但**一个字都不敲**，回答的是「现在按续跑，
 * 这串提示词会落到哪儿」——以及落不到的时候，卡在哪一环。
 */
export interface ResumeProbe {
  session_id: string;
  /** `exact` 精确 | `window` 只到窗口 | `none` 定位不到 */
  certainty: "exact" | "window" | "none";
  certainty_label: string;
  channel: string;
  target: string | null;
  detail: string;
  would_deliver: boolean;
  terminal_app: string | null;
  tty: string | null;
  project_name: string;
  allow_blind: boolean;
  /** macOS 上缺「辅助功能」权限：界面据此给一个「去开权限」按钮 */
  needs_permission_fix: boolean;
  tools: ToolStatus[];
}
