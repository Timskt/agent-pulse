<p align="center">
  <img src="src-tauri/icons/icon.png" width="88" alt="AgentPulse">
</p>

# AgentPulse 项目现状与规划

> 对应提交：`cee4248`（`main` == `origin/main`，工作区干净）
> CI：run `30605028183` 全绿（Frontend / Rust macOS / Rust Ubuntu / Rust Windows）
> 文档日期：2026-08-01

这份文档写给三种人看：接手这个仓库的人、半年后忘了细节的我自己、以及想知道"到底做完了没有"的你。
所以它有两条约定：

1. **凡是"做了"的，都能在代码里指到具体位置**（文件 + 符号名），不能指的一律不写成已完成。
2. **凡是没在真机上跑通过的，一律进第 11 节「未验证清单」**，不管代码写得多完整、单测多绿。
   续跑这种"往别人终端里敲字"的功能，单测绿 ≠ 真的能用。

---

## 目录

1. [一页速览](#1-一页速览)
2. [产品定位：这个工具刻意不做什么](#2-产品定位这个工具刻意不做什么)
3. [进度总览](#3-进度总览)
4. [代码地图](#4-代码地图)
5. [运行时架构与数据流](#5-运行时架构与数据流)
6. [检测引擎：两条正交的轴](#6-检测引擎两条正交的轴)
7. [续跑层：跨平台的"确定性分级"](#7-续跑层跨平台的确定性分级)
8. [洞察层：成本、限流预测、统计](#8-洞察层成本限流预测统计)
9. [远程层与通知层](#9-远程层与通知层)
10. [前端、i18n 与配置](#10-前端i18n-与配置)
11. [工程实践：CI、cfg 纪律、图标流水线](#11-工程实践cicfg-纪律图标流水线)
12. [已知欠账与未验证清单](#12-已知欠账与未验证清单)
13. [未来规划与设计](#13-未来规划与设计)
14. [附录](#14-附录)

---

## 1. 一页速览

| 维度 | 现状 |
|---|---|
| 版本 | `1.0.0`（`package.json` / `src/App.tsx: APP_VERSION`） |
| 后端 | Rust，17 个文件，**8479 行**（含单测） |
| 前端 | TypeScript + React 19，26 个 `.ts/.tsx`，**3740 行**（+ `index.css` 71 行） |
| 单元测试 | **80 个**，`cargo test` 全过；三个平台的 CI 都跑 |
| Tauri 命令 | 21 个 `#[tauri::command]` |
| 支持的 Agent | Claude Code / Codex CLI / OpenCode（`all_adapters()`） |
| 续跑平台 | macOS / Windows / Linux 三套实现均已落地 |
| i18n 词条 | 后端 89 条（`(key, zh, en)`），前端 202 条（`[zh, en]`） |
| 持久化 | SQLite 6 张表 |
| 功能层次 | v1.0 核心 ✅ · v1.1 感知 ✅ · v1.2 洞察 ✅ · v1.3 远程 ✅ · v2.0 编排 ⏸ · v2.1 自治 ⏸ |

**这个版本能做到的事**：在你不改变任何使用习惯的前提下，后台盯着 Claude Code / Codex / OpenCode
的会话文件与进程，判断它是"还在干活"、"卡住了"、"在等你回话"、"限流了"还是"报错了"；
该叫人的时候发通知（带托盘角标和声音），该补一句话的时候把续跑提示词**静默**送进它自己那个终端窗口；
同时把 token 用量折算成钱、预测下一次限流窗口、把这一切用一个只读网页暴露到手机上。

**这个版本刻意做不到的事**：它不会帮你启动 agent，不会接管你的终端，不会在拿不准的时候乱敲键盘。
详见下一节。

---

## 2. 产品定位：这个工具刻意不做什么

AgentPulse 是 **非侵入式的 AI Agent 守护神**。这不是一句宣传语，它是三条会否决具体实现的红线：

| 红线 | 具体含义 | 代码里的体现 |
|---|---|---|
| **不代替你启动 agent** | 不 spawn 子进程、不包 pty、不做 wrapper、不要求你从它的入口进 | 全仓库没有任何"启动 agent"的路径；`adapters` 只做 `discover_sessions`（发现已存在的进程） |
| **不确定就不动手** | 认不出这个会话属于哪个窗口/标签时，宁可放弃续跑，也不往别人的窗口里回车 | `Resumer::allow_blind()` 默认 `false`；三个平台的脚本在匹配失败时 `return "refused"` |
| **不改变会话的所有权** | 只在原地补一句话，之后的一切仍然是你和你的 agent 之间的事 | 续跑就是"投递一段文本 + 一个回车"，没有任何接管逻辑 |

### 为什么"非侵入"是护城河而不是限制

同类工具（各种 agent runner / orchestrator / babysitter）的通行做法是：**你从我这里启动 agent**，
于是我拿到 stdin/stdout，卡住了我就写一句 "continue"。这条路技术上简单得多，代价是：

- 你得放弃自己的终端习惯（iTerm 的 profile、VS Code 里的集成终端、tmux 布局全部作废）；
- agent 的输出经过一层转发，TUI 的重绘、颜色、快捷键容易出问题；
- 它挂了你的 agent 也跟着挂；
- 已经跑到一半的会话没法被接管——你必须重开。

AgentPulse 走的是另一条：**附着在你已有的工作方式上**。代价是所有难题都落在"从外部辨认状态"和
"从外部投递输入"这两件事上，也就是本文档第 6、7 节的全部内容。好处是它对你现有的流程零要求，
装上就能守护**已经在跑**的会话，卸掉也不影响任何东西。

这条定位同时解释了为什么 v2.0「编排层」和 v2.1+「自治层」被**主动搁置**：一旦开始编排任务、
自主决定下一步做什么，工具就从"守护"变成了"驾驶"，上面三条红线全部作废。详见 [13.4](#134-被主动搁置的两层v20-编排--v21-自治)。

---

## 3. 进度总览

| 版本 | 主题 | 状态 | 交付内容 |
|---|---|---|---|
| v1.0 | 核心引擎 | ✅ | 适配器发现、四信号检测、双重校验、macOS 续跑、Dashboard、配置持久化、SQLite 记录 |
| v1.1 | **感知层** | ✅ | `TurnState` 回合状态、`error_output` 结构性证据通道、`AttentionLevel` 注意力分级、系统通知 + 声音 + 托盘角标 + 节流、三平台续跑（Windows / Linux 补齐）、剪贴板投递绕开输入法 |
| v1.2 | **洞察层** | ✅ | 21 个模型的价目表、用量归集与去重游标、按天/按项目成本、限流窗口预测、统计面板、会话历史 |
| v1.3 | **远程层** | ✅ | 只读手机看板（令牌 + CSP nonce + 默认只听 127.0.0.1）、Webhook（Slack / Discord / ntfy / Bark / 自定义） |
| v1.3 P2 | 远程审批 | ❌ | 手机上点一下"续跑"——**未实现**，见 [13.2](#132-v15-候选) |
| v2.0 | 编排层 | ⏸ | 主动搁置，与非侵入定位冲突，动工前需确认 |
| v2.1+ | 自治层 | ⏸ | 同上 |

### 最近的提交脉络

```
cee4248  fix(ci): pin resumer_with to macOS, widen the blind-typing test to all three
f3999c3  ci: lint and test all three platforms on every push
a888d91  feat(icon): regenerate the whole icon set from a vector master
49d16a6  feat: perception, insight and remote layers + clipboard-based resume
045e571  fix(startup): remove non-functional updater plugin that crashed app on launch
29ea671  fix(windows): glob backslash escaping + drive letter encoding; perf: single process snapshot per scan
eb09819  fix(ci): replace deprecated macos-13 runner with cross-compilation on macos-latest
```

`49d16a6` 是 v1.1–v1.3 三层的主体；后面三个提交在收拾工程侧的尾巴（图标、CI 矩阵、cfg 纪律）。

---

## 4. 代码地图

### 后端（`src-tauri/src/`，8479 行）

| 文件 | 行数 | 职责 | 关键符号 |
|---|---:|---|---|
| `resumer/mod.rs` | 1775 | 续跑执行器，**唯一带平台 cfg 的文件** | `Resumer::resume`、`resume_macos/windows/linux`、`macos_script`、`windows_resume_script`、`stage_clipboard`、`focus_session`、`TERMINAL_PATTERNS`、`WINDOWS_MULTI_TAB_HOSTS` |
| `monitor/mod.rs` | 890 | 引擎主循环：一轮扫描的编排、事件流、成本告警 | `MonitorEngine::{start,stop,scan_once,collect,run_resumes,check_cost_alerts}`、`EngineEvent`、`ScanOutcome` |
| `detector/mod.rs` | 823 | 判定：要不要续跑（`Verdict`）+ 要不要叫人（`AttentionLevel`） | `Detector::detect`、`make_verdict`、`grade_attention`、`contains_keyword`、`SignalKind` |
| `remote/mod.rs` | 718 | 只读手机看板的 HTTP 服务（手写，无框架） | `RemoteService::{new,sync,generate_token}`、`secret_eq`、`respond_with_nonce`、`page` |
| `storage/mod.rs` | 651 | SQLite 持久化，6 张表 | `record_resume/record_detection/record_scan/record_usage_batch/…` |
| `adapters/claude_code.rs` | 602 | Claude Code 适配器（最完整的那个） | `extract_text_from_jsonl`、`error_output`、`classify_turn`、尾部窗口读取 |
| `lib.rs` | 544 | Tauri 装配：AppState、托盘、事件泵、21 个命令 | `run()`、`setup_tray`、`engine-events` 泵 |
| `cost/mod.rs` | 512 | 价目表、用量归集、按天/项目成本、限流预测 | `PRICE_TABLE`、`price_for`、`CostTracker`、`forecast_rate_limit` |
| `i18n/mod.rs` | 393 | 后端文案表（89 条 `(key, zh, en)`） | `I18n::t`、`TABLE` |
| `config/mod.rs` | 357 | 配置结构与持久化 | `AppConfig` + 6 个子配置、`ConfigManager` |
| `webhook/mod.rs` | 346 | 五种 Webhook 目标的载荷构造 | `WebhookConfig`、`provider` 分派 |
| `notify/mod.rs` | 309 | 系统通知、节流、托盘角标 | `Notifier::{allow,notify_attention,update_tray_badge}`、`composite_badge` |
| `adapters/mod.rs` | 242 | 适配器抽象与进程快照 | `AgentAdapter` trait、`AgentSession`、`TurnState`、`take_process_snapshot` |
| `ai_judge/mod.rs` | 158 | 可选的 LLM 兜底判定（默认关闭，供应商中立） | `AiJudgeConfig` |
| `adapters/opencode.rs` | 78 | OpenCode 适配器 | |
| `adapters/codex.rs` | 75 | Codex CLI 适配器 | |
| `main.rs` | 6 | 入口 | |

### 前端（`src/`，3740 行 + 71 行 CSS）

| 文件 | 行数 | 职责 |
|---|---:|---|
| `components/ConfigPanel.tsx` | 813 | 设置页，配置项最多的地方（也是唯一需要拆分的文件，见欠账） |
| `i18n/index.ts` | 408 | 前端文案表（202 条 `[zh, en]`） |
| `stores/useAppStore.ts` | 320 | Zustand store：状态、事件、命令封装 |
| `types.ts` | 259 | 与 Rust 侧结构一一对应的类型 |
| `components/SessionList.tsx` | 235 | 会话卡片：状态、注意力标记、续跑按钮 |
| `components/CostPanel.tsx` | 208 | 成本页：按天柱状图、按项目、限流预测 |
| `components/StatsPanel.tsx` | 168 | 统计页 |
| `App.tsx` | 161 | 5 个 Tab 的外壳、页头徽标、页脚 |
| `components/ui/Field.tsx` | 157 | 表单字段的统一封装 |
| `components/HistoryPanel.tsx` | 141 | 会话历史 |
| `components/ui/BarChart.tsx` | 113 | 纯 SVG 柱状图（不引图表库） |
| `components/ui/Card.tsx` | 103 | 卡片 |
| `components/ui/Switch.tsx` | 84 | Radix Switch 封装 |
| `components/LogPanel.tsx` | 79 | 活动日志流 |
| `components/ui/{Select,Button,Tooltip,Tabs,index}.tsx` | 33–63 | Radix 组件封装 |
| `lib/display.ts` | 65 | 展示层格式化（时间、金额、截断） |
| `lib/chime.ts` | 60 | WebAudio 提示音（不带音频文件） |
| `components/StatusCards.tsx` | 48 | 顶部四张状态卡 |
| `lib/useNotice.ts` | 36 | 轻量 toast |
| `lib/utils.ts` | 41 | `cn()` 等工具 |

---

## 5. 运行时架构与数据流

### 一轮扫描（`MonitorEngine::scan_once`）

```
                        ┌─────────────────── 每 poll_interval_secs 一轮 ───────────────────┐
                        │                                                                 │
take_process_snapshot() │  ① 发现            ② 取证            ③ 判定          ④ 执行     │
  一次 System::new()    │                                                                 │
  只刷 cmd + cwd  ──────┼─► discover_sessions ─► session_files  ─► Detector  ─► resume_    │
  （整轮共用一份快照，  │   Claude Code         recent_output      ::detect      actions   │
    避免 N 次进程枚举） │   Codex               error_output     ┌──────────┐   ─► Resumer │
                        │   OpenCode            turn_state       │ Verdict  │      ::resume│
                        │                                        │ Attention│              │
                        │                                        └──────────┘              │
                        │  ⑤ 落库 + 事件：storage.record_* / EngineEvent / Webhook / 通知  │
                        └─────────────────────────────────────────────────────────────────┘
```

五个阶段各自的要点：

**① 发现** — `adapters::all_adapters()` 里三个适配器各自扫进程快照。这里有个刻意的性能约束：
整轮扫描只做**一次** `take_process_snapshot()`，且只刷新 `cmd` 与 `cwd` 两个字段；早期版本每个适配器
各枚举一遍进程，在进程多的机器上一轮要几百毫秒。

**② 取证** — 每个会话最多取四份证据，各自可缺：
`session_files`（会话文件路径 + mtime）、`recent_output`（尾部文本，含 assistant 散文）、
`error_output`（**只含被运行时标成故障的行**，不含散文）、`turn_state`（最后一条记录属于谁的回合）。

**③ 判定** — `Detector::detect` 产出 `DetectionResult`，里面同时装着两条独立结论（第 6 节）。

**④ 执行** — 只有 `Verdict::ConfirmInterrupt` 且过了冷却、且 `auto_resume_enabled` 为真，
才会进入 `resume_actions`；随后 `run_resumes` 串行调 `Resumer::resume`。

**⑤ 落库与广播** — 检测、续跑、扫描、用量分别落到 SQLite；`EngineEvent` 进内存环形队列；
达到条件时触发系统通知与 Webhook。

### 进程内的装配（`lib.rs`）

```
AppState {
  engine:          MonitorEngine     // 主循环 + 事件队列
  config_manager:  ConfigManager     // 读写 config.json
  storage:         Storage           // SQLite
  notifier:        Notifier          // 系统通知 + 托盘角标
  remote:          RemoteService     // 只读看板
}
```

- **托盘**：`TrayIconBuilder::with_id(notify::TRAY_ID)`，图标取 `app.default_window_icon()`；
  菜单 5 项（显示 / 开始 / 停止 / 立即扫描 / 退出），左键点击切换主窗口显示。
- **事件泵**：一个 800 ms 的 `spawn` 循环，把引擎新产生的事件批量 `emit("engine-events", …)` 给前端。
  前端因此不需要轮询 `invoke`，也不会因为一轮扫描很慢而卡住 UI。
- **自启动**：`tauri_plugin_autostart`（macOS 用 `LaunchAgent`）。
- **注意**：`045e571` 移除了 updater 插件——它在没有配置签名公钥的情况下会让应用启动即崩。
  自动更新要重做（见 [13.2](#132-v15-候选)）。

### SQLite 表（`storage/mod.rs`）

| 表 | 用途 |
|---|---|
| `resume_records` | 每次续跑：会话、时间、用的哪个提示词、结果文本 |
| `detection_records` | 每次确认中断：信号、判定、注意力级别 |
| `daily_stats` | 每天的扫描/检测/续跑计数 |
| `usage_records` | 从会话文件里归集的 token 用量（按会话文件 + 行号去重） |
| `usage_cursors` | 每个会话文件的读取游标，保证重启不重复累计 |
| `session_history` | 会话的生命周期快照，供历史页回看 |

---

## 6. 检测引擎：两条正交的轴

整个 `detector` 模块的设计核心是一句话：**"要不要替你敲一句话"和"要不要现在叫你过来"是两个问题，
不能用同一个阈值回答。**

| | `Verdict`（中断判定） | `AttentionLevel`（注意力分级） |
|---|---|---|
| 回答的问题 | 要不要**续跑** | 要不要**现在叫人** |
| 取值 | `Running` / `Suspicious` / `ConfirmInterrupt` / `TaskCompleted` | `None` / `NeedsInput` / `Completed` / `RateLimited` / `Error` |
| 判错的代价 | **高**——往一个正在干活的会话里回车，会打断它、污染上下文 | **低**——多发一条通知，代价只是你瞥一眼 |
| 因此阈值 | 极严，只认两种确定情形 | 明显更松，宁可多叫一次 |

这条区分是被实际 bug 逼出来的：早期版本用同一套信号同时决定"通知"和"续跑"，
结果要么通知太少（漏掉真的在等你的会话），要么续跑太猛（在压缩上下文的间隙里敲字）。

### 6.1 四种信号（`SignalKind`）

| 信号 | 含义 | 来源 |
|---|---|---|
| `FileStale` | 会话文件在 `idle_timeout_secs` 内没更新 | `session_files` 的 mtime |
| `KeywordMatch` | 输出里命中了中断关键词 | `recent_output` |
| `ProcessExited` | 进程没了 | 进程快照 |
| `HeartbeatTimeout` | 心跳超时 | 会话文件时间线 |

曾经存在的第五种信号 `ProcessIdle`（CPU 占用为 0）**已被删除**：CPU 0% 分不清"在等 API 返回"
和"在等人打字"，这两种情况一个不该动、一个该动，用一个分不开的信号去投票只会污染判定。

### 6.2 回合状态（`TurnState`）——最强的一份证据

```rust
enum TurnState { Unknown, ToolRunning, Busy, AwaitingUser }
```

`TurnState::is_busy()` 对 `ToolRunning` 与 `Busy` 为真，**对 `Unknown` 为假**（拿不准不等于在忙）。
它由 `adapters::claude_code::classify_turn` 从会话文件的最后一条**有效**记录推出：

| 最后一条记录 | 结论 | 理由 |
|---|---|---|
| assistant 且含 `tool_use` | `ToolRunning` | 它发起了工具调用，正等结果 |
| assistant 且只有文本 | `AwaitingUser` | 它说完话停下了——**这就是"它以为干完了，其实没干完"的形态** |
| `user` / `attachment` / `system` | `Busy` | 球在模型那边（`tool_result`、压缩产物都走这条） |
| 记账类记录 | **跳过，继续往前找** | 见下 |
| 读不出来 | `Unknown` | |

被跳过的记账类记录共 8 种：`mode`、`permission-mode`、`file-history-snapshot`、`file-history-delta`、
`last-prompt`、`queue-operation`、`ai-title`、`summary`。它们会在会话文件里追加行，但**不代表任何一方在动**。
如果不跳过，一次切换模型或一次自动改标题就会把 `AwaitingUser` 冲成 `Busy`，于是"它停下等你"的
信号被吃掉——这正是"每次都要我去发继续"的根因之一。
（测试：`bookkeeping_lines_do_not_change_the_turn`）

### 6.3 判定表（`make_verdict`）

```rust
// 在忙 → 永不确认中断，最多标为可疑
if turn_state.is_busy() {
    return if transcript_idle { Verdict::Suspicious } else { Verdict::Running };
}
match (turn_state, transcript_idle) {
    (TurnState::AwaitingUser, true)  => Verdict::ConfirmInterrupt, // 它停下了 + 文件也静了
    (TurnState::AwaitingUser, false) => Verdict::Running,          // 刚停，再等等
    (_, true)                        => Verdict::ConfirmInterrupt,
    (_, false) => if keyword_hit { Verdict::Suspicious } else { Verdict::Running },
}
```

只有两种情形会被升成 `ConfirmInterrupt`，其余最多到 `Suspicious`，而 `Suspicious` **永远不触发续跑**。
另外：`keyword_hit` 单独出现从不足以续跑（测试 `keyword_hit_alone_never_types`）——
输出里出现 "rate limit" 四个字，可能只是 agent 在跟你解释什么是限流。

### 6.4 两条必须同时成立的铁律

这两条互相拉扯，是整个检测逻辑最容易被改坏的地方，各自都有专门的测试钉住：

**铁律 A：长时间的上下文压缩不能被当成卡住。**
压缩期间会话文件可以几分钟不动，但球在模型那边。实现上有两道保险：
① 压缩产物是 `system`/`user` 类记录，`classify_turn` 归为 `Busy`；
② `const BUSY_GRACE_MULTIPLIER: u64 = 10` —— 只要处于忙碌回合，静默容忍度直接放大 10 倍。
（测试：`long_compaction_pause_is_not_a_stall`、`busy_turn_is_never_confirmed_even_when_stale`、
`tool_result_and_compaction_both_count_as_busy`）

**铁律 B：真正的 `AwaitingUser` 停滞必须仍然能续跑。**
不能为了不误判而把忙碌门槛收得太紧，否则就回到"它其实没干完活，每次都要我去发继续"。
`(AwaitingUser, transcript_idle)` 这一格是**唯一**允许在没有任何错误信号时直接确认中断的路径。
（测试：`awaiting_user_plus_silence_confirms`、`text_only_reply_means_it_stopped_for_a_human`）

### 6.5 注意力分级（`grade_attention`）与"散文不是证据"

优先级自上而下短路：**完成 > 出错 > 限流 > 等待输入 > 卡住**。

关键设计：**「出错」和「限流」两级只读 `error_output`，永不读 assistant 散文。**

这条规则来自一个真实误报：界面上出现过 `⚫ 出错 · agent-pulse 进程 52652 Terminal ttys001`，
理由是"输出里出现了「500」"——而那个 `500` 只是 agent 在正常讲话时提到的一个数字。
修法不是给关键词表打补丁，而是**换证据通道**：适配器额外提供一个 `error_output`，
只收集被运行时明确标成故障的行：

- `system` 类型且 `level == "error"` 的行；
- `isApiErrorMessage` 为真的行；
- 带 `apiErrorStatus` 的行。

assistant 说的话一律不进这个通道。（测试：`talking_about_an_error_is_not_having_one`、
`token_counts_do_not_look_like_errors`、`api_error_lines_are_picked_up`、`error_level_system_lines_are_kept`）

同源的第二个决定：**工具调用的载荷永远不进关键词匹配器**。工具参数里出现 "error"、"failed"
是家常便饭（比如在 grep 这些词）。（测试：`tool_payloads_never_reach_the_keyword_matcher`）

### 6.6 关键词匹配（`contains_keyword`）

- 英文按 **ASCII 词边界**匹配，`overloaded` 不会被 `reloaded` 命中；
- 中文走旁路（中文没有空格边界，按子串匹配）；
- 匹配对象按级别分流到 `recent_output` 或 `error_output`，见上。

### 6.7 结论对象

```rust
struct DetectionResult {
    session_id, interrupted, signals, has_completion_marker, matched_marker,
    has_active_goal,        // 是否存在未完成的 goal → 决定用哪条续跑提示词
    verdict, attention, attention_detail, detected_at,
}
```

`has_active_goal` 为真时，续跑用 `goal_resume_prompt`（明确要求"不要重新规划，从中断处继续"），
否则用普通 `resume_prompt`。

另外，取证阶段的参数是用一个 `AttentionInput<'a>` 结构体打包传的，不是一长串裸参数——
这个改动纯粹是为了防止同类型参数被写反位置（几个 `&str` 和 `bool` 挨在一起时编译器帮不了你）。

---

## 7. 续跑层：跨平台的"确定性分级"

续跑要解决两个完全独立的难题。

### 7.1 难题一：投递什么——提示词走剪贴板，不走合成按键

**症状**：点"带目标续跑"后，终端里出现的是
`啊啊啊啊啊啊啊啊啊goal啊啊啊啊啊啊，aaaaaaaaaa.aaaaaa，aaaaaaaaaaaa。`

**原因**：合成按键（AppleScript `keystroke`、Windows `SendKeys`、`xdotool type`）走的是键盘事件通道，
会**经过输入法**。中文输入法把 ASCII 字母当拼音吃掉，于是 `goal` 之外的中文字符被重新组词，
拼音残留就变成了一串"啊"。这不是转义 bug，是通道选错了。

**方案**：非 ASCII 提示词一律**先进系统剪贴板，再发一次纯 ASCII 的粘贴组合键**：

| 平台 | 写剪贴板 | 粘贴键 |
|---|---|---|
| macOS | `stage_clipboard()` 生成的 AppleScript（`set the clipboard to …`） | `⌘V` |
| Windows | PowerShell `Set-Clipboard`（单引号字面量） | `^v` |
| Linux | `wl-copy` / `xclip` / `xsel`（**文本从 stdin 进，不拼进命令行**） | `Ctrl+Shift+V` |

粘贴组合键本身全是 ASCII，输入法不会改写它。附带两个细节：
- **用完把剪贴板还给你**（测试 `the_users_clipboard_is_given_back`）——不能因为续跑吞掉你复制的东西；
- **iTerm2 是例外**：AppleScript 的 `write text` 直接写进伪终端，根本不经过键盘事件，
  所以 iTerm2 走原生通道，不动剪贴板（测试 `iterm_writes_straight_to_the_pty`）。

**安全约束**：用户可自由编辑的提示词**永不**被拼进 shell 字符串。Linux 侧走 stdin，
Windows 侧用 PowerShell 单引号字面量并转义内部单引号（测试
`windows_prompt_quotes_cannot_break_out_of_the_literal`、`prompts_go_through_the_clipboard_not_the_keyboard`）。

### 7.2 难题二：投给谁——"定位不到就不敲"

这是整个项目里最要命的一条：**敲错窗口比不敲严重得多。** 往同事的编辑器、往另一个正在跑测试的
终端里回车，后果不可控。所以续跑路径按"我能多确定这些按键会落在哪"分成三级：

| 确定性 | 依据 | 行为 |
|---|---|---|
| **精确** | TTY 号能对上具体会话（iTerm2 遍历 session 比对 `tty`；Terminal 比对 tty） | 直接投递，返回 `"matched"` |
| **窗口级** | 认不到 TTY，但**窗口标题里含项目名** | 投递，`title_matched_script` |
| **不确定** | 两者都对不上 | 默认 `return "refused"`，**什么都不敲**；只有开了「跟随最新会话」（`auto_follow_latest`，默认 `false`）才允许盲敲 |

`Resumer::allow_blind()` 是这条策略的唯一开关，三个平台共用。它默认为假，且有一个跨三平台运行的
测试钉住这件事（`blind_typing_is_off_by_default`）；VS Code 系的两个测试专门验证"标题没对上就
提前退出、一个键都不发"（`vscode_refuses_when_there_is_no_window_to_match`、
`vscode_bails_out_before_typing_when_the_title_missed`）。

### 7.3 窗口级确定性 vs 标签级确定性

这是 Windows 侧一个容易忽略的区别：

- **`cmd` / `conhost`**：一个控制台窗口就是一个会话。找到宿主窗口 = 找到了会话，**不需要标题匹配**。
- **Windows Terminal / Hyper / Tabby / ConEmu / VS Code / Cursor / Windsurf / JetBrains 全家桶**：
  一个窗口里有 N 个标签，找到窗口**不等于**找到会话。必须靠"窗口标题含项目名"再确认一次，
  否则可能敲进同一个 VS Code 的另一个标签。

于是有了 `pub const WINDOWS_MULTI_TAB_HOSTS`（24 个进程名），**刻意不包含 `cmd` 和 `conhost`**。
它是纯数据表，所以每个平台的 CI 都能测它（测试 `windows_multi_tab_hosts_need_a_title_match`、
`windows_blind_permission_is_the_only_way_past_an_unmatched_tab`）。

### 7.4 三平台实现

| 平台 | 技术栈 | 定位链路 | 关键实现 |
|---|---|---|---|
| **macOS** | AppleScript（`osascript`，8 秒 timeout 兜底） | 进程 → TTY（`session_tty`）→ 终端 App（`session_terminal_app`，按 bundle 名而非可执行名）→ 精确/标题/拒绝 | `macos_script`、`title_matched_script`、`focus_session` |
| **Windows** | PowerShell + Win32（`tokio::process::Command` 全限定调用） | 进程 → 沿父进程链向上走到宿主窗口（`find_terminal_for_pid`）→ 若在多标签名单里则要求标题含项目名 → 前台化后**确认窗口真的到前台了**再投递 | `windows_resume_script`（测试 `windows_walks_up_to_the_host_window`、`windows_confirms_the_window_actually_came_forward`） |
| **Linux** | `xdotool`（X11）/ `ydotool`（Wayland 兜底） | 进程 → `find_x11_window_for_pid` → 窗口激活 → 粘贴 | `resume_linux`、`resume_linux_ydotool`（测试 `ydotool_releases_every_modifier_it_presses`）、`set_clipboard_linux`（覆盖 Wayland + X11 三种工具，测试 `clipboard_tools_cover_wayland_and_x11`） |

### 7.5 终端识别的数据表

| 表 | 条数 | 内容 |
|---|---:|---|
| `HELPER_MARKERS` | 8 | `helper` / `crashpad` / `renderer` / `gpu` / `utility` / `plugin` / `codesigning` 等——Electron 应用的辅助进程，**不是终端**（测试 `helper_processes_are_never_terminals`） |
| `TERMINAL_PATTERNS` | 26 | iTerm2、VS Code / VSCodium、Cursor、Windsurf、Trae、Qoder、Warp、WezTerm、kitty、Alacritty、Ghostty、Konsole、Tilix、xterm，以及 JetBrains 全家桶（IDEA / PyCharm / WebStorm / GoLand / CLion / RustRover / PhpStorm / RubyMine / DataGrip / Rider / Android Studio） |
| `TITLE_MATCHED_APPS` | 16 | macOS 上必须靠标题匹配才敢投递的 App |
| `WINDOWS_MULTI_TAB_HOSTS` | 24 | 见 7.3 |

识别规则有三条被测试钉住的边界：按 **bundle 名**而不是可执行名判断
（`bundle_name_decides_not_the_executable`、`app_bundle_name_only_matches_real_bundles`）、
IDE 按**进程名前缀**匹配而不是精确等于（`ide_paths_match_by_process_prefix_not_exact_app_name`）、
名字里**碰巧含 `code`** 的进程不算 VS Code（`names_that_merely_contain_code_are_not_vs_code`）。

### 7.6 生成脚本的语法自检

`macos_script` 生成的是 AppleScript 文本，写错一个引号要到运行时才炸。所以有一个测试
（`every_generated_script_compiles`）把 **11 种终端 × 2 条刁钻提示词（含中文、引号、反斜杠）×
盲敲开/关** 组合生成的脚本，逐个交给 `osacompile` 真编译一遍，并断言至少编成 8 个
（否则说明测试自己在空转）。需要应用词典才能编译的（iTerm2 / Terminal）先用
`id of application` 确认已安装——查 bundle 不会启动应用；`osacompile` 不存在则整体跳过，
不让环境差异变成红灯。

另有一个专门的负向断言：**VS Code 系的脚本里绝不能出现 `using control down`**
（`vscode_script_never_sends_ctrl_c`）。理由很具体：如果焦点不在集成终端而在编辑器里，
`Ctrl-C` 是"复制"，紧接着的提示词就会被写进你的源文件。

### 7.7 保护机制

| 机制 | 默认值 | 作用 |
|---|---|---|
| `max_resume_count` | 5 | 单会话最多续跑几次，防止无限循环（测试 `resume_cap_stops_further_typing`） |
| `resume_cooldown_secs` | 30 | 两次续跑之间的冷却（测试 `cooldown_*`） |
| `auto_resume_enabled` | `true` | 总开关；关掉后只通知不动手 |
| `auto_follow_latest` | `false` | 盲敲授权，见 7.2 |

`resumer` 模块共 **26 个单元测试**，是全仓库测试最密的模块——因为它是唯一会"对外界产生副作用"的模块。

---

## 8. 洞察层：成本、限流预测、统计

### 8.1 价目表（`cost::PRICE_TABLE`，21 个模型）

| 模型 | 输入 $/1M | 输出 $/1M |
|---|---:|---:|
| `claude-fable-5` / `claude-mythos-5` | 10 | 50 |
| `claude-opus-5` / `4-8` / `4-7` / `4-6` / `4-5` | 5 | 25 |
| `claude-sonnet-5` | 3 | 15（引入期 2 / 10，至 `2026-08-31`） |
| `claude-sonnet-4-6` / `4-5` / `4` | 3 | 15 |
| `claude-haiku-4-5` | 1 | 5 |
| `claude-opus-4-1` / `opus-4` / `3-opus` | 15 | 75 |
| `3-7-sonnet` / `3-5-sonnet` | 3 | 15 |
| `3-5-haiku` | 0.8 | 4 |
| `3-haiku` | 0.25 | 1.25 |

三个设计点：

- `price_for(model, date, overrides)` 走**最长前缀匹配**——会话文件里的模型名常带日期后缀或前缀变体，
  精确匹配会静默漏算；最长前缀能把 `claude-opus-5-xxxx` 正确落到 `claude-opus-5`。
- **引入期价格按日期生效**：`SONNET_5_INTRO = 2 / 10`，`SONNET_5_INTRO_END = "2026-08-31"`；
  过了这天自动回到 3 / 15，不需要发版。
- **`price_overrides` 可由用户覆盖**（`ModelPriceOverride`）——价目表一定会过时，
  这是让你不用等更新就能算对钱的逃生口。

### 8.2 用量归集

用量从会话文件里读，因此必须解决"重启后不重复累计"：`usage_cursors` 表记录每个会话文件读到哪，
`record_usage_batch` 按（会话文件 + 位置）去重。另有 `MIN_REFRESH_INTERVAL_SECS = 20` 限制刷新频率，
避免 UI 频繁触发全量重算。

### 8.3 限流预测（`forecast_rate_limit`）

用 `rate_limit_window_hours`（默认 5，对齐 Anthropic 的滚动窗口）和 `rate_limit_token_budget`
算出"按当前速度还能撑多久"。`tokens_in_last_hours` 从 SQLite 直接聚合。
预算为 0 时视为未设置，不出预测。

### 8.4 成本告警

`CostConfig{daily_budget_usd, session_budget_usd, alert_at_percent: 80}`，
由 `MonitorEngine::check_cost_alerts` 在每轮扫描后检查，走通知层。预算默认为 0（不告警）。

### 8.5 统计与历史

- `daily_stats` 支撑统计页（扫描/检测/续跑计数、成功率）；
- `session_history` + `upsert_session_history` 支撑历史页，能回看已经退出的会话；
- 成本页有按天柱状图（自绘 SVG，`ui/BarChart.tsx`，没有引入图表库）和按项目聚合。

---

## 9. 远程层与通知层

### 9.1 只读手机看板（`remote/mod.rs`）

一个手写的 HTTP 服务（没有 web 框架），设计上把"只读"做成**结构性保证**而不是承诺：

| 约束 | 实现 |
|---|---|
| 只有两个端点 | `/`（页面）和 `/api/state`（JSON）；路由表里没有别的 |
| 非 GET 一律拒绝 | 返回 `405` |
| 两个端点都要令牌 | `Authorization: Bearer` 头或 `?token=` 查询参数 |
| 令牌比较不泄漏时序 | `secret_eq` 定长比较（测试 `secret_eq_matches_only_exact`） |
| 空令牌 fail-closed | 没设令牌就不放行 |
| 默认只听本机 | `bind_all: false` → `127.0.0.1`；`port: 17650` |
| 无 CORS | 不发 `Access-Control-Allow-*` |
| 每请求一个 CSP nonce | `respond_with_nonce` |
| HTML 转义 | `escape_html`，页面模板有"占位符必须全部被替换"的测试（`page_has_no_leftover_placeholders`） |

**安全提示是产品的一部分**：把 `bind_all` 从关到开时，设置页会明确告知
「从 127.0.0.1 改成 0.0.0.0：同一个网络里的人只要拿到令牌就能看你的会话，请只在可信网络里开」
（英文版同义）。**令牌只进剪贴板**，不显示在设置页、不写进活动日志——
日志是会被截图发出去的东西。

远程层共 8 个测试，覆盖两种令牌来源、定长比较、页面渲染。

### 9.2 通知（`notify/mod.rs`）

| 能力 | 说明 |
|---|---|
| 触发点 | `on_needs_input` / `on_completed` / `on_rate_limited` / `on_error` 默认开，`on_resumed` 默认关 |
| 节流 | `throttle_secs: 120`，`Notifier::allow()` 统一把关 |
| 声音 | `sound_enabled` + `sound_volume: 60`；前端用 WebAudio 合成（`lib/chime.ts`），不带音频文件 |
| 托盘角标 | `tray_badge: true`，`composite_badge` 把待处理数量画到托盘图标上 |

为什么 `on_resumed` 默认关：续跑成功是"系统正常工作"，不需要打断你；只有需要你**做决定**的事才配得上一次通知。

### 9.3 Webhook（`webhook/mod.rs`）

支持 `slack` / `discord` / `ntfy` / `bark` / `custom` 五种目标，占位符
`{agent_name}` `{session_id}` `{verdict}` `{message}`。触发点：确认中断时，以及
`TaskCompleted` 的**状态跃迁那一轮**（不是每轮都发——否则一个跑完的会话会每 10 秒骚扰你一次）。
6 个测试覆盖各家载荷格式。

---

## 10. 前端、i18n 与配置

### 10.1 结构

Radix 组件 + Tailwind，5 个 Tab：`dashboard` / `stats` / `cost` / `history` / `config`。
`ui/` 下自建了 7 个封装组件（Button / Card / Field / Select / Switch / Tabs / Tooltip + BarChart），
业务组件只用封装层，不直接碰 Radix——这是为了让样式改动只落在一个地方。

状态走 Zustand（`stores/useAppStore.ts`），事件靠后端 `emit("engine-events")` 推送而非前端轮询。
页脚显示会话数、**真实的**轮询间隔（读配置而不是写死）、上次扫描时间。

### 10.2 i18n 边界：谁渲染，谁持有文案

这是被明确要求过的一条规矩，也是唯一可行的分法：

| 侧 | 词条数 | 形态 | 覆盖 |
|---|---:|---|---|
| 后端 | 89 | `(key, zh, en)` | 托盘菜单、系统通知、活动日志、续跑结果文本、远程页面 |
| 前端 | 202 | `[zh, en]` | 所有界面文案 |

两边都由 `config.language` 驱动。后端 i18n 有 5 个测试，其中两个是防腐的：
`no_duplicate_keys`（重复 key 会静默覆盖）和 `placeholders_are_all_documented`
（文案里的 `{}` 占位符必须都有出处）。

**没有任何用户可见文案是硬编码的**，包括续跑失败的原因、托盘菜单项、远程页面的字段名。
对应的一条产品约束：中英夹杂的"不土不洋"搭配要避免——中文界面下不是不能出现英文（模型名、
`token` 这类专有名词照旧），而是不做 `会话 Session 状态 Active` 这种混排。

### 10.3 配置全表（`config/mod.rs`）

配置文件：`dirs::config_dir()/agent-pulse/config.json`
（macOS 上是 `~/Library/Application Support/agent-pulse/config.json`）。

**主配置**

| 键 | 默认值 | 说明 |
|---|---|---|
| `poll_interval_secs` | `10` | 轮询间隔 |
| `idle_timeout_secs` | `60` | 会话文件静默多久算 stale（忙碌时 ×10，见 6.4） |
| `idle_threshold` | `3` | 连续多少轮才升级判定 |
| `max_resume_count` | `5` | 单会话续跑上限 |
| `resume_cooldown_secs` | `30` | 续跑冷却 |
| `check_on_startup` | `true` | 启动即扫一次 |
| `auto_resume_enabled` | `true` | 自动续跑总开关 |
| `auto_follow_latest` | `false` | **盲敲授权**，默认关 |
| `heartbeat_log` | `false` | 把每轮心跳也写进活动日志（调试用） |
| `language` | `"zh"` | 驱动前后端两张文案表 |
| `enabled_adapters` | `["claude-code","codex","opencode"]` | |
| `resume_prompt` | `"请继续完成刚才未完成的任务，不要重新开始。"` | |
| `goal_resume_prompt` | `"你之前有一个活跃的 goal 目标还未完成，请立即恢复并继续执行。不要重新规划，直接从上次中断的地方继续。"` | |
| `goal_keywords` | 9 条 | 判断是否存在未完成目标 |
| `completion_markers` | 5 条 | 命中即视为完成，**不续跑** |
| `custom_keywords` | 4 条 | 中断关键词（rate limit / overloaded …） |
| `input_keywords` | 12 条 | 「在等你回话」的证据 |
| `rate_limit_keywords` | 8 条 | 只对 `error_output` 生效 |
| `error_keywords` | 10 条 | 只对 `error_output` 生效 |

**子配置**

| 结构 | 默认值要点 |
|---|---|
| `NotificationConfig` | `enabled: true`，四类事件默认开、`on_resumed` 关，`sound_volume: 60`，`throttle_secs: 120`，`tray_badge: true` |
| `CostConfig` | `enabled: true`，两个预算默认 `0.0`（不告警），`alert_at_percent: 80`，`rate_limit_window_hours: 5` |
| `RemoteConfig` | `enabled: false`，`port: 17650`，`bind_all: false`，`token: ""` |
| `WebhookConfig` | 默认关；provider + url + topic + template |
| `AiJudgeConfig` | 默认关；供应商中立（见 12.4） |
| `CustomAdapterConfig` | 自定义适配器的配置载体（UI 未做，见 13.1） |
| `ModelPriceOverride` | 用户覆盖价目表 |

---

## 11. 工程实践：CI、cfg 纪律、图标流水线

### 11.1 CI 现状（`.github/workflows/ci.yml`）

| Job | 触发 | 内容 |
|---|---|---|
| `check-rust` | 每次 push / PR | **三平台矩阵**（ubuntu / macos / windows，`fail-fast: false`）→ `cargo clippy --all-targets -- -D warnings` → `cargo test` |
| `check-frontend` | 每次 push / PR | pnpm 11 + Node 22 → `pnpm build`（`tsc && vite build`，类型错误即红） |
| `build-tauri` | **仅 `v*` 标签** | 4 个目标：`aarch64-apple-darwin`、`x86_64-apple-darwin`、`x86_64-unknown-linux-gnu`、`x86_64-pc-windows-msvc` |
| `release` | 仅 `v*` 标签 | 汇总产物 → `softprops/action-gh-release@v2` |

两个刻意的选择：

- **三个平台都跑 lint 和测试**。续跑层几乎全是 `#[cfg(target_os = …)]`，只在 ubuntu 上跑 clippy
  等于 Windows 和 macOS 分支从来没被编译过——上一版就是这么让一个 Windows 专属编译错误
  躺到打标签才暴露的。
- **`--all-targets`**。不加它，`#[cfg(test)]` 里的代码不会被编译；80 个单元测试是跨平台脚本
  唯一的自动化保障，"能编译"和"测试也能编译"是两件事。

最近一次绿灯：run `30605028183`（Frontend 16s / macOS 1m / Ubuntu 2m / Windows 1m；
`build-tauri` 与 `release` 因为不是标签推送而 Skipped，符合预期）。

### 11.2 一个值得记住的教训：cfg 必须跟调用点严格对齐

连续两次 CI 失败是同一个缺陷类：

1. `error: method 'outcome_text' is never used` —— 它挂了三个平台的 cfg，实际只有 macOS 和 Windows 调它，
   Linux 上就成了死代码，`-D warnings` 直接红。
2. `error: function 'resumer_with' is never used`（`lib test` 单元）—— 加上 `--all-targets`
   之后测试代码第一次被编译，暴露出这个只有 macOS 测试在用的 helper 挂了过宽的 cfg。

抽出来的规则：

> **`pub` 项免疫 `dead_code`，私有项不免疫。** 所以私有项（含 `#[cfg(test)]` 里的 helper）
> 的 `#[cfg]` 必须**恰好等于**它所有调用点的并集——多挂一个平台，那个平台就红。

反过来，跨平台的**脚本生成器**（`windows_resume_script` 等纯字符串函数）和数据表
（`WINDOWS_MULTI_TAB_HOSTS`）是**故意不加 cfg 且声明为 `pub`** 的：它们不碰系统 API，
不加 cfg 才能让三个平台的 CI 都编译它们、都跑它们的测试。这是"用 `pub` 换测试覆盖"的有意取舍。

### 11.3 本地验证的天花板

开发机上只装了 `aarch64-apple-darwin` 一个 target，而 `libsqlite3-sys`（`rusqlite` 的 bundled 特性）
需要目标平台的 C 工具链，所以**本地无法交叉 lint Windows / Linux**。这不是懒，是环境约束。
补偿手段有三层：

1. 三平台 CI 矩阵（11.1）；
2. 平台无关的纯字符串生成器 + 数据表故意不加 cfg，让每个平台都能跑它们的测试（11.2）；
3. 改动 cfg 时做一遍静态审计：逐个确认每个带 Linux/Windows cfg 的私有项都有同平台调用点。

### 11.4 图标流水线（`scripts/gen-icons.sh`）

原来仓库里最大的图标只有 256px，`.icns`/`.ico` 里的小尺寸全是位图缩出来的，边缘一圈毛刺。
现在是**从 SVG 母版矢量渲染每一个尺寸**，缩放这一步彻底不存在：

- 母版两份：`master.svg`（满幅，用于 Windows / Linux / 窗口 / 托盘）和
  `master-macos.svg`（内容 824/1024，给 Dock 网格留白——macOS 的图标规范要求留边，
  满幅图标在 Dock 里会显得比邻居大一号）。
- 渲染器用 **headless Chrome**（`--force-device-scale-factor=1 --default-background-color=00000000`）：
  这台机器上没有 rsvg-convert / inkscape / magick，而 Chrome 的 SVG 光栅化质量与它们同级，
  且几乎人人都装了。脚本会依次探测 Chrome / Chromium / `google-chrome` / `chromium`。
- `.icns` 用 `iconutil` 从 10 张图组成的 iconset 打包；`.ico` 用 `scripts/make_ico.py`
  打包 16/24/32/48/64/128/256 七个尺寸各一张真实渲染图。
- 顺手产出网页端 favicon（`public/icon.svg` + `public/favicon.png`）。

一个相关的细节：`index.html` 的首屏底色写死成 `#fafafa`。界面早就换成浅色了，但 HTML 里
还留着 `class="dark"` 和 `bg-gray-950`，于是 React 挂载前会闪一下深色，且底部露白处是黑的。

---

## 12. 已知欠账与未验证清单

这一节是全文最重要的部分。**代码写完 ≠ 功能可用**，尤其是"往别人终端里敲字"这种事。

### 12.1 从未在真机上跑通过（⚠️ 高优先级）

| 项 | 状态 | 怎么验 |
|---|---|---|
| **剪贴板续跑路径** | ⚠️ 三个平台**都没有实机验证过**。单测只保证生成的脚本语法正确、内容里走的是剪贴板而不是合成按键 | 在 macOS 的 iTerm2 / Terminal / VS Code 集成终端里各跑一次真实续跑，看中文有没有变成"啊啊啊" |
| **Windows 续跑** | ⚠️ 只有 CI 编译 + 纯字符串单测 | 在 cmd、Windows Terminal、VS Code 各验一次；重点看多标签宿主的标题匹配 |
| **Linux 续跑** | ⚠️ 同上，且 X11 / Wayland 两条路都没实测 | 各验一次；`ydotool` 需要 uinput 权限，估计会有坑 |
| **手机看板** | ⚠️ 没有用真实浏览器打开过 | 手机连同一 WLAN，开 `bind_all`，扫码进页面 |
| **打包路径** | ⚠️ 4 个目标的 `build-tauri` 自这些改动以来**没有跑过**（它只在 `v*` 标签上触发） | 打一个 `v1.0.1` 或先推个测试标签 |

### 12.2 文档与配置的陈旧项（小、但会误导人）

| 项 | 问题 | 修法 |
|---|---|---|
| `docs/architecture.md` | 写于 `49d16a6` 之前，**完全没提感知层 / 洞察层 / 远程层**，读它会以为项目停在 v1.0 | 重写，或直接指向本文档 |
| `src-tauri/tauri.conf.json` | `icon` 数组只有 5 项（`32x32` / `128x128` / `128x128@2x` / `.icns` / `.ico`），**漏了 `icons/icon.png`**（512px，脚本已经生成） | 补一行 |
| `README.md` 路线图 | 把 Windows/Linux 支持、Codex 适配器、系统托盘列在"v0.2.0 未完成"里，实际都已交付；版本号也还停在 v0.1.0 的叙述 | 按第 3 节的进度表更新 |
| `README.md` 前置要求 | 写 Node 20+ / pnpm 9+，CI 实际用 Node 22 / pnpm 11 | 对齐 |
| `README.md` 配置说明 | 只列了 7 个配置项，实际有 20+ 个主配置 + 7 个子配置 | 指向本文档 10.3 |

### 12.3 结构性欠账

- **`src/components/ConfigPanel.tsx` 813 行**，是前端唯一明显该拆的文件。
  按 Tab 内的分组（检测 / 续跑 / 通知 / 成本 / 远程 / Webhook / 高级）拆成 6–7 个子组件即可，
  `ui/Field.tsx` 已经把表单样板抽干了，拆分是纯搬运。
- **前端没有任何自动化测试**（无 vitest / playwright）。目前的保障只有 `tsc` 类型检查。
  真正值得测的是 `lib/display.ts` 的格式化和 `stores/useAppStore.ts` 的状态归约。
- **自动更新缺失**。`045e571` 移除了 updater 插件（没配签名公钥会导致启动即崩），
  现在只能手动下载新版本。

### 12.4 AI 兜底判定只做了一半

`ai_judge/mod.rs` 存在、可配置、有 Tauri 命令入口（`lib.rs:306` 附近），
但**没有接进自动检测回路**——`detector` 不会调它。也就是说它现在是一个"你可以手动问一句"的功能，
不是"拿不准时自动请求二次判断"的机制。

另外它刻意保持**供应商中立**（OpenAI 兼容的 `api_url` + 可换模型），这是给自建/代理端点留的口子，
不改成任何单一供应商的专用 SDK。

### 12.5 主动搁置（不是欠账，是决定）

- **v1.3 P2 远程审批**：手机上点一下就续跑。没做，因为它把只读服务变成可写服务，
  整个 9.1 的安全模型要重做。设计见 13.2。
- **v2.0 编排层 / v2.1+ 自治层**：与非侵入定位冲突，见 13.4。**动工前必须先确认。**

---

## 13. 未来规划与设计

规划的第一原则：**下一个版本的价值不在新功能，而在让已经写完的功能变得可信。**
现在最大的风险不是"少了什么"，而是"续跑这条主链路没有任何实机证据"。

### 13.1 v1.4 候选：把已有功能钉成可信（建议优先做完这一整节再谈新功能）

**P0 — 续跑演练（dry-run）按钮** ★ 最推荐做的一件事

在会话卡片上加一个"演练定位"按钮：走**完整的定位链路**（找 TTY → 找终端 App → 找窗口 →
标题匹配），但**在投递前停下**，把结果如实报给你：

```
✅ 精确匹配   iTerm2 · session tty /dev/ttys003
⚠️ 窗口级匹配 Code · 窗口标题含「agent-pulse」——同一窗口若有多个标签，可能敲错
❌ 定位不到   Windsurf · 标题里没有项目名；当前设置下不会投递（可开启「跟随最新会话」强制）
```

为什么这是性价比最高的一项：它把 12.1 里那张"从没实机验证过"的表，从**需要冒风险去试**
变成**零风险随时可查**。它同时是一个诊断工具（用户报"续跑没反应"时第一句话就问演练结果是什么）
和一个信任建立工具（你能看见它认出了你的窗口，才会放心开自动续跑）。
实现成本很低——三个平台的脚本本来就返回 `matched` / `refused` / `fallback`，
只要加一个"到此为止不要投递"的开关（`focus_session` 已经是半个现成品）。

**P0 — 三平台实机验证清单**：把 12.1 的表变成 `docs/manual-test.md`，
每个平台每个终端一行，走一遍打勾。这是发 v1.1 tag 之前的准入条件。

**P1 — 自定义适配器 UI**：`CustomAdapterConfig` 后端已就绪，缺一个"会话文件在哪、
进程名叫什么、怎么判断回合"的表单。做完之后 Aider / Cline / Gemini CLI 这类工具
用户自己就能加，不必等我发版。

**P1 — 拆 `ConfigPanel.tsx`** + 给 `lib/display.ts`、`stores/useAppStore.ts` 补 vitest。

**P2 — 文档与元数据对齐**：重写 `docs/architecture.md`、更新 README 路线图/前置要求/配置表、
`tauri.conf.json` 补 `icons/icon.png`。

**P2 — 自动更新重做**：生成签名密钥对、配 `tauri-plugin-updater`、在 CI 的 `release` job 里
产出 `latest.json`。上次失败的原因很具体（没有公钥就崩），别重复。

### 13.2 v1.5 候选

**① 远程审批（原 v1.3 P2）——需要一次完整的安全设计**

想要的体验：手机收到"会话在等你回话"的通知，点一下就让它续跑，不用回到电脑前。
难点在于这会把一个**结构性只读**的服务变成可写服务，9.1 那张表里的每一条都要重新论证。
建议的设计（还没实现）：

| 设计点 | 方案 |
|---|---|
| 动作白名单 | 只允许一个动作：`resume(session_id)`。没有"改配置"、没有"停止引擎"、没有自由文本 |
| 提示词不可远程指定 | 用的是本机配置里的 `resume_prompt` / `goal_resume_prompt`，**手机不能传任意文本**——否则远程看板就变成了远程命令注入面 |
| 一次性动作令牌 | 通知里带一个绑定 `session_id` + 过期时间的一次性令牌，用完即废；读令牌（看板）和写令牌（审批）分开 |
| 幂等 | 同一个动作令牌重复提交只生效一次 |
| 默认关闭 | 独立开关 `remote.allow_approval`，默认 `false`；开启时明确告知"手机上的人可以让你的 agent 继续跑" |
| 审计 | 每次远程审批落 `resume_records`，标注来源是远程；活动日志里可见 |
| 仍不放宽绑定 | `bind_all` 的告知文案不变，且远程审批建议只在 `127.0.0.1` + 反代/隧道场景下用 |

**② 判定证据面板**：在会话卡片上展开"为什么是这个结论"——列出四个信号各自的取值、
`TurnState` 是什么、忙碌宽限有没有生效、命中了哪个关键词。检测逻辑的复杂度已经到了
"用户看不懂它为什么这么判"的程度，而这个项目的信任成本很高，值得把推理过程摊开。

**③ 多项目视图**：按 `cwd` 项目名分组会话，成本页已经有按项目聚合，主面板还没有。

### 13.3 更远的候选（都仍在"非侵入"边界内）

| 想法 | 价值 | 备注 |
|---|---|---|
| **tmux / screen 支持** ★ | `tmux send-keys -t <pane>` 是**确定性最高**的投递通道：pane id 精确、完全不经过输入法、不需要窗口在前台、不需要任何辅助功能权限 | 对重度终端用户来说这会是最可靠的一条路，优先级应该排在很多 GUI 终端之前 |
| **SSH 远端守护** | 守护跑在远程机器上的 agent（会话文件通过 ssh 读，续跑通过 `ssh + tmux send-keys` 投递） | 天然要求 tmux，所以是上一条的延伸 |
| **更多适配器** | Aider / Cline / Gemini CLI / Continue | 做完 13.1 的自定义适配器 UI 之后，这件事可以交给用户 |
| **Windows 用 UIAutomation 取代标题匹配** | 能直接枚举标签页，把"窗口级确定性"提升到"标签级确定性"，多标签宿主不再需要靠标题猜 | 工程量不小，但这是 Windows 侧唯一的确定性天花板突破口 |
| **从历史里学阈值** | 你每次手动续跑（说明它漏判了）和每次撤销（说明它误判了）都是标注数据，可以用来给这台机器调 `idle_timeout_secs` | 要小心：不能让学习结果绕过 6.4 的两条铁律 |
| **成本报表导出** | 按周/月汇总、导出 CSV，给报销和团队分摊用 | 数据都在 SQLite 里，纯前端工作 |
| **健康自检** | 一键检查：辅助功能权限、`xdotool`/`ydotool`/剪贴板工具是否就位、会话目录能不能读 | 与 13.1 的演练按钮同属"让不确定变可见" |

### 13.4 被主动搁置的两层：v2.0 编排 / v2.1 自治

这两层在最初的需求文档里，但**没有实现，并且建议在动工前重新确认一次**。理由不是工作量，是定位冲突：

| 层 | 原设想 | 与非侵入定位的冲突 |
|---|---|---|
| **v2.0 编排层** | 编排多个 agent、分配任务、串起工作流 | 编排的前提是**由它来启动和调度** agent。一旦如此，第 2 节三条红线里的"不代替你启动"和"不改变会话所有权"同时作废，产品就变成了另一类工具（agent runner），要跟一大堆成熟框架正面竞争，而且失去了"装上就能守护已在跑的会话"这个唯一的独特点 |
| **v2.1+ 自治层** | 自主决定下一步做什么、自动修复、自动重试策略 | "守护"变成"驾驶"。它会开始替你做产品决策，而它掌握的上下文（会话文件的尾部 + 进程状态）**远少于**做这种决策所需要的。误判的代价从"多发一条通知"变成"改坏你的代码" |

如果确实想要这类能力，有一条不破坏定位的折中路径：**把编排交给别人，只提供守护**。
即开放一个本地 API / CLI，让外部编排器（你自己的脚本、n8n、CI）来问
"这个会话现在什么状态"、"帮我续一下"。AgentPulse 仍然只做它擅长的那件事，
编排的责任和风险留在调用方。这条路值得在动工 v2.0 之前先讨论。

### 13.5 需要你拍板的决策点

1. **v1.4 是否就按 13.1 的顺序做**（演练按钮 → 实机验证清单 → 自定义适配器 UI → 拆前端 → 文档）？
2. **tmux 支持要不要提前**到 v1.4？如果你日常用 tmux，它比任何 GUI 终端的适配都更值得先做。
3. **远程审批做不做**？做的话按 13.2 ① 的安全设计走。
4. **v2.0 / v2.1 是否重新启动**？如果要，是走原设想还是 13.4 末尾的"开放 API 给外部编排器"折中路径？
5. **要不要先打一个 tag** 把 `build-tauri` 那条 4 目标打包链路验一遍（它自这些改动以来没跑过）？

---

## 14. 附录

### 14.1 常用命令

```bash
pnpm install                     # 装前端依赖
pnpm tauri:dev                   # 开发（含 Rust 热重载）
pnpm tauri:build                 # 打包，产物在 src-tauri/target/release/bundle/
pnpm build                       # 仅前端：tsc && vite build（CI 用的就是这条）

cd src-tauri
cargo clippy --all-targets -- -D warnings   # CI 的 lint，本地务必先跑
cargo test                                  # 80 个单元测试
cargo test -- --list                        # 列出全部测试名

./scripts/gen-icons.sh           # 从 SVG 母版重出整套图标（需要 Chrome/Chromium）
```

### 14.2 测试分布（80 个）

| 模块 | 个数 | 守的是什么 |
|---|---:|---|
| `resumer` | 26 | 生成脚本的语法、剪贴板通道、拒绝盲敲、终端识别边界 |
| `detector` | 14 | 两条铁律、散文不是证据、词边界、续跑上限 |
| `adapters::claude_code` | 10 | 回合分类、记账行跳过、错误行提取、尾部读取的多字节边界 |
| `remote` | 8 | 两种令牌来源、定长比较、页面渲染 |
| `cost` | 7 | 最长前缀匹配、引入期价格、聚合 |
| `webhook` | 6 | 五家载荷格式 |
| `i18n` | 5 | 无重复 key、占位符有出处 |
| `monitor` | 5 | 冷却、标签 |

值得单独一提的几个测试名（它们本身就是文档）：

- `long_compaction_pause_is_not_a_stall` —— 铁律 A
- `awaiting_user_plus_silence_confirms` —— 铁律 B
- `talking_about_an_error_is_not_having_one` —— 散文不是故障证据
- `keyword_hit_alone_never_types` —— 关键词单独出现不足以动手
- `the_users_clipboard_is_given_back` —— 不能吞掉你复制的东西
- `vscode_script_never_sends_ctrl_c` —— 焦点在编辑器里时 Ctrl-C 是复制
- `blind_typing_is_off_by_default` —— 定位不到就不敲
- `tail_survives_multibyte_at_the_window_edge` —— 尾部读取不能把 UTF-8 字符切一半

### 14.3 技术栈

| 层 | 技术 |
|---|---|
| 桌面框架 | Tauri 2（`tray-icon` 特性；插件 shell / notification / autostart） |
| 后端 | Rust — tokio(full)、sysinfo 0.35、notify 8、rusqlite 0.32(bundled)、reqwest 0.12(json)、glob 0.3、chrono、tracing、dirs 6、uuid v4、serde/serde_json |
| 前端 | React 19.1 + TypeScript 5.8 + Vite 6.3 + TailwindCSS 3.4 |
| 组件 | Radix UI（tabs / select / switch / tooltip / slot）+ cva + clsx + tailwind-merge |
| 状态 | Zustand 5 |
| 桥接 | `@tauri-apps/api` 2.5 |

### 14.4 目录结构

```
agent-pulse/
├── PROJECT_STATUS.md          ← 本文档
├── README.md                  ← 面向使用者（路线图待更新，见 12.2）
├── docs/architecture.md       ← 已陈旧（见 12.2）
├── index.html                 ← 首屏底色写死 #fafafa
├── public/{icon.svg,favicon.png}
├── scripts/{gen-icons.sh,make_ico.py}
├── src/                       ← 前端
│   ├── App.tsx  main.tsx  types.ts  index.css
│   ├── components/{ConfigPanel,CostPanel,HistoryPanel,LogPanel,SessionList,StatsPanel,StatusCards}.tsx
│   ├── components/ui/         ← Radix 封装 + 自绘 BarChart
│   ├── i18n/index.ts          ← 202 条前端文案
│   ├── lib/{display,chime,useNotice,utils}.ts
│   └── stores/useAppStore.ts
└── src-tauri/
    ├── tauri.conf.json        ← icon 数组缺 icons/icon.png（见 12.2）
    ├── icons/{master.svg,master-macos.svg,…}
    └── src/
        ├── lib.rs             ← 装配：AppState / 托盘 / 事件泵 / 21 个命令
        ├── adapters/{mod,claude_code,codex,opencode}.rs
        ├── detector/mod.rs    ← Verdict + AttentionLevel
        ├── monitor/mod.rs     ← 主循环
        ├── resumer/mod.rs     ← 三平台续跑（唯一带平台 cfg 的文件）
        ├── cost/mod.rs  storage/mod.rs  notify/mod.rs  webhook/mod.rs
        ├── remote/mod.rs      ← 只读看板
        ├── config/mod.rs  i18n/mod.rs  ai_judge/mod.rs
        └── main.rs
```

