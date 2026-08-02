import { useI18n } from "../../i18n";
import type { AppConfig, CostConfig } from "../../types";
import { Field, NumberInput, ToggleRow } from "../ui";
import { ConfigNested, ConfigSection } from "./ConfigSection";

type Setter = <K extends keyof AppConfig>(key: K, value: AppConfig[K]) => void;
export function CostSection({ config, set }: { config: AppConfig; set: Setter }) {
  const { t } = useI18n();
  const c = config.cost;
  const setC = (partial: Partial<CostConfig>) => set("cost", { ...c, ...partial });
  return <ConfigSection title={t("cfg.cost")} desc={t("cfg.cost.desc")}><div className="space-y-3">
    <ToggleRow label={t("cfg.cost_enabled")} desc={t("cfg.cost_enabled.desc")} checked={c.enabled} onCheckedChange={(v) => setC({ enabled: v })} />
    {c.enabled && <ConfigNested><div className="grid grid-cols-2 gap-4">
      <Field label={t("cfg.daily_budget")}><NumberInput value={c.daily_budget_usd} min={0} max={10_000} step={1} onValueChange={(v) => setC({ daily_budget_usd: v })} /></Field>
      <Field label={t("cfg.session_budget")}><NumberInput value={c.session_budget_usd} min={0} max={1_000} step={0.5} onValueChange={(v) => setC({ session_budget_usd: v })} /></Field>
      <Field label={t("cfg.alert_percent")}><NumberInput value={c.alert_at_percent} min={10} max={100} onValueChange={(v) => setC({ alert_at_percent: v })} /></Field>
      <Field label={t("cfg.rate_window")}><NumberInput value={c.rate_limit_window_hours} min={1} max={24} onValueChange={(v) => setC({ rate_limit_window_hours: v })} /></Field>
      <Field label={t("cfg.rate_budget")} className="col-span-2"><NumberInput value={c.rate_limit_token_budget} min={0} step={100_000} onValueChange={(v) => setC({ rate_limit_token_budget: v })} /></Field>
    </div></ConfigNested>}
  </div></ConfigSection>;
}
