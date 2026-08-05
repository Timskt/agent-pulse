import { describe, expect, it } from "vitest";
import { ALL_KEYS } from "../i18n";
import { asStuckSecs, deltaTone, durationParts, tileView } from "./display";

/**
 * 「不知道」和「零」必须分得开
 *
 * 库里用 `-1` 当哨兵值。漏了这层过滤，一条 v1.7 之前的记录会在界面上
 * 显示成「卡了 -1 秒」，而把它当成 0 更糟：那会让人以为守护是瞬间反应的。
 */
describe("asStuckSecs — 哨兵值收敛", () => {
  it("负数一律是「不知道」", () => {
    expect(asStuckSecs(-1)).toBeNull();
    expect(asStuckSecs(-999)).toBeNull();
  });

  it("0 秒是一个真实的答案，不能跟「不知道」混在一起", () => {
    expect(asStuckSecs(0)).toBe(0);
  });

  it("正常值原样通过", () => {
    expect(asStuckSecs(90)).toBe(90);
  });

  it("NaN / Infinity 也算不知道——JSON 里出现过就说明上游已经错了", () => {
    expect(asStuckSecs(Number.NaN)).toBeNull();
    expect(asStuckSecs(Number.POSITIVE_INFINITY)).toBeNull();
  });
});

describe("durationParts — 秒 / 分 / 小时三档", () => {
  it("90 秒以内报秒", () => {
    expect(durationParts(0)).toEqual({ key: "dur.secs", vars: { n: "0" } });
    expect(durationParts(89)).toEqual({ key: "dur.secs", vars: { n: "89" } });
  });

  it("90 秒到 90 分钟报分钟", () => {
    expect(durationParts(90)).toEqual({ key: "dur.mins", vars: { n: "2" } });
    expect(durationParts(600)).toEqual({ key: "dur.mins", vars: { n: "10" } });
  });

  it("再往上报小时，带一位小数", () => {
    expect(durationParts(90 * 60)).toEqual({ key: "dur.hours", vars: { n: "1.5" } });
    expect(durationParts(7200)).toEqual({ key: "dur.hours", vars: { n: "2.0" } });
  });

  it("三个档的文案都在 i18n 表里", () => {
    for (const secs of [10, 600, 7200]) {
      expect(ALL_KEYS).toContain(durationParts(secs).key);
    }
  });

  it("负数不会漏出来——调用方该先走 asStuckSecs，但这里也不产出负数文案", () => {
    expect(durationParts(-5)).toEqual({ key: "dur.secs", vars: { n: "0" } });
  });
});

/**
 * 涨跌的颜色是「好不好」，不是「涨没涨」
 *
 * 这是最容易写反的一处：把方向和颜色绑在一起（涨=绿、跌=红），
 * 中断次数翻倍时界面会一片绿，用户会当成好消息。
 */
/**
 * 三个分支里最要紧的一条：上期没有 ≠ 上期是 0
 *
 * 写成 `current - (previous ?? 0)` 的话，全新安装的第一天会显示
 * 「中断次数 +4」——可上期压根不存在，那个 +4 是拿一个不存在的基准算出来的。
 */
describe("tileView — 有没有可比的上期", () => {
  it("两边都有数才报涨跌", () => {
    expect(tileView(10, 6)).toEqual({ mode: "compared", delta: 4 });
    expect(tileView(6, 10)).toEqual({ mode: "compared", delta: -4 });
  });

  it("上期是 0 仍然可比——0 是一个真实的基准", () => {
    expect(tileView(3, 0)).toEqual({ mode: "compared", delta: 3 });
  });

  it("上期没有就只报本期，不拿 0 当基准", () => {
    const view = tileView(4, null);
    expect(view.mode).toBe("current_only");
    expect(view.delta).toBeNull();
  });

  it("本期是 0、上期没有，也不该冒出一个 0 的涨跌", () => {
    expect(tileView(0, null)).toEqual({ mode: "current_only", delta: null });
  });

  it("本期算不出来就是 unknown，跟「没有上期」是两回事", () => {
    expect(tileView(null, 5).mode).toBe("unknown");
    expect(tileView(null, null).mode).toBe("unknown");
  });

  it("持平时 delta 是 0 而不是 null——那是「比过了，一样」", () => {
    expect(tileView(7, 7)).toEqual({ mode: "compared", delta: 0 });
  });
});

describe("deltaTone — 颜色跟着语义走，不跟着方向走", () => {
  it("中断次数涨了是坏事，得是红的", () => {
    expect(deltaTone(3, "up_is_bad")).toBe("red");
    expect(deltaTone(-3, "up_is_bad")).toBe("green");
  });

  it("成功率涨了是好事", () => {
    expect(deltaTone(5, "up_is_good")).toBe("green");
    expect(deltaTone(-5, "up_is_good")).toBe("red");
  });

  it("持平一律中性——0 没有方向", () => {
    for (const polarity of ["up_is_good", "up_is_bad", "neutral"] as const) {
      expect(deltaTone(0, polarity)).toBe("neutral");
    }
  });

  it("说不清好坏的指标不上色", () => {
    // 续跑次数变多既可能是会话老卡（坏），也可能是以前漏了现在管上了（好）。
    // 分不出来的时候，别用颜色替用户下结论。
    expect(deltaTone(10, "neutral")).toBe("neutral");
    expect(deltaTone(-10, "neutral")).toBe("neutral");
  });
});
