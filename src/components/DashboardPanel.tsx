import { selectStatus, useAppStore } from "../stores/useAppStore";
import { LogPanel } from "./LogPanel";
import { OnboardingPanel } from "./OnboardingPanel";
import { SessionList } from "./SessionList";
import { StatusCards } from "./StatusCards";

/** 总览页编排；App 只保留应用壳和导航。 */
export function DashboardPanel() {
  const status = useAppStore(selectStatus);

  return (
    <div className="space-y-5">
      <StatusCards />
      {status.sessions_total === 0 ? <OnboardingPanel /> : <SessionList />}
      <LogPanel />
    </div>
  );
}
