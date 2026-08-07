import { useI18n } from "../i18n";
import {
  selectLoading,
  selectRunning,
  selectStatus,
  useAppStore,
} from "../stores/useAppStore";
import { Badge, Button, Card, CardBody, CardHeader } from "./ui";

/**
 * 首次使用引导。
 *
 * 这里只串起已有命令，不替用户启动 Agent。AgentPulse 的安全边界也必须在第一屏
 * 说清楚：观察进程与记录、精确定位后才投递，定位不确定就停手。
 */
export function OnboardingPanel() {
  const { t } = useI18n();
  const running = useAppStore(selectRunning);
  const loading = useAppStore(selectLoading);
  const status = useAppStore(selectStatus);
  const startMonitoring = useAppStore((state) => state.startMonitoring);
  const scanNow = useAppStore((state) => state.scanNow);
  const setActiveTab = useAppStore((state) => state.setActiveTab);
  const scanned = status.last_scan_at !== null;

  return (
    <Card>
      <CardBody className="space-y-5">
        <CardHeader
          title={t("onboarding.title")}
          desc={t("onboarding.desc")}
          aside={
            <Button
              size="sm"
              variant="ghost"
              disabled={loading}
              onClick={() => setActiveTab("config")}
            >
              {t("onboarding.settings")}
            </Button>
          }
        />

        <ol className="grid gap-3 lg:grid-cols-3">
          <OnboardingStep
            number={1}
            title={t("onboarding.monitor.title")}
            body={t("onboarding.monitor.body")}
            complete={running}
            completeLabel={t("onboarding.monitor.ready")}
          >
            <Button
              size="sm"
              variant={running ? "outline" : "primary"}
              disabled={loading || running}
              onClick={() => void startMonitoring()}
            >
              {running
                ? t("onboarding.monitor.ready")
                : t("onboarding.monitor.action")}
            </Button>
          </OnboardingStep>

          <OnboardingStep
            number={2}
            title={t("onboarding.agent.title")}
            body={t("onboarding.agent.body")}
          >
            <div className="flex flex-wrap gap-1.5" aria-label={t("onboarding.agent.commands")}>
              {["claude", "codex", "opencode"].map((command) => (
                <code
                  key={command}
                  className="rounded bg-neutral-100 px-2 py-1 text-[11px] text-neutral-700"
                >
                  {command}
                </code>
              ))}
            </div>
          </OnboardingStep>

          <OnboardingStep
            number={3}
            title={t("onboarding.scan.title")}
            body={t("onboarding.scan.body")}
            complete={scanned}
            completeLabel={t("onboarding.scan.ready")}
          >
            <Button
              size="sm"
              variant="outline"
              disabled={loading}
              onClick={() => void scanNow()}
            >
              {t("onboarding.scan.action")}
            </Button>
          </OnboardingStep>
        </ol>

        <div className="rounded-md border border-amber-100 bg-amber-50/60 px-3 py-2.5">
          <p className="text-[11px] font-medium text-amber-800">
            {t("onboarding.boundary.title")}
          </p>
          <p className="mt-0.5 text-[10px] leading-relaxed text-amber-700">
            {t("onboarding.boundary.body")}
          </p>
        </div>
      </CardBody>
    </Card>
  );
}

function OnboardingStep({
  number,
  title,
  body,
  complete = false,
  completeLabel,
  children,
}: {
  number: number;
  title: string;
  body: string;
  complete?: boolean;
  completeLabel?: string;
  children: React.ReactNode;
}) {
  return (
    <li className="flex min-h-40 flex-col rounded-lg border border-neutral-100 bg-neutral-50/60 p-4">
      <div className="flex items-center justify-between gap-2">
        <span className="flex h-6 w-6 items-center justify-center rounded-full bg-neutral-900 text-[10px] font-semibold text-white">
          {number}
        </span>
        {complete && completeLabel && <Badge tone="green">{completeLabel}</Badge>}
      </div>
      <h4 className="mt-3 text-xs font-semibold text-neutral-800">{title}</h4>
      <p className="mt-1 flex-1 text-[11px] leading-relaxed text-neutral-500">{body}</p>
      <div className="mt-3">{children}</div>
    </li>
  );
}
