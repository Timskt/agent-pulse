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
    <div className="flex h-screen flex-col bg-gray-950">
      {/* 顶部渐变流光条 */}
      <div
        className={`h-[2px] w-full animate-gradient-flow bg-gradient-to-r from-indigo-500 via-purple-500 to-emerald-500 transition-opacity duration-700 ${
          monitorState.running ? "opacity-100" : "opacity-30"
        }`}
      />

      {/* 标题栏 */}
      <header className="titlebar-drag flex items-center justify-between border-b border-gray-800/60 bg-gray-950/80 px-5 py-3 backdrop-blur-xl">
        <div className="flex items-center gap-3">
          <div className="relative flex h-9 w-9 items-center justify-center rounded-xl bg-gradient-to-br from-indigo-500 via-purple-500 to-emerald-500 text-sm font-bold text-white shadow-lg shadow-purple-500/20">
            ⚡
            {monitorState.running && (
              <span className="absolute -right-0.5 -top-0.5 h-2.5 w-2.5 rounded-full border-2 border-gray-950 bg-emerald-400 animate-pulse-dot" />
            )}
          </div>
          <div>
            <h1 className="text-sm font-bold tracking-wide text-gray-100">
              AgentPulse
            </h1>
            <p className="text-[10px] tracking-wider text-gray-500">
              AI Agent 守护 · Goal 自动恢复 · 跨平台精准续跑
            </p>
          </div>
        </div>

        {/* 导航标签 */}
        <nav className="titlebar-no-drag flex items-center gap-0.5 rounded-xl border border-gray-800/60 bg-gray-900/80 p-1">
          {(
            [
              ["dashboard", "监控面板", "📡"],
              ["stats", "统计分析", "📊"],
              ["config", "配置", "⚙️"],
            ] as const
          ).map(([tab, label, icon]) => (
            <button
              key={tab}
              onClick={() => setActiveTab(tab)}
              className={`flex items-center gap-1.5 rounded-lg px-4 py-1.5 text-xs font-medium transition-all duration-200 ${
                activeTab === tab
                  ? "bg-gradient-to-r from-indigo-600 to-purple-600 text-white shadow-md shadow-indigo-500/25"
                  : "text-gray-400 hover:bg-gray-800/60 hover:text-gray-200"
              }`}
            >
              <span className="text-[11px]">{icon}</span>
              {label}
            </button>
          ))}
        </nav>

        {/* 控制按钮 */}
        <div className="titlebar-no-drag flex items-center gap-2">
          <button
            onClick={scanNow}
            disabled={loading}
            className="group flex items-center gap-1.5 rounded-lg border border-gray-700/60 bg-gray-800/60 px-3 py-1.5 text-xs text-gray-300 transition-all duration-200 hover:border-gray-600 hover:bg-gray-700/60 disabled:opacity-50"
          >
            <span className={`inline-block text-[11px] ${loading ? "animate-spin-slow" : "group-hover:rotate-12 transition-transform"}`}>
              🔍
            </span>
            立即分析
          </button>
          {monitorState.running ? (
            <button
              onClick={stopMonitoring}
              disabled={loading}
              className="flex items-center gap-1.5 rounded-lg bg-gradient-to-r from-red-600 to-rose-600 px-4 py-1.5 text-xs font-medium text-white shadow-lg shadow-red-500/20 transition-all duration-200 hover:shadow-red-500/40 hover:brightness-110 disabled:opacity-50"
            >
              <span className="h-1.5 w-1.5 rounded-sm bg-white/90" />
              停止监听
            </button>
          ) : (
            <button
              onClick={startMonitoring}
              disabled={loading}
              className="flex items-center gap-1.5 rounded-lg bg-gradient-to-r from-emerald-600 to-teal-600 px-4 py-1.5 text-xs font-medium text-white shadow-lg shadow-emerald-500/20 transition-all duration-200 hover:shadow-emerald-500/40 hover:brightness-110 disabled:opacity-50"
            >
              <span className="text-[10px]">▶</span>
              开始监听
            </button>
          )}
        </div>
      </header>

      {/* 主内容区 */}
      <main className="flex-1 overflow-y-auto p-5">
        {activeTab === "dashboard" ? (
          <div className="space-y-4 animate-fade-in-up">
            <StatusCards />

            {/* 会话列表 */}
            <section>
              <div className="mb-2.5 flex items-center justify-between">
                <h2 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wider text-gray-400">
                  <span className="h-3.5 w-1 rounded-full bg-gradient-to-b from-indigo-500 to-purple-500" />
                  监控会话列表
                </h2>
                <span className="rounded-full border border-gray-800 bg-gray-900/60 px-2.5 py-0.5 text-[10px] text-gray-500">
                  Claude Code · Codex CLI · OpenCode
                </span>
              </div>
              <SessionList />
            </section>

            {/* 日志面板 */}
            <LogPanel />
          </div>
        ) : activeTab === "stats" ? (
          <div className="animate-fade-in-up">
            <StatsPanel />
          </div>
        ) : (
          <div className="animate-fade-in-up">
            <ConfigPanel />
          </div>
        )}
      </main>

      {/* 底部状态栏 */}
      <footer className="flex items-center justify-between border-t border-gray-800/60 bg-gray-950/80 px-5 py-2 text-[10px] text-gray-600 backdrop-blur">
        <div className="flex items-center gap-2">
          <span className="font-medium text-gray-500">AgentPulse</span>
          <span className="rounded bg-gray-800/80 px-1.5 py-0.5 font-mono text-[9px] text-gray-400">
            v1.0.0
          </span>
          <span>跨平台 · AI 智能判断</span>
        </div>
        <div className="flex items-center gap-3">
          {monitorState.running ? (
            <>
              <span className="flex items-center gap-1.5 text-emerald-500/80">
                <span className="h-1.5 w-1.5 rounded-full bg-emerald-400 animate-pulse-dot" />
                双重校验模式
              </span>
              <span>轮询 10s</span>
            </>
          ) : (
            <span className="text-gray-600">监控未启动</span>
          )}
        </div>
      </footer>
    </div>
  );
}
