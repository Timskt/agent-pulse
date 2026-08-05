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
  HistoryStatusFilter,
  MonitorState,
  ProjectCost,
  RateLimitForecast,
  ResumeOutcome,
  ResumeProbe,
  ResumeRecord,
  ResumeRecordPage,
  SessionDetail,
  SessionHistoryEntry,
  SessionHistoryPage,
  SessionHistorySummary,
  StatsOverview,
  StatsTrend,
  TrendWindow,
} from "../types";

export type TabId = "dashboard" | "stats" | "cost" | "history" | "config";

/**
 * 推送来的事件在前端最多攒多少条
 *
 * 这里不是显示上限：`LogPanel` 把这批和 `get_state` 快照里的合并、去重之后
 * 自己裁到 200 行。这个数的唯一作用是**别让它无界增长**——挂机一整天下来
 * 推送来的事件成千上万，全留着就是白占内存，而屏幕上只看得到最近 200 行。
 *
 * 它和后端的 `EVENT_RING_CAP`（500）数值一样，但**不是**同一个约束，别为了
 * 「保持一致」把两者绑成一个数：后端那个决定快照里能回看多少，这个只是本地
 * 缓冲的天花板。后端调大调小都不用动这里。
 */
const LOCAL_EVENT_CAP = 500;

/**
 * 续跑记录中心的筛选条件
 *
 * `outcome` 和 `prompt_type` 用 `"all"` 而不是 `null` 表示不筛，因为这个值
 * 直接就是发给后端的那个字符串——多一层「null 就传 all」的翻译，翻错了
 * 只会静默地少给几条记录。
 */
export interface ResumeFilter {
  query: string;
  outcome: ResumeOutcome | "all";
  promptType: "goal" | "generic" | "all";
  offset: number;
}

/** 续跑记录每页条数；组件的翻页步长要跟这个对齐 */
export const RESUME_PAGE_SIZE = 20;

const defaultResumeFilter: ResumeFilter = {
  query: "",
  outcome: "all",
  promptType: "all",
  offset: 0,
};

/**
 * 历史页的筛选条件
 *
 * 跟 `ResumeFilter` 同构，连 `"all"` 而不是 `null` 的取舍也一样：这个值直接
 * 就是发给后端的那个字符串。
 */
export interface HistoryFilter {
  query: string;
  status: HistoryStatusFilter;
  offset: number;
}

export const HISTORY_PAGE_SIZE = 20;

const defaultHistoryFilter: HistoryFilter = {
  query: "",
  status: "all",
  offset: 0,
};

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
  /** 续跑记录中心当前那一页 */
  resumeRecords: ResumeRecord[];
  /** 满足当前筛选条件的总条数，用来算页数 */
  resumeRecordsTotal: number;
  statsOverview: StatsOverview | null;
  /** (检测数, 续跑数, 成功数) */
  totals: [number, number, number] | null;
  /** 本期 vs 上期；还没取到是 `null` */
  statsTrend: StatsTrend | null;
  /**
   * 趋势卡当前看的窗口
   *
   * 存在 store 里而不是组件里：统计页是 Radix `Tabs` 的一个面板，切走再切回来
   * 会重新挂载，局部 state 会把用户选的「近 7 天」悄悄弹回「今日」。
   */
  trendWindow: TrendWindow;

  costDaily: DailyCost[];
  costProjects: ProjectCost[];
  costModels: import("../types").ModelCost[];
  usageSummary: import("../types").UsageSnapshot | null;
  rateForecast: RateLimitForecast | null;

  sessionHistory: SessionHistoryEntry[];
  sessionHistoryTotal: number;
  /** 历史页的筛选条件；跟 `resumeFilter` 一个路子，翻页要沿用 */
  historyFilter: HistoryFilter;
  /**
   * 会话历史的汇总数字。**跟着搜索条件走，不跟着分页走**——
   * 顶部那条「共 N 个会话」问的是筛出来一共多少，不是这一页几行。
   */
  sessionHistorySummary: SessionHistorySummary | null;
  /**
   * 抽屉开在哪个会话上；`null` 表示抽屉关着。
   *
   * **抽屉的开合看这个，不看 `sessionDetail`。** 档案是异步取的，正在取的那
   * 一拍 `sessionDetail` 也是 `null`，拿它当开关会让抽屉刚点开就自己关掉。
   */
  detailKey: string | null;
  /** 抽屉里那个会话的档案；`null` 且 `detailKey` 非空表示还在取 */
  sessionDetail: SessionDetail | null;

  /** 续跑记录中心的筛选条件；翻页要沿用，所以存在 store 里而不是组件里 */
  resumeFilter: ResumeFilter;

  setActiveTab: (tab: TabId) => void;
  setFocusedSession: (sessionId: string | null) => void;
  fetchState: () => Promise<void>;
  fetchConfig: () => Promise<void>;
  fetchStats: () => Promise<void>;
  /** 换趋势窗口并重取；不传就用当前窗口 */
  fetchStatsTrend: (window?: TrendWindow) => Promise<void>;
  fetchCost: () => Promise<void>;
  /** 按筛选条件重取会话历史；只传变化的那部分，其余沿用当前条件 */
  fetchSessionHistory: (patch?: Partial<HistoryFilter>) => Promise<void>;
  /** 打开某个会话的档案抽屉 */
  openSessionDetail: (sessionKey: string) => Promise<void>;
  closeSessionDetail: () => void;
  /** 按筛选条件重取续跑记录；只传变化的那部分，其余沿用当前条件 */
  fetchResumeRecords: (patch?: Partial<ResumeFilter>) => Promise<void>;
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
  resumeRecords: [],
  resumeRecordsTotal: 0,
  resumeFilter: defaultResumeFilter,
  statsOverview: null,
  totals: null,
  statsTrend: null,
  trendWindow: 1,
  costDaily: [],
  costProjects: [],
  costModels: [],
  usageSummary: null,
  rateForecast: null,
  sessionHistory: [],
  sessionHistoryTotal: 0,
  historyFilter: defaultHistoryFilter,
  sessionHistorySummary: null,
  detailKey: null,
  sessionDetail: null,

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
      const [dailyStats, totals, statsOverview, statsTrend] = await Promise.all([
        invoke<DailyStats[]>("get_stats", { days: 30 }),
        invoke<[number, number, number]>("get_totals"),
        invoke<StatsOverview>("get_stats_overview"),
        invoke<StatsTrend>("get_stats_trend", { days: get().trendWindow }),
      ]);
      set({ dailyStats, totals, statsOverview, statsTrend });
    } catch (e) {
      console.error("get_stats", e);
    }
  },

  /**
   * 换窗口时**先落 `trendWindow`**，段控件立刻跟手
   *
   * 等请求回来再切的话，点下去要愣一下才动，用户会以为没点着又点一次。
   * 反过来，数据回来时要确认窗口还是当时那个——快速点两下，先发的那个请求
   * 可能后回来，把「近 7 天」的卡片填成今日的数。
   */
  fetchStatsTrend: async (window) => {
    const target = window ?? get().trendWindow;
    set({ trendWindow: target });
    try {
      const statsTrend = await invoke<StatsTrend>("get_stats_trend", { days: target });
      if (get().trendWindow === target) set({ statsTrend });
    } catch (e) {
      console.error("get_stats_trend", e);
    }
  },

  /**
   * 拉一页续跑记录
   *
   * 传进来的是**增量**：`{ outcome: "silent" }` 只改结果筛选，其他条件留着。
   * 改条件（而不是翻页）时 `offset` 自动归零——第 3 页筛出来只有 5 条的话，
   * 用户会看到一个空列表，然后以为「筛完什么都没有」。
   */
  fetchResumeRecords: async (patch) => {
    const prev = get().resumeFilter;
    const changesFilter =
      patch !== undefined &&
      (["query", "outcome", "promptType"] as const).some(
        (k) => patch[k] !== undefined && patch[k] !== prev[k],
      );
    const filter: ResumeFilter = {
      ...prev,
      ...patch,
      ...(changesFilter && patch?.offset === undefined ? { offset: 0 } : {}),
    };
    set({ resumeFilter: filter });
    try {
      const page = await invoke<ResumeRecordPage>("get_resume_page", {
        limit: RESUME_PAGE_SIZE,
        offset: filter.offset,
        query: filter.query,
        outcome: filter.outcome,
        promptType: filter.promptType,
      });
      // 请求是防抖后发出的，回来时筛选条件可能又变了；只有仍然对得上才落库，
      // 否则快速切筛选会出现「列表显示的是上一个条件的结果」
      if (get().resumeFilter === filter) {
        set({ resumeRecords: page.records, resumeRecordsTotal: page.total });
      }
    } catch (e) {
      console.error("get_resume_page", e);
    }
  },

  fetchCost: async () => {
    try {
      const [costDaily, costProjects, costModels, usageSummary, rateForecast] = await Promise.all([
        invoke<DailyCost[]>("get_cost_daily", { days: 14 }),
        invoke<ProjectCost[]>("get_cost_projects", { days: 30, limit: 8 }),
        invoke<import("../types").ModelCost[]>("get_cost_models", { days: 30, limit: 8 }),
        invoke<import("../types").UsageSnapshot>("get_usage_summary", { days: 30 }),
        invoke<RateLimitForecast>("get_rate_forecast"),
      ]);
      set({ costDaily, costProjects, costModels, usageSummary, rateForecast });
    } catch (e) {
      console.error("get_cost_daily", e);
    }
  },

  /**
   * 改条件（而不是翻页）时 `offset` 归零，理由同 `fetchResumeRecords`。
   *
   * 汇总跟列表一趟发出去，但只跟 `query` 有关——状态筛选不传给它，因为
   * 那条汇总本身就要同时说出「活着 N 个」和「一共 M 个」，跟着状态筛选
   * 走的话选中「已结束」后它会自称「活着 0 个」，把自己要回答的问题抹掉。
   */
  fetchSessionHistory: async (patch) => {
    const prev = get().historyFilter;
    const changesFilter =
      patch !== undefined &&
      (["query", "status"] as const).some(
        (k) => patch[k] !== undefined && patch[k] !== prev[k],
      );
    const filter: HistoryFilter = {
      ...prev,
      ...patch,
      ...(changesFilter && patch?.offset === undefined ? { offset: 0 } : {}),
    };
    set({ historyFilter: filter });
    try {
      const [page, summary] = await Promise.all([
        invoke<SessionHistoryPage>("get_session_history_page", {
          limit: HISTORY_PAGE_SIZE,
          offset: filter.offset,
          query: filter.query,
          status: filter.status,
        }),
        invoke<SessionHistorySummary>("get_session_history_summary", {
          query: filter.query,
        }),
      ]);
      // 防抖发出的请求回来时条件可能又变了；只有仍然对得上才落库
      if (get().historyFilter === filter) {
        set({
          sessionHistory: page.entries,
          sessionHistoryTotal: page.total,
          sessionHistorySummary: summary,
        });
      }
    } catch (e) {
      console.error("get_session_history_page", e);
    }
  },

  openSessionDetail: async (sessionKey) => {
    // 先把上一个会话的档案清掉：不清的话点第二行会先闪一下第一行的内容
    set({ detailKey: sessionKey, sessionDetail: null });
    try {
      const detail = await invoke<SessionDetail | null>("get_session_detail", {
        sessionKey,
      });
      // 取的过程中用户可能已经关掉抽屉、或者点开了另一个会话
      if (get().detailKey === sessionKey) {
        set({ sessionDetail: detail });
      }
    } catch (e) {
      console.error("get_session_detail", e);
    }
  },

  closeSessionDetail: () => set({ detailKey: null, sessionDetail: null }),

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
          localEvents: [...state.localEvents, ...event.payload].slice(
            -LOCAL_EVENT_CAP,
          ),
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
export const selectResumeRecords = (s: AppStore) => s.resumeRecords;
export const selectResumeRecordsTotal = (s: AppStore) => s.resumeRecordsTotal;
export const selectResumeFilter = (s: AppStore) => s.resumeFilter;
export const selectStatsOverview = (s: AppStore) => s.statsOverview;
export const selectTotals = (s: AppStore) => s.totals;
export const selectStatsTrend = (s: AppStore) => s.statsTrend;
export const selectTrendWindow = (s: AppStore) => s.trendWindow;
export const selectCostDaily = (s: AppStore) => s.costDaily;
export const selectCostProjects = (s: AppStore) => s.costProjects;
export const selectCostModels = (s: AppStore) => s.costModels;
export const selectUsageSummary = (s: AppStore) => s.usageSummary;
export const selectRateForecast = (s: AppStore) => s.rateForecast;
export const selectSessionHistory = (s: AppStore) => s.sessionHistory;
export const selectSessionHistoryTotal = (s: AppStore) => s.sessionHistoryTotal;
export const selectHistoryFilter = (s: AppStore) => s.historyFilter;
export const selectSessionHistorySummary = (s: AppStore) =>
  s.sessionHistorySummary;
export const selectDetailKey = (s: AppStore) => s.detailKey;
export const selectSessionDetail = (s: AppStore) => s.sessionDetail;
