/**
 * 组件基元
 *
 * 全站的按钮、卡片、表单控件都从这里出去。规矩只有一条：
 * **样式在基元里，业务组件只传数据**。以前同一种次要按钮在四个文件里
 * 有四种内边距，就是因为每处都手写 Tailwind。
 */

export { Button, type ButtonProps } from "./Button";
export {
  Card,
  CardBar,
  CardBody,
  CardHeader,
  Badge,
  type BadgeProps,
  type BadgeTone,
  EmptyState,
} from "./Card";
export {
  Field,
  TextInput,
  NumberInput,
  TextArea,
  CommaListInput,
  Slider,
  Chip,
} from "./Field";
export { Switch, ToggleRow } from "./Switch";
export { Select, type SelectOption } from "./Select";
export { Tabs, TabsContent, TabsList, TabsTrigger } from "./Tabs";
export { Tooltip, TooltipProvider } from "./Tooltip";
export { BarChart, LegendDot, type BarDatum } from "./BarChart";
