import { useEffect, useMemo } from "react";
import { useI18n } from "../i18n";
import { formatShortTime } from "../lib/utils";
import {
  selectDailyStats,
  selectResumeHistory,
  selectTotals,
  useAppStore,
} from "../stores/useAppStore";
import {
  Badge,
  BarChart,
  Card,
  CardBody,
  CardHeader,
  EmptyState,
  LegendDot,
  type BarDatum,
} from "./ui";

/** 与 store 里 `get_stats { days: 30 }` / `get_resume_history { limit: 50 }` 保持一致 */
const DAYS = 30;
const HISTORY_LIMIT = 50;

/**
 * 统计页
 *
 * 图表以前是空白的：外层 `flex items-end` 里柱子写 `height: N%`，
 * 没有确定的父高度可参照就被解析成 0。现在统一交给 `BarChart`，
 * 这一页只负责把 `DailyStats` 翻成 `BarDatum`。
 */
export function StatsPanel() {
  const { t } = useI18n();
  const dailyStats = useAppStore(selectDailyStats);
  const resumeHistory = useAppStore(selectResumeHistory);
  const totals = useAppStore(selectTotals);
  const fetchStats = useAppStore((s) => s.fetchStats);

  useEffect(() => {
    void fetchStats();
  }, [fetchStats]);

  const [detections, resumes, successful] = totals ?? [0, 0, 0];
  const successRate = resumes > 0 ? Math.round((successful / resumes) * 100) : 0;

  const bars = useMemo<BarDatum[]>(
    () =>
      dailyStats.map((day) => ({
        key: day.date,
        value: day.total_detections + day.total_resumes,
        overlay: day.successful_resumes,
        tooltip: t("stats.bar_tooltip", {
          date: day.date,
          detections: day.total_detections,
          resumes: day.total_resumes,
          successful: day.successful_resumes,
        }),
      })),
    [dailyStats, t]
  );

  return (
    <div className="mx-auto max-w-3xl space-y-4">
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <StatCard label={t("stats.detections")} value={detections} />
        <StatCard label={t("stats.resumes")} value={resumes} />
        <StatCard label={t("stats.successful")} value={successful} />
        <StatCard label={t("stats.success_rate")} value={`${successRate}%`} />
      </div>

      <Card>
        <CardBody>
          <CardHeader
            className="mb-4"
            title={t("stats.activity", { days: DAYS })}
            desc={t("stats.activity_desc")}
            aside={
              <div className="flex items-center gap-3">
                <LegendDot>{t("stats.legend_total")}</LegendDot>
                <LegendDot className="bg-emerald-500">{t("stats.legend_success")}</LegendDot>
              </div>
            }
          />
          {bars.length === 0 ? (
            <EmptyState title={t("stats.no_activity")} />
          ) : (
            <BarChart
              data={bars}
              axis={
                <>
                  <span>{shortDate(dailyStats[0].date)}</span>
                  <span>{shortDate(dailyStats[dailyStats.length - 1].date)}</span>
                </>
              }
            />
          )}
        </CardBody>
      </Card>

      <Card>
        <CardBody>
          <CardHeader
            className="mb-3"
            title={t("stats.history")}
            desc={t("stats.history_desc", { limit: HISTORY_LIMIT })}
            aside={
              <span className="text-[10px] tabular-nums text-neutral-400">
                {t("stats.records", { count: resumeHistory.length })}
              </span>
            }
          />
          {resumeHistory.length === 0 ? (
            <EmptyState title={t("stats.no_history")} className="py-6" />
          ) : (
            <div className="max-h-72 divide-y divide-neutral-100 overflow-y-auto">
              {resumeHistory.map((record) => (
                <div key={record.id} className="flex items-center gap-3 py-2.5">
                  <span
                    className={
                      record.success
                        ? "flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-emerald-50 text-[10px] text-emerald-600"
                        : "flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-red-50 text-[10px] text-red-500"
                    }
                  >
                    {record.success ? "✓" : "✗"}
                  </span>
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="truncate text-xs font-medium text-neutral-700">
                        {record.agent_name}
                      </span>
                      <Badge tone={record.prompt_type === "goal" ? "violet" : "neutral"}>
                        {record.prompt_type === "goal"
                          ? t("stats.prompt_goal")
                          : t("stats.prompt_generic")}
                      </Badge>
                    </div>
                    {/* 成功的那条不用解释，失败的才需要看后端那句话 */}
                    <p className="truncate text-[10px] text-neutral-400">
                      {record.success ? record.working_dir : record.message || record.working_dir}
                    </p>
                  </div>
                  <span className="shrink-0 text-[10px] tabular-nums text-neutral-300">
                    {formatShortTime(record.created_at)}
                  </span>
                </div>
              ))}
            </div>
          )}
        </CardBody>
      </Card>
    </div>
  );
}

function StatCard({ label, value }: { label: string; value: number | string }) {
  return (
    <Card className="px-4 py-3.5">
      <p className="text-2xl font-semibold tabular-nums text-neutral-900">{value}</p>
      <p className="mt-0.5 truncate text-[11px] text-neutral-400">{label}</p>
    </Card>
  );
}

/** `2026-07-30` → `07-30`；坐标轴上年份是噪音 */
function shortDate(date: string): string {
  return date.length >= 10 ? date.slice(5) : date;
}
