import { useEffect, useRef } from "react";
import { useAppStore } from "../stores/useAppStore";
import type { EngineEvent, LogLevel } from "../types";

const LEVEL_STYLES: Record<LogLevel, string> = {
  info: "text-neutral-500",
  warn: "text-amber-600",
  error: "text-red-600",
  success: "text-emerald-600",
};

const LEVEL_TAGS: Record<LogLevel, string> = {
  info: "INF",
  warn: "WRN",
  error: "ERR",
  success: "OK",
};

/** 日志面板 */
export function LogPanel() {
  const { monitorState, localEvents } = useAppStore();
  const bottomRef = useRef<HTMLDivElement>(null);

  const allEvents: EngineEvent[] = [...monitorState.events, ...localEvents]
    .sort((a, b) => a.timestamp.localeCompare(b.timestamp))
    .slice(-200);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [allEvents.length]);

  return (
    <div className="rounded-lg border border-neutral-200 bg-white">
      <div className="flex items-center justify-between border-b border-neutral-100 px-4 py-2.5">
        <h2 className="text-xs font-medium text-neutral-500">Activity Log</h2>
        <span className="text-[10px] text-neutral-300">
          {allEvents.length} entries
        </span>
      </div>
      <div className="log-panel h-44 overflow-y-auto px-4 py-3">
        {allEvents.length === 0 ? (
          <p className="text-neutral-300">No activity yet</p>
        ) : (
          allEvents.map((event, i) => (
            <div key={i} className="flex gap-2 py-0.5">
              <span className="shrink-0 text-neutral-300">{event.timestamp}</span>
              <span
                className={`shrink-0 font-medium ${LEVEL_STYLES[event.level]}`}
              >
                {LEVEL_TAGS[event.level]}
              </span>
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
