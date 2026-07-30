import { useMemo, useState } from "react";
import { useI18n } from "../i18n";
import {
  ATTENTION_ICON,
  ATTENTION_TONE,
  attentionKey,
  STATUS_DOT,
  STATUS_TONE,
  statusKey,
} from "../lib/display";
import { baseName, cn, formatTokens, formatUsd } from "../lib/utils";
import { useNotice, type Notice } from "../lib/useNotice";
import {
  selectConfig,
  selectFocusedSessionId,
  selectSessions,
  useAppStore,
} from "../stores/useAppStore";
import type { AgentSession, AttentionLevel } from "../types";
import { Badge, Button, Card, CardBar, EmptyState, Tooltip } from "./ui";

/**
 * 会话列表
 *
 * 痛点 #2 是「5-10 个终端标签页，认不出哪个是哪个」，所以每行都要说清
 * 「这是谁、在哪、要不要理它」：项目名、TTY、注意力级别、花费。
 * 排序也按「谁在等我」而不是发现顺序——需要人介入的永远浮在最上面。
 */

/** 注意力级别的紧急程度，数字越小越靠前 */
const ATTENTION_WEIGHT: Record<AttentionLevel, number> = {
  needs_input: 0,
  error: 1,
  rate_limited: 2,
  completed: 3,
  none: 4,
};

const STATUS_WEIGHT: Record<AgentSession["status"], number> = {
  interrupted: 0,
  suspended: 1,
  active: 2,
  completed: 3,
  exited: 4,
};

export function SessionList() {
  const { t } = useI18n();
  const sessions = useAppStore(selectSessions);
  const focusedSessionId = useAppStore(selectFocusedSessionId);
  const config = useAppStore(selectConfig);
  const { notice, show } = useNotice();

  const ordered = useMemo(
    () =>
      [...sessions].sort(
        (a, b) =>
          ATTENTION_WEIGHT[a.attention] - ATTENTION_WEIGHT[b.attention] ||
          STATUS_WEIGHT[a.status] - STATUS_WEIGHT[b.status] ||
          b.last_activity.localeCompare(a.last_activity)
      ),
    [sessions]
  );

  return (
    <Card className="overflow-hidden">
      <CardBar>
        <h3 className="text-xs font-semibold text-neutral-800">{t("session.title")}</h3>
        <span className="text-[10px] tabular-nums text-neutral-400">{ordered.length}</span>
      </CardBar>

      {notice && (
        <p
          className={cn(
            "border-b border-neutral-100 px-4 py-2 text-[11px] leading-relaxed",
            notice.ok ? "text-emerald-600" : "text-red-500"
          )}
        >
          {notice.message}
        </p>
      )}

      {ordered.length === 0 ? (
        <EmptyState title={t("session.empty")} hint={t("session.empty_hint")} />
      ) : (
        <div className="divide-y divide-neutral-100">
          {ordered.map((session) => (
            <SessionRow
              key={session.id}
              session={session}
              focused={session.id === focusedSessionId}
              aiEnabled={config?.ai_judge.enabled ?? false}
              onNotice={show}
            />
          ))}
        </div>
      )}
    </Card>
  );
}

function SessionRow({
  session,
  focused,
  aiEnabled,
  onNotice,
}: {
  session: AgentSession;
  focused: boolean;
  aiEnabled: boolean;
  onNotice: (notice: Notice) => void;
}) {
  const { t } = useI18n();
  const manualResume = useAppStore((s) => s.manualResume);
  const focusTerminal = useAppStore((s) => s.focusTerminal);
  const aiAnalyze = useAppStore((s) => s.aiAnalyze);
  const [busy, setBusy] = useState(false);

  const stalled = session.status === "interrupted" || session.status === "suspended";
  const attention = session.attention;

  /** 三个按钮的公共部分：跑的时候整行按钮禁用，回来的一句话原样展示 */
  const act = async (run: () => Promise<Notice>) => {
    setBusy(true);
    try {
      const result = await run();
      if (result.message) onNotice(result);
    } finally {
      setBusy(false);
    }
  };

  const analyze = () =>
    act(async () => {
      try {
        const verdict = await aiAnalyze(session.id);
        return { ok: !verdict.is_interrupted, message: verdict.reasoning };
      } catch (e) {
        return { ok: false, message: t("common.error", { detail: String(e) }) };
      }
    });

  return (
    <div
      className={cn(
        "flex flex-col gap-2 px-4 py-3 transition-colors sm:flex-row sm:items-center sm:justify-between",
        // 最近一次提醒指向的会话高亮，点通知过来时不用自己找
        focused ? "bg-amber-50/70" : "hover:bg-neutral-50/60"
      )}
    >
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-1.5">
          <span className={cn("h-1.5 w-1.5 shrink-0 rounded-full", STATUS_DOT[session.status])} />
          <span className="truncate text-xs font-medium text-neutral-800">
            {session.agent_name}
          </span>
          <Badge tone={STATUS_TONE[session.status]}>{t(statusKey(session.status))}</Badge>
          {attention !== "none" && (
            <Badge tone={ATTENTION_TONE[attention]}>
              {ATTENTION_ICON[attention]} {t(attentionKey(attention))}
            </Badge>
          )}
          {session.resume_count > 0 && (
            <Badge tone="violet">{t("session.resumed", { count: session.resume_count })}</Badge>
          )}
        </div>

        <div className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[10px] text-neutral-400">
          <Tooltip content={session.working_dir || session.command}>
            <span className="max-w-[15rem] truncate font-mono">
              {baseName(session.working_dir) || session.command}
            </span>
          </Tooltip>
          <span>{t("session.pid", { pid: session.pid })}</span>
          {session.tty && (
            // 多标签页时这一串才是「哪个窗口」的答案
            <span className="font-mono">
              {[session.terminal_app, session.tty.replace("/dev/", "")]
                .filter(Boolean)
                .join(" ")}
            </span>
          )}
          {session.usage && session.usage.total_tokens > 0 && (
            <span className="tabular-nums">
              {t("session.usage", {
                tokens: formatTokens(session.usage.total_tokens),
                cost: formatUsd(session.usage.cost_usd),
              })}
            </span>
          )}
        </div>

        {session.attention_detail && (
          <p className="mt-1 truncate text-[10px] leading-relaxed text-neutral-500">
            {session.attention_detail}
          </p>
        )}
      </div>

      <div className="flex shrink-0 flex-wrap items-center gap-1">
        {session.tty && (
          <Button
            size="xs"
            variant="ghost"
            disabled={busy}
            onClick={() => act(() => focusTerminal(session.id))}
          >
            {t("session.focus")}
          </Button>
        )}
        {aiEnabled && (
          <Button size="xs" variant="ghost" disabled={busy} onClick={analyze}>
            {t("session.analyze")}
          </Button>
        )}
        <Button
          size="xs"
          variant="outline"
          disabled={busy}
          onClick={() => act(() => manualResume(session.id, true))}
        >
          {t("session.resume_goal")}
        </Button>
        <Button
          size="xs"
          variant={stalled ? "primary" : "outline"}
          disabled={busy}
          onClick={() => act(() => manualResume(session.id, false))}
        >
          {t("session.resume")}
        </Button>
      </div>
    </div>
  );
}
