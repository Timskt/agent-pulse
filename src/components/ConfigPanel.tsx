import { useState } from "react";
import { useAppStore } from "../stores/useAppStore";
import type { AppConfig, WebhookConfig, AiJudgeConfig, CustomAdapterConfig } from "../types";

/** 配置面板 */
export function ConfigPanel() {
  const { config, updateConfig } = useAppStore();
  const [saved, setSaved] = useState(false);

  if (!config) {
    return (
      <div className="flex h-64 flex-col items-center justify-center gap-3 text-neutral-400">
        <span className="h-5 w-5 animate-spin rounded-full border-2 border-neutral-200 border-t-neutral-500" />
        <span className="text-xs">Loading configuration...</span>
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
    <div className="mx-auto max-w-3xl space-y-4">
      {/* 检测设置 */}
      <Section title="Detection" desc="中断检测的灵敏度与轮询频率">
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
      <Section title="Behavior" desc="检测到中断后的自动行为">
        <div className="space-y-2">
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
      <Section title="System" desc="系统托盘、开机自启与语言">
        <div className="space-y-2">
          <div className="flex items-center justify-between rounded-lg border border-neutral-100 bg-neutral-50/50 px-4 py-3">
            <div>
              <span className="text-xs font-medium text-neutral-700">系统托盘常驻</span>
              <p className="mt-0.5 text-[10px] text-neutral-400">
                关闭窗口时最小化到托盘，右键托盘图标可控制监控
              </p>
            </div>
            <span className="rounded-full bg-emerald-50 px-2.5 py-1 text-[10px] font-medium text-emerald-600">
              Enabled
            </span>
          </div>
          <div className="flex items-center justify-between rounded-lg border border-neutral-100 bg-neutral-50/50 px-4 py-3">
            <div>
              <span className="text-xs font-medium text-neutral-700">开机自启</span>
              <p className="mt-0.5 text-[10px] text-neutral-400">
                系统登录时自动启动 AgentPulse（通过系统设置管理）
              </p>
            </div>
            <span className="rounded-full bg-neutral-100 px-2.5 py-1 text-[10px] font-medium text-neutral-500">
              OS Settings
            </span>
          </div>
          <div className="flex items-center justify-between rounded-lg border border-neutral-100 bg-neutral-50/50 px-4 py-3">
            <div>
              <span className="text-xs font-medium text-neutral-700">界面语言</span>
              <p className="mt-0.5 text-[10px] text-neutral-400">切换应用界面显示语言</p>
            </div>
            <select
              value={config.language}
              onChange={(e) => set("language", e.target.value)}
              className="rounded-md border border-neutral-200 bg-white px-3 py-1.5 text-xs text-neutral-700 outline-none focus:border-neutral-400"
            >
              <option value="zh">中文</option>
              <option value="en">English</option>
            </select>
          </div>
        </div>
      </Section>

      {/* 续跑提示词 */}
      <Section title="Resume Prompts" desc="检测到中断后自动发送给 Agent 的指令">
        <div className="space-y-4">
          <div>
            <label className="mb-1.5 block text-[11px] font-medium text-neutral-500">
              通用续跑提示词
            </label>
            <textarea
              className="w-full resize-none rounded-lg border border-neutral-200 bg-white px-3 py-2.5 text-xs leading-relaxed text-neutral-700 outline-none transition-colors focus:border-neutral-400"
              rows={2}
              value={config.resume_prompt}
              onChange={(e) => set("resume_prompt", e.target.value)}
            />
          </div>
          <div className="rounded-lg border border-neutral-200 bg-neutral-50/50 p-3">
            <label className="mb-1.5 flex items-center gap-2 text-[11px] font-medium text-neutral-500">
              Goal 恢复专用提示词
              <span className="rounded bg-neutral-100 px-1.5 py-0.5 text-[9px] text-neutral-400">
                检测到活跃 Goal 时自动使用
              </span>
            </label>
            <textarea
              className="w-full resize-none rounded-lg border border-neutral-200 bg-white px-3 py-2.5 text-xs leading-relaxed text-neutral-700 outline-none transition-colors focus:border-neutral-400"
              rows={2}
              value={config.goal_resume_prompt}
              onChange={(e) => set("goal_resume_prompt", e.target.value)}
            />
            <p className="mt-1.5 text-[10px] leading-relaxed text-neutral-400">
              当 Agent 输出中检测到 goal / objective / turn_budget 等关键词时，
              判定存在活跃 Goal，续跑时将使用此专用提示词确保目标被主动恢复
            </p>
          </div>
        </div>
      </Section>

      {/* 关键词设置 */}
      <Section title="Keywords" desc="自定义中断检测与完成判定的关键词">
        <div className="space-y-4">
          <div>
            <label className="mb-1.5 block text-[11px] font-medium text-neutral-500">
              中断触发关键词（逗号分隔）
            </label>
            <textarea
              className="w-full resize-none rounded-lg border border-neutral-200 bg-white px-3 py-2.5 text-xs leading-relaxed text-neutral-700 outline-none transition-colors focus:border-neutral-400"
              rows={2}
              value={config.custom_keywords.join(", ")}
              onChange={(e) =>
                set(
                  "custom_keywords",
                  e.target.value.split(",").map((s) => s.trim()).filter(Boolean)
                )
              }
            />
            <p className="mt-1 text-[10px] text-neutral-400">
              输出中出现这些关键词且没有完成标记时，触发续跑
            </p>
          </div>
          <div>
            <label className="mb-1.5 block text-[11px] font-medium text-neutral-500">
              完成标记（逗号分隔）
            </label>
            <textarea
              className="w-full resize-none rounded-lg border border-neutral-200 bg-white px-3 py-2.5 text-xs leading-relaxed text-neutral-700 outline-none transition-colors focus:border-neutral-400"
              rows={2}
              value={config.completion_markers.join(", ")}
              onChange={(e) =>
                set(
                  "completion_markers",
                  e.target.value.split(",").map((s) => s.trim()).filter(Boolean)
                )
              }
            />
            <p className="mt-1 text-[10px] text-neutral-400">
              出现完成标记时不会触发续跑，防止重复执行
            </p>
          </div>
          <div>
            <label className="mb-1.5 block text-[11px] font-medium text-neutral-500">
              Goal 检测关键词（逗号分隔）
            </label>
            <textarea
              className="w-full resize-none rounded-lg border border-neutral-200 bg-white px-3 py-2.5 text-xs leading-relaxed text-neutral-700 outline-none transition-colors focus:border-neutral-400"
              rows={2}
              value={config.goal_keywords.join(", ")}
              onChange={(e) =>
                set(
                  "goal_keywords",
                  e.target.value.split(",").map((s) => s.trim()).filter(Boolean)
                )
              }
            />
            <p className="mt-1 text-[10px] text-neutral-400">
              匹配到这些关键词时判定存在活跃 Goal，使用 Goal 专用提示词续跑
            </p>
          </div>
        </div>
      </Section>

      {/* Webhook 通知 */}
      <Section title="Webhook" desc="中断/续跑事件推送到 Slack、Discord 或自定义端点">
        <WebhookSection config={config} set={set} />
      </Section>

      {/* AI 智能判断 */}
      <Section title="AI Judge" desc="使用 LLM 分析 Agent 输出，减少误判">
        <AiJudgeSection config={config} set={set} />
      </Section>

      {/* 自定义适配器 */}
      <Section title="Custom Adapters" desc="添加自定义 Agent 进程匹配规则">
        <CustomAdapterSection config={config} set={set} />
      </Section>

      {/* 保存按钮 */}
      <div className="flex justify-end pb-4">
        <button
          onClick={handleSave}
          className={`rounded-lg px-6 py-2 text-xs font-medium transition-all active:scale-[0.98] ${
            saved
              ? "bg-emerald-600 text-white"
              : "bg-neutral-900 text-white hover:bg-neutral-700"
          }`}
        >
          {saved ? "Saved" : "Save Changes"}
        </button>
      </div>
    </div>
  );
}

/** 配置区块容器 */
function Section({
  title,
  desc,
  children,
}: {
  title: string;
  desc: string;
  children: React.ReactNode;
}) {
  return (
    <section className="rounded-lg border border-neutral-200 bg-white p-5">
      <div className="mb-4">
        <h3 className="text-xs font-semibold text-neutral-800">{title}</h3>
        <p className="mt-0.5 text-[10px] text-neutral-400">{desc}</p>
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
      <label className="mb-1.5 block text-[11px] font-medium text-neutral-500">
        {label}
      </label>
      <input
        type="number"
        min={min}
        max={max}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-xs tabular-nums text-neutral-700 outline-none transition-colors focus:border-neutral-400"
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
    <div className="flex items-center justify-between rounded-lg border border-neutral-100 bg-neutral-50/50 px-4 py-3 transition-colors hover:bg-neutral-50">
      <div>
        <span className="text-xs font-medium text-neutral-700">{label}</span>
        <p className="mt-0.5 text-[10px] text-neutral-400">{desc}</p>
      </div>
      <button
        onClick={() => onChange(!checked)}
        className={`relative h-5 w-9 shrink-0 rounded-full transition-colors duration-200 ${
          checked ? "bg-neutral-900" : "bg-neutral-200"
        }`}
      >
        <span
          className={`absolute top-0.5 h-4 w-4 rounded-full bg-white shadow-sm transition-transform duration-200 ${
            checked ? "translate-x-[18px]" : "translate-x-0.5"
          }`}
        />
      </button>
    </div>
  );
}

/** Webhook 配置区 */
function WebhookSection({
  config,
  set,
}: {
  config: AppConfig;
  set: <K extends keyof AppConfig>(key: K, value: AppConfig[K]) => void;
}) {
  const { testWebhook } = useAppStore();
  const [testResult, setTestResult] = useState<string | null>(null);
  const wh = config.webhook;

  const setWh = (partial: Partial<WebhookConfig>) => {
    set("webhook", { ...wh, ...partial });
  };

  return (
    <div className="space-y-3">
      <ToggleField
        label="启用 Webhook"
        desc="检测到中断/续跑时发送 HTTP 通知"
        checked={wh.enabled}
        onChange={(v) => setWh({ enabled: v })}
      />
      {wh.enabled && (
        <div className="space-y-3 rounded-lg border border-neutral-100 bg-neutral-50/50 p-3">
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="mb-1 block text-[10px] font-medium text-neutral-400">
                Webhook URL
              </label>
              <input
                type="url"
                placeholder="https://hooks.slack.com/..."
                value={wh.url}
                onChange={(e) => setWh({ url: e.target.value })}
                className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-xs text-neutral-700 outline-none focus:border-neutral-400"
              />
            </div>
            <div>
              <label className="mb-1 block text-[10px] font-medium text-neutral-400">
                通知类型
              </label>
              <select
                value={wh.provider}
                onChange={(e) => setWh({ provider: e.target.value })}
                className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-xs text-neutral-700 outline-none focus:border-neutral-400"
              >
                <option value="slack">Slack</option>
                <option value="discord">Discord</option>
                <option value="custom">自定义</option>
              </select>
            </div>
          </div>
          <div>
            <label className="mb-1 block text-[10px] font-medium text-neutral-400">
              消息模板（支持 {'{agent_name}'} {'{session_id}'} {'{message}'}）
            </label>
            <textarea
              rows={2}
              value={wh.template}
              onChange={(e) => setWh({ template: e.target.value })}
              className="w-full resize-none rounded-lg border border-neutral-200 bg-white px-3 py-2 text-xs text-neutral-700 outline-none focus:border-neutral-400"
            />
          </div>
          <div className="flex flex-wrap gap-2">
            <NotifyToggle label="中断时通知" checked={wh.notify_on_interrupt} onChange={(v) => setWh({ notify_on_interrupt: v })} />
            <NotifyToggle label="续跑时通知" checked={wh.notify_on_resume} onChange={(v) => setWh({ notify_on_resume: v })} />
            <NotifyToggle label="完成时通知" checked={wh.notify_on_complete} onChange={(v) => setWh({ notify_on_complete: v })} />
          </div>
          <button
            onClick={async () => {
              const res = await testWebhook();
              setTestResult(res);
              setTimeout(() => setTestResult(null), 3000);
            }}
            className="rounded-md border border-neutral-200 px-3 py-1.5 text-[10px] font-medium text-neutral-600 transition-colors hover:bg-neutral-100"
          >
            Test Send
          </button>
          {testResult && (
            <p className="text-[10px] text-neutral-400">{testResult}</p>
          )}
        </div>
      )}
    </div>
  );
}

function NotifyToggle({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      onClick={() => onChange(!checked)}
      className={`rounded-full border px-2.5 py-1 text-[10px] font-medium transition-colors ${
        checked
          ? "border-emerald-200 bg-emerald-50 text-emerald-600"
          : "border-neutral-200 bg-white text-neutral-400"
      }`}
    >
      {label}
    </button>
  );
}

/** AI 智能判断配置区 */
function AiJudgeSection({
  config,
  set,
}: {
  config: AppConfig;
  set: <K extends keyof AppConfig>(key: K, value: AppConfig[K]) => void;
}) {
  const ai = config.ai_judge;
  const setAi = (partial: Partial<AiJudgeConfig>) => {
    set("ai_judge", { ...ai, ...partial });
  };

  return (
    <div className="space-y-3">
      <ToggleField
        label="启用 AI 辅助判断"
        desc="使用 LLM 分析 Agent 输出，降低误判率（需配置 API Key）"
        checked={ai.enabled}
        onChange={(v) => setAi({ enabled: v })}
      />
      {ai.enabled && (
        <div className="space-y-3 rounded-lg border border-neutral-100 bg-neutral-50/50 p-3">
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="mb-1 block text-[10px] font-medium text-neutral-400">API 端点</label>
              <input
                type="url"
                value={ai.api_url}
                onChange={(e) => setAi({ api_url: e.target.value })}
                className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-xs text-neutral-700 outline-none focus:border-neutral-400"
              />
            </div>
            <div>
              <label className="mb-1 block text-[10px] font-medium text-neutral-400">模型</label>
              <input
                value={ai.model}
                onChange={(e) => setAi({ model: e.target.value })}
                className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-xs text-neutral-700 outline-none focus:border-neutral-400"
              />
            </div>
          </div>
          <div>
            <label className="mb-1 block text-[10px] font-medium text-neutral-400">API Key</label>
            <input
              type="password"
              placeholder="sk-..."
              value={ai.api_key}
              onChange={(e) => setAi({ api_key: e.target.value })}
              className="w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-xs text-neutral-700 outline-none focus:border-neutral-400"
            />
          </div>
          <div>
            <label className="mb-1 block text-[10px] font-medium text-neutral-400">
              置信度阈值: {ai.confidence_threshold}%
            </label>
            <input
              type="range"
              min={50}
              max={99}
              value={ai.confidence_threshold}
              onChange={(e) => setAi({ confidence_threshold: Number(e.target.value) })}
              className="w-full accent-neutral-900"
            />
            <p className="mt-1 text-[10px] text-neutral-400">
              AI 判断中断概率超过此值才触发续跑，越高越保守
            </p>
          </div>
        </div>
      )}
    </div>
  );
}

/** 自定义适配器配置区 */
function CustomAdapterSection({
  config,
  set,
}: {
  config: AppConfig;
  set: <K extends keyof AppConfig>(key: K, value: AppConfig[K]) => void;
}) {
  const adapters = config.custom_adapters;

  const addAdapter = () => {
    set("custom_adapters", [
      ...adapters,
      { name: "", process_pattern: "", session_file_pattern: "" },
    ]);
  };

  const updateAdapter = (idx: number, partial: Partial<CustomAdapterConfig>) => {
    const updated = adapters.map((a, i) => (i === idx ? { ...a, ...partial } : a));
    set("custom_adapters", updated);
  };

  const removeAdapter = (idx: number) => {
    set("custom_adapters", adapters.filter((_, i) => i !== idx));
  };

  return (
    <div className="space-y-3">
      {adapters.length === 0 && (
        <p className="text-[11px] text-neutral-400">
          暂无自定义适配器，内置支持 Claude Code / Codex CLI / OpenCode
        </p>
      )}
      {adapters.map((adapter, idx) => (
        <div key={idx} className="space-y-2 rounded-lg border border-neutral-100 bg-neutral-50/50 p-3">
          <div className="flex items-center gap-2">
            <input
              placeholder="适配器名称"
              value={adapter.name}
              onChange={(e) => updateAdapter(idx, { name: e.target.value })}
              className="flex-1 rounded-lg border border-neutral-200 bg-white px-3 py-1.5 text-xs text-neutral-700 outline-none focus:border-neutral-400"
            />
            <button
              onClick={() => removeAdapter(idx)}
              className="rounded-md px-2 py-1.5 text-xs text-red-500 transition-colors hover:bg-red-50"
            >
              Remove
            </button>
          </div>
          <div className="grid grid-cols-2 gap-2">
            <input
              placeholder="进程匹配（如 aider）"
              value={adapter.process_pattern}
              onChange={(e) => updateAdapter(idx, { process_pattern: e.target.value })}
              className="rounded-lg border border-neutral-200 bg-white px-3 py-1.5 text-xs text-neutral-700 outline-none focus:border-neutral-400"
            />
            <input
              placeholder="会话文件路径模式（可选）"
              value={adapter.session_file_pattern}
              onChange={(e) => updateAdapter(idx, { session_file_pattern: e.target.value })}
              className="rounded-lg border border-neutral-200 bg-white px-3 py-1.5 text-xs text-neutral-700 outline-none focus:border-neutral-400"
            />
          </div>
        </div>
      ))}
      <button
        onClick={addAdapter}
        className="w-full rounded-lg border border-dashed border-neutral-200 py-2 text-[11px] text-neutral-400 transition-colors hover:border-neutral-400 hover:text-neutral-600"
      >
        + Add Adapter
      </button>
    </div>
  );
}
