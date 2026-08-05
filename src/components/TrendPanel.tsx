import { useI18n, type I18nKey, type Translator } from "../i18n";
import {
  deltaTone,
  durationParts,
  tileView,
  type MetricPolarity,
  type MetricUnit,
} from "../lib/display";
import {
  selectStatsTrend,
  selectTrendWindow,
  useAppStore,
} from "../stores/useAppStore";
import type { TrendMetric } from "../types";
import {
  Badge,
  Card,
  CardBody,
  CardHeader,
  EmptyState,
  Segmented,
  type SegmentedOption,
  Tooltip,
} from "./ui";

/** 一个指标怎么读、涨了算好还是算坏 */
interface MetricSpec {
  key: keyof Pick<
    import("../types").StatsTrend,
    "interruptions" | "resumes" | "landed_rate" | "stuck_secs"
  >;
  label: I18nKey;
  unit: MetricUnit;
  polarity: MetricPolarity;
}

/**
 * 四个指标的顺序就是阅读顺序
 *
 * 前两个是「发生了多少事」，后两个是「我们干得怎么样」。中断次数排第一是因为
 * 它不由我们决定——先看清外部情况，再看自己的表现，否则「成功率跌了」会
 * 被读成守护变差了，其实是那一期一次中断都没有、分母只有 1。
 */
const METRICS: readonly MetricSpec[] = [
  { key: "interruptions", label: "trend.interruptions", unit: "count", polarity: "up_is_bad" },
  // 续跑次数刻意不上色：变多既可能是会话老卡（坏），也可能是以前漏了现在管上了（好）
  { key: "resumes", label: "trend.resumes", unit: "count", polarity: "neutral" },
  { key: "landed_rate", label: "trend.landed_rate", unit: "percent", polarity: "up_is_good" },
  { key: "stuck_secs", label: "trend.stuck_secs", unit: "duration", polarity: "up_is_bad" },
];

/**
 * 趋势卡：本期 vs 上期
 *
 * 这一页原先只有累计数。累计数回答不了用户真正在问的那个问题——
 * 「装了这东西之后，情况在变好吗」。一个跑了三个月的库，累计成功率永远稳在
 * 某个数附近，昨天开始失灵了也看不出来。
 */
export function TrendPanel() {
  const { t } = useI18n();
  const trend = useAppStore(selectStatsTrend);
  const window = useAppStore(selectTrendWindow);
  const fetchStatsTrend = useAppStore((s) => s.fetchStatsTrend);

  // 段控件的值走 DOM，只能是字符串；`TrendWindow` 是数字（后端收的是天数）。
  // 在这一处显式转，而不是把类型改成 `"1" | "7"`——那样 `invoke` 那边又得
  // `parseInt` 一次，转换点从一个变成两个。
  const options: SegmentedOption<string>[] = [
    { value: "1", label: t("trend.window_1") },
    { value: "7", label: t("trend.window_7") },
  ];

  // 上期压根不存在时，四张卡会整整齐齐写四遍「没有可比的上期」——
  // 那是用同一句话占了四个格子的地方。合成一句，并且说清楚要等多久。
  const noBaseline =
    trend !== null &&
    trend.interruptions.previous === null &&
    trend.resumes.previous === null;

  return (
    <Card>
      <CardBody>
        <CardHeader
          className="mb-4"
          title={t("trend.title")}
          desc={t("trend.desc")}
          aside={
            <Segmented
              value={String(window)}
              options={options}
              onChange={(next) => void fetchStatsTrend(next === "7" ? 7 : 1)}
              ariaLabel={t("trend.title")}
            />
          }
        />

        {noBaseline ? (
          <EmptyState
            title={t("trend.no_baseline")}
            hint={t("trend.too_new", { days: window })}
          />
        ) : (
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
            {METRICS.map((spec) => (
              <MetricTile
                key={spec.key}
                spec={spec}
                metric={trend?.[spec.key] ?? { current: null, previous: null }}
                t={t}
              />
            ))}
          </div>
        )}
      </CardBody>
    </Card>
  );
}

function MetricTile({
  spec,
  metric,
  t,
}: {
  spec: MetricSpec;
  metric: TrendMetric;
  t: Translator["t"];
}) {
  const { current, previous } = metric;
  const { mode, delta } = tileView(current, previous);

  return (
    <div className="rounded-lg border border-neutral-200 px-4 py-3.5">
      <p className="text-2xl font-semibold tabular-nums leading-none text-neutral-900">
        {current === null ? (
          <span className="text-neutral-300">{t("trend.no_data")}</span>
        ) : (
          formatValue(current, spec.unit, t)
        )}
      </p>
      <p className="mt-1.5 truncate text-[11px] text-neutral-400">{t(spec.label)}</p>

      {/* 固定最小高度：四格里只要有一格显示徽标、别的显示一行小字，
          没有这个高度它们的顶边就会错开一两像素 */}
      <div className="mt-2 flex min-h-[18px] items-center gap-1.5">
        {mode === "unknown" && spec.unit === "duration" ? (
          // 卡住时长是唯一一个「本期也可能算不出来」的指标。要说的不是
          // 「没有上期」，而是「这类会话报不了这个数」——那不是故障
          <Tooltip content={t("trend.stuck_unknown")}>
            <span className="cursor-default text-[10px] text-neutral-300 underline decoration-dotted underline-offset-2">
              {t("trend.no_baseline")}
            </span>
          </Tooltip>
        ) : delta === null ? (
          <span className="text-[10px] text-neutral-300">{t("trend.no_baseline")}</span>
        ) : (
          <>
            <Badge tone={deltaTone(delta, spec.polarity)} className="gap-0.5">
              <span aria-hidden="true">{delta === 0 ? "→" : delta > 0 ? "↑" : "↓"}</span>
              {delta === 0 ? t("trend.flat") : formatValue(Math.abs(delta), spec.unit, t)}
            </Badge>
            <span className="truncate text-[10px] text-neutral-400">
              {t("trend.baseline", { value: formatValue(previous as number, spec.unit, t) })}
            </span>
          </>
        )}
      </div>
    </div>
  );
}

/**
 * 数值 → 带单位的文案
 *
 * 差值也走这里，所以百分比的差是「个百分点」而不是「百分之几的百分之几」：
 * 80% 跌到 75% 显示 `↓5%`，不是 `↓6.25%`。旁边那句「上期 80%」把基准写明了，
 * 两个数一减就是箭头上的数，不会有第二种读法。
 */
function formatValue(value: number, unit: MetricUnit, t: Translator["t"]): string {
  if (unit === "percent") return `${Math.round(value)}%`;
  if (unit === "duration") {
    const { key, vars } = durationParts(value);
    return t(key, vars);
  }
  // 计数一定是整数；后端按 f64 传，四舍五入是为了不让浮点误差露出 `2.0000000004`
  return String(Math.round(value));
}
