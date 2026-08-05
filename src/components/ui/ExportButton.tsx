import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { cn } from "../../lib/utils";
import { Button } from "./Button";

/** 后端 `ExportResult`：落盘路径 + 实际导了多少行 */
interface ExportResult {
  path: string;
  rows: number;
}

/**
 * 导出按钮
 *
 * 四个面板都要这个东西，而它的状态机比看起来复杂：点击 → 正在写 → 写完了
 * （要说清导到哪儿、导了几行）→ 过一会儿回到初始态。抄四遍的结果一定是
 * 四种不同的措辞和四种不同的回落时机。
 *
 * 为什么导完要**显示路径**而不是只弹一句「导出成功」：这个应用没有系统保存
 * 对话框（那需要多引一个插件和一条权限），文件是直接写到下载夹的。不说去哪儿了，
 * 用户就得自己猜——而「导出成功」之后找不到文件，比导出失败更让人恼火。
 */
export function ExportButton({
  command,
  args,
  label,
  className,
}: {
  /** Tauri 命令名（`export_resumes` / `export_sessions` / …） */
  command: string;
  /** 传给命令的参数，通常就是当前的筛选条件 */
  args?: Record<string, unknown>;
  /** 按钮上的字；不给就用通用的「导出 CSV」 */
  label?: string;
  className?: string;
}) {
  const { t } = useI18n();
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<ExportResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  // 卸载之后不许再 setState：导出中切走标签页是很自然的操作
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  // 成功提示自己退场。不自动退的话它会一直挂在标题旁边，
  // 下次再点导出时用户分不清那句话说的是这次还是上次
  useEffect(() => {
    if (!done) return;
    const timer = window.setTimeout(() => {
      if (alive.current) setDone(null);
    }, 8000);
    return () => window.clearTimeout(timer);
  }, [done]);

  async function run() {
    setBusy(true);
    setError(null);
    setDone(null);
    try {
      const result = await invoke<ExportResult>(command, args ?? {});
      if (alive.current) setDone(result);
    } catch (e) {
      // 失败原因要露出来。写盘失败的真实原因通常是「磁盘满」或「没权限」，
      // 只说一句「导出失败」等于让用户没法自救
      if (alive.current) setError(String(e));
    } finally {
      if (alive.current) setBusy(false);
    }
  }

  return (
    <div className={cn("flex items-center gap-2", className)}>
      {done && (
        <button
          type="button"
          onClick={() => void invoke("reveal_export", { path: done.path })}
          className="max-w-[13rem] truncate text-[10px] text-emerald-600 underline decoration-emerald-200 underline-offset-2 hover:decoration-emerald-500"
          title={done.path}
        >
          {t("export.done", { rows: done.rows })}
        </button>
      )}
      {error && (
        <span className="max-w-[13rem] truncate text-[10px] text-red-500" title={error}>
          {t("export.failed")}
        </span>
      )}
      <Button size="xs" variant="outline" disabled={busy} onClick={() => void run()}>
        {busy ? t("export.running") : (label ?? t("export.csv"))}
      </Button>
    </div>
  );
}
