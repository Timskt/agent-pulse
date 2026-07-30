import * as TabsPrimitive from "@radix-ui/react-tabs";
import { cn } from "../../lib/utils";

/**
 * 顶部导航
 *
 * 以前是一排手写 `<button>`：键盘只能 Tab 逐个走，也没有 `role="tab"`。
 * 换成 Radix 之后左右方向键能切换，选中状态由 `data-state` 驱动。
 */
export const Tabs = TabsPrimitive.Root;

export function TabsList({ className, ...props }: TabsPrimitive.TabsListProps) {
  return (
    <TabsPrimitive.List
      className={cn("flex items-center gap-1 rounded-lg bg-neutral-100 p-0.5", className)}
      {...props}
    />
  );
}

export function TabsTrigger({ className, ...props }: TabsPrimitive.TabsTriggerProps) {
  return (
    <TabsPrimitive.Trigger
      className={cn(
        "rounded-md px-3 py-1 text-[11px] font-medium text-neutral-500 outline-none transition-colors",
        "hover:text-neutral-700 focus-visible:ring-2 focus-visible:ring-neutral-300",
        "data-[state=active]:bg-white data-[state=active]:text-neutral-900 data-[state=active]:shadow-sm",
        className
      )}
      {...props}
    />
  );
}

export function TabsContent({ className, ...props }: TabsPrimitive.TabsContentProps) {
  return <TabsPrimitive.Content className={cn("outline-none", className)} {...props} />;
}
