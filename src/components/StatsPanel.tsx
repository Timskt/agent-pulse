import { useEffect } from "react";
import { useAppStore } from "../stores/useAppStore";

/** 统计分析面板 */
export function StatsPanel() {
  const { dailyStats, resumeHistory, totals, fetchStats } = useAppStore();

  useEffect(() => {
    fetchStats();
  }, [fetchStats]);

  const [totalDetections, totalResumes, successfulResumes] = totals ?? [0, 0, 0];
  const successRate =
    totalResumes > 0
      ? Math.round((successfulResumes / totalResumes) * 100)
      : 0;

  return (
    <div className="mx-auto max-w-3xl space-y-4">
      {/* 总览卡片 */}
      <div className="grid grid-cols-4 gap-3">
        <StatCard label="中断检测" value={totalDetections} />
        <StatCard label="自动续跑" value={totalResumes} />
        <StatCard label="续跑成功" value={successfulResumes} />
        <StatCard label="成功率" value={`${successRate}%`} />
      </div>

      {/* 30 天趋势图 */}
      <section className="rounded-lg border border-neutral-200 bg-white p-5">
        <div className="mb-4 flex items-center justify-between">
          <div>
            <h3 className="text-xs font-semibold text-neutral-800">30-Day Activity</h3>
            <p className="mt-0.5 text-[10px] text-neutral-400">每日检测与续跑统计</p>
          </div>
          <div className="flex items-center gap-3 text-[9px] text-neutral-400">
            <span className="flex items-center gap-1">
              <span className="h-2 w-2 rounded-sm bg-neutral-800" /> Total
            </span>
            <span className="flex items-center gap-1">
              <span className="h-2 w-2 rounded-sm bg-emerald-500" /> Success
            </span>
          </div>
        </div>

        {dailyStats.length === 0 ? (
          <div className="flex h-32 items-center justify-center text-[11px] text-neutral-300">
            暂无历史数据，开始监控后将自动记录
          </div>
        ) : (
          <div className="flex h-32 items-end gap-[2px] overflow-x-auto pb-1">
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
                    className="w-full rounded-t-sm bg-neutral-800/80 transition-colors group-hover:bg-neutral-900"
                    style={{ height: `${height}%` }}
                  />
                  {day.successful_resumes > 0 && (
                    <div
                      className="absolute bottom-0 w-full rounded-t-sm bg-emerald-500/60"
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
      <section className="rounded-lg border border-neutral-200 bg-white p-5">
        <div className="mb-4 flex items-center justify-between">
          <div>
            <h3 className="text-xs font-semibold text-neutral-800">Resume History</h3>
            <p className="mt-0.5 text-[10px] text-neutral-400">最近 50 条续跑操作</p>
          </div>
          <span className="text-[10px] tabular-nums text-neutral-300">
            {resumeHistory.length} records
          </span>
        </div>

        {resumeHistory.length === 0 ? (
          <div className="flex h-24 items-center justify-center text-[11px] text-neutral-300">
            暂无续跑记录
          </div>
        ) : (
          <div className="max-h-64 divide-y divide-neutral-50 overflow-y-auto">
            {resumeHistory.map((record) => (
              <div
                key={record.id}
                className="flex items-center gap-3 px-1 py-2.5 transition-colors hover:bg-neutral-50/50"
              >
                <span
                  className={`flex h-5 w-5 shrink-0 items-center justify-center rounded-full text-[10px] font-medium ${
                    record.success
                      ? "bg-emerald-50 text-emerald-600"
                      : "bg-red-50 text-red-500"
                  }`}
                >
                  {record.success ? "✓" : "✗"}
                </span>
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="text-xs font-medium text-neutral-700">
                      {record.agent_name}
                    </span>
                    <span
                      className={`rounded px-1.5 py-0.5 text-[9px] font-medium ${
                        record.prompt_type === "goal"
                          ? "bg-violet-50 text-violet-600"
                          : "bg-neutral-100 text-neutral-500"
                      }`}
                    >
                      {record.prompt_type === "goal" ? "Goal" : "Normal"}
                    </span>
                  </div>
                  <p className="truncate text-[10px] text-neutral-400">
                    {record.working_dir}
                  </p>
                </div>
                <span className="shrink-0 text-[10px] tabular-nums text-neutral-300">
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

function StatCard({ label, value }: { label: string; value: number | string }) {
  return (
    <div className="rounded-lg border border-neutral-200 bg-white px-4 py-3.5">
      <p className="text-2xl font-semibold tabular-nums text-neutral-900">{value}</p>
      <p className="mt-0.5 text-[11px] text-neutral-400">{label}</p>
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
