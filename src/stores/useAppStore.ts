import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AppConfig,
  EngineEvent,
  MonitorState,
  DailyStats,
  ResumeRecord,
  AiVerdict,
} from "../types";

export type TabId = "dashboard" | "stats" | "config";

interface AppStore {
  // 状态
  monitorState: MonitorState;
  config: AppConfig | null;
  localEvents: EngineEvent[];
  activeTab: TabId;
  loading: boolean;
  // 统计数据
  dailyStats: DailyStats[];
  resumeHistory: ResumeRecord[];
  totals: [number, number, number] | null;

  // 动作
  setActiveTab: (tab: TabId) => void;
  fetchState: () => Promise<void>;
  fetchConfig: () => Promise<void>;
  fetchStats: () => Promise<void>;
  startMonitoring: () => Promise<void>;
  stopMonitoring: () => Promise<void>;
  scanNow: () => Promise<void>;
  updateConfig: (config: AppConfig) => Promise<void>;
  manualResume: (sessionId: string, useGoalPrompt?: boolean) => Promise<void>;
  testWebhook: () => Promise<string>;
  aiAnalyze: (sessionId: string) => Promise<AiVerdict>;
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
    total_resumes: 0,
    total_detections: 0,
    last_scan_at: null,
    uptime_secs: 0,
  },
};

export const useAppStore = create<AppStore>((set, get) => ({
  monitorState: defaultMonitorState,
  config: null,
  localEvents: [],
  activeTab: "dashboard",
  loading: false,
  dailyStats: [],
  resumeHistory: [],
  totals: null,

  setActiveTab: (tab) => set({ activeTab: tab }),

  fetchState: async () => {
    try {
      const state = await invoke<MonitorState>("get_state");
      set({ monitorState: state });
    } catch (e) {
      console.error("获取状态失败:", e);
    }
  },

  fetchConfig: async () => {
    try {
      const config = await invoke<AppConfig>("get_config");
      set({ config });
    } catch (e) {
      console.error("获取配置失败:", e);
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
      console.error("获取统计失败:", e);
    }
  },

  startMonitoring: async () => {
    set({ loading: true });
    try {
      await invoke("start_monitoring");
      await get().fetchState();
    } catch (e) {
      console.error("启动监控失败:", e);
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
      console.error("停止监控失败:", e);
    } finally {
      set({ loading: false });
    }
  },

  scanNow: async () => {
    set({ loading: true });
    try {
      const state = await invoke<MonitorState>("scan_now");
      set({ monitorState: state });
    } catch (e) {
      console.error("扫描失败:", e);
    } finally {
      set({ loading: false });
    }
  },

  updateConfig: async (config) => {
    try {
      await invoke("update_config", { config });
      set({ config });
    } catch (e) {
      console.error("更新配置失败:", e);
    }
  },

  manualResume: async (sessionId, useGoalPrompt = false) => {
    try {
      await invoke("manual_resume", { sessionId, useGoalPrompt });
      await get().fetchState();
    } catch (e) {
      console.error("手动续跑失败:", e);
    }
  },

  testWebhook: async () => {
    try {
      return await invoke<string>("test_webhook");
    } catch (e) {
      return `错误: ${e}`;
    }
  },

  aiAnalyze: async (sessionId) => {
    return await invoke<AiVerdict>("ai_analyze", { sessionId });
  },

  initEventListeners: async () => {
    const unlistenEvents = await listen<EngineEvent[]>(
      "engine-events",
      (event) => {
        set((state) => ({
          localEvents: [...state.localEvents, ...event.payload].slice(-500),
        }));
      }
    );

    const unlistenStopped = await listen("engine-stopped", () => {
      get().fetchState();
    });

    // 定时轮询状态（兜底）
    const timer = setInterval(() => {
      get().fetchState();
    }, 3000);

    return () => {
      unlistenEvents();
      unlistenStopped();
      clearInterval(timer);
    };
  },
}));
