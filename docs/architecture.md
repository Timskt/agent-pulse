# AgentPulse 架构设计文档

## 系统概览

AgentPulse 是一个跨平台桌面应用，用于监控 AI Coding Agent（Claude Code / Codex CLI 等）的会话状态，自动检测中断并发送续跑指令。

```
┌─────────────────────────────────────────────┐
│              Tauri 2.0 Desktop Shell         │
│  ┌───────────────────────────────────────┐  │
│  │   React 19 + TypeScript + TailwindCSS │  │
│  │   Dashboard / Config / Logs           │  │
│  └──────────────────┬────────────────────┘  │
│                     │ IPC (commands/events)  │
│  ┌──────────────────▼────────────────────┐  │
│  │         Rust Core Engine              │  │
│  │  ┌─────────┐ ┌──────────┐ ┌────────┐ │  │
│  │  │ Monitor │ │ Detector │ │ Resumer│ │  │
│  │  │  Engine │ │ Strategy │ │ Action │ │  │
│  │  └────┬────┘ └────┬─────┘ └───┬────┘ │  │
│  │       │           │           │       │  │
│  │  ┌────▼───────────▼───────────▼────┐  │  │
│  │  │      Agent Adapters (插件式)     │  │  │
│  │  │  Claude Code │ Codex │ Custom   │  │  │
│  │  └─────────────────────────────────┘  │  │
│  └───────────────────────────────────────┘  │
└─────────────────────────────────────────────┘
```

## 模块职责

### 1. Monitor Engine (`src-tauri/src/monitor/`)

核心调度器，负责：
- 按配置的轮询间隔执行扫描循环（tokio async interval）
- 调用适配器发现会话 → 检测器判定状态 → 续跑执行器恢复任务
- 维护全局 `MonitorState`（会话列表、事件日志、统计信息）
- 通过 Tauri Event 将日志实时推送到前端

### 2. Detector (`src-tauri/src/detector/`)

多策略检测引擎，信号融合判定：

| 策略 | 信号 | 权重 |
|------|------|------|
| 进程存活检测 | 进程退出 = 强信号 | 单独即可确认中断 |
| 会话文件新鲜度 | JSONL 文件超过 idle_timeout 未更新 | 需组合判定 |
| 关键词匹配 | 输出含 rate limit / overloaded 等 | 强信号 |
| 心跳超时 | last_activity 超过 threshold × timeout | 强信号 |

**判定逻辑（Verdict）**：
```
完成标记存在 → TaskCompleted（永不续跑）
进程退出 + 无完成标记 → ConfirmInterrupt
强信号 OR 信号数≥2 → ConfirmInterrupt
仅弱信号 → Suspicious（继续观察）
无信号 → Running
```

### 3. Resumer (`src-tauri/src/resumer/`)

平台适配的续跑执行器：
- **macOS**: 通过进程父子关系识别终端应用（iTerm2/Terminal/VS Code/Cursor/Warp），使用 AppleScript + TTY 精确匹配发送续跑 prompt
- **Windows**: PowerShell + Win32 API（SetForegroundWindow + SendKeys），通过 PID 定位控制台窗口
- **Linux**: xdotool（X11）通过 /proc/PID/stat 遍历父进程查找窗口，Wayland 回退到 ydotool

安全机制：
- `max_resume_count`: 单会话最大续跑次数
- `resume_cooldown_secs`: 两次续跑最小间隔
- 完成标记双重校验防重复执行

### 4. Adapters (`src-tauri/src/adapters/`)

插件式 Agent 适配器，实现 `AgentAdapter` trait：

```rust
pub trait AgentAdapter: Send + Sync {
    fn id(&self) -> &str;                    // 适配器标识
    fn name(&self) -> &str;                  // 显示名称
    fn discover_sessions(&self) -> Vec<AgentSession>;  // 进程发现
    fn session_files(&self) -> Vec<PathBuf>; // 会话文件
    fn recent_output(&self, session: &AgentSession) -> Option<String>; // 最近输出
}
```

**Claude Code 适配器**：
- 进程扫描：匹配 `claude` 进程名或 node + claude 命令行
- 会话文件：`~/.claude/projects/**/*.jsonl`，按修改时间排序取最新
- 输出解析：读取 JSONL 尾部，提取 assistant 消息和 result 字段

### 5. Config (`src-tauri/src/config/`)

JSON 文件持久化配置，位于平台标准配置目录：
- macOS: `~/Library/Application Support/agent-pulse/config.json`
- Linux: `~/.config/agent-pulse/config.json`
- Windows: `%APPDATA%/agent-pulse/config.json`

### 6. 前端 (React 19 + Zustand)

- **StatusCards**: 会话总数/活跃/中断/续跑次数/检测次数 + 运行状态条
- **SessionList**: 会话详情（PID、工作目录、状态标签、续跑计数）+ 手动续跑按钮
- **ConfigPanel**: 检测参数、行为开关、关键词/完成标记、续跑提示词
- **LogPanel**: 实时日志流（时间戳 + 级别着色 + 会话 ID）

## IPC 通信设计

### Commands（前端 → Rust）

| Command | 说明 |
|---------|------|
| `get_state` | 获取完整监控状态 |
| `get_status` | 获取状态摘要 |
| `start_monitoring` / `stop_monitoring` | 启停引擎 |
| `scan_now` | 立即执行一次扫描 |
| `get_config` / `update_config` | 配置读写 |
| `manual_resume` | 手动续跑指定会话 |

### Events（Rust → 前端）

| Event | Payload | 说明 |
|-------|---------|------|
| `engine-events` | `Vec<EngineEvent>` | 增量日志推送（800ms 批次） |
| `engine-stopped` | `()` | 引擎停止通知 |

前端另有 3s 轮询 `get_state` 作为兜底同步。

## 数据流

```
适配器发现会话 → 合并已有状态(保留resume_count)
    → 检测器逐会话检测 → 判定Verdict
    → ConfirmInterrupt + 冷却通过 + 自动续跑开启
    → Resumer执行续跑 → 更新计数 → 推送日志事件
```

## 版本规划

- **v0.1.0**: 核心引擎 + Claude Code 适配器 + macOS 续跑 + Dashboard UI
- **v0.2.0** ✅: Windows/Linux 续跑 + 系统托盘常驻 + 开机自启 + Goal 智能恢复
- **v0.3.0**: SQLite 统计持久化 + Webhook 通知 + 自定义适配器 UI
- **v1.0.0**: AI 智能判断（LLM 分析是否真中断）+ i18n + 自动更新
