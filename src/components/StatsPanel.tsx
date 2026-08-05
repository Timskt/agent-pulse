import { useEffect, useMemo } from "react";
import { useI18n } from "../i18n";
import { shortDate } from "../lib/utils";
import {
  selectDailyStats,
  selectStatsOverview,
  selectTotals,
  useAppStore,
} from "../stores/useAppStore";
import { TrendPanel } from "./TrendPanel";
import {
  BarChart,
  Card,
  CardBody,
  CardHeader,
  EmptyState,
  ExportButton,
  LegendDot,
  type BarDatum,
} from "./ui";

/** 与 store 里 `get_stats { days: 30 }` 保持一致 */
const DAYS = 30;

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
  const overview = useAppStore(selectStatsOverview);
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
        label: shortDate(day.date),
        tooltip: t("stats.bar_tooltip", {
          date: day.date,
          detections: day.total_detections,
          resumes: day.total_resumes,
          successful: day.successful_resumes,
        }),
      })),
    [dailyStats, t]
  );

  const peak = useMemo(() => bars.reduce((acc, b) => Math.max(acc, b.value), 0), [bars]);

  return (
    <div className="mx-auto max-w-3xl space-y-4">
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <StatCard label={t("stats.detections")} value={overview?.total_detections ?? detections} />
        <StatCard label={t("stats.resumes")} value={overview?.total_resumes ?? resumes} />
        <StatCard label={t("stats.successful")} value={overview?.successful_resumes ?? successful} />
        <StatCard label={t("stats.success_rate")} value={`${successRate}%`} />
      </div>

      {/* 趋势排在累计数之后、活动图之前：累计数是「一共」，趋势是「最近怎么样」，
          后者才是用户打开这一页真正想知道的，所以放在视线第二站而不是最末 */}
      <TrendPanel />

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
                <ExportButton
                  command="export_stats"
                  args={{ days: DAYS }}
                  label={t("export.stats")}
                />
              </div>
            }
          />
          {bars.length === 0 ? (
            <EmptyState title={t("stats.no_activity")} />
          ) : (
            <BarChart data={bars} peakLabel={t("stats.peak", { count: peak })} />
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

