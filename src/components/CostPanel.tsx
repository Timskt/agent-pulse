import { useEffect, useMemo } from "react";
import { useI18n } from "../i18n";
import { baseName, cn, formatTokens, formatUsd } from "../lib/utils";
import {
  selectCostDaily,
  selectCostModels,
  selectCostProjects,
  selectRateForecast,
  selectStatus,
  selectUsageSummary,
  useAppStore,
} from "../stores/useAppStore";
import type { RateLimitForecast } from "../types";
import {
  BarChart,
  Card,
  CardBody,
  CardHeader,
  EmptyState,
  Tooltip,
  type BarDatum,
} from "./ui";

/** 与 store 里 `get_cost_daily { days: 14 }` / `get_cost_projects { days: 30 }` 对齐 */
const TREND_DAYS = 14;
const PROJECT_DAYS = 30;

/**
 * 花费页（痛点 #3「账单杀手」）
 *
 * 数据全来自 `~/.claude/projects/**\/*.jsonl` 里的 `usage` 字段，按模型计价，
 * 所以这里显示的是真实花费，不是估算。三块内容对应三个问题：
 * 花了多少、花在哪个项目、还有多久撞到限流。
 */
export function CostPanel() {
  const { t } = useI18n();
  const costDaily = useAppStore(selectCostDaily);
  const costProjects = useAppStore(selectCostProjects);
  const costModels = useAppStore(selectCostModels);
  const usageSummary = useAppStore(selectUsageSummary);
  const forecast = useAppStore(selectRateForecast);
  const status = useAppStore(selectStatus);
  const fetchCost = useAppStore((s) => s.fetchCost);

  useEffect(() => {
    void fetchCost();
  }, [fetchCost]);

  const bars = useMemo<BarDatum[]>(
    () =>
      costDaily.map((day) => ({
        key: day.date,
        value: day.cost_usd,
        tooltip: t("cost.bar_tooltip", {
          date: day.date,
          cost: formatUsd(day.cost_usd),
          tokens: formatTokens(day.total_tokens),
          requests: day.requests,
        }),
      })),
    [costDaily, t]
  );

  const rangeTotal = useMemo(
    () => costDaily.reduce((acc, day) => acc + day.cost_usd, 0),
    [costDaily]
  );
  const cacheHitRate = usageSummary && usageSummary.total_tokens > 0
    ? Math.round((usageSummary.cache_read_tokens / usageSummary.total_tokens) * 100)
    : 0;

  return (
    <div className="mx-auto max-w-3xl space-y-4">
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <CostStat label={t("cost.period_spend")} value={`$${formatUsd(usageSummary?.cost_usd ?? rangeTotal)}`} />
        <CostStat label={t("cost.period_tokens")} value={formatTokens(usageSummary?.total_tokens ?? 0)} />
        <CostStat label={t("cost.cache_hit_rate")} value={`${cacheHitRate}%`} />
        <CostStat label={t("cost.period_requests")} value={usageSummary?.requests ?? 0} />
      </div>
      <Card>
        <CardBody>
          <CardHeader
            className="mb-4"
            title={t("cost.trend")}
            desc={t("cost.trend_desc", { days: TREND_DAYS })}
            aside={
              <div className="text-right">
                <p className="text-xl font-semibold tabular-nums text-neutral-900">
                  ${formatUsd(status.cost_today)}
                </p>
                <p className="text-[10px] text-neutral-400">{t("cost.today")}</p>
              </div>
            }
          />
          {bars.length === 0 ? (
            <EmptyState title={t("cost.no_data")} />
          ) : (
            <>
              <BarChart
                data={bars}
                barClassName="bg-blue-500/70 group-hover:bg-blue-500"
                axis={
                  <>
                    <span>{shortDate(costDaily[0].date)}</span>
                    <span>{shortDate(costDaily[costDaily.length - 1].date)}</span>
                  </>
                }
              />
              <p className="mt-2 text-[10px] tabular-nums text-neutral-400">
                {t("cost.range_total", { days: TREND_DAYS, cost: formatUsd(rangeTotal) })}
              </p>
            </>
          )}
        </CardBody>
      </Card>

      <Card>
        <CardBody>
          <CardHeader
            className="mb-3"
            title={t("cost.forecast")}
            desc={t("cost.forecast_desc")}
          />
          <Forecast forecast={forecast} />
        </CardBody>
      </Card>

      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <Card>
          <CardBody>
            <CardHeader className="mb-3" title={t("cost.projects")} desc={t("cost.projects_desc", { days: PROJECT_DAYS })} />
            <RankList rows={costProjects.map((project) => ({ key: project.project, label: baseName(project.project), detail: `${t("cost.tokens", { tokens: formatTokens(project.total_tokens) })} · ${t("cost.requests", { count: project.requests })}`, value: project.cost_usd, tooltip: project.project }))} />
          </CardBody>
        </Card>
        <Card>
          <CardBody>
            <CardHeader className="mb-3" title={t("cost.models")} desc={t("cost.models_desc", { days: PROJECT_DAYS })} />
            <RankList rows={costModels.map((model) => ({ key: model.model, label: model.model, detail: `${t("cost.tokens", { tokens: formatTokens(model.total_tokens) })} · ${t("cost.requests", { count: model.requests })}`, value: model.cost_usd }))} />
          </CardBody>
        </Card>
      </div>
    </div>
  );
}

function Forecast({ forecast }: { forecast: RateLimitForecast | null }) {
  const { t } = useI18n();

  if (!forecast || forecast.budget_tokens === 0) {
    return <p className="text-[11px] text-neutral-400">{t("cost.budget_unset")}</p>;
  }

  const percent = Math.min(Math.max(forecast.used_percent, 0), 100);
  // 越接近额度颜色越急，和预算告警的语义一致
  const tone =
    percent >= 80 ? "bg-red-500" : percent >= 60 ? "bg-amber-500" : "bg-neutral-800/70";

  return (
    <div>
      <div className="flex items-baseline justify-between gap-3">
        <span className="text-[11px] text-neutral-600">
          {t("cost.window_used", {
            hours: forecast.window_hours,
            percent: Math.round(forecast.used_percent),
          })}
        </span>
        <span className="shrink-0 text-[10px] tabular-nums text-neutral-400">
          {t("cost.window_tokens", {
            used: formatTokens(forecast.used_tokens),
            budget: formatTokens(forecast.budget_tokens),
          })}
        </span>
      </div>
      <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-neutral-100">
        <div className={cn("h-full rounded-full", tone)} style={{ width: `${percent}%` }} />
      </div>
      <p className="mt-2 text-[10px] text-neutral-400">
        {forecast.minutes_to_limit === null
          ? t("cost.no_limit")
          : t("cost.minutes_left", { minutes: forecast.minutes_to_limit })}
      </p>
    </div>
  );
}

function CostStat({ label, value }: { label: string; value: number | string }) {
  return <Card className="px-4 py-3"><p className="text-lg font-semibold tabular-nums text-neutral-900">{value}</p><p className="mt-0.5 truncate text-[10px] text-neutral-400">{label}</p></Card>;
}

function RankList({ rows }: { rows: readonly { key: string; label: string; detail: string; value: number; tooltip?: string }[] }) {
  const { t } = useI18n();
  const max = rows.reduce((acc, row) => Math.max(acc, row.value), 0);
  if (rows.length === 0) return <EmptyState title={t("cost.no_data")} className="py-6" />;
  return <div className="space-y-3">{rows.map((row) => <div key={row.key}><div className="flex items-baseline justify-between gap-3"><Tooltip content={row.tooltip ?? row.label}><span className="truncate font-mono text-[11px] text-neutral-700">{row.label}</span></Tooltip><span className="shrink-0 text-[11px] font-medium tabular-nums text-neutral-800">${formatUsd(row.value)}</span></div><div className="mt-1 h-1 overflow-hidden rounded-full bg-neutral-100"><div className="h-full rounded-full bg-neutral-800/70" style={{ width: `${max === 0 ? 0 : (row.value / max) * 100}%` }} /></div><p className="mt-1 text-[10px] tabular-nums text-neutral-400">{row.detail}</p></div>)}</div>;
}

/** `2026-07-30` → `07-30` */
function shortDate(date: string): string {
  return date.length >= 10 ? date.slice(5) : date;
}
