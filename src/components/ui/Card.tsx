import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "../../lib/utils";

/** 卡片外壳：全站所有分区都用它，边框和圆角只有一处定义 */
export function Card({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cn("rounded-lg border border-neutral-200 bg-white", className)}
      {...props}
    />
  );
}

/** 卡片头：左边标题+说明，右边放图例或计数 */
export function CardHeader({
  title,
  desc,
  aside,
  className,
}: {
  title: React.ReactNode;
  desc?: React.ReactNode;
  aside?: React.ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("flex items-start justify-between gap-3", className)}>
      <div className="min-w-0">
        <h3 className="text-xs font-semibold text-neutral-800">{title}</h3>
        {desc && <p className="mt-0.5 text-[10px] leading-relaxed text-neutral-400">{desc}</p>}
      </div>
      {aside && <div className="shrink-0">{aside}</div>}
    </div>
  );
}

export function CardBody({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return <div className={cn("p-5", className)} {...props} />;
}

/** 带分隔线的紧凑列表头（会话列表、日志用） */
export function CardBar({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cn(
        "flex items-center justify-between border-b border-neutral-100 px-4 py-2.5",
        className
      )}
      {...props}
    />
  );
}

const badgeVariants = cva(
  "inline-flex items-center gap-1 whitespace-nowrap rounded-full px-2 py-0.5 text-[10px] font-medium",
  {
    variants: {
      tone: {
        neutral: "bg-neutral-100 text-neutral-500",
        green: "bg-emerald-50 text-emerald-600",
        amber: "bg-amber-50 text-amber-600",
        red: "bg-red-50 text-red-500",
        blue: "bg-blue-50 text-blue-600",
        violet: "bg-violet-50 text-violet-600",
      },
    },
    defaultVariants: { tone: "neutral" },
  }
);

export interface BadgeProps
  extends React.HTMLAttributes<HTMLSpanElement>,
    VariantProps<typeof badgeVariants> {}

/** 状态标签 */
export function Badge({ className, tone, ...props }: BadgeProps) {
  return <span className={cn(badgeVariants({ tone }), className)} {...props} />;
}

export type BadgeTone = NonNullable<BadgeProps["tone"]>;

/** 空状态：以前每个面板自己写一版，措辞和高度都不一样 */
export function EmptyState({
  title,
  hint,
  className,
}: {
  title: React.ReactNode;
  hint?: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "flex flex-col items-center justify-center gap-1 py-10 text-center",
        className
      )}
    >
      <p className="text-xs text-neutral-400">{title}</p>
      {hint && <p className="max-w-md text-[11px] leading-relaxed text-neutral-300">{hint}</p>}
    </div>
  );
}
