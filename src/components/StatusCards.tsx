import { useAppStore } from "../stores/useAppStore";

/** 顶部状态卡片组 */
export function StatusCards() {
  const { monitorState } = useAppStore();
  const { status } = monitorState;

  const cards = [
    {
      label: "监控会话",
      value: status.sessions_total,
      color: "text-indigo-400",
      glow: "shadow-indigo-500/10",
      icon: "📡",
      accent: "from-indigo-500/20 to-transparent",
    },
    {
      label: "活跃中",
      value: status.sessions_active,
      color: "text-emerald-400",
      glow: "shadow-emerald-500/10",
      icon: "⚡",
      accent: "from-emerald-500/20 to-transparent",
    },
    {
      label: "已中断",
      value: status.sessions_interrupted,
      color: "text-red-400",
      glow: "shadow-red-500/10",
      icon: "⚠️",
      accent: "from-red-500/20 to-transparent",
    },
    {
      label: "自动续跑",
      value: status.total_resumes,
      color: "text-amber-400",
      glow: "shadow-amber-500/10",
      icon: "🔄",
      accent: "from-amber-500/20 to-transparent",
    },
    {
      label: "检测次数",
      value: status.total_detections,
      color: "text-purple-400",
      glow: "shadow-purple-500/10",
      icon: "🔍",
      accent: "from-purple-500/20 to-transparent",
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
        {cards.map((card, i) => (
          <div
            key={card.label}
            className={`group relative overflow-hidden rounded-xl border border-gray-800/60 bg-gray-900/70 p-4 backdrop-blur transition-all duration-300 hover:-translate-y-0.5 hover:border-gray-700/60 hover:shadow-xl ${card.glow}`}
            style={{ animationDelay: `${i * 60}ms` }}
          >
            {/* 顶部渐变装饰 */}
            <div
              className={`absolute inset-x-0 top-0 h-8 bg-gradient-to-b ${card.accent} opacity-60`}
            />
            <div className="relative flex items-center justify-between">
              <span className="text-[11px] font-medium text-gray-500">
                {card.label}
              </span>
              <span className="text-sm opacity-70 transition-transform duration-300 group-hover:scale-110">
                {card.icon}
              </span>
            </div>
            <div
              className={`relative mt-2 text-3xl font-bold tabular-nums tracking-tight ${card.color}`}
            >
              {card.value}
            </div>
          </div>
        ))}
      </div>

      {/* 状态条 */}
      <div className="flex items-center gap-5 rounded-xl border border-gray-800/60 bg-gray-900/50 px-4 py-2.5 text-xs text-gray-500 backdrop-blur">
        <div className="flex items-center gap-2">
          <span
            className={`h-2 w-2 rounded-full transition-colors duration-500 ${
              status.running
                ? "bg-emerald-400 text-emerald-400 animate-pulse-dot"
                : "bg-gray-600"
            }`}
          />
          <span
            className={`font-medium transition-colors duration-500 ${
              status.running ? "text-emerald-400" : "text-gray-500"
            }`}
          >
            {status.running ? "监控运行中" : "监控已停止"}
          </span>
        </div>
        <div className="h-3 w-px bg-gray-800" />
        <span className="flex items-center gap-1.5">
          <span className="text-gray-600">⏱</span>
          运行 {formatUptime(status.uptime_secs)}
        </span>
        <div className="h-3 w-px bg-gray-800" />
        <span className="flex items-center gap-1.5">
          <span className="text-gray-600">🕐</span>
          上次扫描 {status.last_scan_at ?? "尚未扫描"}
        </span>
        <div className="flex-1" />
        <span className="rounded-full bg-purple-500/10 px-2 py-0.5 text-[10px] text-purple-400/80">
          Goal 智能恢复已启用
        </span>
      </div>
    </div>
  );
}
