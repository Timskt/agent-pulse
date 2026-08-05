/**
 * 状态 → 颜色 / 词条 的映射
 *
 * 以前 `types.ts` 里有一份写死中文的 `STATUS_LABELS`，切英文时就露馅了。
 * 现在类型文件只管数据形状，显示相关的东西全在这里，而且只映射到
 * **i18n 的 key**，不映射到具体文字。
 */

import type { BadgeTone } from "../components/ui";
import type { I18nKey, Translator } from "../i18n";
import type {
  AttentionLevel,
  InterruptReason,
  LogLevel,
  ResumeOutcome,
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

/**
 * 投递核验的四个结论 → 徽标语气
 *
 * 语气刻意不按「成功/失败」二分：
 * - `silent` 给琥珀色而不是红色——字确实发出去了，坏的是落点，用户要去查的是
 *   焦点和输入法，不是权限；
 * - `unverifiable` 给中性色，因为它压根不是问题，只是这类 agent 不落盘。
 *   给它红色会让人去修一个不存在的故障。
 */
export const OUTCOME_TONE: Record<ResumeOutcome, BadgeTone> = {
  landed: "green",
  silent: "amber",
  failed: "red",
  unverifiable: "neutral",
};

/** 记录行左侧那个小圆标里的字符 */
export const OUTCOME_GLYPH: Record<ResumeOutcome, string> = {
  landed: "✓",
  silent: "?",
  failed: "✗",
  unverifiable: "–",
};

export function outcomeKey(outcome: ResumeOutcome): I18nKey {
  return `outcome.${outcome}`;
}

/**
 * 悬浮解释的文案键
 *
 * 单独一个函数而不是在调用点拼 `` `${outcomeKey(o)}_hint` ``：模板字符串
 * 的类型是 `string`，不是 `I18nKey`，拼错了要等运行时才发现（界面上显示
 * 一个键名）。写成返回 `I18nKey` 的函数，漏了哪条 `tsc` 立刻报。
 */
export function outcomeHintKey(outcome: ResumeOutcome): I18nKey {
  return `outcome.${outcome}_hint`;
}

const KNOWN_OUTCOME = new Set<string>(Object.keys(OUTCOME_TONE));

/**
 * 库里那一列 → 四个核验态之一，认不出就是 `null`
 *
 * v1.6 之前的行这一列是空串：那时候只存了 `success` 这一个布尔，没人问过
 * 「字真的进去了吗」。**不要拿 `success` 把空串补成 `landed`**——那是替
 * 历史数据编造一个当时并不存在的结论。界面遇到 `null` 就退回朴素的成/败，
 * 少说一句总好过说一句假的。
 */
export function asOutcome(raw: string): ResumeOutcome | null {
  return KNOWN_OUTCOME.has(raw) ? (raw as ResumeOutcome) : null;
}

/**
 * 库里的 `stuck_secs` → 能显示的秒数，`-1` 收成 `null`
 *
 * 跟 [`asOutcome`] 同一个道理：负数是「不知道」的哨兵值。不过滤的话，
 * 一条 v1.7 之前的记录会显示成「卡了 -1 秒」。
 */
export function asStuckSecs(raw: number): number | null {
  return Number.isFinite(raw) && raw >= 0 ? raw : null;
}

/**
 * 中断原因的全集，`none` 除外
 *
 * 写成 `Record<…, true>` 而不是一个手抄的字符串数组：数组抄漏一个，类型依然
 * 通得过，只有跑到那条数据时界面才少说一句话。这个形状下少一个键 `tsc` 就报。
 */
const KNOWN_REASON: Record<Exclude<InterruptReason, "none">, true> = {
  process_crashed: true,
  rate_limited: true,
  upstream_rejected: true,
  awaiting_input: true,
  runtime_error: true,
  stalled: true,
  unknown: true,
};

/**
 * 库里那一列 → 中断原因，认不出就是 `null`
 *
 * `detection_records.reason` 从 v1.6 开始写，更早的行是空串。跟
 * [`asOutcome`] 一样，认不出来就少说一句，不要替历史数据编一个原因——
 * 尤其别把空串当成 `unknown`：那是一个真实存在的判定结果
 * （「确实中断了，但说不出为什么」），跟「那时候压根没记」不是一回事。
 */
export function asReason(raw: string): Exclude<InterruptReason, "none"> | null {
  return raw in KNOWN_REASON
    ? (raw as Exclude<InterruptReason, "none">)
    : null;
}

/**
 * 秒 → 「多久」的文案键与变量
 *
 * 返回 key 而不是拼好的字符串：`"3 分 20 秒"` 这种写法在英文里是
 * `"3m 20s"`，语序和单位都不一样，拼在这里就等于把翻译写死在库函数里。
 *
 * 三档粒度的分界不是整数关口：91 秒说「1.5 分钟」比说「91 秒」难比较，
 * 而 89 秒说「1 分钟」会把 29 秒的差别抹掉。所以 90 秒以内报秒，
 * 90 分钟以内报分，再往上报小时——每一档里，读数的有效位都还剩两位。
 */
export function durationParts(secs: number): { key: I18nKey; vars: { n: string } } {
  const n = Math.max(0, Math.round(secs));
  if (n < 90) return { key: "dur.secs", vars: { n: String(n) } };
  if (n < 90 * 60) return { key: "dur.mins", vars: { n: String(Math.round(n / 60)) } };
  return { key: "dur.hours", vars: { n: (n / 3600).toFixed(1) } };
}

/**
 * 秒 → 译好的时长短语
 *
 * `durationParts` 返回 key + 变量，调用点还得自己 `t(key, vars)` 拆一次。
 * 那行拆包以前是个内联箭头函数，被抄到了三个地方
 * （`records.stuck` / 趋势格 / 会话档案）。收成一个函数，
 * 顺带让「时长怎么念」只有一个入口。
 */
export function durationText(secs: number, t: Translator["t"]): string {
  const { key, vars } = durationParts(secs);
  return t(key, vars);
}

/** 趋势里一个指标的单位，决定差值怎么念 */
export type MetricUnit = "count" | "percent" | "duration";

/**
 * 一格趋势该显示什么
 *
 * - `compared`：两边都有数，可以报涨跌
 * - `current_only`：本期有数、上期没有（应用那时候还没跑），只报本期
 * - `unknown`：本期这个指标压根算不出来（比如没有可读会话记录，报不了卡住时长）
 */
export type TileMode = "compared" | "current_only" | "unknown";

export interface TileView {
  mode: TileMode;
  /** 只在 `compared` 时有值 */
  delta: number | null;
}

/**
 * 决定一格怎么显示
 *
 * 抽成纯函数是为了能直接测：这里有三个分支，而写反其中任何一个都会让界面
 * 说一句假话——最要紧的是「上期没有」不能被算成 `current - 0`，那会把
 * 全新安装的第一天显示成「中断次数 +4」，可上期压根不存在。
 */
export function tileView(current: number | null, previous: number | null): TileView {
  if (current === null) return { mode: "unknown", delta: null };
  if (previous === null) return { mode: "current_only", delta: null };
  return { mode: "compared", delta: current - previous };
}

/**
 * 「涨了是好事还是坏事」
 *
 * 这张表非要单独存在，是因为箭头方向和颜色在这里是**两件事**：中断次数涨了，
 * 箭头朝上、颜色得是红的。把两者绑在一起（涨=绿、跌=红）就会出现
 * 「中断次数翻倍，界面一片绿」——用户会以为这是好消息。
 *
 * `resumes` 刻意是 `neutral`：续跑次数变多既可能是「会话老卡」（坏），
 * 也可能是「以前漏了现在管上了」（好），光看这个数分不出来，
 * 那就别用颜色替用户下结论。
 */
export type MetricPolarity = "up_is_good" | "up_is_bad" | "neutral";

/** 差值 → 语气；`neutral` 指标一律不上色 */
export function deltaTone(delta: number, polarity: MetricPolarity): BadgeTone {
  if (delta === 0 || polarity === "neutral") return "neutral";
  const good = polarity === "up_is_good" ? delta > 0 : delta < 0;
  return good ? "green" : "red";
}
