import { describe, expect, it } from "vitest";
import tauriConf from "../src-tauri/tauri.conf.json";
import historyPanel from "./components/HistoryPanel.tsx?raw";
import resumeRecordsPanel from "./components/ResumeRecordsPanel.tsx?raw";

describe("窄屏窗口契约", () => {
  it("主窗口允许缩放到 360x700", () => {
    const mainWindow = tauriConf.app.windows[0];

    expect(mainWindow.minWidth).toBeLessThanOrEqual(360);
    expect(mainWindow.minHeight).toBeLessThanOrEqual(700);
  });
});

describe("历史长文本契约", () => {
  it("历史行里的超长 agent 名允许任意断行", () => {
    expect(historyPanel).toMatch(
      /className="min-w-0 max-w-full break-anywhere"[^>]*>\{entry\.agent_name\}/,
    );
  });

  it("续跑路径和错误详情可选择且有明确复制操作", () => {
    expect(resumeRecordsPanel).toContain("select-text");
    expect(resumeRecordsPanel).toMatch(
      /<CopyTextButton\s+text=\{record\.working_dir\}\s+label=\{t\("records\.copy_dir"\)\}/,
    );
    expect(resumeRecordsPanel).toMatch(
      /<CopyTextButton\s+text=\{record\.message\}\s+label=\{t\("records\.copy_details"\)\}/,
    );
  });
});
