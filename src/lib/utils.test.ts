import { describe, expect, it } from "vitest";
import { baseName, cn, formatShortTime, formatTokens, formatUsd } from "./utils";

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
