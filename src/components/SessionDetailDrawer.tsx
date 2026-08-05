import { useEffect, useState } from "react";
import { useI18n } from "../i18n";
import {
  asOutcome,
  asReason,
  asStuckSecs,
  durationText,
  OUTCOME_GLYPH,
  OUTCOME_TONE,
  outcomeKey,
  reasonKey,
  STATUS_TONE,
  statusKey,
} from "../lib/display";
import {
  baseName,
  cn,
  formatShortTime,
  formatTokens,
  formatUsd,
  secondsBetween,
} from "../lib/utils";
import {
  selectDetailKey,
  selectSessionDetail,
  useAppStore,
} from "../stores/useAppStore";
import type {
  DetectionRecord,
  ResumeRecord,
  SessionDetail,
  SessionStatus,
} from "../types";
import { Badge, Button, Drawer, DrawerRow, DrawerSection } from "./ui";

const KNOWN_STATUS = new Set<string>(Object.keys(STATUS_TONE));

/**
 * 会话档案抽屉
 *
 * 存在的理由是列表行答不了的那个问题：「这个会话到底经历了什么」。
 * 列表一行只能放下项目名、状态和花费；而用户真正带着问题来翻历史的时候，
 * 想知道的是「它被打断过几次」「每次续跑敲进去了吗」「最后那次为什么失败」。
 *
 * 三张表按 `session_id` 关起来（`session_history` / `resume_records` /
 * `detection_records`），后端一次给全，这里只负责画。
 */
export function SessionDetailDrawer() {
  const { t } = useI18n();
  const detailKey = useAppStore(selectDetailKey);
  const detail = useAppStore(selectSessionDetail);
  const closeSessionDetail = useAppStore((s) => s.closeSessionDetail);

  return (
    <Drawer
      open={detailKey !== null}
      onClose={closeSessionDetail}
      title={
        detail ? baseName(detail.entry.working_dir) : t("history.detail_title")
      }
      desc={detail?.entry.working_dir}
      footer={detail && <CopyRow detail={detail} />}
    >
      {!detail ? (
        // `detailKey` 有值但档案还没到 = 正在读；到了却是 `null` = 库里没这行
        <p className="py-8 text-center text-[11px] text-neutral-400">
          {t("history.detail_loading")}
        </p>
      ) : (
        <DetailBody detail={detail} />
      )}
    </Drawer>
  );
}

function DetailBody({ detail }: { detail: SessionDetail }) {
  const { t } = useI18n();
  const { entry, resumes, detections } = detail;
  const live = entry.ended_at === "";
  const status = KNOWN_STATUS.has(entry.last_status)
    ? (entry.last_status as SessionStatus)
    : null;
  const end = entry.ended_at || entry.last_seen;
  const secs = secondsBetween(entry.first_seen, end);
  const terminal = [entry.terminal_app, entry.tty.replace("/dev/", "")]
    .filter(Boolean)
    .join(" ");

  return (
    <>
      <DrawerSection
        title={t("history.lifecycle")}
        aside={
          live ? (
            <Badge tone="green">{t("history.live")}</Badge>
          ) : (
            <Badge tone="neutral">{t("history.ended")}</Badge>
          )
        }
      >
        <DrawerRow
          label={t("history.first_seen_at")}
          value={formatShortTime(entry.first_seen)}
        />
        <DrawerRow
          label={live ? t("history.last_seen_at") : t("history.ended")}
          value={formatShortTime(end)}
        />
        {secs !== null && (
          <DrawerRow
            label={t("history.duration")}
            value={durationText(secs, t)}
          />
        )}
        {status && (
          <DrawerRow
            label={t("history.final_status")}
            value={
              <Badge tone={STATUS_TONE[status]}>{t(statusKey(status))}</Badge>
            }
          />
        )}
        <DrawerRow label={t("history.agent")} value={entry.agent_name} />
        {terminal && (
          <DrawerRow
            label={t("history.terminal")}
            value={<span className="font-mono">{terminal}</span>}
          />
        )}
      </DrawerSection>

      {entry.total_tokens > 0 && (
        <DrawerSection title={t("history.detail_usage")}>
          <DrawerRow
            label={t("history.detail_tokens")}
            value={formatTokens(entry.total_tokens)}
          />
          <DrawerRow
            label={t("history.sum_cost")}
            value={`$${formatUsd(entry.cost_usd)}`}
          />
        </DrawerSection>
      )}

      <DrawerSection
        title={t("history.detection_timeline")}
        aside={
          detections.length > 0 && (
            <span className="text-[10px] tabular-nums text-neutral-400">
              {t("history.interruptions", { count: detections.length })}
            </span>
          )
        }
      >
        {detections.length === 0 ? (
          <p className="text-[11px] text-neutral-400">
            {t("history.no_interruptions")}
          </p>
        ) : (
          <Timeline>
            {detections.map((record) => (
              <DetectionItem key={record.id} record={record} />
            ))}
          </Timeline>
        )}
      </DrawerSection>

      <DrawerSection
        title={t("history.resume_timeline")}
        aside={
          resumes.length > 0 && (
            <span className="text-[10px] tabular-nums text-neutral-400">
              {t("session.resumed", { count: resumes.length })}
            </span>
          )
        }
      >
        {resumes.length === 0 ? (
          <p className="text-[11px] text-neutral-400">{t("history.no_resumes")}</p>
        ) : (
          <Timeline>
            {resumes.map((record) => (
              <ResumeItem key={record.id} record={record} />
            ))}
          </Timeline>
        )}
      </DrawerSection>
    </>
  );
}

/** 左侧一条竖线串起来的时间线 */
function Timeline({ children }: { children: React.ReactNode }) {
  return (
    <ol className="space-y-2.5 border-l border-neutral-150 pl-3">{children}</ol>
  );
}

function DetectionItem({ record }: { record: DetectionRecord }) {
  const { t } = useI18n();
  const reason = asReason(record.reason);
  return (
    <li className="relative">
      <Dot className="bg-red-400" />
      <div className="flex items-center gap-1.5">
        <span className="text-[11px] tabular-nums text-neutral-500">
          {formatShortTime(record.created_at)}
        </span>
        {/* v1.6 之前的行没存原因，那就少说一句，不编 */}
        {reason && <Badge tone="neutral">{t(reasonKey(reason))}</Badge>}
        {record.has_active_goal && (
          <Badge tone="violet">{t("stats.prompt_goal")}</Badge>
        )}
      </div>
    </li>
  );
}

function ResumeItem({ record }: { record: ResumeRecord }) {
  const { t } = useI18n();
  const outcome = asOutcome(record.outcome);
  const stuck = asStuckSecs(record.stuck_secs);
  return (
    <li className="relative">
      <Dot
        className={
          outcome === "landed"
            ? "bg-emerald-400"
            : outcome === "failed"
              ? "bg-red-400"
              : "bg-amber-400"
        }
      />
      <div className="flex flex-wrap items-center gap-1.5">
        <span className="text-[11px] tabular-nums text-neutral-500">
          {formatShortTime(record.created_at)}
        </span>
        {outcome ? (
          <Badge tone={OUTCOME_TONE[outcome]}>
            <span aria-hidden="true">{OUTCOME_GLYPH[outcome]}</span>
            {t(outcomeKey(outcome))}
          </Badge>
        ) : (
          // 空串是 v1.6 之前的行：那时只存了成/败，别拿 success 补一个当时
          // 并不存在的核验结论
          <Badge tone="neutral">{t("outcome.legacy")}</Badge>
        )}
        {stuck !== null && (
          <span className="text-[10px] text-neutral-400">
            {t("history.stuck_for", { duration: durationText(stuck, t) })}
          </span>
        )}
      </div>
      {record.message && (
        <p className="mt-0.5 break-words text-[10px] leading-relaxed text-neutral-400">
          {record.message}
        </p>
      )}
    </li>
  );
}

function Dot({ className }: { className: string }) {
  return (
    <span
      className={cn(
        "absolute -left-[15px] top-1.5 h-1.5 w-1.5 rounded-full ring-2 ring-white",
        className,
      )}
    />
  );
}

/** 抽屉底部两个复制按钮：把路径交回给用户的终端 */
function CopyRow({ detail }: { detail: SessionDetail }) {
  const { t } = useI18n();
  return (
    <div className="flex items-center gap-2">
      <CopyButton
        text={detail.entry.working_dir}
        label={t("history.copy_dir")}
      />
      {detail.entry.session_file ? (
        <CopyButton
          text={detail.entry.session_file}
          label={t("history.copy_transcript")}
        />
      ) : (
        <span className="text-[10px] text-neutral-300">
          {t("history.no_transcript")}
        </span>
      )}
    </div>
  );
}

function CopyButton({ text, label }: { text: string; label: string }) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), 1500);
    return () => window.clearTimeout(timer);
  }, [copied]);

  return (
    <Button
      size="xs"
      variant="ghost"
      title={text}
      onClick={() => {
        void navigator.clipboard?.writeText(text).then(() => setCopied(true));
      }}
    >
      {copied ? t("common.copied") : label}
    </Button>
  );
}
