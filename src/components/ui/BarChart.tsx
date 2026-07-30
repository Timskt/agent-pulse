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
}

/** 值大于 0 但太小时给一条看得见的底边，否则用户以为那天没数据 */
const MIN_VISIBLE_PERCENT = 3;

/**
 * 柱状图
 *
 * 统计页和花费页各写过一版，其中一版的柱子永远不显示：
 * 外层是 `flex items-end`，交叉轴尺寸是 auto，柱子的 `height: N%`
 * 没有可参照的父高度，于是解析成 0。修法是让每列包一层 `h-full`
 * 把父高度定下来——收在这里之后，后面新加的图不会再踩一次。
 *
 * `max` 也在这里算一次；原来写在 `map` 回调里，每根柱子都要重扫整个数组。
 */
export function BarChart({
  data,
  className,
  barClassName,
  overlayClassName,
  axis,
}: {
  data: readonly BarDatum[];
  className?: string;
  barClassName?: string;
  overlayClassName?: string;
  axis?: React.ReactNode;
}) {
  const max = useMemo(() => data.reduce((acc, d) => Math.max(acc, d.value), 0), [data]);

  if (data.length === 0) return null;

  return (
    <div>
      {/* 固定高度在这一层，下面每列 h-full 继承它 */}
      <div className={cn("flex h-32 items-end gap-[2px]", className)}>
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
      {axis && (
        <div className="mt-1.5 flex justify-between text-[9px] tabular-nums text-neutral-300">
          {axis}
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
