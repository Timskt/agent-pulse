import { useState } from "react";
import { useAppStore } from "../stores/useAppStore";
import type { AppConfig } from "../types";

/** 配置面板 */
export function ConfigPanel() {
  const { config, updateConfig } = useAppStore();
  const [saved, setSaved] = useState(false);

  if (!config) {
    return (
      <div className="flex h-64 flex-col items-center justify-center gap-3 text-gray-500">
        <span className="h-6 w-6 animate-spin rounded-full border-2 border-gray-700 border-t-indigo-500" />
        <span className="text-sm">加载配置中...</span>
      </div>
    );
  }

  const handleSave = async () => {
    await updateConfig(config);
    setSaved(true);
    setTimeout(() => setSaved(false), 2000);
  };

  const set = <K extends keyof AppConfig>(key: K, value: AppConfig[K]) => {
    useAppStore.setState({ config: { ...config, [key]: value } });
  };

  return (
    <div className="mx-auto max-w-3xl space-y-5">
      {/* 检测设置 */}
      <Section title="检测设置" icon="🔬" desc="控制中断检测的灵敏度与轮询频率">
        <div className="grid grid-cols-2 gap-4">
          <NumberField
            label="轮询间隔（秒）"
            value={config.poll_interval_secs}
            min={3}
            max={300}
            onChange={(v) => set("poll_interval_secs", v)}
          />
          <NumberField
            label="空闲超时判定（秒）"
            value={config.idle_timeout_secs}
            min={10}
            max={600}
            onChange={(v) => set("idle_timeout_secs", v)}
          />
          <NumberField
            label="连续无活动阈值（次）"
            value={config.idle_threshold}
            min={1}
            max={10}
            onChange={(v) => set("idle_threshold", v)}
          />
          <NumberField
            label="最大续跑次数"
            value={config.max_resume_count}
            min={1}
            max={20}
            onChange={(v) => set("max_resume_count", v)}
          />
          <NumberField
            label="续跑冷却时间（秒）"
            value={config.resume_cooldown_secs}
            min={5}
            max={600}
            onChange={(v) => set("resume_cooldown_secs", v)}
          />
        </div>
      </Section>

      {/* 行为设置 */}
      <Section title="行为设置" icon="🎛️" desc="控制检测到中断后的自动行为">
        <div className="space-y-2.5">
          <ToggleField
            label="自动续跑"
            desc="检测到中断后自动发送续跑指令（关闭则仅记录通知）"
            checked={config.auto_resume_enabled}
            onChange={(v) => set("auto_resume_enabled", v)}
          />
          <ToggleField
            label="启动时立即扫描"
            desc="应用启动后自动开始监控"
            checked={config.check_on_startup}
            onChange={(v) => set("check_on_startup", v)}
          />
          <ToggleField
            label="自动跟随最新会话"
            desc="多窗口时慎用，可能误触发非目标任务"
            checked={config.auto_follow_latest}
            onChange={(v) => set("auto_follow_latest", v)}
          />
          <ToggleField
            label="心跳日志"
            desc="每次轮询输出心跳日志，用于诊断检测问题"
            checked={config.heartbeat_log}
            onChange={(v) => set("heartbeat_log", v)}
          />
        </div>
      </Section>

      {/* 系统设置 */}
      <Section title="系统设置" icon="🖥️" desc="系统托盘与开机自启">
        <div className="space-y-2.5">
          <div className="flex items-center justify-between rounded-lg border border-gray-800/50 bg-gray-800/30 px-4 py-3">
            <div>
              <span className="text-sm font-medium text-gray-200">系统托盘常驻</span>
              <p className="mt-0.5 text-[10px] text-gray-500">
                关闭窗口时最小化到托盘，右键托盘图标可控制监控
              </p>
            </div>
            <span className="rounded-full bg-emerald-400/10 px-2.5 py-1 text-[10px] font-medium text-emerald-400">
              ✓ 已启用
            </span>
          </div>
          <div className="flex items-center justify-between rounded-lg border border-gray-800/50 bg-gray-800/30 px-4 py-3">
            <div>
              <span className="text-sm font-medium text-gray-200">开机自启</span>
              <p className="mt-0.5 text-[10px] text-gray-500">
                系统登录时自动启动 AgentPulse（通过系统设置管理）
              </p>
            </div>
            <span className="rounded-full bg-indigo-400/10 px-2.5 py-1 text-[10px] font-medium text-indigo-400">
              系统设置中配置
            </span>
          </div>
          <div className="flex items-center justify-between rounded-lg border border-gray-800/50 bg-gray-800/30 px-4 py-3">
            <div>
              <span className="text-sm font-medium text-gray-200">跨平台续跑</span>
              <p className="mt-0.5 text-[10px] text-gray-500">
                macOS (AppleScript) · Windows (PowerShell) · Linux (xdotool)
              </p>
            </div>
            <span className="rounded-full bg-purple-400/10 px-2.5 py-1 text-[10px] font-medium text-purple-400">
              v0.2.0 新增
            </span>
          </div>
        </div>
      </Section>

      {/* 续跑提示词 */}
      <Section title="续跑提示词" icon="💬" desc="检测到中断后自动发送给 Agent 的指令">
        <div className="space-y-4">
          <div>
            <label className="mb-1.5 block text-xs font-medium text-gray-400">
              通用续跑提示词
            </label>
            <textarea
              className="w-full rounded-lg border border-gray-700/60 bg-gray-800/60 px-3 py-2.5 text-sm text-gray-200 outline-none transition-colors focus:border-indigo-500/60 focus:ring-1 focus:ring-indigo-500/20 resize-none"
              rows={2}
              value={config.resume_prompt}
              onChange={(e) => set("resume_prompt", e.target.value)}
            />
          </div>
          <div className="rounded-lg border border-purple-500/20 bg-purple-500/5 p-3">
            <label className="mb-1.5 flex items-center gap-1.5 text-xs font-medium text-purple-300">
              🎯 Goal 恢复专用提示词
              <span className="rounded bg-purple-400/10 px-1.5 py-0.5 text-[9px] text-purple-400">
                检测到活跃 Goal 时自动使用
              </span>
            </label>
            <textarea
              className="w-full rounded-lg border border-purple-500/20 bg-gray-800/60 px-3 py-2.5 text-sm text-gray-200 outline-none transition-colors focus:border-purple-500/60 focus:ring-1 focus:ring-purple-500/20 resize-none"
              rows={2}
              value={config.goal_resume_prompt}
              onChange={(e) => set("goal_resume_prompt", e.target.value)}
            />
            <p className="mt-1.5 text-[10px] leading-relaxed text-gray-500">
              当 Agent 输出中检测到 goal / objective / turn_budget 等关键词时，
              判定存在活跃 Goal，续跑时将使用此专用提示词确保目标被主动恢复
            </p>
          </div>
        </div>
      </Section>

      {/* 关键词设置 */}
      <Section title="关键词触发" icon="🏷️" desc="自定义中断检测与完成判定的关键词">
        <div className="space-y-4">
          <div>
            <label className="mb-1.5 block text-xs font-medium text-gray-400">
              中断触发关键词（逗号分隔）
            </label>
            <textarea
              className="w-full rounded-lg border border-gray-700/60 bg-gray-800/60 px-3 py-2.5 text-sm text-gray-200 outline-none transition-colors focus:border-indigo-500/60 focus:ring-1 focus:ring-indigo-500/20 resize-none"
              rows={2}
              value={config.custom_keywords.join(", ")}
              onChange={(e) =>
                set(
                  "custom_keywords",
                  e.target.value
                    .split(",")
                    .map((s) => s.trim())
                    .filter(Boolean)
                )
              }
            />
            <p className="mt-1 text-[10px] text-gray-600">
              输出中出现这些关键词且没有完成标记时，触发续跑
            </p>
          </div>
          <div>
            <label className="mb-1.5 block text-xs font-medium text-gray-400">
              完成标记（逗号分隔）
            </label>
            <textarea
              className="w-full rounded-lg border border-gray-700/60 bg-gray-800/60 px-3 py-2.5 text-sm text-gray-200 outline-none transition-colors focus:border-indigo-500/60 focus:ring-1 focus:ring-indigo-500/20 resize-none"
              rows={2}
              value={config.completion_markers.join(", ")}
              onChange={(e) =>
                set(
                  "completion_markers",
                  e.target.value
                    .split(",")
                    .map((s) => s.trim())
                    .filter(Boolean)
                )
              }
            />
            <p className="mt-1 text-[10px] text-gray-600">
              出现完成标记时不会触发续跑，防止重复执行
            </p>
          </div>
          <div>
            <label className="mb-1.5 block text-xs font-medium text-gray-400">
              Goal 检测关键词（逗号分隔）
            </label>
            <textarea
              className="w-full rounded-lg border border-purple-500/20 bg-gray-800/60 px-3 py-2.5 text-sm text-gray-200 outline-none transition-colors focus:border-purple-500/60 focus:ring-1 focus:ring-purple-500/20 resize-none"
              rows={2}
              value={config.goal_keywords.join(", ")}
              onChange={(e) =>
                set(
                  "goal_keywords",
                  e.target.value
                    .split(",")
                    .map((s) => s.trim())
                    .filter(Boolean)
                )
              }
            />
            <p className="mt-1 text-[10px] text-gray-600">
              匹配到这些关键词时判定存在活跃 Goal，使用 Goal 专用提示词续跑
            </p>
          </div>
        </div>
      </Section>

      {/* 保存按钮 */}
      <div className="flex justify-end pb-4">
        <button
          onClick={handleSave}
          className={`flex items-center gap-2 rounded-xl px-7 py-2.5 text-sm font-medium text-white shadow-lg transition-all duration-300 active:scale-95 ${
            saved
              ? "bg-gradient-to-r from-emerald-600 to-teal-600 shadow-emerald-500/25"
              : "bg-gradient-to-r from-indigo-600 to-purple-600 shadow-indigo-500/25 hover:shadow-indigo-500/40 hover:brightness-110"
          }`}
        >
          {saved ? "✓ 已保存" : "保存配置"}
        </button>
      </div>
    </div>
  );
}

/** 配置区块容器 */
function Section({
  title,
  icon,
  desc,
  children,
}: {
  title: string;
  icon: string;
  desc: string;
  children: React.ReactNode;
}) {
  return (
    <section className="rounded-xl border border-gray-800/60 bg-gray-900/70 p-5 backdrop-blur transition-colors hover:border-gray-700/50">
      <div className="mb-4 flex items-center gap-2.5">
        <span className="flex h-7 w-7 items-center justify-center rounded-lg bg-gray-800/80 text-sm">
          {icon}
        </span>
        <div>
          <h3 className="text-sm font-semibold text-gray-200">{title}</h3>
          <p className="text-[10px] text-gray-500">{desc}</p>
        </div>
      </div>
      {children}
    </section>
  );
}

function NumberField({
  label,
  value,
  min,
  max,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  onChange: (v: number) => void;
}) {
  return (
    <div>
      <label className="mb-1.5 block text-xs font-medium text-gray-400">
        {label}
      </label>
      <input
        type="number"
        min={min}
        max={max}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        className="w-full rounded-lg border border-gray-700/60 bg-gray-800/60 px-3 py-2.5 text-sm tabular-nums text-gray-200 outline-none transition-colors focus:border-indigo-500/60 focus:ring-1 focus:ring-indigo-500/20"
      />
    </div>
  );
}

function ToggleField({
  label,
  desc,
  checked,
  onChange,
}: {
  label: string;
  desc: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div className="flex items-center justify-between rounded-lg border border-gray-800/50 bg-gray-800/30 px-4 py-3 transition-colors hover:bg-gray-800/50">
      <div>
        <span className="text-sm font-medium text-gray-200">{label}</span>
        <p className="mt-0.5 text-[10px] text-gray-500">{desc}</p>
      </div>
      <button
        onClick={() => onChange(!checked)}
        className={`relative h-6 w-11 shrink-0 rounded-full transition-all duration-300 ${
          checked
            ? "bg-gradient-to-r from-indigo-600 to-purple-600 shadow-md shadow-indigo-500/30"
            : "bg-gray-700"
        }`}
      >
        <span
          className={`absolute top-0.5 h-5 w-5 rounded-full bg-white shadow-sm transition-transform duration-300 ${
            checked ? "translate-x-[22px]" : "translate-x-0.5"
          }`}
        />
      </button>
    </div>
  );
}
