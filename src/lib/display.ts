/**
 * 状态 → 颜色 / 词条 的映射
 *
 * 以前 `types.ts` 里有一份写死中文的 `STATUS_LABELS`，切英文时就露馅了。
 * 现在类型文件只管数据形状，显示相关的东西全在这里，而且只映射到
 * **i18n 的 key**，不映射到具体文字。
 */

import type { BadgeTone } from "../components/ui";
import type { I18nKey } from "../i18n";
import type {
  AttentionLevel,
  InterruptReason,
  LogLevel,
  ResumeTactic,
  SessionStatus,
} from "../types";

export const STATUS_TONE: Record<SessionStatus, BadgeTone> = {
  active: "green",
  suspended: "amber",
  interrupted: "red",
  completed: "blue",
  exited: "neutral",
};

/** 会话列表左侧的小圆点 */
export const STATUS_DOT: Record<SessionStatus, string> = {
  active: "bg-emerald-500",
  suspended: "bg-amber-500",
  interrupted: "bg-red-500",
  completed: "bg-blue-500",
  exited: "bg-neutral-300",
};

export function statusKey(status: SessionStatus): I18nKey {
  return `status.${status}`;
}

/** `none` 不显示徽标，所以这里刻意不给它配色 */
export const ATTENTION_TONE: Record<Exclude<AttentionLevel, "none">, BadgeTone> = {
  needs_input: "red",
  completed: "green",
  rate_limited: "amber",
  error: "neutral",
};

/**
 * 注意力级别的图标
 *
 * 需求文档里定的四色语义：🔴 等我输入、🟢 已完成、🟡 限流等待、⚫ 出错。
 * 用 emoji 而不是自绘圆点，是因为系统通知里也是这几个字符，两处对得上。
 */
export const ATTENTION_ICON: Record<Exclude<AttentionLevel, "none">, string> = {
  needs_input: "🔴",
  completed: "🟢",
  rate_limited: "🟡",
  error: "⚫",
};

export function attentionKey(level: Exclude<AttentionLevel, "none">): I18nKey {
  return `attention.${level}`;
}

export function reasonKey(reason: Exclude<InterruptReason, "none">): I18nKey {
  return `reason.${reason}`;
}

/**
 * 「这次故意没敲字」该怎么说
 *
 * 注意这里查的是**手段**，不是原因。第一版写的是一份原因名单
 * （`process_crashed` / `rate_limited` / `awaiting_input`），照着 Rust 的
 * `tactic()` 抄的——那就是同一条策略存了两份：下次加一个原因，
 * 两边的类型都还是通的，界面却会凭空多出一句「这次没帮你按继续」，
 * 或者更糟，该说的时候不说。
 *
 * 现在手段由后端算好一起发上来（`session.resume_tactic`），界面只负责画。
 * `nudge` 不在表里是因为那是默认动作，没什么可解释的。
 */
export const TACTIC_NOTE: Record<Exclude<ResumeTactic, "nudge">, I18nKey> = {
  wait: "tactic.wait",
  hand_off: "tactic.hand_off",
};

/** 日志级别的文字颜色 */
export const LOG_TONE: Record<LogLevel, string> = {
  info: "text-neutral-500",
  warn: "text-amber-600",
  error: "text-red-500",
  success: "text-emerald-600",
};
