import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AppConfig, EngineEvent, MonitorState } from "../types";

interface AppStore {
  // 状态
  monitorState: MonitorState;
  config: AppConfig | null;
  localEvents: EngineEvent[];
  activeTab: "dashboard" | "config";
  loading: boolean;

  // 动作
  setActiveTab: (tab: "dashboard" | "config") => void;
  fetchState: () => Promise<void>;
  fetchConfig: () => Promise<void>;
  startMonitoring: () => Promise<void>;
  stopMonitoring: () => Promise<void>;
  scanNow: () => Promise<void>;
  updateConfig: (config: AppConfig) => Promise<void>;
  manualResume: (sessionId: string) => Promise<void>;
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

  manualResume: async (sessionId) => {
    try {
      await invoke("manual_resume", { sessionId });
      await get().fetchState();
    } catch (e) {
      console.error("手动续跑失败:", e);
    }
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
