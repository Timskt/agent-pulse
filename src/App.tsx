import { useEffect } from "react";
import { useAppStore } from "./stores/useAppStore";
import { StatusCards } from "./components/StatusCards";
import { SessionList } from "./components/SessionList";
import { LogPanel } from "./components/LogPanel";
import { ConfigPanel } from "./components/ConfigPanel";
import { StatsPanel } from "./components/StatsPanel";

export default function App() {
  const {
    monitorState,
    activeTab,
    setActiveTab,
    loading,
    fetchState,
    fetchConfig,
    startMonitoring,
    stopMonitoring,
    scanNow,
    initEventListeners,
  } = useAppStore();

  useEffect(() => {
    fetchState();
    fetchConfig();
    const cleanup = initEventListeners();
    return () => {
      cleanup.then((fn) => fn());
    };
  }, [fetchState, fetchConfig, initEventListeners]);

  return (
    <div className="flex h-screen flex-col bg-[#fafafa]">
      {/* 标题栏 */}
      <header className="titlebar-drag flex items-center justify-between border-b border-neutral-200 bg-white px-6 py-3">
        <div className="flex items-center gap-3">
          <div className="flex h-7 w-7 items-center justify-center rounded-md bg-neutral-900 text-xs font-bold text-white">
            A
          </div>
          <div>
            <h1 className="text-[13px] font-semibold tracking-tight text-neutral-900">
              AgentPulse
            </h1>
            <p className="text-[10px] text-neutral-400">
              AI Agent Monitor & Auto-Resume
            </p>
          </div>
          {monitorState.running && (
            <span className="ml-2 flex items-center gap-1.5 rounded-full bg-emerald-50 px-2 py-0.5 text-[10px] font-medium text-emerald-600">
              <span className="h-1.5 w-1.5 rounded-full bg-emerald-500 animate-pulse-soft" />
              Running
            </span>
          )}
        </div>

        {/* 导航 */}
        <nav className="titlebar-no-drag flex items-center gap-1 rounded-lg bg-neutral-100 p-0.5">
          {(
            [
              ["dashboard", "Dashboard"],
              ["stats", "Statistics"],
              ["config", "Settings"],
            ] as const
          ).map(([tab, label]) => (
            <button
              key={tab}
              onClick={() => setActiveTab(tab)}
              className={`rounded-md px-3.5 py-1.5 text-xs font-medium transition-colors ${
                activeTab === tab
                  ? "bg-white text-neutral-900 shadow-sm"
                  : "text-neutral-500 hover:text-neutral-700"
              }`}
            >
              {label}
            </button>
          ))}
        </nav>

        {/* 控制 */}
        <div className="titlebar-no-drag flex items-center gap-2">
          <button
            onClick={scanNow}
            disabled={loading}
            className="rounded-md border border-neutral-200 px-3 py-1.5 text-xs text-neutral-600 transition-colors hover:bg-neutral-50 disabled:opacity-40"
          >
            Scan Now
          </button>
          {monitorState.running ? (
            <button
              onClick={stopMonitoring}
              disabled={loading}
              className="rounded-md bg-neutral-900 px-3.5 py-1.5 text-xs font-medium text-white transition-colors hover:bg-neutral-700 disabled:opacity-40"
            >
              Stop
            </button>
          ) : (
            <button
              onClick={startMonitoring}
              disabled={loading}
              className="rounded-md bg-neutral-900 px-3.5 py-1.5 text-xs font-medium text-white transition-colors hover:bg-neutral-700 disabled:opacity-40"
            >
              Start
            </button>
          )}
        </div>
      </header>

      {/* 主内容 */}
      <main className="flex-1 overflow-y-auto p-6">
        {activeTab === "dashboard" ? (
          <div className="space-y-5 animate-fade-in">
            <StatusCards />
            <SessionList />
            <LogPanel />
          </div>
        ) : activeTab === "stats" ? (
          <div className="animate-fade-in">
            <StatsPanel />
          </div>
        ) : (
          <div className="animate-fade-in">
            <ConfigPanel />
          </div>
        )}
      </main>

      {/* 底部状态栏 */}
      <footer className="flex items-center justify-between border-t border-neutral-200 bg-white px-6 py-2 text-[10px] text-neutral-400">
        <span>AgentPulse v1.0.0</span>
        <div className="flex items-center gap-4">
          {monitorState.running && (
            <>
              <span>Sessions: {monitorState.status.sessions_total}</span>
              <span>Interval: 10s</span>
            </>
          )}
          <span className="text-neutral-300">
            {monitorState.status.last_scan_at ?? "Not scanning"}
          </span>
        </div>
      </footer>
    </div>
  );
}
