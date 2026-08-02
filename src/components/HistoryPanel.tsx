import { useEffect, useState } from "react";
import { useI18n } from "../i18n";
import { STATUS_DOT, STATUS_TONE, statusKey } from "../lib/display";
import { baseName, cn, formatShortTime, formatTokens, formatUsd } from "../lib/utils";
import { selectHistoryQuery, selectSessionHistory, selectSessionHistoryTotal, useAppStore } from "../stores/useAppStore";
import type { SessionStatus } from "../types";
import { Badge, Button, Card, CardBody, CardHeader, EmptyState, TextInput, Tooltip } from "./ui";

const PAGE_SIZE = 20;
const KNOWN_STATUS = new Set<string>(Object.keys(STATUS_DOT));

export function HistoryPanel() {
  const { t } = useI18n();
  const history = useAppStore(selectSessionHistory);
  const total = useAppStore(selectSessionHistoryTotal);
  const savedQuery = useAppStore(selectHistoryQuery);
  const fetchSessionHistory = useAppStore((s) => s.fetchSessionHistory);
  const [query, setQuery] = useState(savedQuery);
  const [page, setPage] = useState(0);

  useEffect(() => {
    const timer = window.setTimeout(() => void fetchSessionHistory(query, page * PAGE_SIZE), 250);
    return () => window.clearTimeout(timer);
  }, [query, page, fetchSessionHistory]);

  const pageCount = Math.max(1, Math.ceil(total / PAGE_SIZE));
  return (
    <div className="mx-auto max-w-3xl space-y-4">
      <Card>
        <CardBody>
          <CardHeader className="mb-3" title={t("history.title")} desc={t("history.desc")} aside={<span className="text-[10px] tabular-nums text-neutral-400">{t("history.records", { count: total })}</span>} />
          <TextInput value={query} placeholder={t("history.search")} onChange={(e) => { setQuery(e.target.value); setPage(0); }} />
          {history.length === 0 ? <EmptyState className="py-8" title={query ? t("history.no_match") : t("history.empty")} /> : <div className="mt-3 divide-y divide-neutral-100">{history.map((entry) => <HistoryRow key={entry.session_key} entry={entry} />)}</div>}
          {total > PAGE_SIZE && <Pager page={page} pageCount={pageCount} onChange={setPage} />}
        </CardBody>
      </Card>
    </div>
  );
}

function HistoryRow({ entry }: { entry: import("../types").SessionHistoryEntry }) {
  const { t } = useI18n();
  const status = KNOWN_STATUS.has(entry.last_status) ? (entry.last_status as SessionStatus) : null;
  const terminal = [entry.terminal_app, entry.tty.replace("/dev/", "")].filter(Boolean).join(" ");
  return <div className="flex items-start gap-3 py-3"><span className={cn("mt-1.5 h-1.5 w-1.5 shrink-0 rounded-full", status ? STATUS_DOT[status] : "bg-neutral-300")} /><div className="min-w-0 flex-1"><div className="flex flex-wrap items-center gap-1.5"><Tooltip content={entry.working_dir}><span className="truncate font-mono text-[11px] text-neutral-700">{baseName(entry.working_dir)}</span></Tooltip><span className="text-[10px] text-neutral-400">{entry.agent_name}</span>{status && <Badge tone={STATUS_TONE[status]}>{t(statusKey(status))}</Badge>}{entry.resume_count > 0 && <Badge tone="violet">{t("session.resumed", { count: entry.resume_count })}</Badge>}</div><p className="mt-1 text-[10px] tabular-nums text-neutral-400">{t("history.seen", { first: formatShortTime(entry.first_seen), last: formatShortTime(entry.last_seen) })}{terminal && <span className="ml-2 font-mono">{terminal}</span>}</p>{entry.session_file && <CopyPath path={entry.session_file} />}</div>{entry.total_tokens > 0 && <span className="shrink-0 text-[10px] tabular-nums text-neutral-400">{t("session.usage", { tokens: formatTokens(entry.total_tokens), cost: formatUsd(entry.cost_usd) })}</span>}</div>;
}

function Pager({ page, pageCount, onChange }: { page: number; pageCount: number; onChange: (page: number) => void }) {
  const { t } = useI18n();
  return <div className="mt-4 flex items-center justify-between border-t border-neutral-100 pt-3"><Button size="xs" variant="ghost" disabled={page === 0} onClick={() => onChange(page - 1)}>{t("history.previous")}</Button><span className="text-[10px] tabular-nums text-neutral-400">{t("history.page", { page: page + 1, total: pageCount })}</span><Button size="xs" variant="ghost" disabled={page + 1 >= pageCount} onClick={() => onChange(page + 1)}>{t("history.next")}</Button></div>;
}

function CopyPath({ path }: { path: string }) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);
  useEffect(() => { if (!copied) return; const timer = window.setTimeout(() => setCopied(false), 1500); return () => window.clearTimeout(timer); }, [copied]);
  return <button type="button" title={path} onClick={() => { void navigator.clipboard?.writeText(path).then(() => setCopied(true)); }} className="mt-1 flex max-w-full items-center gap-1.5 text-left text-[10px] text-neutral-300 transition-colors hover:text-neutral-500"><span className="truncate font-mono">{path}</span><span className="shrink-0 text-neutral-400">{copied ? t("common.copied") : t("common.copy")}</span></button>;
}
