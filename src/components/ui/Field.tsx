import { cn } from "../../lib/utils";

const inputClass =
  "w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-xs text-neutral-700 outline-none transition-colors placeholder:text-neutral-300 focus:border-neutral-400";

/** 字段标签 + 说明 + 控件，三者的间距只在这里定义一次 */
export function Field({
  label,
  hint,
  children,
  className,
}: {
  label?: React.ReactNode;
  hint?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div className={className}>
      {label && (
        <label className="mb-1.5 block text-[11px] font-medium text-neutral-500">{label}</label>
      )}
      {children}
      {hint && <p className="mt-1 text-[10px] leading-relaxed text-neutral-400">{hint}</p>}
    </div>
  );
}

export function TextInput({
  className,
  ...props
}: React.InputHTMLAttributes<HTMLInputElement>) {
  return <input className={cn(inputClass, className)} {...props} />;
}

/** 数字输入：把 clamp 收在组件里，免得每个调用点各写一遍 */
export function NumberInput({
  value,
  min,
  max,
  step,
  onValueChange,
  className,
}: {
  value: number;
  min?: number;
  max?: number;
  step?: number;
  onValueChange: (value: number) => void;
  className?: string;
}) {
  return (
    <input
      type="number"
      value={value}
      min={min}
      max={max}
      step={step}
      onChange={(e) => {
        const raw = Number(e.target.value);
        if (Number.isNaN(raw)) return;
        const lower = min === undefined ? raw : Math.max(min, raw);
        onValueChange(max === undefined ? lower : Math.min(max, lower));
      }}
      className={cn(inputClass, "tabular-nums", className)}
    />
  );
}

export function TextArea({
  className,
  rows = 2,
  ...props
}: React.TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return (
    <textarea
      rows={rows}
      className={cn(inputClass, "resize-none leading-relaxed", className)}
      {...props}
    />
  );
}

/** 逗号分隔的字符串列表：解析规则统一，不再每处 `split(",").map(trim)` */
export function CommaListInput({
  value,
  onValueChange,
  rows = 2,
}: {
  value: string[];
  onValueChange: (value: string[]) => void;
  rows?: number;
}) {
  return (
    <TextArea
      rows={rows}
      value={value.join(", ")}
      onChange={(e) =>
        onValueChange(
          e.target.value
            .split(",")
            .map((s) => s.trim())
            .filter(Boolean)
        )
      }
    />
  );
}

export function Slider({
  value,
  min,
  max,
  onValueChange,
}: {
  value: number;
  min: number;
  max: number;
  onValueChange: (value: number) => void;
}) {
  return (
    <input
      type="range"
      min={min}
      max={max}
      value={value}
      onChange={(e) => onValueChange(Number(e.target.value))}
      className="w-full accent-neutral-900"
    />
  );
}

/** 圆角药丸开关，用于「推哪些事件」这类多选 */
export function Chip({
  active,
  children,
  onClick,
}: {
  active: boolean;
  children: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "rounded-full border px-2.5 py-1 text-[10px] font-medium transition-colors",
        active
          ? "border-emerald-200 bg-emerald-50 text-emerald-600"
          : "border-neutral-200 bg-white text-neutral-400 hover:text-neutral-600"
      )}
    >
      {children}
    </button>
  );
}
