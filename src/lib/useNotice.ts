import { useCallback, useEffect, useRef, useState } from "react";

export interface Notice {
  ok: boolean;
  message: string;
}

/**
 * 一句话提示
 *
 * 续跑、跳终端、测试推送这些操作后端都会回一句话（已经是当前语言的成品文案），
 * 以前它们要么被 `console.error` 吞掉，要么各组件自己写一个 `useState` +
 * `setTimeout`，还都忘了清定时器。这里统一一次：切页面时不会向已卸载的组件写状态。
 */
export function useNotice(timeoutMs = 4000) {
  const [notice, setNotice] = useState<Notice | null>(null);
  const timer = useRef<number | undefined>(undefined);

  const show = useCallback(
    (next: Notice) => {
      setNotice(next);
      if (timer.current !== undefined) window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setNotice(null), timeoutMs);
    },
    [timeoutMs]
  );

  useEffect(
    () => () => {
      if (timer.current !== undefined) window.clearTimeout(timer.current);
    },
    []
  );

  return { notice, show };
}
