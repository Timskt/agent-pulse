import { useI18n } from "../../i18n";
import type { Notice } from "../../lib/useNotice";
import { useAppStore } from "../../stores/useAppStore";
import type { AppConfig, NotificationConfig } from "../../types";
import { Button, Chip, Field, NumberInput, Slider, ToggleRow } from "../ui";
import { ConfigNested, ConfigSection } from "./ConfigSection";

type Setter = <K extends keyof AppConfig>(key: K, value: AppConfig[K]) => void;

export function NotifySection({ config, set, onNotice }: { config: AppConfig; set: Setter; onNotice: (notice: Notice) => void }) {
  const { t } = useI18n();
  const testNotify = useAppStore((s) => s.testNotify);
  const n = config.notification;
  const setN = (partial: Partial<NotificationConfig>) => set("notification", { ...n, ...partial });
  const events = [
    [t("cfg.notify_needs_input"), n.on_needs_input, () => setN({ on_needs_input: !n.on_needs_input })],
    [t("cfg.notify_completed"), n.on_completed, () => setN({ on_completed: !n.on_completed })],
    [t("cfg.notify_rate_limited"), n.on_rate_limited, () => setN({ on_rate_limited: !n.on_rate_limited })],
    [t("cfg.notify_error"), n.on_error, () => setN({ on_error: !n.on_error })],
    [t("cfg.notify_resumed"), n.on_resumed, () => setN({ on_resumed: !n.on_resumed })],
  ] as const;
  return <ConfigSection title={t("cfg.notify")} desc={t("cfg.notify.desc")}><div className="space-y-3">
    <ToggleRow label={t("cfg.notify_enabled")} desc={t("cfg.notify_enabled.desc")} checked={n.enabled} onCheckedChange={(v) => setN({ enabled: v })} />
    {n.enabled && <ConfigNested>
      <div className="flex flex-wrap gap-2">{events.map(([label, on, toggle]) => <Chip key={label} active={on} onClick={toggle}>{label}</Chip>)}</div>
      <ToggleRow label={t("cfg.notify_sound")} desc={t("cfg.notify_sound.desc")} checked={n.sound_enabled} onCheckedChange={(v) => setN({ sound_enabled: v })} />
      {n.sound_enabled && <Field label={t("cfg.notify_volume", { value: n.sound_volume })}><Slider value={n.sound_volume} min={0} max={100} onValueChange={(v) => setN({ sound_volume: v })} /></Field>}
      <ToggleRow label={t("cfg.notify_badge")} desc={t("cfg.notify_badge.desc")} checked={n.tray_badge} onCheckedChange={(v) => setN({ tray_badge: v })} />
      <Field label={t("cfg.notify_throttle")}><NumberInput value={n.throttle_secs} min={0} max={3600} onValueChange={(v) => setN({ throttle_secs: v })} /></Field>
      <Button size="sm" onClick={async () => { const result = await testNotify(); if (result.message) onNotice(result); }}>{t("cfg.notify_test")}</Button>
    </ConfigNested>}
  </div></ConfigSection>;
}
