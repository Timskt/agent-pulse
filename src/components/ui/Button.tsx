import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import { forwardRef } from "react";
import { cn } from "../../lib/utils";

/**
 * 按钮
 *
 * 之前每个按钮都是一串手写的 Tailwind，同一种「次要按钮」在四个文件里
 * 有四种内边距。变体收在这里之后，改一次全站生效。
 */
const buttonVariants = cva(
  "inline-flex shrink-0 items-center justify-center gap-1.5 whitespace-nowrap rounded-md font-medium transition-colors outline-none focus-visible:ring-2 focus-visible:ring-neutral-300 disabled:pointer-events-none disabled:opacity-40",
  {
    variants: {
      variant: {
        primary: "bg-neutral-900 text-white hover:bg-neutral-700",
        outline: "border border-neutral-200 text-neutral-600 hover:bg-neutral-50",
        ghost: "text-neutral-500 hover:bg-neutral-100 hover:text-neutral-700",
        success: "bg-emerald-600 text-white hover:bg-emerald-500",
        danger: "text-red-500 hover:bg-red-50",
      },
      size: {
        xs: "px-2 py-1 text-[10px]",
        sm: "px-2.5 py-1 text-[10px]",
        md: "px-3.5 py-1.5 text-xs",
        lg: "px-6 py-2 text-xs",
      },
    },
    defaultVariants: { variant: "outline", size: "md" },
  }
);

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  /** 渲染成子元素（例如包一个 `<a>`），样式照旧 */
  asChild?: boolean;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, asChild = false, type = "button", ...props }, ref) => {
    const Comp = asChild ? Slot : "button";
    return (
      <Comp
        ref={ref}
        type={asChild ? undefined : type}
        className={cn(buttonVariants({ variant, size }), className)}
        {...props}
      />
    );
  }
);
Button.displayName = "Button";
