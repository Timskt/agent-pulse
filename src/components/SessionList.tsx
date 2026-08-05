import { useId, useMemo, useState } from "react";
import { useI18n } from "../i18n";
import {
  ATTENTION_ICON,
  ATTENTION_TONE,
  attentionKey,
  reasonKey,
  STATUS_DOT,
  STATUS_TONE,
  statusKey,
  TACTIC_NOTE,
} from "../lib/display";
import { baseName, cn, formatTokens, formatUsd } from "../lib/utils";
import { useNotice, type Notice } from "../lib/useNotice";
import {
  selectConfig,
  selectFocusedSessionId,
  selectSessions,
  useAppStore,
} from "../stores/useAppStore";
import type {
  AgentSession,
  AttentionLevel,
  DetectionEvidence,
  ResumeProbe,
} from "../types";
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
          b.last_activity.localeCompare(a.last_activity),
      ),
    [sessions],
  );

  return (
    <Card className="overflow-hidden">
      <CardBar>
        <h3 className="text-xs font-semibold text-neutral-800">
          {t("session.title")}
        </h3>
        <span className="text-[10px] tabular-nums text-neutral-400">
          {ordered.length}
        </span>
      </CardBar>

      {notice && (
        <p
          className={cn(
            "border-b border-neutral-100 px-4 py-2 text-[11px] leading-relaxed",
            notice.ok ? "text-emerald-600" : "text-red-500",
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
              maxNudges={config?.max_resume_count ?? 0}
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
  maxNudges,
  onNotice,
}: {
  session: AgentSession;
  focused: boolean;
  aiEnabled: boolean;
  /** 连着催几次没反应就停手；0 表示没配上限 */
  maxNudges: number;
  onNotice: (notice: Notice) => void;
}) {
  const { t } = useI18n();
  const manualResume = useAppStore((s) => s.manualResume);
  const focusTerminal = useAppStore((s) => s.focusTerminal);
  const aiAnalyze = useAppStore((s) => s.aiAnalyze);
  const probeResume = useAppStore((s) => s.probeResume);
  const [busy, setBusy] = useState(false);
  const [probe, setProbe] = useState<ResumeProbe | null>(null);
  const [probing, setProbing] = useState(false);
  const [evidenceOpen, setEvidenceOpen] = useState(false);
  // 每张卡片一个：列表里同时展开好几张的话，写死的 id 会重复，
  // aria-controls 就会指向别人那块
  const evidenceId = useId();

  const stalled =
    session.status === "interrupted" || session.status === "suspended";
  const attention = session.attention;

  /**
   * 「为什么停」和「这次怎么办」
   *
   * 两个都由后端算好发上来，这里一个字都不推。`tacticNote` 是那句解释，
   * 它的措辞里已经把原因念了一遍，所以下面那枚原因小标签只在**没有**
   * 解释句的时候才挂——同一句话在同一行出现两遍，看着像两件事。
   */
  const reason =
    session.interrupt_reason === "none" ? null : session.interrupt_reason;
  const tacticNote =
    session.resume_tactic === "nudge"
      ? null
      : TACTIC_NOTE[session.resume_tactic];

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

  /**
   * 演练：定位但不投递
   *
   * 结果展开在行内而不是弹窗——它要跟这一行的 TTY、终端名一起看才有意义。
   * 再点一次收起，省一个关闭按钮。
   */
  const runProbe = async () => {
    if (probe) {
      setProbe(null);
      return;
    }
    setProbing(true);
    try {
      setProbe(await probeResume(session.id));
    } catch (e) {
      onNotice({
        ok: false,
        message: t("common.error", { detail: String(e) }),
      });
    } finally {
      setProbing(false);
    }
  };

  return (
    <div
      className={cn(
        "transition-colors",
        // 最近一次提醒指向的会话高亮，点通知过来时不用自己找
        focused ? "bg-amber-50/70" : "hover:bg-neutral-50/60",
      )}
    >
      <div className="flex flex-col gap-2 px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5">
            <span
              className={cn(
                "h-1.5 w-1.5 shrink-0 rounded-full",
                STATUS_DOT[session.status],
              )}
            />
            <span className="truncate text-xs font-medium text-neutral-800">
              {session.agent_name}
            </span>
            <Badge tone={STATUS_TONE[session.status]}>
              {t(statusKey(session.status))}
            </Badge>
            {attention !== "none" && (
              <Badge tone={ATTENTION_TONE[attention]}>
                {ATTENTION_ICON[attention]} {t(attentionKey(attention))}
              </Badge>
            )}
            {session.resume_count > 0 && (
              <Badge tone="violet">
                {t("session.resumed", { count: session.resume_count })}
              </Badge>
            )}
            {/* 敲不进去要当场说。这个功能平时不出声，所以「不出声」不能同时
                是它坏掉的样子——否则用户只能靠「怎么一直没人帮我按继续」猜。 */}
            {session.resume_failures > 0 && (
              <Badge tone="red">
                {t("session.resume_failing", {
                  count: session.resume_failures,
                })}
              </Badge>
            )}
            {maxNudges > 0 && session.resume_streak >= maxNudges && (
              <Badge tone="amber">{t("session.stood_down")}</Badge>
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
            {/* 「为什么停」放在这一行而不是上面的徽标区：级别徽标是在叫人，
                原因只是陈述事实，两者抢同一种颜色只会让人分不清哪个要理。 */}
            {reason && !tacticNote && (
              <span className="text-neutral-500">{t(reasonKey(reason))}</span>
            )}
          </div>

          {session.attention_detail && (
            <p className="mt-1 truncate text-[10px] leading-relaxed text-neutral-500">
              {session.attention_detail}
            </p>
          )}

          {/* 「这次故意没敲字」必须写出来。三种情况下敲字帮不上忙甚至帮倒忙
              （进程没了、撞限流、它在问一个具体问题），可界面上只写「已中断」
              的话，用户看到的是守护神漏了一次，而不是它做了一个正确的决定。
              手段由后端算好发上来，这里只画，不重推一遍原因表。 */}
          {tacticNote && reason && (
            <p className="mt-1 text-[10px] leading-relaxed text-amber-600">
              {t(tacticNote, { reason: t(reasonKey(reason)) })}
            </p>
          )}
        </div>

        <div className="flex shrink-0 flex-wrap items-center gap-1">
          {session.detection_evidence && (
            <Button
              size="xs"
              variant="ghost"
              disabled={busy || probing}
              // 按钮的字已经会在「查看判据 / 收起」之间换，所以状态本身不算丢。
              // 补这两条是为了把按钮和它展开的那块**关联**起来：读屏才能说出
              // 「已展开」并直接跳到那块内容，而不是让人自己在页面里找。
              //
              // `aria-controls` 只在真的展开时才给：收起时那块根本没渲染，
              // 指过去就是一个悬空的 id，而 ARIA 要求它必须指到存在的元素
              aria-expanded={evidenceOpen}
              aria-controls={evidenceOpen ? evidenceId : undefined}
              onClick={() => setEvidenceOpen((open) => !open)}
            >
              {evidenceOpen ? t("evidence.hide") : t("evidence.button")}
            </Button>
          )}
          <Tooltip content={t("probe.nothing_typed")}>
            <Button
              size="xs"
              variant="ghost"
              disabled={busy || probing}
              onClick={runProbe}
            >
              {probing ? t("probe.running") : t("probe.button")}
            </Button>
          </Tooltip>
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

      {probe && <ProbePanel probe={probe} onClose={() => setProbe(null)} />}
      {evidenceOpen && session.detection_evidence && (
        <EvidencePanel id={evidenceId} evidence={session.detection_evidence} />
      )}
    </div>
  );
}

/** 判定证据：展示事实，不在前端重算结论 */
function EvidencePanel({ id, evidence }: { id?: string; evidence: DetectionEvidence }) {
  const { t } = useI18n();
  const turn = t(`evidence.turn.${evidence.turn_state}` as Parameters<typeof t>[0]);
  const signalKinds = Array.from(
    new Set(
      evidence.signal_kinds.map((kind) =>
        kind === "file_stale" || kind === "heartbeat_timeout"
          ? "evidence.signal.transcript_idle"
          : `evidence.signal.${kind}`,
      ),
    ),
  )
    .map((key) => t(key as Parameters<typeof t>[0]))
    .join("、");
  return (
    <div id={id} className="border-t border-neutral-100 bg-sky-50/50 px-4 py-3">
      <p className="text-[11px] font-semibold text-neutral-700">{t("evidence.title")}</p>
      <div className="mt-1.5 grid grid-cols-1 gap-1 text-[10px] text-neutral-600 sm:grid-cols-2">
        <span>{t("evidence.signals")}: {signalKinds || t("evidence.none")}</span>
        <span>{t("evidence.process")}: {evidence.process_alive ? t("evidence.yes") : t("evidence.no")}</span>
        <span>{t("evidence.turn")}: {turn}</span>
        <span>{t("evidence.grace")}: ×{evidence.busy_grace_multiplier}</span>
        <span>{t("evidence.keyword")}: {evidence.matched_interrupt_keyword ?? t("evidence.none")}</span>
        <span>{t("evidence.completion")}: {evidence.matched_completion_marker ?? t("evidence.none")}</span>
        <span>{t("evidence.second_opinion")}: {evidence.second_opinion ? t(`evidence.opinion.${evidence.second_opinion}` as Parameters<typeof t>[0]) : t("evidence.none")}</span>
      </div>
    </div>
  );
}

/** 演练结果：定位到哪儿、会不会敲进去、卡在哪一环 */
function ProbePanel({
  probe,
  onClose,
}: {
  probe: ResumeProbe;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const openAccessibilitySettings = useAppStore(
    (s) => s.openAccessibilitySettings,
  );

  const tone = probe.would_deliver
    ? probe.certainty === "exact"
      ? "green"
      : "amber"
    : "red";

  return (
    <div className="border-t border-neutral-100 bg-neutral-50/80 px-4 py-3">
      <div className="flex flex-wrap items-center gap-1.5">
        <span className="text-[11px] font-semibold text-neutral-700">
          {t("probe.title")}
        </span>
        <Badge tone={tone}>{probe.certainty_label}</Badge>
        <Badge tone={probe.would_deliver ? "green" : "red"}>
          {probe.would_deliver
            ? t("probe.would_deliver")
            : t("probe.would_not_deliver")}
        </Badge>
        <span className="ml-auto flex items-center gap-1">
          {probe.needs_permission_fix && (
            <Button
              size="xs"
              variant="primary"
              onClick={() => openAccessibilitySettings()}
            >
              {t("probe.fix_accessibility")}
            </Button>
          )}
          <Button size="xs" variant="ghost" onClick={onClose}>
            {t("probe.close")}
          </Button>
        </span>
      </div>

      <p className="mt-1.5 whitespace-pre-line text-[11px] leading-relaxed text-neutral-600">
        {probe.detail}
      </p>

      <div className="mt-2 flex flex-wrap gap-x-3 gap-y-0.5 text-[10px] text-neutral-400">
        <span>
          {t("probe.channel")}:{" "}
          <span className="text-neutral-600">{probe.channel}</span>
        </span>
        {probe.target && (
          <span>
            {t("probe.target")}:{" "}
            <span className="font-mono text-neutral-600">{probe.target}</span>
          </span>
        )}
        {probe.tty && <span className="font-mono">{probe.tty}</span>}
      </div>

      {probe.tools.length > 0 && (
        <div className="mt-2">
          <p className="text-[10px] font-medium text-neutral-500">
            {t("probe.deps")}
          </p>
          <ul className="mt-1 space-y-0.5">
            {probe.tools.map((tool) => (
              <li
                key={tool.name}
                className="flex flex-wrap items-baseline gap-1.5 text-[10px]"
              >
                <span
                  className={cn(
                    "h-1.5 w-1.5 shrink-0 rounded-full",
                    tool.available ? "bg-emerald-500" : "bg-neutral-300",
                  )}
                />
                <span className="font-mono text-neutral-600">{tool.name}</span>
                <span
                  className={
                    tool.available ? "text-emerald-600" : "text-neutral-400"
                  }
                >
                  {tool.available ? t("probe.dep_ok") : t("probe.dep_missing")}
                </span>
                <span className="text-neutral-400">— {tool.purpose}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
