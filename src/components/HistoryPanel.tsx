import { useEffect, useMemo, useState } from "react";
import { useI18n } from "../i18n";
import {
  durationText,
  STATUS_DOT,
  STATUS_TONE,
  statusKey,
} from "../lib/display";
import { dayLabel, groupByDay } from "../lib/history";
import {
  baseName,
  cn,
  formatShortTime,
  formatTokens,
  formatUsd,
  secondsBetween,
} from "../lib/utils";
import {
  HISTORY_PAGE_SIZE,
  selectHistoryFilter,
  selectSessionHistory,
  selectSessionHistorySummary,
  selectSessionHistoryTotal,
  useAppStore,
} from "../stores/useAppStore";
import type {
  HistoryStatusFilter,
  SessionHistoryEntry,
  SessionStatus,
} from "../types";
import { SessionDetailDrawer } from "./SessionDetailDrawer";
import {
  Badge,
  Button,
  Card,
  CardBody,
  CardHeader,
  EmptyState,
  ExportButton,
  Segmented,
  type SegmentedOption,
  TextInput,
  Tooltip,
} from "./ui";

const KNOWN_STATUS = new Set<string>(Object.keys(STATUS_DOT));

/**
 * 会话档案
 *
 * 这一页以前是一条平铺的列表，用户的评价是「布局不合理，也不知道有什么作用」。
 * 那句话是对的，而且原因比排版更深：当时的行标识里含着 `discovered_at`，
 * 应用一重启同一个会话就多出一行——**它列的其实是重启记录，不是会话**。
 * 真实库里一个会话摊成过 16 行。修好键之后这一页才第一次真的「一行一个会话」。
 *
 * 现在它回答三个问题，从上到下：
 * 1. 顶上的汇总条——总共攒了多少、还有几个在跑、一共花了多少；
 * 2. 中间的列表按天分组——「上周四那天我在忙什么」是按日子回忆的，
 *    不是按分页序号；
 * 3. 点开任意一行——那个会话到底经历了什么（生命周期、中断、每次续跑的结果）。
 */
export function HistoryPanel() {
  const { t } = useI18n();
  const history = useAppStore(selectSessionHistory);
  const total = useAppStore(selectSessionHistoryTotal);
  const summary = useAppStore(selectSessionHistorySummary);
  const filter = useAppStore(selectHistoryFilter);
  const fetchSessionHistory = useAppStore((s) => s.fetchSessionHistory);
  const openSessionDetail = useAppStore((s) => s.openSessionDetail);

  // 输入框自己拿着字，防抖之后才发请求；store 里那份是「已经问过后端的条件」
  const [query, setQuery] = useState(filter.query);

  useEffect(() => {
    const timer = window.setTimeout(() => void fetchSessionHistory({ query }), 250);
    return () => window.clearTimeout(timer);
  }, [query, fetchSessionHistory]);

  const statusOptions: readonly SegmentedOption<HistoryStatusFilter>[] = useMemo(
    () => [
      { value: "all", label: t("history.filter_all") },
      { value: "live", label: t("history.filter_live") },
      { value: "ended", label: t("history.filter_ended") },
    ],
    [t],
  );

  const groups = useMemo(() => groupByDay(history), [history]);
  // 一次渲染里所有分组标题共用同一个「现在」：每个标题各取一次 `new Date()`
  // 的话，跨午夜那一瞬间会出现两条都叫「今天」
  const now = useMemo(() => new Date(), [history]);
  const page = Math.floor(filter.offset / HISTORY_PAGE_SIZE);
  const pageCount = Math.max(1, Math.ceil(total / HISTORY_PAGE_SIZE));

  return (
    <div className="mx-auto max-w-3xl space-y-4">
      <Card>
        <CardBody>
          <CardHeader
            className="mb-4"
            title={t("history.title")}
            desc={t("history.desc")}
            aside={
              // 导出跟着筛选条件走，不跟着分页走：用户想要的是「我筛出来的这些」，
              // 而不是「屏幕上这 20 行」
              <ExportButton
                command="export_sessions"
                args={{ query: filter.query, status: filter.status }}
              />
            }
          />

          {summary && <SummaryStrip summary={summary} />}

          <div className="mt-4 flex items-center gap-2">
            <TextInput
              className="flex-1"
              value={query}
              placeholder={t("history.search")}
              onChange={(e) => setQuery(e.target.value)}
            />
            <Segmented
              value={filter.status}
              options={statusOptions}
              ariaLabel={t("history.title")}
              onChange={(status) => void fetchSessionHistory({ status })}
            />
          </div>

          {history.length === 0 ? (
            <EmptyState
              className="py-8"
              title={
                filter.query || filter.status !== "all"
                  ? t("history.no_match")
                  : t("history.empty")
              }
            />
          ) : (
            <div className="mt-3">
              {groups.map(([day, entries]) => (
                <section key={day}>
                  {/* 日期这一条要粘住：往下滚的时候得始终知道在看哪一天 */}
                  <h4 className="sticky top-0 z-10 bg-white/95 py-1.5 text-[10px] font-medium tabular-nums text-neutral-400 backdrop-blur-sm">
                    {dayLabel(day, now, t)}
                  </h4>
                  <div className="divide-y divide-neutral-100">
                    {entries.map((entry) => (
                      <HistoryRow
                        key={entry.session_key}
                        entry={entry}
                        onOpen={() => void openSessionDetail(entry.session_key)}
                      />
                    ))}
                  </div>
                </section>
              ))}
            </div>
          )}

          {total > HISTORY_PAGE_SIZE && (
            <Pager
              page={page}
              pageCount={pageCount}
              onChange={(next) =>
                void fetchSessionHistory({ offset: next * HISTORY_PAGE_SIZE })
              }
            />
          )}
        </CardBody>
      </Card>

      <SessionDetailDrawer />
    </div>
  );
}

/**
 * 顶上的四个数
 *
 * 「仍在运行」单独占一格，是因为这一页最容易读错的就是这件事——
 * 用户关掉的会话曾经在这儿一直写着「运行中」。把「活着几个」摆到最显眼处，
 * 数字对不对一眼就能被质疑，比藏在每一行里安全。
 */
function SummaryStrip({
  summary,
}: {
  summary: import("../types").SessionHistorySummary;
}) {
  const { t } = useI18n();
  const tiles: readonly { label: string; value: string; tone?: string }[] = [
    { label: t("history.sum_total"), value: String(summary.total) },
    {
      label: t("history.sum_live"),
      value: String(summary.live),
      tone: summary.live > 0 ? "text-emerald-600" : undefined,
    },
    { label: t("history.sum_resumes"), value: String(summary.resumes) },
    { label: t("history.sum_cost"), value: `$${formatUsd(summary.cost_usd)}` },
  ];
  return (
    <div className="grid grid-cols-4 gap-2 rounded-lg bg-neutral-50 p-3">
      {tiles.map((tile) => (
        <div key={tile.label} className="min-w-0">
          <p className="truncate text-[10px] text-neutral-400">{tile.label}</p>
          <p
            className={cn(
              "mt-0.5 truncate text-sm font-semibold tabular-nums text-neutral-800",
              tile.tone,
            )}
          >
            {tile.value}
          </p>
        </div>
      ))}
    </div>
  );
}

function HistoryRow({
  entry,
  onOpen,
}: {
  entry: SessionHistoryEntry;
  onOpen: () => void;
}) {
  const { t } = useI18n();
  const live = entry.ended_at === "";
  const status = KNOWN_STATUS.has(entry.last_status)
    ? (entry.last_status as SessionStatus)
    : null;
  const terminal = [entry.terminal_app, entry.tty.replace("/dev/", "")]
    .filter(Boolean)
    .join(" ");

  return (
    // 整行是个按钮：点哪儿都能打开，比在行尾藏一个「详情」链接好点得多
    <button
      type="button"
      onClick={onOpen}
      className={cn(
        "flex w-full items-start gap-3 py-2.5 text-left outline-none transition-colors",
        "hover:bg-neutral-50 focus-visible:bg-neutral-50",
      )}
    >
      <span
        className={cn(
          "mt-1.5 h-1.5 w-1.5 shrink-0 rounded-full",
          status ? STATUS_DOT[status] : "bg-neutral-300",
          // 只有真的还活着才呼吸；已结束的会话留着原来的颜色但不再闪，
          // 否则「已结束」和「运行中」在余光里长得一模一样
          live && status === "active" && "animate-pulse-soft",
        )}
      />

      <div className="min-w-0 flex-1">
        {/* 第一行：项目名是主角，字号和颜色都比别的重 */}
        <div className="flex items-center gap-1.5">
          <Tooltip content={entry.working_dir}>
            <span className="truncate text-xs font-medium text-neutral-800">
              {baseName(entry.working_dir)}
            </span>
          </Tooltip>
          {live ? (
            <Badge tone="green">{t("history.live")}</Badge>
          ) : (
            status && (
              <Tooltip content={t("history.last_seen_as", { status: t(statusKey(status)) })}>
                <Badge tone={STATUS_TONE[status]}>{t(statusKey(status))}</Badge>
              </Tooltip>
            )
          )}
          {entry.resume_count > 0 && (
            <Badge tone="violet">
              {t("session.resumed", { count: entry.resume_count })}
            </Badge>
          )}
        </div>

        {/* 第二行：谁、在哪个终端、活了多久——都是次要信息，压成一行灰字 */}
        <p className="mt-1 flex flex-wrap items-center gap-x-2 text-[10px] tabular-nums text-neutral-400">
          <span>{entry.agent_name}</span>
          {terminal && <span className="font-mono">{terminal}</span>}
          <span>{formatShortTime(entry.first_seen)}</span>
          <Lifespan entry={entry} />
        </p>
      </div>

      {entry.total_tokens > 0 && (
        <span className="shrink-0 text-right text-[10px] tabular-nums text-neutral-400">
          <span className="block font-medium text-neutral-600">
            ${formatUsd(entry.cost_usd)}
          </span>
          {formatTokens(entry.total_tokens)}
        </span>
      )}
    </button>
  );
}

/** 「持续 N 分钟」；算不出来就什么都不说 */
function Lifespan({ entry }: { entry: SessionHistoryEntry }) {
  const { t } = useI18n();
  const end = entry.ended_at || entry.last_seen;
  const secs = secondsBetween(entry.first_seen, end);
  if (secs === null || secs < 60) return null;
  return <span>{t("history.lasted", { duration: durationText(secs, t) })}</span>;
}

function Pager({
  page,
  pageCount,
  onChange,
}: {
  page: number;
  pageCount: number;
  onChange: (page: number) => void;
}) {
  const { t } = useI18n();
  return (
    <div className="mt-4 flex items-center justify-between border-t border-neutral-100 pt-3">
      <Button
        size="xs"
        variant="ghost"
        disabled={page === 0}
        onClick={() => onChange(page - 1)}
      >
        {t("history.previous")}
      </Button>
      <span className="text-[10px] tabular-nums text-neutral-400">
        {t("history.page", { page: page + 1, total: pageCount })}
      </span>
      <Button
        size="xs"
        variant="ghost"
        disabled={page + 1 >= pageCount}
        onClick={() => onChange(page + 1)}
      >
        {t("history.next")}
      </Button>
    </div>
  );
}

