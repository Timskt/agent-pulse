import { useEffect } from "react";
import { useAppStore } from "../stores/useAppStore";

/** 统计分析面板 */
export function StatsPanel() {
  const { dailyStats, resumeHistory, totals, fetchStats } = useAppStore();

  useEffect(() => {
    fetchStats();
  }, [fetchStats]);

  const [totalScans, totalDetections, totalResumes] = totals ?? [0, 0, 0];
  const successRate =
    resumeHistory.length > 0
      ? Math.round(
          (resumeHistory.filter((r) => r.success).length /
            resumeHistory.length) *
            100
        )
      : 0;

  return (
    <div className="space-y-5">
      {/* 总览卡片 */}
      <div className="grid grid-cols-4 gap-3">
        <StatCard
          label="总扫描次数"
          value={totalScans}
          icon="🔍"
          color="text-indigo-400"
          bg="from-indigo-500/10"
        />
        <StatCard
          label="中断检测"
          value={totalDetections}
          icon="⚠️"
          color="text-amber-400"
          bg="from-amber-500/10"
        />
        <StatCard
          label="自动续跑"
          value={totalResumes}
          icon="🚀"
          color="text-emerald-400"
          bg="from-emerald-500/10"
        />
        <StatCard
          label="续跑成功率"
          value={`${successRate}%`}
          icon="📊"
          color="text-purple-400"
          bg="from-purple-500/10"
        />
      </div>

      {/* 30 天趋势图（纯 CSS 柱状图） */}
      <section className="rounded-xl border border-gray-800/60 bg-gray-900/70 p-5 backdrop-blur">
        <div className="mb-4 flex items-center gap-2.5">
          <span className="flex h-7 w-7 items-center justify-center rounded-lg bg-gray-800/80 text-sm">
            📈
          </span>
          <div>
            <h3 className="text-sm font-semibold text-gray-200">
              30 天活动趋势
            </h3>
            <p className="text-[10px] text-gray-500">
              每日检测与续跑统计
            </p>
          </div>
        </div>

        {dailyStats.length === 0 ? (
          <div className="flex h-32 items-center justify-center text-xs text-gray-600">
            暂无历史数据，开始监控后将自动记录
          </div>
        ) : (
          <div className="flex h-36 items-end gap-[2px] overflow-x-auto pb-1">
            {dailyStats.map((day) => {
              const maxVal = Math.max(
                ...dailyStats.map((d) => d.total_resumes + d.total_detections),
                1
              );
              const height = Math.max(
                ((day.total_resumes + day.total_detections) / maxVal) * 100,
                4
              );
              return (
                <div
                  key={day.date}
                  className="group relative flex min-w-[8px] flex-1 flex-col items-center justify-end"
                  title={`${day.date}: 检测 ${day.total_detections} / 续跑 ${day.total_resumes}`}
                >
                  <div
                    className="w-full rounded-t-sm bg-gradient-to-t from-indigo-600/80 to-purple-500/60 transition-all duration-200 group-hover:from-indigo-500 group-hover:to-purple-400"
                    style={{ height: `${height}%` }}
                  />
                  {/* 续跑成功部分 */}
                  {day.successful_resumes > 0 && (
                    <div
                      className="absolute bottom-0 w-full rounded-t-sm bg-emerald-500/40"
                      style={{
                        height: `${(day.successful_resumes / maxVal) * 100}%`,
                      }}
                    />
                  )}
                </div>
              );
            })}
          </div>
        )}
      </section>

      {/* 续跑历史 */}
      <section className="rounded-xl border border-gray-800/60 bg-gray-900/70 p-5 backdrop-blur">
        <div className="mb-4 flex items-center gap-2.5">
          <span className="flex h-7 w-7 items-center justify-center rounded-lg bg-gray-800/80 text-sm">
            📋
          </span>
          <div>
            <h3 className="text-sm font-semibold text-gray-200">续跑记录</h3>
            <p className="text-[10px] text-gray-500">最近 50 条续跑操作</p>
          </div>
        </div>

        {resumeHistory.length === 0 ? (
          <div className="flex h-24 items-center justify-center text-xs text-gray-600">
            暂无续跑记录
          </div>
        ) : (
          <div className="max-h-64 space-y-1.5 overflow-y-auto pr-1">
            {resumeHistory.map((record) => (
              <div
                key={record.id}
                className="flex items-center gap-3 rounded-lg border border-gray-800/40 bg-gray-800/30 px-3 py-2 transition-colors hover:bg-gray-800/50"
              >
                <span
                  className={`flex h-5 w-5 shrink-0 items-center justify-center rounded-full text-[10px] ${
                    record.success
                      ? "bg-emerald-400/10 text-emerald-400"
                      : "bg-red-400/10 text-red-400"
                  }`}
                >
                  {record.success ? "✓" : "✗"}
                </span>
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="text-xs font-medium text-gray-300">
                      {record.agent_name}
                    </span>
                    <span
                      className={`rounded px-1.5 py-0.5 text-[9px] font-medium ${
                        record.prompt_type === "goal"
                          ? "bg-purple-400/10 text-purple-400"
                          : "bg-indigo-400/10 text-indigo-400"
                      }`}
                    >
                      {record.prompt_type === "goal" ? "Goal恢复" : "通用"}
                    </span>
                  </div>
                  <p className="truncate text-[10px] text-gray-500">
                    {record.working_dir}
                  </p>
                </div>
                <span className="shrink-0 text-[10px] tabular-nums text-gray-600">
                  {formatTime(record.created_at)}
                </span>
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}

function StatCard({
  label,
  value,
  icon,
  color,
  bg,
}: {
  label: string;
  value: number | string;
  icon: string;
  color: string;
  bg: string;
}) {
  return (
    <div
      className={`relative overflow-hidden rounded-xl border border-gray-800/60 bg-gradient-to-br ${bg} to-transparent p-4 backdrop-blur transition-all duration-200 hover:border-gray-700/60`}
    >
      <div className="flex items-center justify-between">
        <span className="text-lg">{icon}</span>
      </div>
      <p className={`mt-2 text-2xl font-bold tabular-nums ${color}`}>
        {value}
      </p>
      <p className="mt-0.5 text-[10px] text-gray-500">{label}</p>
    </div>
  );
}

function formatTime(iso: string): string {
  try {
    const d = new Date(iso);
    return `${d.getMonth() + 1}/${d.getDate()} ${d.getHours().toString().padStart(2, "0")}:${d.getMinutes().toString().padStart(2, "0")}`;
  } catch {
    return iso;
  }
}
