import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ALL_KEYS } from "../i18n";
import type { SessionHistoryEntry } from "../types";
import { dayLabelKey, groupByDay, todayKey } from "./history";

/** 只填这几个字段够用：分组只看 `last_seen` */
function entry(patch: Partial<SessionHistoryEntry>): SessionHistoryEntry {
  return {
    session_key: "k",
    session_id: "s",
    agent_name: "Claude Code",
    working_dir: "/tmp/proj",
    session_file: "",
    tty: "",
    terminal_app: "",
    first_seen: "2026-07-30 09:00:00",
    last_seen: "2026-07-30 09:00:00",
    last_status: "active",
    ended_at: "",
    resume_count: 0,
    total_tokens: 0,
    cost_usd: 0,
    ...patch,
  };
}

describe("groupByDay", () => {
  it("同一天的会话归到一组", () => {
    const groups = groupByDay([
      entry({ session_key: "a", last_seen: "2026-07-30 09:00:00" }),
      entry({ session_key: "b", last_seen: "2026-07-30 22:41:07" }),
    ]);
    expect(groups).toHaveLength(1);
    expect(groups[0][0]).toBe("2026-07-30");
    expect(groups[0][1].map((e) => e.session_key)).toEqual(["a", "b"]);
  });

  it("不同的天分开，且保持后端给的顺序", () => {
    const groups = groupByDay([
      entry({ session_key: "a", last_seen: "2026-07-30 09:00:00" }),
      entry({ session_key: "b", last_seen: "2026-07-28 09:00:00" }),
      entry({ session_key: "c", last_seen: "2026-07-29 09:00:00" }),
    ]);
    // 后端排的序就是最终顺序，这里不许重排——列表的排序规则是
    // 「活着的在前」，按日期重排会把那个规则悄悄推翻
    expect(groups.map(([day]) => day)).toEqual([
      "2026-07-30",
      "2026-07-28",
      "2026-07-29",
    ]);
  });

  it("空列表就是空分组", () => {
    expect(groupByDay([])).toEqual([]);
  });
});

describe("todayKey", () => {
  it("按本地时区取日期，不是 UTC", () => {
    // 东八区的 2026-07-30 23:30 在 UTC 上已经是 07-30 15:30，
    // 但如果实现用了 toISOString()，跑在 UTC+9 以东的机器上就会报出 07-31。
    // 构造一个本地时间来断言：不管测试机在哪个时区，本地日历上的那天必须一致
    const d = new Date(2026, 6, 30, 23, 30, 0);
    expect(todayKey(d)).toBe("2026-07-30");
  });

  it("月、日补零", () => {
    expect(todayKey(new Date(2026, 0, 5))).toBe("2026-01-05");
  });
});

describe("dayLabelKey", () => {
  const now = new Date(2026, 6, 30, 14, 0, 0);

  it("当天说「今天」", () => {
    expect(dayLabelKey("2026-07-30", now)).toEqual({ key: "history.today" });
  });

  it("前一天说「昨天」", () => {
    expect(dayLabelKey("2026-07-29", now)).toEqual({ key: "history.yesterday" });
  });

  it("更早的日子原样显示", () => {
    expect(dayLabelKey("2026-07-28", now)).toEqual({ literal: "2026-07-28" });
  });

  it("跨月的「昨天」也算得对", () => {
    const firstOfAugust = new Date(2026, 7, 1, 0, 30, 0);
    expect(dayLabelKey("2026-07-31", firstOfAugust)).toEqual({
      key: "history.yesterday",
    });
  });

  it("跨年的「昨天」也算得对", () => {
    const newYear = new Date(2027, 0, 1, 1, 0, 0);
    expect(dayLabelKey("2026-12-31", newYear)).toEqual({
      key: "history.yesterday",
    });
  });

  it("返回的 key 都在词表里", () => {
    // 拼错一个 key，界面上会显示键名本身；这条测试比肉眼可靠
    for (const day of ["2026-07-30", "2026-07-29"]) {
      const label = dayLabelKey(day, now);
      expect("key" in label && ALL_KEYS).toContain(
        "key" in label ? label.key : "",
      );
    }
  });
});

/**
 * 夏令时：换季那两天不是 24 小时
 *
 * 这一组必须自己钉住时区。上面那些用例在「减 24 小时」的错实现下全是绿的——
 * 它们落的日子都不换季，测试机在上海更是一年都撞不上。所以这里临时把 `TZ`
 * 改成一个会换季的区，再取本地时间构造 `now`。
 *
 * 改 `TZ` 走 `vi.stubEnv`，不直接写 `process.env`——前端这份 tsconfig 没装
 * Node 的类型，`process` 在这里不存在。底下同样是给 `process.env.TZ` 赋值，
 * Node 18 起这个赋值会让时区缓存失效，`new Date(y, m, d)` 立刻按新区解释。
 * 下面第一条用例就是在验这件事，免得时区没真的切过去、后两条假绿。
 */
describe("dayLabelKey 跨夏令时", () => {
  beforeEach(() => {
    vi.stubEnv("TZ", "America/Los_Angeles");
  });
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it("TZ 真的切过去了（否则下面两条不算数）", () => {
    // 洛杉矶 2026-03-09 是 PDT，UTC-7
    expect(new Date(2026, 2, 9, 0, 30).getTimezoneOffset()).toBe(420);
  });

  it("春天少一小时的那天，昨天还是「昨天」", () => {
    // 03-08 是向前跳的那天，只有 23 小时。从 03-09 00:30 减 86400000 毫秒
    // 会落到 03-07，把 03-08 整天跳过去，昨天那组就退回裸日期了
    const afterSpringForward = new Date(2026, 2, 9, 0, 30);
    expect(dayLabelKey("2026-03-08", afterSpringForward)).toEqual({
      key: "history.yesterday",
    });
  });

  it("秋天多一小时的那天，昨天还是「昨天」", () => {
    // 11-01 是向后跳的那天，有 25 小时。从 11-01 23:30 减 86400000 毫秒
    // 还在 11-01 当天，算出来的「昨天」跟今天重合，10-31 那组认不出来
    const afterFallBack = new Date(2026, 10, 1, 23, 30);
    expect(dayLabelKey("2026-10-31", afterFallBack)).toEqual({
      key: "history.yesterday",
    });
    // 顺带确认今天没被带歪
    expect(dayLabelKey("2026-11-01", afterFallBack)).toEqual({
      key: "history.today",
    });
  });
});
