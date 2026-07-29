import { useAppStore } from "../stores/useAppStore";
import { STATUS_COLORS, STATUS_LABELS } from "../types";
import type { AgentSession } from "../types";

/** 会话监控列表 */
export function SessionList() {
  const { monitorState, manualResume } = useAppStore();
  const sessions = monitorState.sessions;

  if (sessions.length === 0) {
    return (
      <div className="flex h-48 flex-col items-center justify-center rounded-xl border border-dashed border-gray-700 bg-gray-900/40 text-gray-500">
        <span className="text-3xl">🔍</span>
        <p className="mt-2 text-sm">暂未发现 AI Agent 会话</p>
        <p className="mt-1 text-xs text-gray-600">
          启动 Claude Code / Codex 后将自动检测
        </p>
      </div>
    );
  }

  return (
    <div className="space-y-2">
      {sessions.map((session) => (
        <SessionRow key={session.id} session={session} onResume={manualResume} />
      ))}
    </div>
  );
}

function SessionRow({
  session,
  onResume,
}: {
  session: AgentSession;
  onResume: (id: string) => void;
}) {
  const statusClass = STATUS_COLORS[session.status];
  const statusLabel = STATUS_LABELS[session.status];

  const shortDir = session.working_dir
    ? session.working_dir.split("/").slice(-2).join("/")
    : "未知目录";

  return (
    <div className="flex items-center gap-3 rounded-xl border border-gray-800 bg-gray-900/80 px-4 py-3 transition-colors hover:border-gray-700">
      {/* Agent 图标 */}
      <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-gray-800 text-lg">
        {session.adapter_id === "claude-code" ? "🤖" : session.adapter_id === "opencode" ? "🧑‍💻" : "🧠"}
      </div>

      {/* 会话信息 */}
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="text-sm font-medium text-gray-200">
            {session.agent_name}
          </span>
          <span
            className={`rounded-full border px-2 py-0.5 text-[10px] ${statusClass}`}
          >
            {statusLabel}
          </span>
          {session.resume_count > 0 && (
            <span className="rounded-full bg-amber-400/10 px-2 py-0.5 text-[10px] text-amber-400">
              续跑 ×{session.resume_count}
            </span>
          )}
        </div>
        <div className="mt-0.5 flex items-center gap-3 text-xs text-gray-500">
          <span className="font-mono">PID {session.pid}</span>
          <span className="truncate">📁 {shortDir}</span>
          <span>最后活动: {session.last_activity}</span>
        </div>
      </div>

      {/* 操作按钮 */}
      <div className="flex items-center gap-2">
        {(session.status === "interrupted" || session.status === "suspended") && (
          <button
            onClick={() => onResume(session.id)}
            className="rounded-lg bg-pulse-600 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-pulse-500"
          >
            ▶ 续跑
          </button>
        )}
      </div>
    </div>
  );
}
