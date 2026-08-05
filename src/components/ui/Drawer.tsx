import * as DialogPrimitive from "@radix-ui/react-dialog";
import { useI18n } from "../../i18n";
import { cn } from "../../lib/utils";

/**
 * 右侧抽屉
 *
 * 为什么是抽屉而不是新开一页：会话档案是**带着列表里那一行的上下文**看的
 * ——「就是这条，它到底经历了什么」。跳走再回来会丢掉滚动位置和筛选条件，
 * 用户得重新找回刚才那一行。
 *
 * 用 Radix 的 `Dialog` 而不是自己摆一个绝对定位的面板：焦点要困在抽屉里、
 * Esc 要能关、背后的列表不能被 Tab 走进去、打开时 `aria-modal` 要对。
 * 这些手写一遍就是一遍 bug。
 */
export function Drawer({
  open,
  onClose,
  title,
  desc,
  children,
  footer,
}: {
  open: boolean;
  onClose: () => void;
  title: React.ReactNode;
  desc?: React.ReactNode;
  children: React.ReactNode;
  footer?: React.ReactNode;
}) {
  // 关闭按钮的读屏名字在这里取，不做成 prop：做成 prop 就有调用方忘了传，
  // 而忘了传的后果是读屏只念「按钮」，肉眼完全看不出来
  const { t } = useI18n();

  return (
    <DialogPrimitive.Root open={open} onOpenChange={(next) => !next && onClose()}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="animate-overlay-in fixed inset-0 z-40 bg-neutral-900/20 backdrop-blur-[1px]" />
        <DialogPrimitive.Content
          className={cn(
            "animate-slide-in fixed inset-y-0 right-0 z-50 flex w-full max-w-md flex-col",
            "border-l border-neutral-200 bg-white shadow-xl outline-none"
          )}
        >
          <div className="flex items-start justify-between gap-3 border-b border-neutral-100 px-5 py-4">
            <div className="min-w-0">
              <DialogPrimitive.Title className="truncate text-xs font-semibold text-neutral-800">
                {title}
              </DialogPrimitive.Title>
              {desc && (
                <DialogPrimitive.Description className="mt-0.5 truncate font-mono text-[10px] text-neutral-400">
                  {desc}
                </DialogPrimitive.Description>
              )}
            </div>
            <DialogPrimitive.Close
              aria-label={t("common.close")}
              className={cn(
                "shrink-0 rounded-md p-1 text-neutral-400 outline-none transition-colors",
                "hover:bg-neutral-100 hover:text-neutral-600 focus-visible:ring-2 focus-visible:ring-neutral-300"
              )}
            >
              {/* 图标是内联的：全站没引图标库，为一个叉号引一个不值得 */}
              <svg viewBox="0 0 16 16" className="h-3.5 w-3.5" aria-hidden="true">
                <path
                  d="M4 4l8 8M12 4l-8 8"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.5"
                  strokeLinecap="round"
                />
              </svg>
            </DialogPrimitive.Close>
          </div>

          {/* 滚动只发生在这一层：头和脚要钉住，否则长时间线一滚标题就没了 */}
          <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">{children}</div>

          {footer && (
            <div className="border-t border-neutral-100 px-5 py-3">{footer}</div>
          )}
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}

/** 抽屉里的一个分区：小标题 + 内容 */
export function DrawerSection({
  title,
  aside,
  children,
  className,
}: {
  title: React.ReactNode;
  aside?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <section className={cn("mb-5 last:mb-0", className)}>
      <div className="mb-2 flex items-center justify-between gap-2">
        <h4 className="text-[10px] font-semibold uppercase tracking-wide text-neutral-400">
          {title}
        </h4>
        {aside}
      </div>
      {children}
    </section>
  );
}

/**
 * 键值对一行
 *
 * 值用 `tabular-nums`：时间戳和 token 数上下对齐，扫一眼能比大小。
 */
export function DrawerRow({
  label,
  value,
}: {
  label: React.ReactNode;
  value: React.ReactNode;
}) {
  return (
    <div className="flex items-baseline justify-between gap-3 py-1">
      <span className="shrink-0 text-[11px] text-neutral-400">{label}</span>
      <span className="min-w-0 truncate text-right text-[11px] tabular-nums text-neutral-700">
        {value}
      </span>
    </div>
  );
}
