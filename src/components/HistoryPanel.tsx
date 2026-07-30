import { useEffect, useState } from "react";
import { useI18n } from "../i18n";
import { STATUS_DOT, STATUS_TONE, statusKey } from "../lib/display";
import { baseName, cn, formatShortTime, formatTokens, formatUsd } from "../lib/utils";
import {
  selectHistoryQuery,
  selectSessionHistory,
  useAppStore,
} from "../stores/useAppStore";
import type { SessionStatus } from "../types";
import { Badge, Card, CardBody, CardHeader, EmptyState, TextInput, Tooltip } from "./ui";

const KNOWN_STATUS = new Set<string>(Object.keys(STATUS_DOT));

/**
 * 会话历史（痛点 #5「会话崩了，上下文找不回来」）
 *
 * 后端把每个会话的 `session_file` 一直记在 SQLite 里，进程退出也留着。
 * 想接着上次干，在这里按项目或终端搜出来，把路径复制走就能 `--resume`。
 */
export function HistoryPanel() {
  const { t } = useI18n();
  const history = useAppStore(selectSessionHistory);
  const savedQuery = useAppStore(selectHistoryQuery);
  const fetchSessionHistory = useAppStore((s) => s.fetchSessionHistory);
  const [query, setQuery] = useState(savedQuery);

  // 搜索是打一个字查一次库，节流一下；空查询也要走一遍，用来还原全部
  useEffect(() => {
    const timer = window.setTimeout(() => void fetchSessionHistory(query), 250);
    return () => window.clearTimeout(timer);
  }, [query, fetchSessionHistory]);

  return (
    <div className="mx-auto max-w-3xl space-y-4">
      <Card>
        <CardBody>
          <CardHeader className="mb-3" title={t("history.title")} desc={t("history.desc")} />
          <TextInput
            value={query}
            placeholder={t("history.search")}
            onChange={(e) => setQuery(e.target.value)}
          />

          {history.length === 0 ? (
            <EmptyState
              className="py-8"
              title={query ? t("history.no_match") : t("history.empty")}
            />
          ) : (
            <div className="mt-3 divide-y divide-neutral-100">
              {history.map((entry) => {
                const status = KNOWN_STATUS.has(entry.last_status)
                  ? (entry.last_status as SessionStatus)
                  : null;
                const terminal = [entry.terminal_app, entry.tty.replace("/dev/", "")]
                  .filter(Boolean)
                  .join(" ");

                return (
                  <div key={entry.session_key} className="flex items-start gap-3 py-3">
                    <span
                      className={cn(
                        "mt-1.5 h-1.5 w-1.5 shrink-0 rounded-full",
                        status ? STATUS_DOT[status] : "bg-neutral-300"
                      )}
                    />
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-1.5">
                        <Tooltip content={entry.working_dir}>
                          <span className="truncate font-mono text-[11px] text-neutral-700">
                            {baseName(entry.working_dir)}
                          </span>
                        </Tooltip>
                        <span className="text-[10px] text-neutral-400">{entry.agent_name}</span>
                        {status && (
                          <Badge tone={STATUS_TONE[status]}>{t(statusKey(status))}</Badge>
                        )}
                        {entry.resume_count > 0 && (
                          <Badge tone="violet">
                            {t("session.resumed", { count: entry.resume_count })}
                          </Badge>
                        )}
                      </div>
                      <p className="mt-1 text-[10px] tabular-nums text-neutral-400">
                        {t("history.seen", {
                          first: formatShortTime(entry.first_seen),
                          last: formatShortTime(entry.last_seen),
                        })}
                        {terminal && <span className="ml-2 font-mono">{terminal}</span>}
                      </p>
                      {entry.session_file && (
                        // 会话文件路径是「接着上次干」的入口，所以给一键复制
                        <CopyPath path={entry.session_file} />
                      )}
                    </div>
                    {entry.total_tokens > 0 && (
                      <span className="shrink-0 text-[10px] tabular-nums text-neutral-400">
                        {t("session.usage", {
                          tokens: formatTokens(entry.total_tokens),
                          cost: formatUsd(entry.cost_usd),
                        })}
                      </span>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </CardBody>
      </Card>
    </div>
  );
}

function CopyPath({ path }: { path: string }) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), 1500);
    return () => window.clearTimeout(timer);
  }, [copied]);

  return (
    <button
      type="button"
      title={path}
      onClick={() => {
        void navigator.clipboard?.writeText(path).then(() => setCopied(true));
      }}
      className="mt-1 flex max-w-full items-center gap-1.5 text-left text-[10px] text-neutral-300 transition-colors hover:text-neutral-500"
    >
      <span className="truncate font-mono">{path}</span>
      <span className="shrink-0 text-neutral-400">
        {copied ? t("common.copied") : t("common.copy")}
      </span>
    </button>
  );
}
