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

/** `2026-07-30 14:03:22` / ISO → `7/30 14:03` */
export function formatShortTime(raw: string): string {
  const normalized = raw.includes("T") ? raw : raw.replace(" ", "T");
  const d = new Date(normalized);
  if (Number.isNaN(d.getTime())) return raw;
  const pad = (n: number) => n.toString().padStart(2, "0");
  return `${d.getMonth() + 1}/${d.getDate()} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** `/Users/sky/code/git/agent-pulse` → `agent-pulse` */
export function baseName(path: string): string {
  const parts = path.split(/[/\\]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}
