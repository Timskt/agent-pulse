import { useState } from "react";
import { useAppStore } from "../stores/useAppStore";
import type { AppConfig } from "../types";

/** 配置面板 */
export function ConfigPanel() {
  const { config, updateConfig } = useAppStore();
  const [saved, setSaved] = useState(false);

  if (!config) {
    return (
      <div className="flex h-64 items-center justify-center text-gray-500">
        加载配置中...
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
    <div className="space-y-6">
      {/* 基础设置 */}
      <section className="rounded-xl border border-gray-800 bg-gray-900/80 p-5">
        <h3 className="mb-4 text-sm font-semibold text-gray-200">检测设置</h3>
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
      </section>

      {/* 开关设置 */}
      <section className="rounded-xl border border-gray-800 bg-gray-900/80 p-5">
        <h3 className="mb-4 text-sm font-semibold text-gray-200">行为设置</h3>
        <div className="space-y-3">
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
      </section>

      {/* 关键词设置 */}
      <section className="rounded-xl border border-gray-800 bg-gray-900/80 p-5">
        <h3 className="mb-4 text-sm font-semibold text-gray-200">
          关键词触发
        </h3>
        <div className="space-y-4">
          <div>
            <label className="mb-1 block text-xs text-gray-400">
              中断触发关键词（逗号分隔）
            </label>
            <textarea
              className="w-full rounded-lg border border-gray-700 bg-gray-800 px-3 py-2 text-sm text-gray-200 outline-none focus:border-pulse-500 resize-none"
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
            <label className="mb-1 block text-xs text-gray-400">
              完成标记（逗号分隔）
            </label>
            <textarea
              className="w-full rounded-lg border border-gray-700 bg-gray-800 px-3 py-2 text-sm text-gray-200 outline-none focus:border-pulse-500 resize-none"
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
        </div>
      </section>

      {/* 续跑提示词 */}
      <section className="rounded-xl border border-gray-800 bg-gray-900/80 p-5">
        <h3 className="mb-4 text-sm font-semibold text-gray-200">续跑提示词</h3>
        <textarea
          className="w-full rounded-lg border border-gray-700 bg-gray-800 px-3 py-2 text-sm text-gray-200 outline-none focus:border-pulse-500 resize-none"
          rows={3}
          value={config.resume_prompt}
          onChange={(e) => set("resume_prompt", e.target.value)}
        />
        <p className="mt-1 text-[10px] text-gray-600">
          检测到中断后，自动发送给 Agent 的续跑指令内容
        </p>
      </section>

      {/* 保存按钮 */}
      <div className="flex justify-end">
        <button
          onClick={handleSave}
          className={`rounded-lg px-6 py-2.5 text-sm font-medium text-white transition-all ${
            saved
              ? "bg-emerald-600"
              : "bg-pulse-600 hover:bg-pulse-500"
          }`}
        >
          {saved ? "✓ 已保存" : "保存配置"}
        </button>
      </div>
    </div>
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
      <label className="mb-1 block text-xs text-gray-400">{label}</label>
      <input
        type="number"
        min={min}
        max={max}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        className="w-full rounded-lg border border-gray-700 bg-gray-800 px-3 py-2 text-sm text-gray-200 outline-none focus:border-pulse-500"
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
    <div className="flex items-center justify-between rounded-lg border border-gray-800 bg-gray-800/40 px-4 py-3">
      <div>
        <span className="text-sm text-gray-200">{label}</span>
        <p className="mt-0.5 text-[10px] text-gray-500">{desc}</p>
      </div>
      <button
        onClick={() => onChange(!checked)}
        className={`relative h-6 w-11 rounded-full transition-colors ${
          checked ? "bg-pulse-600" : "bg-gray-700"
        }`}
      >
        <span
          className={`absolute top-0.5 h-5 w-5 rounded-full bg-white transition-transform ${
            checked ? "translate-x-[22px]" : "translate-x-0.5"
          }`}
        />
      </button>
    </div>
  );
}
