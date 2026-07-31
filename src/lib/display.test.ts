import { describe, expect, it } from "vitest";
import {
  ATTENTION_ICON,
  ATTENTION_TONE,
  LOG_TONE,
  STATUS_DOT,
  STATUS_TONE,
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
