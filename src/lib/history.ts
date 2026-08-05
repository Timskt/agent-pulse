/**
 * 会话档案页的纯逻辑
 *
 * 单独一个文件，而不是留在 `HistoryPanel.tsx` 里：那个文件一 import 就连带
 * 拽进 React、Radix 和 Tauri 的 `invoke`，测一个「按天分组」得先搭一套
 * 浏览器环境。这里的东西全是「数据进、数据出」，直接测。
 *
 * 边界跟 `display.ts` 一致：**只返回 i18n 的 key，不返回具体文字**，
 * 或者接一个 `t` 进来自己译。
 */

import type { I18nKey, Translator } from "../i18n";
import type { SessionHistoryEntry } from "../types";
import { dayOf } from "./utils";

/**
 * 按天分好组，保持后端给的顺序
 *
 * 用 `Map` 而不是先 `reduce` 成对象再 `Object.entries`：对象键是字符串，
 * 而 `2026-07-30` 这种键在 V8 里不算数组索引，顺序其实是稳的——但那是
 * 实现细节，不该赌。`Map` 明确按插入序迭代。
 *
 * 分组只在**当前这一页**内进行，所以同一天可能横跨两页各出现一次。
 * 这是分页的固有结果，不是 bug：后端排序是「活着的在前，再按最后一次
 * 看到的时间倒序」，硬要一天不拆页就得改成按天取数，那样每页行数不定，
 * 翻页器就没法算了。
 */
export function groupByDay(
  entries: readonly SessionHistoryEntry[],
): [string, SessionHistoryEntry[]][] {
  const groups = new Map<string, SessionHistoryEntry[]>();
  for (const entry of entries) {
    const day = dayOf(entry.last_seen);
    const bucket = groups.get(day);
    if (bucket) bucket.push(entry);
    else groups.set(day, [entry]);
  }
  return [...groups];
}

/**
 * 本地时区的 `YYYY-MM-DD`
 *
 * **不要用 `toISOString().slice(0, 10)`。** 那个先转 UTC：东八区晚上八点
 * 之后，它会说今天是明天，于是「今天」这一组的标题落在昨天那堆会话上。
 * 库里的时间戳是 `Local::now()` 写的，两边必须都按本地时区读。
 */
export function todayKey(d: Date): string {
  const pad = (n: number) => n.toString().padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/**
 * 前一天，按**本地日历**退，不是减 24 小时
 *
 * 这个函数存在的唯一理由是夏令时。减 `86_400_000` 毫秒在平常的日子里没问题，
 * 但换季那两天不是 24 小时，两头都会错：
 *
 * - 春天少一小时（洛杉矶 2026-03-09 00:30 减 24 小时 → 03-07）：**03-08 整天
 *   被跳过**，昨天那组永远对不上。
 * - 秋天多一小时（洛杉矶 2026-11-01 23:30 减 24 小时 → 还是 11-01）：算出来的
 *   「昨天」跟今天是同一天，昨天那组同样对不上。
 *
 * 两头的后果一样——昨天那组的标题从「昨天」退回裸日期。DST 区一年撞两次，
 * `setDate(n - 1)` 让运行时按本地日历退一格，两种情况都不会发生。
 */
function previousDay(d: Date): Date {
  const out = new Date(d);
  out.setDate(out.getDate() - 1);
  return out;
}

/**
 * 分组标题该怎么念：今天 / 昨天 / 原样的日期
 *
 * `now` 是参数而不是函数里现取的 `new Date()`，纯粹为了能测——
 * 「跨午夜、跨夏令时的时候这条标题会不会说错」是这个函数唯一容易错的地方，
 * 而那件事没法靠等到半夜来验。
 */
export function dayLabelKey(
  day: string,
  now: Date,
): { key: I18nKey } | { literal: string } {
  if (day === todayKey(now)) return { key: "history.today" };
  if (day === todayKey(previousDay(now))) return { key: "history.yesterday" };
  return { literal: day };
}

/** 译好的分组标题 */
export function dayLabel(day: string, now: Date, t: Translator["t"]): string {
  const label = dayLabelKey(day, now);
  return "key" in label ? t(label.key) : label.literal;
}
