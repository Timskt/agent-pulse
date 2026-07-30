import * as SelectPrimitive from "@radix-ui/react-select";
import { cn } from "../../lib/utils";

export interface SelectOption<T extends string> {
  value: T;
  label: React.ReactNode;
}

/**
 * 下拉选择
 *
 * 原生 `<select>` 在 macOS 上会弹系统样式的菜单，和界面其他部分明显不是
 * 一套东西；Radix 的浮层能沿用同一份边框、圆角和字号。
 */
export function Select<T extends string>({
  value,
  options,
  onValueChange,
  className,
}: {
  value: T;
  options: readonly SelectOption<T>[];
  onValueChange: (value: T) => void;
  className?: string;
}) {
  return (
    <SelectPrimitive.Root value={value} onValueChange={(v) => onValueChange(v as T)}>
      <SelectPrimitive.Trigger
        className={cn(
          "flex w-full items-center justify-between gap-2 rounded-lg border border-neutral-200 bg-white px-3 py-2",
          "text-xs text-neutral-700 outline-none transition-colors",
          "hover:border-neutral-300 focus-visible:border-neutral-400",
          className
        )}
      >
        <SelectPrimitive.Value />
        <SelectPrimitive.Icon className="text-[9px] text-neutral-400">▾</SelectPrimitive.Icon>
      </SelectPrimitive.Trigger>
      <SelectPrimitive.Portal>
        <SelectPrimitive.Content
          position="popper"
          sideOffset={4}
          className="z-50 min-w-[var(--radix-select-trigger-width)] overflow-hidden rounded-lg border border-neutral-200 bg-white py-1 shadow-lg"
        >
          <SelectPrimitive.Viewport>
            {options.map((option) => (
              <SelectPrimitive.Item
                key={option.value}
                value={option.value}
                className={cn(
                  "cursor-default px-3 py-1.5 text-xs text-neutral-600 outline-none",
                  "data-[highlighted]:bg-neutral-100 data-[state=checked]:font-medium data-[state=checked]:text-neutral-900"
                )}
              >
                <SelectPrimitive.ItemText>{option.label}</SelectPrimitive.ItemText>
              </SelectPrimitive.Item>
            ))}
          </SelectPrimitive.Viewport>
        </SelectPrimitive.Content>
      </SelectPrimitive.Portal>
    </SelectPrimitive.Root>
  );
}
