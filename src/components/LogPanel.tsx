import { useEffect, useRef } from "react";
import { useAppStore } from "../stores/useAppStore";
import type { EngineEvent, LogLevel } from "../types";

const LEVEL_STYLES: Record<LogLevel, string> = {
  info: "text-gray-400",
  warn: "text-amber-400",
  error: "text-red-400",
  success: "text-emerald-400",
};

const LEVEL_TAGS: Record<LogLevel, string> = {
  info: "INFO",
  warn: "WARN",
  error: "ERR ",
  success: " OK ",
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
    <div className="flex h-full flex-col rounded-xl border border-gray-800 bg-gray-950 overflow-hidden">
      {/* 面板头部 */}
      <div className="flex items-center justify-between border-b border-gray-800 px-4 py-2">
        <span className="text-xs font-medium text-gray-400">运行日志</span>
        <span className="text-[10px] text-gray-600">
          {allEvents.length} 条记录
        </span>
      </div>

      {/* 日志内容 */}
      <div className="log-panel flex-1 overflow-y-auto p-3 min-h-[160px] max-h-[240px]">
        {allEvents.length === 0 ? (
          <div className="flex h-full items-center justify-center text-gray-600">
            <p className="text-xs">暂无日志，启动监控后将实时输出...</p>
          </div>
        ) : (
          allEvents.map((event, i) => (
            <div key={`${event.timestamp}-${i}`} className="flex gap-2 leading-5">
              <span className="shrink-0 text-gray-600">{event.timestamp}</span>
              <span
                className={`shrink-0 rounded px-1 text-[10px] font-bold ${LEVEL_STYLES[event.level]} bg-gray-900`}
              >
                {LEVEL_TAGS[event.level]}
              </span>
              {event.session_id && (
                <span className="shrink-0 text-purple-400/70">
                  [{event.session_id}]
                </span>
              )}
              <span className={LEVEL_STYLES[event.level]}>{event.message}</span>
            </div>
          ))
        )}
        <div ref={bottomRef} />
      </div>
    </div>
  );
}
