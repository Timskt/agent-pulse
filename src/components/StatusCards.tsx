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
    <div className="grid grid-cols-3 gap-3 lg:grid-cols-6">
      {cards.map((card) => (
        <Card key={card.key} className="px-4 py-3.5">
          <p
            className={
              // 有人在等你的时候这个数字要跳出来，其余保持克制
              card.alert && card.value !== 0
                ? "text-2xl font-semibold tabular-nums text-red-500"
                : "text-2xl font-semibold tabular-nums text-neutral-900"
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
