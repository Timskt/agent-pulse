import { useEffect } from "react";
import { useAppStore } from "./stores/useAppStore";
import { StatusCards } from "./components/StatusCards";
import { SessionList } from "./components/SessionList";
import { LogPanel } from "./components/LogPanel";
import { ConfigPanel } from "./components/ConfigPanel";

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
      {/* 标题栏 */}
      <header className="titlebar-drag flex items-center justify-between border-b border-gray-800 px-5 py-3">
        <div className="flex items-center gap-3">
          <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-gradient-to-br from-pulse-500 to-purple-600 text-sm font-bold text-white">
            ⚡
          </div>
          <div>
            <h1 className="text-sm font-bold text-gray-100">AgentPulse</h1>
            <p className="text-[10px] text-gray-500">
              AI Coding Agent 守护 · 自动续跑
            </p>
          </div>
        </div>

        {/* 导航标签 */}
        <nav className="titlebar-no-drag flex items-center gap-1 rounded-lg bg-gray-900 p-1">
          {(
            [
              ["dashboard", "监控面板"],
              ["config", "配置"],
            ] as const
          ).map(([tab, label]) => (
            <button
              key={tab}
              onClick={() => setActiveTab(tab)}
              className={`rounded-md px-4 py-1.5 text-xs font-medium transition-colors ${
                activeTab === tab
                  ? "bg-pulse-600 text-white"
                  : "text-gray-400 hover:text-gray-200"
              }`}
            >
              {label}
            </button>
          ))}
        </nav>

        {/* 控制按钮 */}
        <div className="titlebar-no-drag flex items-center gap-2">
          <button
            onClick={scanNow}
            disabled={loading}
            className="rounded-lg border border-gray-700 bg-gray-800 px-3 py-1.5 text-xs text-gray-300 transition-colors hover:bg-gray-700 disabled:opacity-50"
          >
            🔍 立即分析
          </button>
          {monitorState.running ? (
            <button
              onClick={stopMonitoring}
              disabled={loading}
              className="rounded-lg bg-red-600/90 px-4 py-1.5 text-xs font-medium text-white transition-colors hover:bg-red-500 disabled:opacity-50"
            >
              ⏹ 停止监听
            </button>
          ) : (
            <button
              onClick={startMonitoring}
              disabled={loading}
              className="rounded-lg bg-emerald-600 px-4 py-1.5 text-xs font-medium text-white transition-colors hover:bg-emerald-500 disabled:opacity-50"
            >
              ▶ 开始监听
            </button>
          )}
        </div>
      </header>

      {/* 主内容区 */}
      <main className="flex-1 overflow-y-auto p-5">
        {activeTab === "dashboard" ? (
          <div className="space-y-4">
            <StatusCards />

            {/* 会话列表 */}
            <section>
              <div className="mb-2 flex items-center justify-between">
                <h2 className="text-xs font-semibold uppercase tracking-wider text-gray-500">
                  监控会话列表
                </h2>
                <span className="text-[10px] text-gray-600">
                  支持: Claude Code · Codex CLI
                </span>
              </div>
              <SessionList />
            </section>

            {/* 日志面板 */}
            <LogPanel />
          </div>
        ) : (
          <ConfigPanel />
        )}
      </main>

      {/* 底部状态栏 */}
      <footer className="flex items-center justify-between border-t border-gray-800 px-5 py-2 text-[10px] text-gray-600">
        <span>AgentPulse v0.1.0 · macOS</span>
        <span>
          {monitorState.running
            ? `轮询间隔 ${10}s · 双重校验模式`
            : "监控未启动"}
        </span>
      </footer>
    </div>
  );
}
