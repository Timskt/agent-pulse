import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

/**
 * 合并 class：后写的 Tailwind 工具类覆盖先写的同族类
 *
 * 组件库的每个基元都收 `className`，没有它就只能靠字符串拼接，
 * 结果是 `px-3` 和 `px-6` 同时出现、谁赢看打包顺序。
 */
export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}

/** `1234567` → `1.23M`，给 token 数用 */
export function formatTokens(tokens: number): string {
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(2)}M`;
  if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}K`;
  return String(tokens);
}

/** 花费统一 4 位小数以内，小额不至于显示成 $0.00 */
export function formatUsd(usd: number): string {
  if (usd === 0) return "0";
  if (usd < 0.01) return usd.toFixed(4);
  return usd.toFixed(2);
}

/**
 * 库里的时间戳 → `Date`，解析不出来就是 `null`
 *
 * 后端写的是本地时间 `2026-07-30 14:03:22`（`Local::now()`），**不带时区**。
 * 那个空格得换成 `T` 才能被当成本地时间解析；直接塞给 `new Date()`，
 * Safari 系的引擎会返回 `Invalid Date`。
 */
function parseStamp(raw: string): Date | null {
  if (!raw) return null;
  const d = new Date(raw.includes("T") ? raw : raw.replace(" ", "T"));
  return Number.isNaN(d.getTime()) ? null : d;
}

/** `2026-07-30 14:03:22` / ISO → `7/30 14:03` */
export function formatShortTime(raw: string): string {
  const d = parseStamp(raw);
  if (!d) return raw;
  const pad = (n: number) => n.toString().padStart(2, "0");
  return `${d.getMonth() + 1}/${d.getDate()} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** `2026-07-30 14:03:22` → `2026-07-30`；分组用的日期键 */
export function dayOf(raw: string): string {
  return raw.slice(0, 10);
}

/**
 * 两个时间戳之间的秒数；任一头解析不出来就是 `null`
 *
 * 返回 `null` 而不是 0：「算不出持续多久」和「持续了 0 秒」在界面上
 * 是两句不同的话，用 0 兼职表示前者会让一条刚开始的会话显示成「持续 0 秒」，
 * 看着像出了故障。
 */
export function secondsBetween(from: string, to: string): number | null {
  const a = parseStamp(from);
  const b = parseStamp(to);
  if (!a || !b) return null;
  return Math.max(0, Math.round((b.getTime() - a.getTime()) / 1000));
}

/** `2026-07-30` → `07-30`；坐标轴上年份是噪音 */
export function shortDate(date: string): string {
  return date.length >= 10 ? date.slice(5) : date;
}

/** `/Users/sky/code/git/agent-pulse` → `agent-pulse` */
export function baseName(path: string): string {
  const parts = path.split(/[/\\]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}
