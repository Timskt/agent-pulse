import { useEffect, useMemo, useRef } from "react";
import { useI18n } from "../i18n";
import { LOG_TONE } from "../lib/display";
import { cn } from "../lib/utils";
import { selectEngineEvents, selectLocalEvents, useAppStore } from "../stores/useAppStore";
import type { EngineEvent, LogLevel } from "../types";
import { Card, CardBar, EmptyState } from "./ui";

/** 级别标签保持三字母，中英文下都一样宽，日志才对得齐 */
const LEVEL_TAGS: Record<LogLevel, string> = {
  info: "INF",
  warn: "WRN",
  error: "ERR",
  success: "OK",
};

const MAX_ROWS = 200;

/**
 * 活动日志
 *
 * 两处修好了：
 * - 合并 + 排序 + 截断以前每次渲染都做一遍（约七百条），现在只在事件真的
 *   变了时算，`key` 也从数组下标换成内容，滚动时不再整列重建。
 * - `get_state` 拿回来的历史事件和推送来的事件本来会重复一遍，顺手去重。
 */
export function LogPanel() {
  const { t } = useI18n();
  const engineEvents = useAppStore(selectEngineEvents);
  const localEvents = useAppStore(selectLocalEvents);
  const scroller = useRef<HTMLDivElement>(null);

  const rows = useMemo(() => {
    const unique = new Map<string, EngineEvent>();
    for (const event of [...engineEvents, ...localEvents]) {
      unique.set(
        `${event.timestamp}|${event.level}|${event.session_id ?? ""}|${event.message}`,
        event
      );
    }
    return [...unique]
      .sort(([, a], [, b]) => a.timestamp.localeCompare(b.timestamp))
      .slice(-MAX_ROWS);
  }, [engineEvents, localEvents]);

  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    // 只有本来就贴着底部时才跟着走，否则会把正在往上翻的人拽回来
    const nearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 60;
    if (nearBottom) el.scrollTop = el.scrollHeight;
  }, [rows]);

  return (
    <Card className="overflow-hidden">
      <CardBar>
        <h3 className="text-xs font-semibold text-neutral-800">{t("log.title")}</h3>
        <span className="text-[10px] tabular-nums text-neutral-400">
          {t("log.count", { count: rows.length })}
        </span>
      </CardBar>
      <div ref={scroller} className="log-panel h-44 overflow-y-auto px-4 py-3">
        {rows.length === 0 ? (
          <EmptyState title={t("log.empty")} className="py-6" />
        ) : (
          rows.map(([key, event]) => (
            <div key={key} className="flex gap-2 py-0.5">
              <span className="shrink-0 tabular-nums text-neutral-300">{event.timestamp}</span>
              <span className={cn("shrink-0 font-medium", LOG_TONE[event.level])}>
                {LEVEL_TAGS[event.level]}
              </span>
              <span className={cn("break-all", LOG_TONE[event.level])}>{event.message}</span>
            </div>
          ))
        )}
      </div>
    </Card>
  );
}
