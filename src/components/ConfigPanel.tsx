import { useEffect, useState } from "react";
import { useI18n } from "../i18n";
import { useNotice, type Notice } from "../lib/useNotice";
import { cn } from "../lib/utils";
import { selectConfig, useAppStore } from "../stores/useAppStore";
import type {
  AiJudgeConfig,
  AppConfig,
  CostConfig,
  CustomAdapterConfig,
  NotificationConfig,
  RemoteConfig,
  WebhookConfig,
  WebhookProvider,
} from "../types";
import {
  Badge,
  Button,
  Card,
  CardBody,
  CardHeader,
  Chip,
  CommaListInput,
  EmptyState,
  Field,
  NumberInput,
  Select,
  Slider,
  TextArea,
  TextInput,
  ToggleRow,
  type SelectOption,
} from "./ui";

/**
 * 设置页
 *
 * 三处返工：文案全部走 i18n（原来一半标题是英文、说明是中文，最土的搭配都在这一页）；
 * 控件全部换成基元（本文件曾自带 Section / NumberField / ToggleField / NotifyToggle
 * 四个私有版本）；补上 v1.1–v1.3 新增的提醒、花费、手机看板和三组关键词字段。
 */

type Setter = <K extends keyof AppConfig>(key: K, value: AppConfig[K]) => void;

interface SectionProps {
  config: AppConfig;
  set: Setter;
}

const LANGUAGES: readonly SelectOption<string>[] = [
  { value: "zh", label: "中文" },
  { value: "en", label: "English" },
];

export function ConfigPanel() {
  const { t } = useI18n();
  const config = useAppStore(selectConfig);
  const updateConfig = useAppStore((s) => s.updateConfig);
  const { notice, show } = useNotice(5000);
  const [saving, setSaving] = useState(false);

  if (!config) return <EmptyState className="h-64" title={t("common.loading")} />;

  // 改动先落在 store 里：切到别的标签页再回来不会丢，按保存才写盘
  const set: Setter = (key, value) =>
    useAppStore.setState({ config: { ...config, [key]: value } });

  const save = async () => {
    setSaving(true);
    const result = await updateConfig(config);
    setSaving(false);
    // 写盘失败时把后端那句话原样显示，界面不能装作已保存
    show({ ok: result.ok, message: result.ok ? t("common.saved") : result.message });
  };

  return (
    <div className="mx-auto max-w-3xl space-y-4">
      <Section title={t("cfg.detection")} desc={t("cfg.detection.desc")}>
        <div className="grid grid-cols-2 gap-4">
          <Field label={t("cfg.poll_interval")}>
            <NumberInput
              value={config.poll_interval_secs}
              min={3}
              max={300}
              onValueChange={(v) => set("poll_interval_secs", v)}
            />
          </Field>
          <Field label={t("cfg.idle_timeout")}>
            <NumberInput
              value={config.idle_timeout_secs}
              min={10}
              max={600}
              onValueChange={(v) => set("idle_timeout_secs", v)}
            />
          </Field>
          <Field label={t("cfg.idle_threshold")}>
            <NumberInput
              value={config.idle_threshold}
              min={1}
              max={10}
              onValueChange={(v) => set("idle_threshold", v)}
            />
          </Field>
          <Field label={t("cfg.max_resume")}>
            <NumberInput
              value={config.max_resume_count}
              min={1}
              max={20}
              onValueChange={(v) => set("max_resume_count", v)}
            />
          </Field>
          <Field label={t("cfg.cooldown")}>
            <NumberInput
              value={config.resume_cooldown_secs}
              min={5}
              max={600}
              onValueChange={(v) => set("resume_cooldown_secs", v)}
            />
          </Field>
        </div>
      </Section>

      <Section title={t("cfg.behavior")} desc={t("cfg.behavior.desc")}>
        <div className="space-y-1.5">
          <ToggleRow
            label={t("cfg.auto_resume")}
            desc={t("cfg.auto_resume.desc")}
            checked={config.auto_resume_enabled}
            onCheckedChange={(v) => set("auto_resume_enabled", v)}
          />
          <ToggleRow
            label={t("cfg.startup_scan")}
            desc={t("cfg.startup_scan.desc")}
            checked={config.check_on_startup}
            onCheckedChange={(v) => set("check_on_startup", v)}
          />
          <ToggleRow
            label={t("cfg.follow_latest")}
            desc={t("cfg.follow_latest.desc")}
            checked={config.auto_follow_latest}
            onCheckedChange={(v) => set("auto_follow_latest", v)}
          />
          <ToggleRow
            label={t("cfg.heartbeat")}
            desc={t("cfg.heartbeat.desc")}
            checked={config.heartbeat_log}
            onCheckedChange={(v) => set("heartbeat_log", v)}
          />
        </div>
      </Section>

      <NotifySection config={config} set={set} onNotice={show} />
      <CostSection config={config} set={set} />
      <Section title={t("cfg.prompts")} desc={t("cfg.prompts.desc")}>
        <div className="space-y-4">
          <Field label={t("cfg.generic_prompt")}>
            <TextArea
              value={config.resume_prompt}
              onChange={(e) => set("resume_prompt", e.target.value)}
            />
          </Field>
          <div className="rounded-lg border border-neutral-100 bg-neutral-50/60 p-3">
            <Field
              label={
                <span className="flex items-center gap-2">
                  {t("cfg.goal_prompt")}
                  <Badge tone="violet">{t("cfg.goal_badge")}</Badge>
                </span>
              }
              hint={t("cfg.goal_prompt.hint")}
            >
              <TextArea
                value={config.goal_resume_prompt}
                onChange={(e) => set("goal_resume_prompt", e.target.value)}
              />
            </Field>
          </div>
        </div>
      </Section>

      <Section title={t("cfg.keywords")} desc={t("cfg.keywords.desc")}>
        <div className="space-y-4">
          <Field label={t("cfg.kw_interrupt")} hint={t("cfg.kw_interrupt.hint")}>
            <CommaListInput
              value={config.custom_keywords}
              onValueChange={(v) => set("custom_keywords", v)}
            />
          </Field>
          <Field label={t("cfg.kw_completion")} hint={t("cfg.kw_completion.hint")}>
            <CommaListInput
              value={config.completion_markers}
              onValueChange={(v) => set("completion_markers", v)}
            />
          </Field>
          <Field label={t("cfg.kw_goal")} hint={t("cfg.kw_goal.hint")}>
            <CommaListInput
              value={config.goal_keywords}
              onValueChange={(v) => set("goal_keywords", v)}
            />
          </Field>
          {/* 下面三组决定注意力分级怎么判，v1.1 新增 */}
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
            <Field label={t("cfg.kw_input")}>
              <CommaListInput
                rows={3}
                value={config.input_keywords}
                onValueChange={(v) => set("input_keywords", v)}
              />
            </Field>
            <Field label={t("cfg.kw_rate_limit")}>
              <CommaListInput
                rows={3}
                value={config.rate_limit_keywords}
                onValueChange={(v) => set("rate_limit_keywords", v)}
              />
            </Field>
            <Field label={t("cfg.kw_error")}>
              <CommaListInput
                rows={3}
                value={config.error_keywords}
                onValueChange={(v) => set("error_keywords", v)}
              />
            </Field>
          </div>
        </div>
      </Section>

      <WebhookSection config={config} set={set} onNotice={show} />
      <AiSection config={config} set={set} />
      <RemoteSection config={config} set={set} />
      <AdapterSection config={config} set={set} />

      <Section title={t("cfg.system")} desc={t("cfg.system.desc")}>
        <div className="space-y-1.5">
          <ToggleRow
            label={t("cfg.tray")}
            desc={t("cfg.tray.desc")}
            aside={<Badge tone="green">{t("cfg.on")}</Badge>}
          />
          <ToggleRow
            label={t("cfg.autostart")}
            desc={t("cfg.autostart.desc")}
            aside={<Badge>{t("cfg.os_managed")}</Badge>}
          />
          <ToggleRow
            label={t("cfg.language")}
            desc={t("cfg.language.desc")}
            aside={
              <Select
                className="w-28"
                value={config.language}
                options={LANGUAGES}
                onValueChange={(v) => set("language", v)}
              />
            }
          />
        </div>
      </Section>
      {/* 保存条贴在底部：这一页很长，滚到哪儿都能存 */}
      <div className="sticky bottom-0 -mx-1 flex items-center justify-end gap-3 border-t border-neutral-100 bg-white/90 px-1 py-3 backdrop-blur">
        {notice && (
          <span
            className={cn(
              "truncate text-[11px]",
              notice.ok ? "text-emerald-600" : "text-red-500"
            )}
          >
            {notice.message}
          </span>
        )}
        <Button size="lg" variant="primary" disabled={saving} onClick={save}>
          {t("common.save")}
        </Button>
      </div>
    </div>
  );
}

/** 配置分区：卡片 + 标题 + 说明，全站一个样子 */
function Section({
  title,
  desc,
  children,
}: {
  title: React.ReactNode;
  desc?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <Card>
      <CardBody>
        <CardHeader className="mb-4" title={title} desc={desc} />
        {children}
      </CardBody>
    </Card>
  );
}

/** 分区里的次级容器，用来放「开关打开后才出现」的一组字段 */
function Nested({ children }: { children: React.ReactNode }) {
  return (
    <div className="space-y-3 rounded-lg border border-neutral-100 bg-neutral-50/60 p-3">
      {children}
    </div>
  );
}
/** 提醒（v1.1 感知层的开关都在这里） */
function NotifySection({
  config,
  set,
  onNotice,
}: SectionProps & { onNotice: (notice: Notice) => void }) {
  const { t } = useI18n();
  const testNotify = useAppStore((s) => s.testNotify);
  const n = config.notification;
  const setN = (partial: Partial<NotificationConfig>) => set("notification", { ...n, ...partial });

  // 逐条写死而不是拿字段名拼，是为了让 TS 真的检查这些字段存在
  const events = [
    {
      label: t("cfg.notify_needs_input"),
      on: n.on_needs_input,
      toggle: () => setN({ on_needs_input: !n.on_needs_input }),
    },
    {
      label: t("cfg.notify_completed"),
      on: n.on_completed,
      toggle: () => setN({ on_completed: !n.on_completed }),
    },
    {
      label: t("cfg.notify_rate_limited"),
      on: n.on_rate_limited,
      toggle: () => setN({ on_rate_limited: !n.on_rate_limited }),
    },
    {
      label: t("cfg.notify_error"),
      on: n.on_error,
      toggle: () => setN({ on_error: !n.on_error }),
    },
    {
      label: t("cfg.notify_resumed"),
      on: n.on_resumed,
      toggle: () => setN({ on_resumed: !n.on_resumed }),
    },
  ];

  return (
    <Section title={t("cfg.notify")} desc={t("cfg.notify.desc")}>
      <div className="space-y-3">
        <ToggleRow
          label={t("cfg.notify_enabled")}
          desc={t("cfg.notify_enabled.desc")}
          checked={n.enabled}
          onCheckedChange={(v) => setN({ enabled: v })}
        />
        {n.enabled && (
          <Nested>
            <div className="flex flex-wrap gap-2">
              {events.map((event) => (
                <Chip key={event.label} active={event.on} onClick={event.toggle}>
                  {event.label}
                </Chip>
              ))}
            </div>
            <ToggleRow
              label={t("cfg.notify_sound")}
              desc={t("cfg.notify_sound.desc")}
              checked={n.sound_enabled}
              onCheckedChange={(v) => setN({ sound_enabled: v })}
            />
            {n.sound_enabled && (
              <Field label={t("cfg.notify_volume", { value: n.sound_volume })}>
                <Slider
                  value={n.sound_volume}
                  min={0}
                  max={100}
                  onValueChange={(v) => setN({ sound_volume: v })}
                />
              </Field>
            )}
            <ToggleRow
              label={t("cfg.notify_badge")}
              desc={t("cfg.notify_badge.desc")}
              checked={n.tray_badge}
              onCheckedChange={(v) => setN({ tray_badge: v })}
            />
            <Field label={t("cfg.notify_throttle")}>
              <NumberInput
                value={n.throttle_secs}
                min={0}
                max={3600}
                onValueChange={(v) => setN({ throttle_secs: v })}
              />
            </Field>
            <Button
              size="sm"
              onClick={async () => {
                const result = await testNotify();
                if (result.message) onNotice(result);
              }}
            >
              {t("cfg.notify_test")}
            </Button>
          </Nested>
        )}
      </div>
    </Section>
  );
}
/** 花费与预算（v1.2 洞察层） */
function CostSection({ config, set }: SectionProps) {
  const { t } = useI18n();
  const c = config.cost;
  const setC = (partial: Partial<CostConfig>) => set("cost", { ...c, ...partial });

  return (
    <Section title={t("cfg.cost")} desc={t("cfg.cost.desc")}>
      <div className="space-y-3">
        <ToggleRow
          label={t("cfg.cost_enabled")}
          desc={t("cfg.cost_enabled.desc")}
          checked={c.enabled}
          onCheckedChange={(v) => setC({ enabled: v })}
        />
        {c.enabled && (
          <Nested>
            <div className="grid grid-cols-2 gap-4">
              <Field label={t("cfg.daily_budget")}>
                <NumberInput
                  value={c.daily_budget_usd}
                  min={0}
                  max={10_000}
                  step={1}
                  onValueChange={(v) => setC({ daily_budget_usd: v })}
                />
              </Field>
              <Field label={t("cfg.session_budget")}>
                <NumberInput
                  value={c.session_budget_usd}
                  min={0}
                  max={1_000}
                  step={0.5}
                  onValueChange={(v) => setC({ session_budget_usd: v })}
                />
              </Field>
              <Field label={t("cfg.alert_percent")}>
                <NumberInput
                  value={c.alert_at_percent}
                  min={10}
                  max={100}
                  onValueChange={(v) => setC({ alert_at_percent: v })}
                />
              </Field>
              <Field label={t("cfg.rate_window")}>
                <NumberInput
                  value={c.rate_limit_window_hours}
                  min={1}
                  max={24}
                  onValueChange={(v) => setC({ rate_limit_window_hours: v })}
                />
              </Field>
              <Field label={t("cfg.rate_budget")} className="col-span-2">
                <NumberInput
                  value={c.rate_limit_token_budget}
                  min={0}
                  step={100_000}
                  onValueChange={(v) => setC({ rate_limit_token_budget: v })}
                />
              </Field>
            </div>
          </Nested>
        )}
      </div>
    </Section>
  );
}

/** 各渠道的地址示例。URL 不是文案，两种语言下都长这样，所以不进 i18n 表 */
const WEBHOOK_URL_HINT: Record<WebhookProvider, string> = {
  slack: "https://hooks.slack.com/services/…",
  discord: "https://discord.com/api/webhooks/…",
  ntfy: "https://ntfy.sh",
  bark: "https://api.day.app",
  custom: "https://example.com/hook",
};

/** 外部推送（v1.3 P0：ntfy / Bark 让人在手机上也收得到） */
function WebhookSection({
  config,
  set,
  onNotice,
}: SectionProps & { onNotice: (notice: Notice) => void }) {
  const { t } = useI18n();
  const testWebhook = useAppStore((s) => s.testWebhook);
  const wh = config.webhook;
  const setWh = (partial: Partial<WebhookConfig>) => set("webhook", { ...wh, ...partial });
  const [testing, setTesting] = useState(false);

  const providers: readonly SelectOption<WebhookProvider>[] = [
    { value: "slack", label: "Slack" },
    { value: "discord", label: "Discord" },
    { value: "ntfy", label: "ntfy" },
    { value: "bark", label: "Bark" },
    { value: "custom", label: t("cfg.webhook_custom") },
  ];
  // 这两家是「主题 / 设备 Key + 服务器」的形式，其余只有一个完整 URL
  const usesTopic = wh.provider === "ntfy" || wh.provider === "bark";

  const events = [
    {
      label: t("cfg.webhook_on_interrupt"),
      on: wh.notify_on_interrupt,
      toggle: () => setWh({ notify_on_interrupt: !wh.notify_on_interrupt }),
    },
    {
      label: t("cfg.webhook_on_resume"),
      on: wh.notify_on_resume,
      toggle: () => setWh({ notify_on_resume: !wh.notify_on_resume }),
    },
    {
      label: t("cfg.webhook_on_complete"),
      on: wh.notify_on_complete,
      toggle: () => setWh({ notify_on_complete: !wh.notify_on_complete }),
    },
  ];

  return (
    <Section title={t("cfg.webhook")} desc={t("cfg.webhook.desc")}>
      <div className="space-y-3">
        <ToggleRow
          label={t("cfg.webhook_enabled")}
          desc={t("cfg.webhook_enabled.desc")}
          checked={wh.enabled}
          onCheckedChange={(v) => setWh({ enabled: v })}
        />
        {wh.enabled && (
          <Nested>
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <Field label={t("cfg.webhook_provider")}>
                <Select
                  value={wh.provider}
                  options={providers}
                  onValueChange={(v) => setWh({ provider: v })}
                />
              </Field>
              <Field label={t("cfg.webhook_url")}>
                <TextInput
                  value={wh.url}
                  placeholder={WEBHOOK_URL_HINT[wh.provider]}
                  onChange={(e) => setWh({ url: e.target.value })}
                />
              </Field>
            </div>
            {usesTopic && (
              <Field label={t("cfg.webhook_topic")} hint={t("cfg.webhook_topic.hint")}>
                <TextInput
                  value={wh.topic}
                  onChange={(e) => setWh({ topic: e.target.value })}
                />
              </Field>
            )}
            <Field label={t("cfg.webhook_template")} hint={t("cfg.webhook_template.hint")}>
              <TextArea
                value={wh.template}
                onChange={(e) => setWh({ template: e.target.value })}
              />
            </Field>
            <div className="flex flex-wrap gap-2">
              {events.map((event) => (
                <Chip key={event.label} active={event.on} onClick={event.toggle}>
                  {event.label}
                </Chip>
              ))}
            </div>
            <Button
              size="sm"
              disabled={testing}
              onClick={async () => {
                setTesting(true);
                const result = await testWebhook();
                setTesting(false);
                // 后端已经按当前语言给好文案，成功失败都原样显示
                if (result.message) onNotice(result);
              }}
            >
              {t("cfg.webhook_test")}
            </Button>
          </Nested>
        )}
      </div>
    </Section>
  );
}
/** AI 辅助判断：关键词判不准时才调一次模型 */
function AiSection({ config, set }: SectionProps) {
  const { t } = useI18n();
  const ai = config.ai_judge;
  const setAi = (partial: Partial<AiJudgeConfig>) => set("ai_judge", { ...ai, ...partial });

  return (
    <Section title={t("cfg.ai")} desc={t("cfg.ai.desc")}>
      <div className="space-y-3">
        <ToggleRow
          label={t("cfg.ai_enabled")}
          desc={t("cfg.ai_enabled.desc")}
          checked={ai.enabled}
          onCheckedChange={(v) => setAi({ enabled: v })}
        />
        {ai.enabled && (
          <Nested>
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <Field label={t("cfg.ai_endpoint")}>
                <TextInput
                  value={ai.api_url}
                  onChange={(e) => setAi({ api_url: e.target.value })}
                />
              </Field>
              <Field label={t("cfg.ai_model")}>
                <TextInput value={ai.model} onChange={(e) => setAi({ model: e.target.value })} />
              </Field>
            </div>
            {/* 密钥用 password 输入框，截图和录屏时不至于直接漏出去 */}
            <Field label={t("cfg.ai_key")}>
              <TextInput
                type="password"
                autoComplete="off"
                value={ai.api_key}
                placeholder="sk-…"
                onChange={(e) => setAi({ api_key: e.target.value })}
              />
            </Field>
            <Field
              label={t("cfg.ai_confidence", { value: ai.confidence_threshold })}
              hint={t("cfg.ai_confidence.hint")}
            >
              <Slider
                value={ai.confidence_threshold}
                min={50}
                max={99}
                onValueChange={(v) => setAi({ confidence_threshold: v })}
              />
            </Field>
          </Nested>
        )}
      </div>
    </Section>
  );
}
/**
 * 手机看板（v1.3 P1）
 *
 * 默认只听 127.0.0.1，并且必须带令牌才返回数据。打开「允许局域网访问」
 * 等于换成 0.0.0.0，同一网络里拿到令牌的人就能读你的会话——开关旁边的
 * 说明文字把这件事写明白了，两种语言都写。
 */
function RemoteSection({ config, set }: SectionProps) {
  const { t } = useI18n();
  const r = config.remote;
  const setR = (partial: Partial<RemoteConfig>) => set("remote", { ...r, ...partial });

  return (
    <Section title={t("cfg.remote")} desc={t("cfg.remote.desc")}>
      <div className="space-y-3">
        <ToggleRow
          label={t("cfg.remote_enabled")}
          desc={t("cfg.remote_enabled.desc")}
          checked={r.enabled}
          onCheckedChange={(v) => setR({ enabled: v })}
        />
        {r.enabled && (
          <Nested>
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <Field label={t("cfg.remote_port")}>
                <NumberInput
                  value={r.port}
                  min={1024}
                  max={65_535}
                  onValueChange={(v) => setR({ port: v })}
                />
              </Field>
              <Field label={t("cfg.remote_token")} hint={t("cfg.remote_token.hint")}>
                <TextInput
                  type="password"
                  autoComplete="off"
                  value={r.token}
                  onChange={(e) => setR({ token: e.target.value })}
                />
              </Field>
            </div>
            <ToggleRow
              label={t("cfg.remote_bind_all")}
              desc={t("cfg.remote_bind_all.desc")}
              checked={r.bind_all}
              onCheckedChange={(v) => setR({ bind_all: v })}
            />
            {/* 地址只显示不带令牌的部分：设置页是会被截图的，令牌走剪贴板 */}
            <Field
              label={t("cfg.remote_url")}
              hint={r.bind_all ? t("cfg.remote_url.lan") : undefined}
            >
              <div className="flex items-center gap-2">
                <p className="font-mono text-[11px] text-neutral-500">
                  http://127.0.0.1:{r.port}/
                </p>
                {r.token && <CopyLink url={`http://127.0.0.1:${r.port}/?token=${r.token}`} />}
              </div>
            </Field>
          </Nested>
        )}
      </div>
    </Section>
  );
}
/**
 * 复制看板链接
 *
 * 链接里带令牌，所以只进剪贴板、不上屏：设置页截图发出去也不会连门钥匙一起送。
 */
function CopyLink({ url }: { url: string }) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), 1500);
    return () => window.clearTimeout(timer);
  }, [copied]);

  return (
    <Button
      size="xs"
      onClick={() => {
        void navigator.clipboard?.writeText(url).then(() => setCopied(true));
      }}
    >
      {copied ? t("common.copied") : t("cfg.remote_copy_link")}
    </Button>
  );
}

/** 自定义适配器：内置三家之外的 CLI 在这里加 */
function AdapterSection({ config, set }: SectionProps) {
  const { t } = useI18n();
  const adapters = config.custom_adapters;

  const update = (index: number, partial: Partial<CustomAdapterConfig>) =>
    set(
      "custom_adapters",
      adapters.map((adapter, i) => (i === index ? { ...adapter, ...partial } : adapter))
    );

  return (
    <Section title={t("cfg.adapters")} desc={t("cfg.adapters.desc")}>
      <div className="space-y-3">
        {adapters.length === 0 ? (
          <EmptyState className="py-6" title={t("cfg.adapters.empty")} />
        ) : (
          adapters.map((adapter, index) => (
            // 用下标当 key 没问题：输入框的值全部来自 config，自己不存 state，
            // 删掉中间一行也不会把内容串到别的行上
            <Nested key={index}>
              <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                <Field label={t("cfg.adapter_name")}>
                  <TextInput
                    value={adapter.name}
                    onChange={(e) => update(index, { name: e.target.value })}
                  />
                </Field>
                <Field label={t("cfg.adapter_process")}>
                  <TextInput
                    value={adapter.process_pattern}
                    onChange={(e) => update(index, { process_pattern: e.target.value })}
                  />
                </Field>
              </div>
              <Field label={t("cfg.adapter_session")}>
                <TextInput
                  value={adapter.session_file_pattern}
                  onChange={(e) => update(index, { session_file_pattern: e.target.value })}
                />
              </Field>
              <div className="flex justify-end">
                <Button
                  size="xs"
                  variant="danger"
                  onClick={() =>
                    set(
                      "custom_adapters",
                      adapters.filter((_, i) => i !== index)
                    )
                  }
                >
                  {t("common.remove")}
                </Button>
              </div>
            </Nested>
          ))
        )}
        <Button
          className="w-full border-dashed"
          onClick={() =>
            set("custom_adapters", [
              ...adapters,
              { name: "", process_pattern: "", session_file_pattern: "" },
            ])
          }
        >
          {t("cfg.adapter_add")}
        </Button>
      </div>
    </Section>
  );
}
