import { useEffect, useRef } from "react";
import { useAppStore } from "../stores/useAppStore";
import type { EngineEvent, LogLevel } from "../types";

const LEVEL_STYLES: Record<LogLevel, string> = {
  info: "text-gray-400",
  warn: "text-amber-300",
  error: "text-red-400",
  success: "text-emerald-400",
};

const LEVEL_TAGS: Record<LogLevel, { text: string; bg: string }> = {
  info: { text: "INFO", bg: "bg-gray-800/80 text-gray-500" },
  warn: { text: "WARN", bg: "bg-amber-400/10 text-amber-400" },
  error: { text: "ERR ", bg: "bg-red-400/10 text-red-400" },
  success: { text: " OK ", bg: "bg-emerald-400/10 text-emerald-400" },
};

/** 实时日志面板 */
export function LogPanel() {
  const { monitorState, localEvents } = useAppStore();
  const bottomRef = useRef<HTMLDivElement>(null);

  // 合并后端事件与本地推送事件
  const allEvents: EngineEvent[] = [...monitorState.events, ...localEvents]
    .sort((a, b) => a.timestamp.localeCompare(b.timestamp))
    .slice(-200);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [allEvents.length]);

  return (
    <div className="flex flex-col overflow-hidden rounded-xl border border-gray-800/60 bg-gray-950/90 backdrop-blur">
      {/* 面板头部 */}
      <div className="flex items-center justify-between border-b border-gray-800/60 px-4 py-2.5">
        <div className="flex items-center gap-2.5">
          {/* 终端风格圆点 */}
          <div className="flex items-center gap-1.5">
            <span className="h-2.5 w-2.5 rounded-full bg-red-500/70" />
            <span className="h-2.5 w-2.5 rounded-full bg-amber-500/70" />
            <span className="h-2.5 w-2.5 rounded-full bg-emerald-500/70" />
          </div>
          <span className="text-xs font-medium text-gray-400">运行日志</span>
        </div>
        <span className="rounded-full bg-gray-800/60 px-2 py-0.5 text-[10px] tabular-nums text-gray-500">
          {allEvents.length} 条
        </span>
      </div>

      {/* 日志内容 */}
      <div className="log-panel flex-1 overflow-y-auto p-3.5 min-h-[160px] max-h-[240px]">
        {allEvents.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center gap-2 text-gray-600">
            <span className="text-lg opacity-50">📋</span>
            <p className="text-xs">暂无日志，启动监控后将实时输出...</p>
          </div>
        ) : (
          allEvents.map((event, i) => (
            <div
              key={`${event.timestamp}-${i}`}
              className="flex items-start gap-2.5 rounded px-1.5 py-0.5 leading-5 transition-colors hover:bg-gray-900/60 animate-log-slide"
            >
              <span className="shrink-0 tabular-nums text-gray-600">
                {event.timestamp.split(" ")[1] ?? event.timestamp}
              </span>
              <span
                className={`shrink-0 rounded px-1.5 text-[9px] font-bold leading-4 ${LEVEL_TAGS[event.level].bg}`}
              >
                {LEVEL_TAGS[event.level].text}
              </span>
              {event.session_id && (
                <span className="shrink-0 rounded bg-purple-400/5 px-1 text-[10px] text-purple-400/60">
                  {event.session_id.slice(0, 12)}
                </span>
              )}
              <span className={`${LEVEL_STYLES[event.level]} break-all`}>
                {event.message}
              </span>
            </div>
          ))
        )}
        <div ref={bottomRef} />
      </div>
    </div>
  );
}
