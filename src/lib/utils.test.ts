import { describe, expect, it } from "vitest";
import {
  baseName,
  cn,
  dayOf,
  formatShortTime,
  formatTokens,
  formatUsd,
  secondsBetween,
  shortDate,
} from "./utils";

describe("formatTokens", () => {
  it("小于 1000 原样输出", () => {
    expect(formatTokens(0)).toBe("0");
    expect(formatTokens(999)).toBe("999");
  });

  it("千级显示 K", () => {
    expect(formatTokens(1_000)).toBe("1.0K");
    expect(formatTokens(12_345)).toBe("12.3K");
  });

  it("百万级显示 M", () => {
    expect(formatTokens(1_000_000)).toBe("1.00M");
    expect(formatTokens(2_567_890)).toBe("2.57M");
  });
});

describe("formatUsd", () => {
  it("零值", () => {
    expect(formatUsd(0)).toBe("0");
  });

  it("极小额保留 4 位", () => {
    expect(formatUsd(0.0012)).toBe("0.0012");
    expect(formatUsd(0.009)).toBe("0.0090");
  });

  it("正常金额保留 2 位", () => {
    expect(formatUsd(1.5)).toBe("1.50");
    expect(formatUsd(0.01)).toBe("0.01");
    expect(formatUsd(123.456)).toBe("123.46");
  });
});

describe("formatShortTime", () => {
  it("ISO 格式", () => {
    const result = formatShortTime("2026-07-30T14:03:22");
    expect(result).toBe("7/30 14:03");
  });

  it("空格分隔格式", () => {
    const result = formatShortTime("2026-07-30 14:03:22");
    expect(result).toBe("7/30 14:03");
  });

  it("无效日期原样返回", () => {
    expect(formatShortTime("not-a-date")).toBe("not-a-date");
  });
});

describe("baseName", () => {
  it("Unix 路径", () => {
    expect(baseName("/Users/sky/code/git/agent-pulse")).toBe("agent-pulse");
  });

  it("Windows 路径", () => {
    expect(baseName("C:\\code\\agent-pulse")).toBe("agent-pulse");
  });

  it("尾部斜杠", () => {
    expect(baseName("/home/user/project/")).toBe("project");
  });

  it("空字符串", () => {
    expect(baseName("")).toBe("");
  });
});

describe("dayOf", () => {
  it("切出日期部分", () => {
    expect(dayOf("2026-07-30 14:03:22")).toBe("2026-07-30");
    expect(dayOf("2026-07-30T14:03:22")).toBe("2026-07-30");
  });

  it("空串照旧是空串", () => {
    expect(dayOf("")).toBe("");
  });
});

describe("shortDate", () => {
  it("去掉年份", () => {
    expect(shortDate("2026-07-30")).toBe("07-30");
  });

  it("短得不像日期就原样返回", () => {
    expect(shortDate("07-30")).toBe("07-30");
  });
});

describe("secondsBetween", () => {
  it("算出两个时间戳之间的秒数", () => {
    expect(secondsBetween("2026-07-30 14:00:00", "2026-07-30 14:01:30")).toBe(90);
  });

  it("跨天也对", () => {
    expect(secondsBetween("2026-07-30 23:59:00", "2026-07-31 00:01:00")).toBe(120);
  });

  it("倒过来的区间收成 0，不给负数", () => {
    expect(secondsBetween("2026-07-30 14:01:00", "2026-07-30 14:00:00")).toBe(0);
  });

  it("解析不出来是「不知道」，不是 0", () => {
    // 这两件事在界面上是两句不同的话：算不出来就什么都不说，
    // 而 0 会显示成「持续 0 秒」，看着像故障
    expect(secondsBetween("", "2026-07-30 14:00:00")).toBeNull();
    expect(secondsBetween("2026-07-30 14:00:00", "")).toBeNull();
    expect(secondsBetween("not-a-date", "2026-07-30 14:00:00")).toBeNull();
  });

  it("同一时刻是 0 秒，不是 null", () => {
    expect(secondsBetween("2026-07-30 14:00:00", "2026-07-30 14:00:00")).toBe(0);
  });
});

describe("cn", () => {
  it("合并类名", () => {
    expect(cn("px-3", "py-2")).toBe("px-3 py-2");
  });

  it("后写覆盖同族", () => {
    expect(cn("px-3", "px-6")).toBe("px-6");
  });

  it("条件类名", () => {
    expect(cn("base", false && "hidden", "visible")).toBe("base visible");
  });
});
