import { useAppStore } from "../stores/useAppStore";

/** 顶部状态指标 */
export function StatusCards() {
  const { monitorState } = useAppStore();
  const { status } = monitorState;

  const cards = [
    { label: "Sessions", value: status.sessions_total },
    { label: "Active", value: status.sessions_active },
    { label: "Interrupted", value: status.sessions_interrupted },
    { label: "Resumes", value: status.total_resumes },
  ];

  return (
    <div className="grid grid-cols-4 gap-3">
      {cards.map((card) => (
        <div
          key={card.label}
          className="rounded-lg border border-neutral-200 bg-white px-4 py-3.5"
        >
          <p className="text-2xl font-semibold tabular-nums text-neutral-900">
            {card.value}
          </p>
          <p className="mt-0.5 text-[11px] text-neutral-400">{card.label}</p>
        </div>
      ))}
    </div>
  );
}
