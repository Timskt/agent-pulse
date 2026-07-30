import { useAppStore } from "../stores/useAppStore";
import { STATUS_COLORS, STATUS_LABELS } from "../types";
import type { AgentSession } from "../types";

/** 会话列表 */
export function SessionList() {
  const { monitorState, manualResume } = useAppStore();
  const sessions = monitorState.sessions;

  if (sessions.length === 0) {
    return (
      <div className="flex h-40 flex-col items-center justify-center rounded-lg border border-dashed border-neutral-200 bg-white text-neutral-400">
        <p className="text-sm">No active sessions</p>
        <p className="mt-1 text-xs text-neutral-300">
          Start Claude Code, Codex CLI or OpenCode to see sessions here
        </p>
      </div>
    );
  }

  return (
    <div className="rounded-lg border border-neutral-200 bg-white">
      <div className="border-b border-neutral-100 px-4 py-2.5">
        <h2 className="text-xs font-medium text-neutral-500">Sessions</h2>
      </div>
      <div className="divide-y divide-neutral-50">
        {sessions.map((session) => (
          <SessionRow key={session.id} session={session} onResume={manualResume} />
        ))}
      </div>
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
  const isInterrupted =
    session.status === "interrupted" || session.status === "suspended";

  return (
    <div className="flex items-center gap-3 px-4 py-3 transition-colors hover:bg-neutral-50/50">
      {/* 状态点 */}
      <span
        className={`h-2 w-2 shrink-0 rounded-full ${statusDot(session.status)}`}
      />

      {/* 信息 */}
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="text-xs font-medium text-neutral-800">
            {session.agent_name}
          </span>
          <span className="text-[10px] text-neutral-300">
            PID {session.pid}
          </span>
          {session.resume_count > 0 && (
            <span className="rounded bg-neutral-100 px-1.5 py-0.5 text-[9px] font-medium text-neutral-500">
              ×{session.resume_count}
            </span>
          )}
        </div>
        <p className="mt-0.5 truncate text-[11px] text-neutral-400">
          {session.working_dir || session.command}
        </p>
      </div>

      {/* 状态标签 */}
      <span
        className={`shrink-0 rounded-full px-2 py-0.5 text-[10px] font-medium ${STATUS_COLORS[session.status]}`}
      >
        {STATUS_LABELS[session.status]}
      </span>

      {/* 操作 */}
      {isInterrupted && (
        <button
          onClick={() => onResume(session.id)}
          className="shrink-0 rounded-md border border-neutral-200 px-2.5 py-1 text-[10px] font-medium text-neutral-600 transition-colors hover:bg-neutral-100"
        >
          Resume
        </button>
      )}
    </div>
  );
}

function statusDot(status: string): string {
  switch (status) {
    case "active":
      return "bg-emerald-500";
    case "suspended":
      return "bg-amber-400";
    case "interrupted":
      return "bg-red-500";
    case "completed":
      return "bg-blue-400";
    default:
      return "bg-neutral-300";
  }
}
