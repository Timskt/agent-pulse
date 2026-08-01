import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { playChime } from "../lib/chime";
import type {
  AiVerdict,
  AppConfig,
  AttentionAlert,
  DailyCost,
  DailyStats,
  EngineEvent,
  MonitorState,
  ProjectCost,
  RateLimitForecast,
  ResumeProbe,
  ResumeRecord,
  SessionHistoryEntry,
} from "../types";

export type TabId = "dashboard" | "stats" | "cost" | "history" | "config";

/** 命令结果：文案由后端按当前语言给，前端只管显示 */
export interface CommandResult {
  ok: boolean;
  message: string;
}

interface AppStore {
  monitorState: MonitorState;
  config: AppConfig | null;
  /** 通过事件推来的日志，和 `monitorState.events` 合并后展示 */
  localEvents: EngineEvent[];
  activeTab: TabId;
  loading: boolean;
  /** 最近一次提醒涉及的会话，用来在列表里高亮 */
  focusedSessionId: string | null;

  dailyStats: DailyStats[];
  resumeHistory: ResumeRecord[];
  /** (检测数, 续跑数, 成功数) */
  totals: [number, number, number] | null;

  costDaily: DailyCost[];
  costProjects: ProjectCost[];
  rateForecast: RateLimitForecast | null;

  sessionHistory: SessionHistoryEntry[];
  historyQuery: string;

  setActiveTab: (tab: TabId) => void;
  setFocusedSession: (sessionId: string | null) => void;
  fetchState: () => Promise<void>;
  fetchConfig: () => Promise<void>;
  fetchStats: () => Promise<void>;
  fetchCost: () => Promise<void>;
  fetchSessionHistory: (query?: string) => Promise<void>;
  startMonitoring: () => Promise<void>;
  stopMonitoring: () => Promise<void>;
  scanNow: () => Promise<void>;
  updateConfig: (config: AppConfig) => Promise<CommandResult>;
  manualResume: (
    sessionId: string,
    useGoalPrompt?: boolean,
  ) => Promise<CommandResult>;
  focusTerminal: (sessionId: string) => Promise<CommandResult>;
  testNotify: () => Promise<CommandResult>;
  testWebhook: () => Promise<CommandResult>;
  aiAnalyze: (sessionId: string) => Promise<AiVerdict>;
  /** 续跑演练：走完定位流程但不投递，用来回答「字会敲到哪儿」 */
  probeResume: (sessionId: string) => Promise<ResumeProbe>;
  openAccessibilitySettings: () => Promise<CommandResult>;
  /** 本机在局域网里的 IPv4；拿不到返回 null，界面退回 127.0.0.1 */
  getLanIp: () => Promise<string | null>;
  /** 生成一个 32 位强令牌，只填进输入框，存不存还是用户说了算 */
  generateRemoteToken: () => Promise<string>;
  initEventListeners: () => Promise<() => void>;
}

const defaultMonitorState: MonitorState = {
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
};

/**
 * 后端返回的错误已经是当前语言的成品文案（`src-tauri/src/i18n`），
 * 所以这里原样往上传，不再拼「错误: 」之类的前缀——那正是中英混杂的来源。
 */
async function run(
  action: () => Promise<string | void>,
): Promise<CommandResult> {
  try {
    const message = await action();
    return { ok: true, message: message ?? "" };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

export const useAppStore = create<AppStore>((set, get) => ({
  monitorState: defaultMonitorState,
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

  setActiveTab: (tab) => set({ activeTab: tab }),
  setFocusedSession: (sessionId) => set({ focusedSessionId: sessionId }),

  fetchState: async () => {
    try {
      set({ monitorState: await invoke<MonitorState>("get_state") });
    } catch (e) {
      console.error("get_state", e);
    }
  },

  fetchConfig: async () => {
    try {
      set({ config: await invoke<AppConfig>("get_config") });
    } catch (e) {
      console.error("get_config", e);
    }
  },

  fetchStats: async () => {
    try {
      const [dailyStats, resumeHistory, totals] = await Promise.all([
        invoke<DailyStats[]>("get_stats", { days: 30 }),
        invoke<ResumeRecord[]>("get_resume_history", { limit: 50 }),
        invoke<[number, number, number]>("get_totals"),
      ]);
      set({ dailyStats, resumeHistory, totals });
    } catch (e) {
      console.error("get_stats", e);
    }
  },

  fetchCost: async () => {
    try {
      const [costDaily, costProjects, rateForecast] = await Promise.all([
        invoke<DailyCost[]>("get_cost_daily", { days: 14 }),
        invoke<ProjectCost[]>("get_cost_projects", { days: 30, limit: 8 }),
        invoke<RateLimitForecast>("get_rate_forecast"),
      ]);
      set({ costDaily, costProjects, rateForecast });
    } catch (e) {
      console.error("get_cost_daily", e);
    }
  },

  fetchSessionHistory: async (query) => {
    const next = query ?? get().historyQuery;
    try {
      const sessionHistory = await invoke<SessionHistoryEntry[]>(
        "get_session_history",
        {
          limit: 100,
          query: next,
        },
      );
      set({ sessionHistory, historyQuery: next });
    } catch (e) {
      console.error("get_session_history", e);
    }
  },

  startMonitoring: async () => {
    set({ loading: true });
    try {
      await invoke("start_monitoring");
      await get().fetchState();
    } catch (e) {
      console.error("start_monitoring", e);
    } finally {
      set({ loading: false });
    }
  },

  stopMonitoring: async () => {
    set({ loading: true });
    try {
      await invoke("stop_monitoring");
      await get().fetchState();
    } catch (e) {
      console.error("stop_monitoring", e);
    } finally {
      set({ loading: false });
    }
  },

  scanNow: async () => {
    set({ loading: true });
    try {
      set({ monitorState: await invoke<MonitorState>("scan_now") });
    } catch (e) {
      console.error("scan_now", e);
    } finally {
      set({ loading: false });
    }
  },

  updateConfig: async (config) => {
    const result = await run(() => invoke<void>("update_config", { config }));
    // 后端写盘失败时不能让界面装作已保存，否则下次重启配置又回去了。
    // 成功则回读一遍：看板令牌留空时是后端补的，只有回读才能显示出来。
    if (result.ok) {
      set({ config });
      await get().fetchConfig();
    }
    return result;
  },

  manualResume: async (sessionId, useGoalPrompt = false) => {
    const result = await run(() =>
      invoke<string>("manual_resume", { sessionId, useGoalPrompt }),
    );
    await get().fetchState();
    return result;
  },

  focusTerminal: async (sessionId) =>
    run(() => invoke<string>("focus_terminal", { sessionId })),

  testNotify: async () => run(() => invoke<string>("test_notify")),

  testWebhook: async () => run(() => invoke<string>("test_webhook")),

  aiAnalyze: async (sessionId) =>
    invoke<AiVerdict>("ai_analyze", { sessionId }),

  probeResume: async (sessionId) =>
    invoke<ResumeProbe>("probe_resume", { sessionId }),

  openAccessibilitySettings: async () =>
    run(() => invoke<string>("open_accessibility_settings")),

  // 拿不到局域网地址不是错误（可能就是没连网），所以吞掉异常返回 null，
  // 界面照旧显示 127.0.0.1，而不是弹一句用户帮不上忙的报错
  getLanIp: async () => {
    try {
      return await invoke<string | null>("get_lan_ip");
    } catch {
      return null;
    }
  },

  generateRemoteToken: async () => invoke<string>("generate_remote_token"),

  /**
   * 事件订阅 + 兜底轮询
   *
   * 原来是无条件 `setInterval(fetchState, 3000)`：没在守护时也每 3 秒
   * 敲一次后端，窗口收进托盘照样敲。现在两处收紧：
   * - 停止守护时降到 10 秒一次（只为了发现托盘里手动启动的情况）；
   * - 窗口不可见时直接跳过这一轮，等切回来立刻补一次。
   * 守护中的间隔取扫描周期的一半（夹在 2–8 秒），比原来的 3 秒更贴合后端节奏。
   */
  initEventListeners: async () => {
    const unlistenEvents = await listen<EngineEvent[]>(
      "engine-events",
      (event) => {
        set((state) => ({
          localEvents: [...state.localEvents, ...event.payload].slice(-500),
        }));
      },
    );

    const unlistenStopped = await listen("engine-stopped", () => {
      void get().fetchState();
    });

    const unlistenAlert = await listen<AttentionAlert>(
      "attention-alert",
      (event) => {
        const alert = event.payload;
        if (alert.sound) playChime(alert.volume, alert.level === "needs_input");
        if (alert.session_id) set({ focusedSessionId: alert.session_id });
        // 提醒意味着状态刚变，别等下一轮轮询
        void get().fetchState();
      },
    );

    let timer: number | undefined;
    let disposed = false;

    const delay = () => {
      const { monitorState, config } = get();
      if (!monitorState.running) return 10_000;
      const half = ((config?.poll_interval_secs ?? 10) * 1000) / 2;
      return Math.min(Math.max(half, 2_000), 8_000);
    };

    const tick = async () => {
      if (disposed) return;
      if (!document.hidden) await get().fetchState();
      if (!disposed) timer = window.setTimeout(tick, delay());
    };

    const onVisible = () => {
      if (!document.hidden) void get().fetchState();
    };
    document.addEventListener("visibilitychange", onVisible);
    timer = window.setTimeout(tick, delay());

    return () => {
      disposed = true;
      if (timer !== undefined) window.clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisible);
      unlistenEvents();
      unlistenStopped();
      unlistenAlert();
    };
  },
}));

/**
 * 选择器
 *
 * 组件以前一律 `const { a, b, c } = useAppStore()`：任何一个字段变了
 * 所有订阅者都要重渲染，日志一刷全屏跟着抖。选择器写在这里而不是
 * 组件内联，是为了保持引用稳定。
 */
export const selectSessions = (s: AppStore) => s.monitorState.sessions;
export const selectStatus = (s: AppStore) => s.monitorState.status;
export const selectRunning = (s: AppStore) => s.monitorState.running;
export const selectEngineEvents = (s: AppStore) => s.monitorState.events;
export const selectLocalEvents = (s: AppStore) => s.localEvents;
export const selectConfig = (s: AppStore) => s.config;
export const selectActiveTab = (s: AppStore) => s.activeTab;
export const selectLoading = (s: AppStore) => s.loading;
export const selectFocusedSessionId = (s: AppStore) => s.focusedSessionId;
export const selectDailyStats = (s: AppStore) => s.dailyStats;
export const selectResumeHistory = (s: AppStore) => s.resumeHistory;
export const selectTotals = (s: AppStore) => s.totals;
export const selectCostDaily = (s: AppStore) => s.costDaily;
export const selectCostProjects = (s: AppStore) => s.costProjects;
export const selectRateForecast = (s: AppStore) => s.rateForecast;
export const selectSessionHistory = (s: AppStore) => s.sessionHistory;
export const selectHistoryQuery = (s: AppStore) => s.historyQuery;
