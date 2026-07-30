import * as TooltipPrimitive from "@radix-ui/react-tooltip";

/** 挂在应用最外层，整个应用共用一份延迟设置 */
export function TooltipProvider({ children }: { children: React.ReactNode }) {
  return (
    <TooltipPrimitive.Provider delayDuration={200} skipDelayDuration={300}>
      {children}
    </TooltipPrimitive.Provider>
  );
}

/**
 * 悬浮提示
 *
 * 图表和长路径以前靠 `title=""`：系统 tooltip 大约要停一秒才出现，
 * 字号也不受控。这个换成受控浮层，柱状图逐日明细才读得舒服。
 */
export function Tooltip({
  content,
  side = "top",
  children,
}: {
  content: React.ReactNode;
  side?: "top" | "right" | "bottom" | "left";
  children: React.ReactNode;
}) {
  return (
    <TooltipPrimitive.Root>
      <TooltipPrimitive.Trigger asChild>{children}</TooltipPrimitive.Trigger>
      <TooltipPrimitive.Portal>
        <TooltipPrimitive.Content
          side={side}
          sideOffset={6}
          collisionPadding={8}
          className="z-50 max-w-xs rounded-md bg-neutral-900 px-2 py-1 text-[10px] leading-relaxed text-white shadow-lg"
        >
          {content}
          <TooltipPrimitive.Arrow className="fill-neutral-900" />
        </TooltipPrimitive.Content>
      </TooltipPrimitive.Portal>
    </TooltipPrimitive.Root>
  );
}
