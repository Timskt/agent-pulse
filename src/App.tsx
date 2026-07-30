import { useEffect } from "react";
import { ConfigPanel } from "./components/ConfigPanel";
import { CostPanel } from "./components/CostPanel";
import { HistoryPanel } from "./components/HistoryPanel";
import { LogPanel } from "./components/LogPanel";
import { SessionList } from "./components/SessionList";
import { StatsPanel } from "./components/StatsPanel";
import { StatusCards } from "./components/StatusCards";
import {
  Badge,
  Button,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
  TooltipProvider,
} from "./components/ui";
import { useI18n, type I18nKey } from "./i18n";
import { formatShortTime } from "./lib/utils";
import {
  selectActiveTab,
  selectConfig,
  selectLoading,
  selectRunning,
  selectStatus,
  useAppStore,
  type TabId,
} from "./stores/useAppStore";

/**
 * 应用外壳
 *
 * 三处变化：导航换成 Radix `Tabs`（左右方向键可切、带 `role="tab"`），
 * 标题栏和页脚的文案全部走 i18n（原来「Dashboard / Statistics / Settings」
 * 和中文界面并排，正是那种土不土洋不洋的搭配），页脚的扫描间隔改成读真实配置。
 *
 * 花费、历史两个标签页的数据由各自面板在挂载时拉取——Radix 只渲染当前
 * 选中的 `TabsContent`，所以切过去才会请求，不会开机就把三份数据全拉一遍。
 */

/** 和 package.json / tauri.conf.json 保持一致 */
const APP_VERSION = "1.0.0";

const TABS: readonly { id: TabId; label: I18nKey }[] = [
  { id: "dashboard", label: "nav.dashboard" },
  { id: "stats", label: "nav.stats" },
  { id: "cost", label: "nav.cost" },
  { id: "history", label: "nav.history" },
  { id: "config", label: "nav.config" },
];
export default function App() {
  const { t } = useI18n();
  const activeTab = useAppStore(selectActiveTab);
  const running = useAppStore(selectRunning);
  const loading = useAppStore(selectLoading);
  const status = useAppStore(selectStatus);
  const config = useAppStore(selectConfig);
  const setActiveTab = useAppStore((s) => s.setActiveTab);
  const fetchState = useAppStore((s) => s.fetchState);
  const fetchConfig = useAppStore((s) => s.fetchConfig);
  const startMonitoring = useAppStore((s) => s.startMonitoring);
  const stopMonitoring = useAppStore((s) => s.stopMonitoring);
  const scanNow = useAppStore((s) => s.scanNow);
  const initEventListeners = useAppStore((s) => s.initEventListeners);

  useEffect(() => {
    void fetchState();
    void fetchConfig();
    // 订阅是异步建立的，卸载时要等它拿到取消函数再调
    const pending = initEventListeners();
    return () => {
      void pending.then((dispose) => dispose());
    };
  }, [fetchState, fetchConfig, initEventListeners]);

  return (
    <TooltipProvider>
      <Tabs
        value={activeTab}
        onValueChange={(value) => setActiveTab(value as TabId)}
        className="flex h-screen flex-col bg-[#fafafa]"
      >
        <header className="titlebar-drag flex items-center justify-between gap-4 border-b border-neutral-200 bg-white px-6 py-3">
          <div className="flex min-w-0 items-center gap-3">
            <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-neutral-900 text-xs font-bold text-white">
              A
            </div>
            <div className="min-w-0">
              <h1 className="text-[13px] font-semibold tracking-tight text-neutral-900">
                AgentPulse
              </h1>
              <p className="truncate text-[10px] text-neutral-400">{t("app.subtitle")}</p>
            </div>
            {running ? (
              <Badge tone="green" className="ml-1">
                <span className="h-1.5 w-1.5 animate-pulse-soft rounded-full bg-emerald-500" />
                {t("app.running")}
              </Badge>
            ) : (
              <Badge className="ml-1">{t("app.stopped")}</Badge>
            )}
          </div>
          <TabsList className="titlebar-no-drag shrink-0">
            {TABS.map((tab) => (
              <TabsTrigger key={tab.id} value={tab.id}>
                {t(tab.label)}
              </TabsTrigger>
            ))}
          </TabsList>

          <div className="titlebar-no-drag flex shrink-0 items-center gap-2">
            <Button disabled={loading} onClick={() => void scanNow()}>
              {t("btn.scan_now")}
            </Button>
            <Button
              variant="primary"
              disabled={loading}
              onClick={() => void (running ? stopMonitoring() : startMonitoring())}
            >
              {running ? t("btn.stop") : t("btn.start")}
            </Button>
          </div>
        </header>

        <main className="flex-1 overflow-y-auto p-6">
          <TabsContent value="dashboard" className="animate-fade-in space-y-5">
            <StatusCards />
            <SessionList />
            <LogPanel />
          </TabsContent>
          <TabsContent value="stats" className="animate-fade-in">
            <StatsPanel />
          </TabsContent>
          <TabsContent value="cost" className="animate-fade-in">
            <CostPanel />
          </TabsContent>
          <TabsContent value="history" className="animate-fade-in">
            <HistoryPanel />
          </TabsContent>
          <TabsContent value="config" className="animate-fade-in">
            <ConfigPanel />
          </TabsContent>
        </main>
        <footer className="flex items-center justify-between gap-4 border-t border-neutral-200 bg-white px-6 py-2 text-[10px] text-neutral-400">
          <span className="tabular-nums">AgentPulse v{APP_VERSION}</span>
          <div className="flex items-center gap-4 tabular-nums">
            <span>{t("footer.sessions", { count: status.sessions_total })}</span>
            {running && config && (
              <span>{t("footer.interval", { secs: config.poll_interval_secs })}</span>
            )}
            <span className="text-neutral-300">
              {status.last_scan_at
                ? t("footer.last_scan", { time: formatShortTime(status.last_scan_at) })
                : t("footer.never_scanned")}
            </span>
          </div>
        </footer>
      </Tabs>
    </TooltipProvider>
  );
}
