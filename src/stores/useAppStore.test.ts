import { beforeEach, describe, expect, it, vi } from "vitest";

/**
 * useAppStore 的状态归约测试
 *
 * 后端 IPC 全部 mock 掉——这里只验前端自己的状态逻辑：
 * 初始值、setter、run() 包装器的 ok/err 归约。
 */

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock("../lib/chime", () => ({
  playChime: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
import { useAppStore } from "./useAppStore";

const mockedInvoke = vi.mocked(invoke);

describe("useAppStore — 初始状态", () => {
  beforeEach(() => {
    // 重置到初始值
    useAppStore.setState({
      monitorState: {
        running: false,
        sessions: [],
        events: [],
        status: {
          running: false,
          sessions_total: 0,
          sessions_active: 0,
          sessions_interrupted: 0,
          pending_attention: 0,
          total_resumes: 0,
          total_detections: 0,
          resume_pending: 0,
          resume_verifying: 0,
          last_scan_at: null,
          uptime_secs: 0,
          cost_today: 0,
        },
      },
      config: null,
      localEvents: [],
      activeTab: "dashboard",
      loading: false,
      focusedSessionId: null,
      dailyStats: [],
      resumeRecords: [],
      resumeRecordsTotal: 0,
      totals: null,
      costDaily: [],
      costProjects: [],
      rateForecast: null,
      sessionHistory: [],
      sessionHistoryTotal: 0,
      historyFilter: { query: "", status: "all", offset: 0 },
      sessionHistorySummary: null,
      detailKey: null,
      sessionDetail: null,
    });
    vi.clearAllMocks();
  });

  it("默认 tab 是 dashboard", () => {
    expect(useAppStore.getState().activeTab).toBe("dashboard");
  });

  it("setActiveTab 切换 tab", () => {
    useAppStore.getState().setActiveTab("config");
    expect(useAppStore.getState().activeTab).toBe("config");
  });

  it("setFocusedSession 设置/清除高亮", () => {
    useAppStore.getState().setFocusedSession("abc-123");
    expect(useAppStore.getState().focusedSessionId).toBe("abc-123");

    useAppStore.getState().setFocusedSession(null);
    expect(useAppStore.getState().focusedSessionId).toBeNull();
  });
});

describe("useAppStore — 命令归约", () => {
  beforeEach(() => vi.clearAllMocks());

  it("manualResume 成功时 ok=true + 回读状态", async () => {
    mockedInvoke.mockResolvedValueOnce("已续跑");
    mockedInvoke.mockResolvedValueOnce({
      running: false,
      sessions: [],
      events: [],
      status: {
        running: false,
        sessions_total: 0,
        sessions_active: 0,
        sessions_interrupted: 0,
        pending_attention: 0,
        total_resumes: 0,
        total_detections: 0,
        last_scan_at: null,
        uptime_secs: 0,
        cost_today: 0,
      },
    });

    const result = await useAppStore.getState().manualResume("s1");
    expect(result.ok).toBe(true);
    expect(result.message).toBe("已续跑");
  });

  it("manualResume 失败时 ok=false + 原样传递错误文案", async () => {
    mockedInvoke.mockRejectedValueOnce("定位不到 Windsurf");
    // fetchState 也会调 invoke
    mockedInvoke.mockResolvedValueOnce(useAppStore.getState().monitorState);

    const result = await useAppStore.getState().manualResume("s1");
    expect(result.ok).toBe(false);
    expect(result.message).toBe("定位不到 Windsurf");
  });

  it("probeResume 原样透传后端的演练结论", async () => {
    // 演练结果整块交给面板渲染，store 不做任何加工——
    // 一旦这里开始「顺手补个默认值」，面板说的话就不再等于后端的判定
    const probe = {
      session_id: "s1",
      certainty: "exact",
      certainty_label: "精确：能定位到具体的终端会话",
      channel: "tmux",
      target: "%3",
      detail: "会敲进 tmux pane %3",
      would_deliver: true,
      terminal_app: "iTerm2",
      tty: "/dev/ttys003",
      project_name: "agent-pulse",
      allow_blind: false,
      needs_permission_fix: false,
      tools: [{ name: "tmux", available: true, purpose: "复用器投递" }],
    };
    mockedInvoke.mockResolvedValueOnce(probe);

    const result = await useAppStore.getState().probeResume("s1");
    expect(result).toEqual(probe);
    expect(mockedInvoke).toHaveBeenCalledWith("probe_resume", {
      sessionId: "s1",
    });
  });

  it("probeResume 定位不到时不吞掉「不会敲」这个结论", async () => {
    // 「定位不到」是演练最要紧的一种回答：吞掉它就等于告诉用户「按吧没事」
    mockedInvoke.mockResolvedValueOnce({
      session_id: "s1",
      certainty: "none",
      certainty_label: "定位不到",
      channel: "未知终端",
      target: null,
      detail: "认不出这个终端，按下续跑不会敲任何字",
      would_deliver: false,
      terminal_app: null,
      tty: null,
      project_name: "agent-pulse",
      allow_blind: false,
      needs_permission_fix: true,
      tools: [],
    });

    const result = await useAppStore.getState().probeResume("s1");
    expect(result.certainty).toBe("none");
    expect(result.would_deliver).toBe(false);
    expect(result.needs_permission_fix).toBe(true);
  });
});
