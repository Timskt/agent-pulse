import { useEffect } from "react";
import { ConfigPanel } from "./components/ConfigPanel";
import { CostPanel } from "./components/CostPanel";
import { DashboardPanel } from "./components/DashboardPanel";
import { HistoryPanel } from "./components/HistoryPanel";
import { StatsPanel } from "./components/StatsPanel";
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

/** 构建时从 package.json 注进来，不再手抄（见 vite.config.ts） */
const APP_VERSION = __APP_VERSION__;

const TABS: readonly { id: TabId; label: I18nKey }[] = [
  { id: "dashboard", label: "nav.dashboard" },
  { id: "stats", label: "nav.stats" },
  { id: "cost", label: "nav.cost" },
  { id: "history", label: "nav.history" },
  { id: "config", label: "nav.config" },
];
export default function App() {
  const { t, lang } = useI18n();
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

  // `index.html` 里写死的是 `lang="zh-CN"`：切成英文界面后读屏软件还会用中文
  // 语音去念英文。跟着配置改，是这一处唯一能保持诚实的办法。
  useEffect(() => {
    document.documentElement.lang = lang === "en" ? "en" : "zh-CN";
  }, [lang]);

  return (
    <TooltipProvider>
      <Tabs
        value={activeTab}
        onValueChange={(value) => setActiveTab(value as TabId)}
        className="app-shell flex h-screen min-w-0 flex-col bg-[#fafafa]"
      >
        <header className="app-header titlebar-drag border-b border-neutral-200 bg-white px-6 py-3">
          <div className="app-brand flex min-w-0 items-center gap-3">
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
              <Badge tone="green" className="app-status-badge ml-1">
                <span className="h-1.5 w-1.5 animate-pulse-soft rounded-full bg-emerald-500" />
                {t("app.running")}
              </Badge>
            ) : (
              <Badge className="app-status-badge ml-1">{t("app.stopped")}</Badge>
            )}
          </div>
          <TabsList className="app-tabs titlebar-no-drag">
            {TABS.map((tab) => (
              <TabsTrigger className="app-tab-trigger" key={tab.id} value={tab.id}>
                {t(tab.label)}
              </TabsTrigger>
            ))}
          </TabsList>

          <div className="app-actions titlebar-no-drag flex items-center gap-2">
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

        <main className="app-main min-w-0 flex-1 overflow-x-hidden overflow-y-auto p-6">
          <TabsContent value="dashboard" className="min-w-0 animate-fade-in">
            <DashboardPanel />
          </TabsContent>
          <TabsContent value="stats" className="min-w-0 animate-fade-in">
            <StatsPanel />
          </TabsContent>
          <TabsContent value="cost" className="min-w-0 animate-fade-in">
            <CostPanel />
          </TabsContent>
          {/* 历史页默认只呈现会话档案；逐次续跑记录由 HistoryPanel 收进
              按需展开的诊断入口，避免与会话详情里的续跑时间线重复。 */}
          <TabsContent value="history" className="min-w-0 animate-fade-in">
            <HistoryPanel />
          </TabsContent>
          <TabsContent value="config" className="min-w-0 animate-fade-in">
            <ConfigPanel />
          </TabsContent>
        </main>
        <footer className="app-footer flex items-center justify-between gap-4 border-t border-neutral-200 bg-white px-6 py-2 text-[10px] text-neutral-400">
          <span className="tabular-nums">AgentPulse v{APP_VERSION}</span>
          <div className="app-footer-meta flex min-w-0 items-center gap-4 tabular-nums">
            {(status.resume_pending > 0 || status.resume_verifying > 0) && (
              <span className="font-medium text-amber-600">
                {t("footer.resume_pipeline", {
                  pending: status.resume_pending,
                  verifying: status.resume_verifying,
                })}
              </span>
            )}
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
