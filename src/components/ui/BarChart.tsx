import { useMemo } from "react";
import { cn } from "../../lib/utils";
import { Tooltip } from "./Tooltip";

export interface BarDatum {
  key: string;
  /** 柱子的总高度依据 */
  value: number;
  /** 叠在柱子底部的子集，例如「续跑里成功的那部分」 */
  overlay?: number;
  tooltip?: React.ReactNode;
  /** 坐标轴上这根柱子的短标签，例如 `07-30`；不给就不参与刻度 */
  label?: string;
}

/** 值大于 0 但太小时给一条看得见的底边，否则用户以为那天没数据 */
const MIN_VISIBLE_PERCENT = 3;

/** 最多摆几个刻度：再多就开始互相压字 */
const MAX_TICKS = 6;

/**
 * 挑出该写字的那几根柱子
 *
 * **从最后一根往前数**，而不是从第一根往后。最右边是「今天」，
 * 是用户看这张图第一个要找的坐标；从左往前排的话 30 天配 5 格步长，
 * 最后一个刻度会落在第 26 天，今天反而没有标注。
 *
 * 返回下标集合而不是数组，因为渲染时是按柱子顺序问「这根要写字吗」。
 */
export function tickIndices(count: number, maxTicks = MAX_TICKS): Set<number> {
  const picked = new Set<number>();
  if (count <= 0) return picked;
  const stride = Math.max(1, Math.ceil(count / maxTicks));
  for (let i = count - 1; i >= 0; i -= stride) {
    picked.add(i);
  }
  return picked;
}

/**
 * 柱状图
 *
 * 统计页和花费页各写过一版，其中一版的柱子永远不显示：
 * 外层是 `flex items-end`，交叉轴尺寸是 auto，柱子的 `height: N%`
 * 没有可参照的父高度，于是解析成 0。修法是让每列包一层 `h-full`
 * 把父高度定下来——收在这里之后，后面新加的图不会再踩一次。
 *
 * `max` 也在这里算一次；原来写在 `map` 回调里，每根柱子都要重扫整个数组。
 *
 * 坐标轴以前是一个 `ReactNode` 插槽配 `justify-between`，调用方只能塞
 * 首尾两个日期——中间二十八根柱子没有任何时间参照，鼠标不悬上去就读不出
 * 「那个尖峰是哪天」。现在轴由数据自己生成（[`tickIndices`]），
 * 刻度对齐到各自的柱子。
 */
export function BarChart({
  data,
  className,
  barClassName,
  overlayClassName,
  /** 纵轴顶端的峰值说明，例如「峰值 12 次」；调用方负责 i18n 和格式 */
  peakLabel,
}: {
  data: readonly BarDatum[];
  className?: string;
  barClassName?: string;
  overlayClassName?: string;
  peakLabel?: React.ReactNode;
}) {
  const max = useMemo(() => data.reduce((acc, d) => Math.max(acc, d.value), 0), [data]);
  const ticks = useMemo(() => tickIndices(data.length), [data.length]);
  const hasLabels = useMemo(() => data.some((d) => d.label), [data]);

  if (data.length === 0) return null;

  return (
    <div>
      {/* 峰值写在图上方而不是纵轴旁：纵轴要占一条竖带，这张图只有 128px 高、
          柱子最窄 5px，让给刻度文字之后柱子就没地方了 */}
      {peakLabel && (
        <div className="mb-1 flex justify-end text-[10px] tabular-nums text-neutral-400">
          {peakLabel}
        </div>
      )}
      <div className="relative">
        {/* 参考线：一半和满格。柱子之间没有网格线的时候，
            两根柱子谁高一点点是看不出来的 */}
        <div aria-hidden className="pointer-events-none absolute inset-x-0 top-0 h-32">
          <div className="absolute inset-x-0 top-0 border-t border-dashed border-neutral-200/70" />
          <div className="absolute inset-x-0 top-1/2 border-t border-dashed border-neutral-200/70" />
        </div>
        {/* 固定高度在这一层，下面每列 h-full 继承它 */}
        <div className={cn("relative flex h-32 items-end gap-[2px]", className)}>
          {data.map((datum) => {
            const percent =
              max === 0 || datum.value === 0
                ? 0
                : Math.max((datum.value / max) * 100, MIN_VISIBLE_PERCENT);
            const overlayPercent =
              datum.overlay && datum.value > 0
                ? Math.min((datum.overlay / datum.value) * 100, 100)
                : 0;

            // 整列都是悬浮热区，所以没有数据的那天也能 hover 出「0 次」
            const column = (
              <div
                key={datum.key}
                className="group flex h-full min-w-[5px] flex-1 flex-col justify-end"
              >
                <div
                  className={cn(
                    "relative w-full overflow-hidden rounded-t-sm bg-neutral-800/80 transition-colors group-hover:bg-neutral-800",
                    barClassName
                  )}
                  style={{ height: `${percent}%` }}
                >
                  {overlayPercent > 0 && (
                    <div
                      className={cn("absolute inset-x-0 bottom-0 bg-emerald-500", overlayClassName)}
                      style={{ height: `${overlayPercent}%` }}
                    />
                  )}
                </div>
              </div>
            );

            return datum.tooltip ? (
              <Tooltip key={datum.key} content={datum.tooltip}>
                {column}
              </Tooltip>
            ) : (
              column
            );
          })}
        </div>
      </div>
      {hasLabels && (
        // 高度写死在这一层：刻度文字是绝对定位的（要溢出邻列才放得下），
        // 撐不起父容器，不给高度这一行就是 0px、字被压在柱子上
        <div className="mt-1.5 flex h-4 gap-[2px] border-t border-neutral-200 pt-1">
          {data.map((datum, i) => {
            const show = ticks.has(i) && datum.label;
            return (
              <div key={datum.key} className="relative min-w-[5px] flex-1">
                {show && (
                  <>
                    {/* 刻度线：光有文字的话，读者得自己猜这个日期归哪根柱子 */}
                    <span
                      aria-hidden
                      className="absolute -top-1 left-1/2 h-1 w-px -translate-x-1/2 bg-neutral-300"
                    />
                    {/* 文字比列宽得多，所以绝对定位居中让它溢出到邻列上方；
                        首尾两格改成贴边，否则半个日期被卡片裁掉 */}
                    <span
                      className={cn(
                        "absolute top-0 whitespace-nowrap text-[10px] tabular-nums text-neutral-400",
                        i === 0 && "left-0",
                        i === data.length - 1 && "right-0",
                        i !== 0 && i !== data.length - 1 && "left-1/2 -translate-x-1/2"
                      )}
                    >
                      {datum.label}
                    </span>
                  </>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

/** 图例点，统计页和花费页都用 */
export function LegendDot({
  className,
  children,
}: {
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <span className="inline-flex items-center gap-1 text-[10px] text-neutral-400">
      <span className={cn("h-2 w-2 rounded-sm bg-neutral-800/80", className)} />
      {children}
    </span>
  );
}
