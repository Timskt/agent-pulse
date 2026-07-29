import { useAppStore } from "../stores/useAppStore";

/** 顶部状态卡片组 */
export function StatusCards() {
  const { monitorState } = useAppStore();
  const { status } = monitorState;

  const cards = [
    {
      label: "监控会话",
      value: status.sessions_total,
      color: "text-pulse-400",
      icon: "📡",
    },
    {
      label: "活跃中",
      value: status.sessions_active,
      color: "text-emerald-400",
      icon: "⚡",
    },
    {
      label: "已中断",
      value: status.sessions_interrupted,
      color: "text-red-400",
      icon: "⚠️",
    },
    {
      label: "自动续跑",
      value: status.total_resumes,
      color: "text-amber-400",
      icon: "🔄",
    },
    {
      label: "检测次数",
      value: status.total_detections,
      color: "text-purple-400",
      icon: "🔍",
    },
  ];

  const formatUptime = (secs: number) => {
    const h = Math.floor(secs / 3600);
    const m = Math.floor((secs % 3600) / 60);
    const s = secs % 60;
    if (h > 0) return `${h}h ${m}m`;
    if (m > 0) return `${m}m ${s}s`;
    return `${s}s`;
  };

  return (
    <div className="space-y-3">
      <div className="grid grid-cols-5 gap-3">
        {cards.map((card) => (
          <div
            key={card.label}
            className="rounded-xl border border-gray-800 bg-gray-900/80 p-4 backdrop-blur"
          >
            <div className="flex items-center justify-between">
              <span className="text-xs text-gray-500">{card.label}</span>
              <span className="text-sm">{card.icon}</span>
            </div>
            <div className={`mt-2 text-3xl font-bold ${card.color}`}>
              {card.value}
            </div>
          </div>
        ))}
      </div>

      {/* 状态条 */}
      <div className="flex items-center gap-4 rounded-lg border border-gray-800 bg-gray-900/60 px-4 py-2 text-xs text-gray-500">
        <div className="flex items-center gap-2">
          <span
            className={`h-2 w-2 rounded-full ${
              status.running
                ? "bg-emerald-400 animate-pulse-dot"
                : "bg-gray-600"
            }`}
          />
          <span className={status.running ? "text-emerald-400" : ""}>
            {status.running ? "监控运行中" : "监控已停止"}
          </span>
        </div>
        <span>运行时长: {formatUptime(status.uptime_secs)}</span>
        <span>
          上次扫描: {status.last_scan_at ?? "尚未扫描"}
        </span>
      </div>
    </div>
  );
}
