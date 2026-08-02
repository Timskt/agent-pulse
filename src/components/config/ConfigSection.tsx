import type { ReactNode } from "react";
import { Card, CardBody, CardHeader } from "../ui";

/** 设置页各分区共用的卡片骨架 */
export function ConfigSection({
  title,
  desc,
  children,
}: {
  title: ReactNode;
  desc?: ReactNode;
  children: ReactNode;
}) {
  return (
    <Card>
      <CardBody>
        <CardHeader className="mb-4" title={title} desc={desc} />
        {children}
      </CardBody>
    </Card>
  );
}

/** 分区里的次级容器，用来放「开关打开后才出现」的一组字段 */
export function ConfigNested({ children }: { children: ReactNode }) {
  return (
    <div className="space-y-3 rounded-lg border border-neutral-100 bg-neutral-50/60 p-3">
      {children}
    </div>
  );
}
