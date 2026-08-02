import { useI18n } from "../../i18n";
import type { AiJudgeConfig, AppConfig } from "../../types";
import { Field, Slider, TextInput, ToggleRow } from "../ui";
import { ConfigNested, ConfigSection } from "./ConfigSection";

type Setter = <K extends keyof AppConfig>(key: K, value: AppConfig[K]) => void;
export function AiSection({ config, set }: { config: AppConfig; set: Setter }) {
  const { t } = useI18n();
  const ai = config.ai_judge;
  const setAi = (partial: Partial<AiJudgeConfig>) => set("ai_judge", { ...ai, ...partial });
  return <ConfigSection title={t("cfg.ai")} desc={t("cfg.ai.desc")}><div className="space-y-3">
    <ToggleRow label={t("cfg.ai_enabled")} desc={t("cfg.ai_enabled.desc")} checked={ai.enabled} onCheckedChange={(v) => setAi({ enabled: v })} />
    {ai.enabled && <ConfigNested>
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <Field label={t("cfg.ai_endpoint")}><TextInput value={ai.api_url} onChange={(e) => setAi({ api_url: e.target.value })} /></Field>
        <Field label={t("cfg.ai_model")}><TextInput value={ai.model} onChange={(e) => setAi({ model: e.target.value })} /></Field>
      </div>
      <Field label={t("cfg.ai_key")}><TextInput type="password" autoComplete="off" value={ai.api_key} placeholder="sk-…" onChange={(e) => setAi({ api_key: e.target.value })} /></Field>
      <Field label={t("cfg.ai_confidence", { value: ai.confidence_threshold })} hint={t("cfg.ai_confidence.hint")}><Slider value={ai.confidence_threshold} min={50} max={99} onValueChange={(v) => setAi({ confidence_threshold: v })} /></Field>
    </ConfigNested>}
  </div></ConfigSection>;
}
