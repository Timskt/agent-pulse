<p align="center">
  <img src="src-tauri/icons/icon.png" width="112" alt="AgentPulse">
</p>

# AgentPulse ⚡

**AI Coding Agent 守护与自动续跑工具** — 跨平台监控 Claude Code / Codex CLI 等 AI 编程助手的会话状态，自动检测中断并发送续跑指令，无需人工干预。

## 解决什么问题？

使用 Claude Code、Codex 等 AI 编程工具时，经常遇到：

- 🌐 网络波动导致连接中断
- 🚦 上游服务限流 (rate limit) 自动暂停
- 🤖 模型超时/崩溃停止响应
- 💤 任务未完成但 Agent 静默退出

**AgentPulse 自动检测这些中断，并发送续跑指令让任务继续执行。**

## 核心特性

- **多策略检测引擎** — 进程状态 + 会话文件监听 + 关键词匹配 + 心跳超时，多信号融合判定
- **智能双重校验** — 中断信号存在 AND 完成标记不存在，才触发续跑，防止重复执行
- **跨平台静默续跑** — macOS (AppleScript) / Windows (PowerShell + Win32) / Linux (xdotool·ydotool)，覆盖 iTerm2、Terminal、Windows Terminal、cmd·conhost，以及 VS Code / Cursor / Windsurf / JetBrains 全家桶里的内置终端
- **提示词走剪贴板，不走合成按键** — 中文提示词经系统剪贴板 + 一次 ASCII 粘贴键投递，绕开输入法，不会再打出「啊啊啊啊」这类拼音残留
- **定位不到就不敲** — 认不出会话属于哪个窗口时宁可放弃续跑，也不往别人的窗口里回车（想要兜底可在设置里打开「跟随最新会话」）
- **插件式适配器** — Claude Code / Codex CLI，可扩展支持更多 Agent
- **安全机制** — 最大续跑次数限制、冷却时间、手动/自动模式切换
- **实时可观测** — 结构化日志流、状态面板、事件时间线

## 技术栈

| 层 | 技术 |
|---|---|
| 桌面框架 | Tauri 2.0 |
| 后端引擎 | Rust (sysinfo / notify / tokio) |
| 前端 | React 19 + TypeScript + TailwindCSS |
| 状态管理 | Zustand |

## 快速开始

### 前置要求

- Rust 1.77+
- Node.js 22+
- pnpm 11+
- macOS: Xcode Command Line Tools

### 开发运行

```bash
# 安装依赖
pnpm install

# 启动开发模式（含 Rust 后端热重载）
pnpm tauri:dev
```

### 构建发布

```bash
pnpm tauri:build
```

产物位于 `src-tauri/target/release/bundle/`。

## 工作原理

```
┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│  适配器发现   │ →  │  检测策略引擎 │ →  │  续跑执行器   │
│  进程扫描     │    │  多信号融合   │    │  平台适配     │
│  会话文件     │    │  双重校验     │    │  AppleScript  │
└──────────────┘    └──────────────┘    └──────────────┘
```

1. **发现** — 适配器扫描系统中的 AI Agent 进程（Claude Code / Codex）
2. **检测** — 轮询检查：会话文件是否停止更新、输出中是否有中断关键词、进程是否退出
3. **决策** — 双重校验：有中断信号 + 无完成标记 → 确认中断
4. **续跑** — 跨平台投递：macOS (AppleScript) / Windows (PowerShell + Win32) / Linux (xdotool·ydotool)，提示词走剪贴板粘贴
5. **保护** — 冷却时间 + 次数上限 + 定位不到就不敲，防止无限循环和误敲

## 配置说明

配置文件位于 `~/Library/Application Support/agent-pulse/config.json`（macOS）。

常用配置项：

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| `poll_interval_secs` | 10 | 轮询间隔 |
| `idle_timeout_secs` | 60 | 空闲超时判定 |
| `max_resume_count` | 5 | 单会话最大续跑次数 |
| `resume_cooldown_secs` | 30 | 续跑冷却时间 |
| `auto_follow_latest` | false | 定位不到时是否盲敲前台窗口 |
| `custom_keywords` | rate limit, overloaded... | 中断触发关键词 |
| `resume_prompt` | 请继续完成... | 续跑提示词 |

完整配置参考（20+ 主配置 + 7 个子配置）见 [PROJECT_STATUS.md § 10.3](./PROJECT_STATUS.md)。

## 路线图

- [x] **v1.0** — 核心引擎 + Claude Code / Codex CLI / OpenCode 适配器 + 三平台续跑 + Dashboard UI + 系统托盘
- [x] **v1.1** — 剪贴板投递 + 定位不到就不敲 + 三平台 CI 矩阵 + 实机验证清单
- [x] **v1.2** — 感知层（成本追踪 + 限流预测）+ 洞察层（AI 判定 + 统计面板）
- [x] **v1.3** — 远程层（手机看板 + Webhook）+ 会话历史 + i18n 中英双语
- [ ] **v1.4** — 续跑演练（dry-run）按钮 + 自定义适配器 UI + 前端测试 + 文档对齐

## License

MIT
