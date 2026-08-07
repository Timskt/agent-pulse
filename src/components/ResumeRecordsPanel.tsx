import { useEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "../i18n";
import {
  asOutcome,
  asStuckSecs,
  durationText,
  OUTCOME_GLYPH,
  OUTCOME_TONE,
  outcomeHintKey,
  outcomeKey,
} from "../lib/display";
import { baseName, cn, formatShortTime } from "../lib/utils";
import {
  RESUME_PAGE_SIZE,
  selectResumeFilter,
  selectResumeRecords,
  selectResumeRecordsTotal,
  useAppStore,
} from "../stores/useAppStore";
import type { ResumeRecord } from "../types";
import {
  Badge,
  Button,
  Card,
  CardBody,
  CardHeader,
  EmptyState,
  ExportButton,
  Select,
  type SelectOption,
  TextInput,
  Tooltip,
} from "./ui";

/**
 * 续跑记录中心
 *
 * 从统计页里抽出来单独成页，是因为这两件事的读法不一样：统计页回答
 * 「最近整体怎么样」，扫一眼就走；这一页回答「刚才那次为什么没敲进去」，
 * 是带着一个具体问题来翻的——那就需要搜索、筛选和翻页，而不是一个
 * 固定 50 条、只能上下滚的列表。
 *
 * 五态徽标是这一页的重点。以前只有 ✓ / ✗，「字发出去了但会话没反应」
 * 和「压根没发出去」挤在同一个 ✗ 里，可这两种情况要查的地方完全不同。
 */
export function ResumeRecordsPanel() {
  const { t } = useI18n();
  const records = useAppStore(selectResumeRecords);
  const total = useAppStore(selectResumeRecordsTotal);
  const filter = useAppStore(selectResumeFilter);
  const fetchResumeRecords = useAppStore((s) => s.fetchResumeRecords);
  const [draft, setDraft] = useState(filter.query);

  // 打字防抖 250ms，但**首屏不防抖**：挂载时也等 250ms 的话，切到这一页会先
  // 闪一下「还没有续跑记录」再换成真实列表，看着像数据丢了又回来。
  const typed = useRef(false);
  useEffect(() => {
    const wait = typed.current ? 250 : 0;
    typed.current = true;
    const timer = window.setTimeout(
      () => void fetchResumeRecords({ query: draft }),
      wait,
    );
    return () => window.clearTimeout(timer);
  }, [draft, fetchResumeRecords]);

  const outcomeOptions = useMemo<SelectOption<typeof filter.outcome>[]>(
    () => [
      { value: "all", label: t("records.all") },
      { value: "deferred", label: t("outcome.deferred") },
      { value: "landed", label: t("outcome.landed") },
      { value: "silent", label: t("outcome.silent") },
      { value: "failed", label: t("outcome.failed") },
      { value: "unverifiable", label: t("outcome.unverifiable") },
    ],
    [t],
  );

  const typeOptions = useMemo<SelectOption<typeof filter.promptType>[]>(
    () => [
      { value: "all", label: t("records.all") },
      { value: "goal", label: t("stats.prompt_goal") },
      { value: "generic", label: t("stats.prompt_generic") },
    ],
    [t],
  );

  const pageCount = Math.max(1, Math.ceil(total / RESUME_PAGE_SIZE));
  const page = Math.floor(filter.offset / RESUME_PAGE_SIZE);
  // 「没有匹配」和「还没有记录」要说不同的话：前者该提示换个词，
  // 后者该说明这一页什么时候会自己长出内容
  const filtered =
    filter.query !== "" ||
    filter.outcome !== "all" ||
    filter.promptType !== "all";

  return (
    <Card className="min-w-0 overflow-hidden">
      <CardBody className="resume-records-card-body min-w-0">
        <CardHeader
          className="resume-records-header mb-3"
          title={t("records.title")}
          desc={t("records.desc")}
          aside={
            <div className="flex flex-wrap items-center justify-end gap-2">
              <span className="text-[10px] tabular-nums text-neutral-400">
                {t("history.records", { count: total })}
              </span>
              <ExportButton
                command="export_resumes"
                args={{
                  query: filter.query,
                  outcome: filter.outcome,
                  promptType: filter.promptType,
                }}
              />
            </div>
          }
        />

        <div className="resume-records-filters flex min-w-0 flex-col gap-2 sm:flex-row">
          <TextInput
            value={draft}
            placeholder={t("records.search")}
            className="min-w-0 flex-1"
            onChange={(e) => setDraft(e.target.value)}
          />
          <div className="grid min-w-0 grid-cols-2 gap-2 sm:flex sm:shrink-0">
            <Select
              className="min-w-0 sm:w-32"
              value={filter.outcome}
              options={outcomeOptions}
              onValueChange={(outcome) => void fetchResumeRecords({ outcome })}
            />
            <Select
              className="min-w-0 sm:w-28"
              value={filter.promptType}
              options={typeOptions}
              onValueChange={(promptType) =>
                void fetchResumeRecords({ promptType })
              }
            />
          </div>
        </div>

        {records.length === 0 ? (
          <EmptyState
            className="py-8"
            title={filtered ? t("records.no_match") : t("records.empty")}
            hint={
              filtered ? t("records.no_match_hint") : t("records.empty_hint")
            }
          />
        ) : (
          <div className="mt-3 divide-y divide-neutral-100">
            {records.map((record) => (
              <RecordRow key={record.id} record={record} />
            ))}
          </div>
        )}

        {total > RESUME_PAGE_SIZE && (
          <div className="mt-4 flex min-w-0 items-center justify-between gap-2 border-t border-neutral-100 pt-3">
            <Button
              size="xs"
              variant="ghost"
              disabled={page === 0}
              onClick={() =>
                void fetchResumeRecords({
                  offset: (page - 1) * RESUME_PAGE_SIZE,
                })
              }
            >
              {t("history.previous")}
            </Button>
            <span className="min-w-0 text-center text-[10px] tabular-nums text-neutral-400">
              {t("history.page", { page: page + 1, total: pageCount })}
            </span>
            <Button
              size="xs"
              variant="ghost"
              disabled={page + 1 >= pageCount}
              onClick={() =>
                void fetchResumeRecords({
                  offset: (page + 1) * RESUME_PAGE_SIZE,
                })
              }
            >
              {t("history.next")}
            </Button>
          </div>
        )}
      </CardBody>
    </Card>
  );
}

/**
 * 一条记录
 *
 * 失败的那条把后端那句原因**完整展开**，不截断：这一行存在的全部意义就是
 * 让人知道为什么没敲进去，而「辅助功能没授权」这类话恰好都长。成功的那条
 * 只显示目录——它没什么要解释的。
 */
function RecordRow({ record }: { record: ResumeRecord }) {
  const { t } = useI18n();
  const outcome = asOutcome(record.outcome);
  const isGoal = record.prompt_type === "goal";
  // `-1` 是「算不出来」（旧记录、或者没有可读会话记录的 agent），那就不显示这个徽标。
  // 显示成「卡了 0 秒」会让人以为守护是瞬间反应的
  const stuck = asStuckSecs(record.stuck_secs);
  // 旧记录没有核验结论，退回朴素的成/败，不替它编一个当时不存在的结论
  const glyph = outcome ? OUTCOME_GLYPH[outcome] : record.success ? "✓" : "✗";
  const tone = outcome
    ? OUTCOME_TONE[outcome]
    : record.success
      ? "green"
      : "red";

  return (
    <div className="resume-record-row flex min-w-0 flex-wrap items-start gap-3 py-2.5">
      <Tooltip
        content={
          outcome ? t(outcomeHintKey(outcome)) : t("outcome.legacy_hint")
        }
      >
        <span
          className={cn(
            "mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full text-[10px] font-medium",
            GLYPH_SKIN[tone],
          )}
        >
          {glyph}
        </span>
      </Tooltip>
      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 flex-wrap items-center gap-1.5">
          <Tooltip content={record.working_dir}>
            <span
              tabIndex={0}
              aria-label={record.working_dir}
              className="min-w-0 max-w-full flex-1 basis-24 truncate select-text font-mono text-[11px] text-neutral-700 outline-none focus-visible:ring-2 focus-visible:ring-neutral-300"
            >
              {baseName(record.working_dir)}
            </span>
          </Tooltip>
          <span className="break-anywhere text-[10px] text-neutral-400">
            {record.agent_name}
          </span>
          <Badge tone={tone}>
            {outcome ? t(outcomeKey(outcome)) : t("outcome.legacy")}
          </Badge>
          {isGoal && <Badge tone="violet">{t("stats.prompt_goal")}</Badge>}
          {stuck !== null && (
            <Tooltip content={t("records.stuck_hint")}>
              <Badge tone="blue" className="cursor-default">
                {t("records.stuck", {
                  dur: durationText(stuck, t),
                })}
              </Badge>
            </Tooltip>
          )}
          <CopyTextButton text={record.working_dir} label={t("records.copy_dir")} />
        </div>
        {record.message && (
          <div className="mt-1 min-w-0">
            <p
              className={cn(
                "break-anywhere whitespace-pre-wrap select-text text-[10px] leading-relaxed",
                record.success ? "text-neutral-400" : "text-neutral-500",
              )}
            >
              {record.message}
            </p>
            <div className="mt-1 flex justify-end">
              <CopyTextButton text={record.message} label={t("records.copy_details")} />
            </div>
          </div>
        )}
      </div>
      <span className="resume-record-time shrink-0 text-[10px] tabular-nums text-neutral-300">
        {formatShortTime(record.created_at)}
      </span>
    </div>
  );
}

/**
 * 诊断记录里的复制操作。
 *
 * 路径和错误详情不能只依赖 hover tooltip：窄屏、触控和键盘用户都需要一个
 * 明确可达的操作。复制成功后短暂反馈，行为与会话详情抽屉保持一致。
 */
function CopyTextButton({ text, label }: { text: string; label: string }) {
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
      aria-label={label}
      onClick={() => {
        if (!navigator.clipboard) return;
        void navigator.clipboard
          .writeText(text)
          .then(() => setCopied(true))
          .catch(() => undefined);
      }}
    >
      {copied ? t("common.copied") : label}
    </Button>
  );
}

/**
 * 圆标的底色
 *
 * 刻意不复用 `Badge` 的配色实现：这里是个实心圆，浅底 + 深字才看得清，
 * 而徽标是圆角矩形，两者的对比度需求不一样。
 */
const GLYPH_SKIN: Record<string, string> = {
  green: "bg-emerald-50 text-emerald-600",
  amber: "bg-amber-50 text-amber-600",
  red: "bg-red-50 text-red-500",
  neutral: "bg-neutral-100 text-neutral-400",
  violet: "bg-violet-50 text-violet-600",
  blue: "bg-blue-50 text-blue-600",
};
