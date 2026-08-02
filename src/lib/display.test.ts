import { describe, expect, it } from "vitest";
// `?raw` 原样读 Rust 源码：这几条断言要跨语言比对，而枚举在两边都是编译期
// 的东西，谁都看不见谁。详见下面「跨语言」那一组的说明。
import detectorRs from "../../src-tauri/src/detector/mod.rs?raw";
import { ALL_KEYS } from "../i18n";
import {
  ATTENTION_ICON,
  ATTENTION_TONE,
  LOG_TONE,
  reasonKey,
  STATUS_DOT,
  STATUS_TONE,
  TACTIC_NOTE,
  attentionKey,
  statusKey,
} from "./display";

describe("display — 状态映射", () => {
  it("STATUS_TONE 覆盖全部五种会话状态", () => {
    const keys = Object.keys(STATUS_TONE);
    expect(keys).toHaveLength(5);
    expect(keys).toContain("active");
    expect(keys).toContain("interrupted");
    expect(keys).toContain("exited");
  });

  it("STATUS_DOT 的每个值都是合法的 Tailwind bg 类", () => {
    for (const cls of Object.values(STATUS_DOT)) {
      expect(cls).toMatch(/^bg-/);
    }
  });

  it("statusKey 映射到 i18n key", () => {
    expect(statusKey("active")).toBe("status.active");
    expect(statusKey("interrupted")).toBe("status.interrupted");
  });
});

describe("display — 注意力级别", () => {
  it("ATTENTION_TONE 不含 none", () => {
    expect(Object.keys(ATTENTION_TONE)).not.toContain("none");
    expect(Object.keys(ATTENTION_TONE)).toHaveLength(4);
  });

  it("ATTENTION_ICON 四色语义", () => {
    expect(ATTENTION_ICON.needs_input).toBe("🔴");
    expect(ATTENTION_ICON.completed).toBe("🟢");
    expect(ATTENTION_ICON.rate_limited).toBe("🟡");
    expect(ATTENTION_ICON.error).toBe("⚫");
  });

  it("attentionKey 映射到 i18n key", () => {
    expect(attentionKey("needs_input")).toBe("attention.needs_input");
    expect(attentionKey("rate_limited")).toBe("attention.rate_limited");
  });
});

describe("display — 日志级别", () => {
  it("LOG_TONE 覆盖四种级别", () => {
    expect(Object.keys(LOG_TONE)).toHaveLength(4);
    expect(LOG_TONE.error).toContain("text-red");
    expect(LOG_TONE.success).toContain("text-emerald");
  });
});

/**
 * 跨语言：Rust 加了一个中断原因，界面上有没有话可说
 *
 * 这是两边编译器都看不见的那道缝。`InterruptReason` 是 Rust 的枚举，
 * 前端的 `InterruptReason` 是一串手写的字面量联合——加一个变体，
 * `cargo build` 过、`tsc` 也过，只有跑起来才发现界面上那一行是空的，
 * 或者更糟：显示的是 `reason.xxx` 这种键名。
 *
 * 所以把「两边对得上」变成一道门：直接读 Rust 的 `key()` 匹配臂当事实来源，
 * 因为那才是真正发到前端的字符串（不是变体名，也不是我们猜的 snake_case）。
 */
describe("display — 跨语言：中断原因", () => {
  /** Rust 侧 `InterruptReason::key()` 里写的全部线上字符串 */
  const rustReasons = (() => {
    // 从 `impl InterruptReason` 起步，不是直接找 `fn key`——`AttentionLevel`
    // 也有一个一模一样签名的 `key()`，而且排在前面，抓错了就会把注意力级别
    // 当成中断原因来比，两边都对不上却每条都绿。
    const body = detectorRs.match(
      /impl InterruptReason \{[\s\S]*?fn key\(&self\)[\s\S]*?\n {4}\}/,
    );
    expect(body, "detector/mod.rs 里没找到 InterruptReason::key()").not.toBeNull();
    return [...(body?.[0] ?? "").matchAll(/InterruptReason::\w+ => "(\w+)"/g)].map(
      (m) => m[1],
    );
  })();

  it("确实读到了 Rust 那边的原因表", () => {
    // 正则失配的话上面会返回空数组，那样后面每条断言都会「通过」
    expect(rustReasons.length).toBeGreaterThan(1);
    expect(rustReasons).toContain("none");
    expect(rustReasons).toContain("process_crashed");
  });

  it("除 none 之外，每个原因在界面上都有一句话", () => {
    for (const reason of rustReasons.filter((r) => r !== "none")) {
      // 走 reasonKey 而不是自己拼字符串：拼错的话这条测试会假绿。
      // 这里的断言从 Rust 读来的是 string，得转成 reasonKey 的入参类型；
      // 用 Parameters<> 取而不是抄一份联合，抄的那份会跟着漏。
      expect(
        ALL_KEYS,
        `reason.${reason} 没有中文/英文措辞，界面会显示键名`,
      ).toContain(reasonKey(reason as Parameters<typeof reasonKey>[0]));
    }
  });

  it("none 不进 i18n 表——它是「没有中断」，没什么可显示的", () => {
    expect(ALL_KEYS).not.toContain("reason.none");
  });
});

/**
 * 跨语言：手段只有一个出处
 *
 * `TACTIC_NOTE` 曾经是照着 Rust `tactic()` 抄的一份原因名单，也就是同一条
 * 策略存了两份。现在手段由后端算好发上来，这里只剩「每个非 nudge 的手段
 * 都得有一句解释」这一条约束——但这一条仍然要看着，因为 Rust 加一个
 * 手段变体时，TS 的 `Record` 只在**前端也改了类型**之后才会报错。
 */
describe("display — 跨语言：续跑手段", () => {
  const rustTactics = (() => {
    const body = detectorRs.match(/pub enum ResumeTactic \{[\s\S]*?\n\}/);
    expect(body, "detector/mod.rs 里没找到 ResumeTactic").not.toBeNull();
    return [...(body?.[0] ?? "").matchAll(/^ {4}([A-Z]\w*),$/gm)].map((m) =>
      // serde(rename_all = "snake_case")：HandOff → hand_off
      m[1].replace(/([a-z0-9])([A-Z])/g, "$1_$2").toLowerCase(),
    );
  })();

  it("确实读到了 Rust 那边的手段表", () => {
    expect(rustTactics).toEqual(expect.arrayContaining(["nudge", "wait"]));
  });

  it("nudge 之外的每个手段都有一句解释，且措辞里带上原因", () => {
    const explained = rustTactics.filter((tac) => tac !== "nudge");
    expect(Object.keys(TACTIC_NOTE).sort()).toEqual(explained.sort());
    for (const key of Object.values(TACTIC_NOTE)) {
      expect(ALL_KEYS).toContain(key);
    }
  });

  it("nudge 没有解释句——那是默认动作，没什么要交代的", () => {
    expect(Object.keys(TACTIC_NOTE)).not.toContain("nudge");
  });
});
