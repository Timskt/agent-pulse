import { useI18n } from "../i18n";
import { formatUsd } from "../lib/utils";
import { selectStatus, useAppStore } from "../stores/useAppStore";
import { Card } from "./ui";

/**
 * 顶部指标卡
 *
 * 六个格子按「先看有没有事、再看花了多少钱」排：等你回应排在最前，
 * 因为痛点 #1 就是「不知道 Agent 在等我」。
 */
export function StatusCards() {
  const { t } = useI18n();
  const status = useAppStore(selectStatus);

  const cards = [
    { key: "pending", label: t("metric.pending"), value: status.pending_attention, alert: true },
    { key: "sessions", label: t("metric.sessions"), value: status.sessions_total },
    { key: "active", label: t("metric.active"), value: status.sessions_active },
    { key: "interrupted", label: t("metric.interrupted"), value: status.sessions_interrupted },
    { key: "resumes", label: t("metric.resumes"), value: status.total_resumes },
    {
      key: "cost",
      label: t("metric.cost_today"),
      value: `$${formatUsd(status.cost_today)}`,
    },
  ];

  return (
    <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-6">
      {cards.map((card) => (
        <Card key={card.key} className="min-w-0 px-3 py-3.5 sm:px-4">
          <p
            title={String(card.value)}
            className={
              // 有人在等你的时候这个数字要跳出来，其余保持克制
              card.alert && card.value !== 0
                ? "min-w-0 break-all text-xl font-semibold tabular-nums text-red-500 sm:text-2xl"
                : "min-w-0 break-all text-xl font-semibold tabular-nums text-neutral-900 sm:text-2xl"
            }
          >
            {card.value}
          </p>
          <p className="mt-0.5 truncate text-[11px] text-neutral-400">{card.label}</p>
        </Card>
      ))}
    </div>
  );
}
