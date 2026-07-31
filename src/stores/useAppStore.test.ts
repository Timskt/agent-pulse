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
      resumeHistory: [],
      totals: null,
      costDaily: [],
      costProjects: [],
      rateForecast: null,
      sessionHistory: [],
      historyQuery: "",
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

  it("locateSession 返回结构化 LocateReport", async () => {
    const report = {
      level: "exact",
      terminal: "iTerm2",
      tty: "/dev/ttys003",
      project: "agent-pulse",
      message: "精确匹配 iTerm2 · tty /dev/ttys003",
    };
    mockedInvoke.mockResolvedValueOnce(report);

    const result = await useAppStore.getState().locateSession("s1");
    expect(result.level).toBe("exact");
    expect(result.terminal).toBe("iTerm2");
    expect(mockedInvoke).toHaveBeenCalledWith("locate_session", { sessionId: "s1" });
  });
});
