import { useAppStore } from "../stores/useAppStore";
import { STATUS_COLORS, STATUS_LABELS } from "../types";
import type { AgentSession } from "../types";

/** 会话监控列表 */
export function SessionList() {
  const { monitorState, manualResume } = useAppStore();
  const sessions = monitorState.sessions;

  if (sessions.length === 0) {
    return (
      <div className="flex h-48 flex-col items-center justify-center rounded-xl border border-dashed border-gray-700/50 bg-gray-900/30 text-gray-500">
        <div className="relative">
          <span className="text-4xl opacity-60">🛰️</span>
          <span className="absolute -right-2 -top-1 h-2 w-2 rounded-full bg-indigo-400/60 animate-ping" />
        </div>
        <p className="mt-3 text-sm font-medium text-gray-400">
          暂未发现 AI Agent 会话
        </p>
        <p className="mt-1 text-xs text-gray-600">
          启动 Claude Code / Codex / OpenCode 后将自动检测
        </p>
      </div>
    );
  }

  return (
    <div className="space-y-2">
      {sessions.map((session, i) => (
        <SessionRow
          key={session.id}
          session={session}
          index={i}
          onResume={manualResume}
        />
      ))}
    </div>
  );
}

/** 状态指示灯颜色 */
const STATUS_DOT: Record<string, string> = {
  active: "bg-emerald-400 text-emerald-400",
  suspended: "bg-amber-400 text-amber-400",
  interrupted: "bg-red-400 text-red-400",
  completed: "bg-blue-400 text-blue-400",
  exited: "bg-gray-500 text-gray-500",
};

/** Agent 图标映射 */
const AGENT_ICONS: Record<string, string> = {
  "claude-code": "🤖",
  codex: "🧠",
  opencode: "🧑‍💻",
};

function SessionRow({
  session,
  index,
  onResume,
}: {
  session: AgentSession;
  index: number;
  onResume: (id: string) => void;
}) {
  const statusClass = STATUS_COLORS[session.status];
  const statusLabel = STATUS_LABELS[session.status];
  const dotClass = STATUS_DOT[session.status] ?? STATUS_DOT.exited;
  const isAlert =
    session.status === "interrupted" || session.status === "suspended";

  const shortDir = session.working_dir
    ? session.working_dir.split("/").slice(-2).join("/")
    : "未知目录";

  return (
    <div
      className={`group flex items-center gap-3.5 rounded-xl border px-4 py-3 backdrop-blur transition-all duration-300 animate-fade-in-up ${
        isAlert
          ? "border-red-500/20 bg-red-950/20 hover:border-red-500/40"
          : "border-gray-800/60 bg-gray-900/70 hover:border-gray-700/60 hover:bg-gray-900/90"
      }`}
      style={{ animationDelay: `${index * 50}ms` }}
    >
      {/* 状态光点 */}
      <span
        className={`h-2 w-2 shrink-0 rounded-full ${dotClass} ${
          session.status === "active" ? "animate-pulse-dot" : ""
        } ${isAlert ? "animate-pulse-dot" : ""}`}
      />

      {/* Agent 图标 */}
      <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-gray-700/40 bg-gray-800/80 text-lg transition-transform duration-300 group-hover:scale-105">
        {AGENT_ICONS[session.adapter_id] ?? "🤖"}
      </div>

      {/* 会话信息 */}
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="text-sm font-medium text-gray-200">
            {session.agent_name}
          </span>
          <span
            className={`rounded-full border px-2 py-0.5 text-[10px] font-medium ${statusClass}`}
          >
            {statusLabel}
          </span>
          {session.resume_count > 0 && (
            <span className="rounded-full bg-amber-400/10 px-2 py-0.5 text-[10px] font-medium text-amber-400">
              🔄 ×{session.resume_count}
            </span>
          )}
        </div>
        <div className="mt-1 flex items-center gap-3 text-[11px] text-gray-500">
          <span className="rounded bg-gray-800/80 px-1.5 py-0.5 font-mono text-[10px] text-gray-400">
            PID {session.pid}
          </span>
          <span className="truncate">📁 {shortDir}</span>
          <span className="shrink-0">🕐 {session.last_activity}</span>
        </div>
      </div>

      {/* 操作按钮 */}
      <div className="flex shrink-0 items-center gap-2">
        {isAlert && (
          <button
            onClick={() => onResume(session.id)}
            className="flex items-center gap-1.5 rounded-lg bg-gradient-to-r from-indigo-600 to-purple-600 px-3.5 py-1.5 text-xs font-medium text-white shadow-md shadow-indigo-500/20 transition-all duration-200 hover:shadow-indigo-500/40 hover:brightness-110 active:scale-95"
          >
            <span className="text-[10px]">▶</span>
            续跑
          </button>
        )}
      </div>
    </div>
  );
}
