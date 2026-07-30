/**
 * 状态 → 颜色 / 词条 的映射
 *
 * 以前 `types.ts` 里有一份写死中文的 `STATUS_LABELS`，切英文时就露馅了。
 * 现在类型文件只管数据形状，显示相关的东西全在这里，而且只映射到
 * **i18n 的 key**，不映射到具体文字。
 */

import type { BadgeTone } from "../components/ui";
import type { I18nKey } from "../i18n";
import type { AttentionLevel, LogLevel, SessionStatus } from "../types";

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

/** 日志级别的文字颜色 */
export const LOG_TONE: Record<LogLevel, string> = {
  info: "text-neutral-500",
  warn: "text-amber-600",
  error: "text-red-500",
  success: "text-emerald-600",
};
