import * as SwitchPrimitive from "@radix-ui/react-switch";
import { cn } from "../../lib/utils";

/**
 * 开关
 *
 * 用 Radix 而不是自己写 `<button>`：键盘操作、`aria-checked`、disabled
 * 这些以前每个开关都得自己记一遍，现在由基元保证。
 * 尺寸沿用原来手写的 9×5 药丸，视觉上看不出换过实现。
 */
export function Switch({
  checked,
  onCheckedChange,
  disabled,
  className,
}: {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <SwitchPrimitive.Root
      checked={checked}
      onCheckedChange={onCheckedChange}
      disabled={disabled}
      className={cn(
        "relative h-5 w-9 shrink-0 rounded-full transition-colors outline-none",
        "focus-visible:ring-2 focus-visible:ring-neutral-300",
        "disabled:cursor-not-allowed disabled:opacity-40",
        checked ? "bg-neutral-900" : "bg-neutral-200",
        className
      )}
    >
      <SwitchPrimitive.Thumb
        className={cn(
          "block h-4 w-4 rounded-full bg-white shadow-sm transition-transform",
          "translate-x-0.5 data-[state=checked]:translate-x-[18px]"
        )}
      />
    </SwitchPrimitive.Root>
  );
}

/**
 * 一行「标题 + 说明 + 开关」
 *
 * 设置页里这个结构出现了二十多次，抽出来之后左右间距和字号只有一处。
 * `aside` 留给「系统设置」这类不是开关的右侧内容。
 */
export function ToggleRow({
  label,
  desc,
  checked,
  onCheckedChange,
  disabled,
  aside,
}: {
  label: React.ReactNode;
  desc?: React.ReactNode;
  checked?: boolean;
  onCheckedChange?: (checked: boolean) => void;
  disabled?: boolean;
  aside?: React.ReactNode;
}) {
  return (
    <div className="flex items-start justify-between gap-4 py-1">
      <div className="min-w-0">
        <p className="text-[11px] font-medium text-neutral-600">{label}</p>
        {desc && <p className="mt-0.5 text-[10px] leading-relaxed text-neutral-400">{desc}</p>}
      </div>
      <div className="shrink-0 pt-0.5">
        {aside ??
          (onCheckedChange && (
            <Switch
              checked={checked ?? false}
              onCheckedChange={onCheckedChange}
              disabled={disabled}
            />
          ))}
      </div>
    </div>
  );
}
