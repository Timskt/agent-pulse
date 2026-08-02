import { useEffect, useState } from "react";
import { useI18n } from "../i18n";
import { useNotice, type Notice } from "../lib/useNotice";
import { cn } from "../lib/utils";
import { selectConfig, useAppStore } from "../stores/useAppStore";
import type {
  AppConfig,
  CustomAdapterConfig,
  RemoteConfig,
  WebhookConfig,
  WebhookProvider,
} from "../types";
import { AiSection } from "./config/AiSection";
import { ConfigNested as Nested, ConfigSection as Section } from "./config/ConfigSection";
import { CostSection } from "./config/CostSection";
import { NotifySection } from "./config/NotifySection";
import {
  Badge,
  Button,
  Chip,
  CommaListInput,
  EmptyState,
  Field,
  NumberInput,
  Select,
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
/**
 * 手机看板（v1.3 P1）
 *
 * 默认只听 127.0.0.1，并且必须带令牌才返回数据。打开「允许局域网访问」
 * 等于换成 0.0.0.0，同一网络里拿到令牌的人就能读你的会话——开关旁边的
 * 说明文字把这件事写明白了，两种语言都写。
 *
 * 地址是算出来的，不是硬写的：以前这里永远显示 `127.0.0.1` 再附一句
 * 「自己换成局域网 IP」，于是「IP 换错了」和「服务根本没起来」在手机上
 * 表现完全一样——都是连接被拒绝，用户没法区分。
 */
function RemoteSection({ config, set }: SectionProps) {
  const { t } = useI18n();
  const getLanIp = useAppStore((s) => s.getLanIp);
  const generateRemoteToken = useAppStore((s) => s.generateRemoteToken);
  const [lanIp, setLanIp] = useState<string | null>(null);
  const r = config.remote;
  const setR = (partial: Partial<RemoteConfig>) =>
    set("remote", { ...r, ...partial });

  // 只在真的要显示局域网地址时才去问：没开局域网访问就没必要算
  useEffect(() => {
    if (!r.enabled || !r.bind_all) return;
    let alive = true;
    void getLanIp().then((ip) => {
      if (alive) setLanIp(ip);
    });
    return () => {
      alive = false;
    };
  }, [r.enabled, r.bind_all, getLanIp]);

  const host = r.bind_all ? (lanIp ?? "127.0.0.1") : "127.0.0.1";
  const url = `http://${host}:${r.port}/`;
  // 令牌里出现 & 或 # 就会把 query 截断，服务端只收到前半段然后判鉴权失败。
  // 编码一下，任何字符都能安全塞进链接。
  const linkWithToken = `${url}?token=${encodeURIComponent(r.token)}`;
  // 绑 loopback 时短令牌无所谓；开到局域网上，它就是唯一那道门
  const weakToken = r.bind_all && r.token.trim().length < 16;

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
              <Field
                label={t("cfg.remote_token")}
                hint={t("cfg.remote_token.hint")}
              >
                <div className="flex items-center gap-2">
                  <TextInput
                    type="password"
                    autoComplete="off"
                    value={r.token}
                    onChange={(e) => setR({ token: e.target.value })}
                  />
                  <Button
                    size="xs"
                    variant="outline"
                    className="shrink-0"
                    onClick={() => {
                      void generateRemoteToken().then((token) =>
                        setR({ token }),
                      );
                    }}
                  >
                    {t("cfg.remote_token_generate")}
                  </Button>
                </div>
              </Field>
            </div>
            <ToggleRow
              label={t("cfg.remote_bind_all")}
              desc={t("cfg.remote_bind_all.desc")}
              checked={r.bind_all}
              onCheckedChange={(v) => setR({ bind_all: v })}
            />
            {weakToken && (
              <p className="rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-[11px] leading-relaxed text-amber-700">
                {t("cfg.remote_token_weak")}
              </p>
            )}
            {/* 地址只显示不带令牌的部分：设置页是会被截图的，令牌走剪贴板 */}
            <Field
              label={t("cfg.remote_url")}
              hint={
                r.bind_all
                  ? lanIp
                    ? t("cfg.remote_url.lan_found")
                    : t("cfg.remote_url.lan_unknown")
                  : undefined
              }
            >
              <div className="flex items-center gap-2">
                <p className="font-mono text-[11px] text-neutral-500">{url}</p>
                {r.token && <CopyLink url={linkWithToken} />}
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
