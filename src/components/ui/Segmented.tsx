import * as ToggleGroupPrimitive from "@radix-ui/react-toggle-group";
import { cn } from "../../lib/utils";

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
}

/**
 * 段控件：在几个互斥的取值里选一个，就地换掉旁边那块数据
 *
 * 跟 `Tabs` 的区别不在长相，在语义：`Tabs` 是「几个面板轮流出现」，
 * 每个 trigger 都 `aria-controls` 一块内容；这里只是一个**参数**
 * （趋势看今日还是近 7 天），面板始终是同一个。拿 Tabs 凑的话，
 * 屏幕阅读器会念出并不存在的第二个面板。
 *
 * Radix 的 `ToggleGroup` 顺带给了方向键切换和 `role="radiogroup"`。
 */
export function Segmented<T extends string>({
  value,
  options,
  onChange,
  className,
  ariaLabel,
}: {
  value: T;
  options: readonly SegmentedOption<T>[];
  onChange: (value: T) => void;
  className?: string;
  ariaLabel: string;
}) {
  return (
    <ToggleGroupPrimitive.Root
      type="single"
      value={value}
      aria-label={ariaLabel}
      // Radix 在「点当前已选中那一项」时会给出空串，意思是「取消选择」。
      // 这里没有「都不选」这个状态——空窗的段控件下面那块数据就无从可取了。
      // 所以空值一律丢掉，点两下当前项 = 什么都没发生。
      onValueChange={(next) => {
        if (next) onChange(next as T);
      }}
      className={cn("flex items-center gap-0.5 rounded-lg bg-neutral-100 p-0.5", className)}
    >
      {options.map((option) => (
        <ToggleGroupPrimitive.Item
          key={option.value}
          value={option.value}
          className={cn(
            "rounded-md px-2.5 py-1 text-[11px] font-medium text-neutral-500 outline-none transition-colors",
            "hover:text-neutral-700 focus-visible:ring-2 focus-visible:ring-neutral-300",
            "data-[state=on]:bg-white data-[state=on]:text-neutral-900 data-[state=on]:shadow-sm"
          )}
        >
          {option.label}
        </ToggleGroupPrimitive.Item>
      ))}
    </ToggleGroupPrimitive.Root>
  );
}
