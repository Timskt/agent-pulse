<p align="center">
  <img src="src-tauri/icons/icon.png" width="88" alt="AgentPulse">
</p>

# AgentPulse 项目现状与规划

> 对应版本：`v1.10.0` 源码状态（尚未打 tag，尚未发布 Release）
> 验证口径：本地门禁见 § 11 / § 14；提交级结果以 GitHub Actions 为准
> 文档日期：2026-08-07

这份文档写给三种人看：接手这个仓库的人、半年后忘了细节的我自己、以及想知道"到底做完了没有"的你。
所以它有两条约定：

1. **凡是"做了"的，都能在代码里指到具体位置**（文件 + 符号名），不能指的一律不写成已完成。
2. **凡是没在真机上跑通过的，一律进第 12 节「未验证清单」**，不管代码写得多完整、单测多绿。
   续跑这种"往别人终端里敲字"的功能，单测绿 ≠ 真的能用。

---

## 目录

1. [一页速览](#1-一页速览)
2. [产品定位：这个工具刻意不做什么](#2-产品定位这个工具刻意不做什么)
3. [进度总览](#3-进度总览)
4. [代码地图](#4-代码地图)
5. [运行时架构与数据流](#5-运行时架构与数据流)
6. [检测引擎：两条正交的轴](#6-检测引擎两条正交的轴)
7. [续跑层：确定性分级与闭环核验](#7-续跑层确定性分级与闭环核验)
8. [洞察层：成本、限流预测、统计](#8-洞察层成本限流预测统计)
9. [远程层与通知层](#9-远程层与通知层)
10. [前端、i18n 与配置](#10-前端i18n-与配置)
11. [工程实践：CI、打包、cfg 纪律](#11-工程实践ci打包cfg-纪律)
12. [已知欠账与未验证清单](#12-已知欠账与未验证清单)
13. [未来规划与设计](#13-未来规划与设计)
14. [附录](#14-附录)

---

## 1. 一页速览

| 维度 | 现状 |
|---|---|
| 版本 | `1.10.0`（`package.json` 是唯一来源，发布前由版本一致性测试锁死；**标签未推，尚未发 Release**） |
| 后端 | Rust，19 个文件；续跑协调器集中在 `monitor/mod.rs` |
| 前端 | TypeScript + React 19，49 个文件 |
| 单元测试 | Rust **262 个**（`cargo test`）+ 前端 **99 个**（`pnpm test`，vitest，8 个文件） |
| Tauri 命令 | 38 个 `#[tauri::command]` |
| 支持的 Agent | Claude Code / Codex CLI / OpenCode（`all_adapters()`） |
| 续跑平台 | macOS / Windows / Linux 三套实现均已落地，另有一条与平台无关的 tmux/screen 通道 |
| i18n 词条 | 后端 200 条（`(key, zh, en)`），前端 349 条（`[zh, en]`） |
| 持久化 | SQLite 6 张表 |
| 功能层次 | v1.0 核心 ✅ · v1.1 感知 ✅ · v1.2 洞察 ✅ · v1.3 远程 ✅ · v1.4 可信化 ✅ · v1.5 闭环 ✅ · v1.6 可解释判定 ✅ · v1.7 记录与导出 ✅ · v1.8 限流保持 ✅ · v1.9 续跑协调器 ✅ · **v1.10 两阶段续跑流水线 ✅** · v2.0 编排 ⏸ · v2.1 自治 ⏸ |

**这个版本能做到的事**：在你不改变任何使用习惯的前提下，后台盯着 Claude Code / Codex / OpenCode
的会话文件与进程，判断它是"还在干活"、"卡住了"、"在等你回话"、"限流了"还是"报错了"；
该叫人的时候发通知（带托盘角标和声音），该补一句话的时候把续跑提示词**静默**送进它自己那个终端窗口；
**敲完之后回头核验那句话有没有真的落地**，没落地就把这件事摆到你眼前，而不是假装续跑成功；
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
| v1.3 P2 | 远程审批 | ❌ | 手机上点一下"续跑"——**未实现**，见 [13.2](#132-v16--v17--v18-已交付与下一步候选) |
| v1.4 | **可信化** | ✅ | tmux/screen 免权限投递通道、续跑演练（`ResumeProbe` 干跑探测）、macOS 辅助功能权限自检与"去开权限"引导、前端 vitest、局域网看板换绑修复 |
| v1.5 | **闭环化** | ✅ | 投递后核验落地（`ResumeOutcome` + `resume_verified`）、一个计数器拆成三个、上限从判定层挪到动作闸门、静默失败会出声、日志区分事件与状态、版本号单一来源 |
| v1.6 | **可解释判定** | ✅ | `InterruptReason` 与 `ResumeTactic` 单一策略源、`DetectionEvidence` 判据面板、结构化 AI 第二意见（单向授权、指纹缓存、每轮最多一问）、自定义适配器 UI、跨语言枚举/i18n 门禁、SQLite 形状迁移 |
| v1.7 | **记录与导出** | ✅ | 单实例守护、续跑记录中心（独立分页 + 筛选）、统计趋势真实对比、会话档案抽屉（生命周期 / 中断次数 / 续跑时间线 / 成本时间线 / 路径一键复制）、柱状图时间刻度、CSV 导出（转义与公式注入分开处理）、会话生命周期收拢（修「关了还显示运行中」）、跨夏令时日期分组修复 |
| v1.8 | **限流保持** | ✅ | 中转站/HTTP 形状兜底、等待时间解析、跨轮保持窗口，证据滚出尾部后仍不误敲 |
| v1.9 | **续跑协调器** | ✅ 待发布 | 扫描/投递解耦、按会话合并队列、常驻 worker、RAII 会话租约、stop 生命周期代数、出队全量重验、并发状态归约、PID + 启动代际身份；首次三步引导与多会话搜索筛选 |
| v1.10 | **两阶段续跑流水线** | ✅ 待发布 | 不可逆桌面投递严格串行、跨会话只读核验并行、忙会话绕行避免队头阻塞、owned RAII 租约覆盖闭环、Rust 单一来源的 pending/verifying 可视化 |
| v2.0 | 编排层 | ⏸ | 主动搁置，与非侵入定位冲突，动工前需确认 |
| v2.1+ | 自治层 | ⏸ | 同上 |

v1.4 这一版没有加任何"新玩法"，全部力气花在**让已经写完的功能变得可信**上——因为此前最大的风险
不是缺功能，而是续跑这条主链路只有单测、没有任何"它到底认到哪儿了"的可观测手段。

v1.5 顺着同一条线再往下走一步：v1.4 解决的是"动手之前能不能先看一眼"，v1.5 解决的是
**"动完手之后知不知道自己成功了没有"**。这两件事合起来才让"自动续跑"这四个字站得住——
一个既不能预演、又不核验结果的自动化功能，本质上是在让用户替它做质检。详见 [7.8](#78-开环--闭环敲完之后要回头看一眼)。

### 最近的提交脉络

```
4b200f6  feat(v1.5): 续跑改成闭环——敲完要核验，放弃要出声
7121d60  chore(v1.5): 版本号收成一个来源，页脚不再手抄
0c00f4b  fix(remote): 换绑地址前等旧监听真的落地，修复「开了局域网手机却被拒绝」
dbd6a5e  merge: 归并两条并行的续跑定位实现，只留一份判定
160429d  docs: 加入项目现状与规划全景文档
635d720  fix(resume): 权限没到位时先说清楚，别只把窗口跳过来
83ca5b3  feat(v1.4): 续跑演练(dry-run)按钮 + 三平台验证清单 + 前端测试 + 文档对齐
cee4248  fix(ci): pin resumer_with to macOS, widen the blind-typing test to all three
f3999c3  ci: lint and test all three platforms on every push
a888d91  feat(icon): regenerate the whole icon set from a vector master
49d16a6  feat: perception, insight and remote layers + clipboard-based resume
```

`49d16a6` 是 v1.1–v1.3 三层的主体；`83ca5b3` 起是 v1.4 可信化；`7121d60`/`4b200f6` 是 v1.5 闭环化。
`dbd6a5e` 是一次真实的归并——本地和远端各自实现过一遍"续跑往哪儿投"的判定，合并时只留了一份，
没有 force-push 覆盖任何一边。

---

## 4. 代码地图

### 后端（`src-tauri/src/`，19 个 Rust 文件）

| 文件 | 行数 | 职责 | 关键符号 |
|---|---:|---|---|
| `monitor/mod.rs` | 2498 | 扫描调度、两阶段续跑流水线、动作闸门、并发状态归约 | `ResumeQueue::pop_ready`、`ResumeRegistry`、`ResumeLease`、`PhaseCounter`、`resume_worker`、`run_auto_resume`、`snapshot`、`merge_resume_runtime` |
| `resumer/mod.rs` | 3420 | 三平台/tmux/screen 投递、定位演练、两阶段落地核验 | `Resumer::{deliver,verify_delivery,resume_verified,probe}`、`ResumeDelivery`、`ResumeOutcome` |
| `storage/mod.rs` | 2392 | SQLite 六张表与历史聚合 | `record_resume`、`upsert_session_history` |
| `detector/mod.rs` | 2098 | 多信号融合、注意力、动作策略、限流保持 | `Detector::detect`、`DetectionResult`、`ResumeTactic` |
| `lib.rs` | 884 | Tauri IPC、托盘、事件泵、单实例装配 | `manual_resume`（下沉到 engine） |
| `remote/mod.rs` | 873 | 只读 HTTP 看板 | `RemoteService` |
| `i18n/mod.rs` | 751 | Rust 用户可见文案 | `I18n` |
| `export/mod.rs` | 684 | CSV 导出与公式注入防护 | `Cell::{Text,Value}` |
| `cost/mod.rs` | 536 | token 成本与限流预测 | `forecast_rate_limit` |
| `detector/rate_limit.rs` | 438 | 限流形状与等待时间纯函数 | `upstream_rejection` |
| `adapters/mod.rs` | 626 | 进程快照、会话模型、进程代际身份 | `process_session_id`、`process_matches_session` |

### 前端（`src/`，49 个文件）

| 文件 | 行数 | 职责 |
|---|---:|---|
| `i18n/index.ts` | 800 | 前端中英双语词典 |
| `components/ConfigPanel.tsx` | 610 | 设置页编排 |
| `components/SessionList.tsx` | 549 | 会话卡片、搜索、筛选与动作入口 |
| `components/OnboardingPanel.tsx` | 140 | 首次三步引导；明确不启动/接管 Agent |
| `lib/sessions.ts` | 90 | 搜索、筛选、注意力优先排序纯函数 |
| `components/DashboardPanel.tsx` | 18 | Dashboard 编排边界 |

前端只对 Rust 返回的会话快照做展示、搜索、筛选和排序；不重新推导 `status`、
`attention`、`interrupt_reason` 或 `resume_tactic`，避免同一策略出现两个事实来源。

## 5. 运行时架构与数据流

### 一轮扫描与续跑协调（`MonitorEngine::scan_once`）

```
进程快照（整轮一次） → 适配器发现 → 记录版本一致取证 → Detector 判定
                                               │
                                               ▼
                         状态合并 + ResumeAction（不执行投递）
                                               │
                               释放 scan_lock，扫描立即结束
                                               │
                                               ▼
                         ResumeQueue 按 session 合并最新动作
                                               │
                                               ▼
                 pop_ready 跳过 leased session，派发不同会话任务
                                               │
                   owned ResumeLease（覆盖整个业务闭环）
                                               │
                                               ▼
       delivery_lock 内重验 → PID 代际复核 → 定位/投递 → 释放全局锁
                                               │
                                               ▼
                 各 session 并行只读核验 → 记账 → 释放 lease
```

v1.9 先做到：**扫描不再等待 AppleScript、PowerShell、xdotool 或最长 6 秒的落地核验。**
扫描只生成动作，`ResumeQueue` 对同一 session 做 upsert，因此扫描越勤也不会堆出旧动作长龙。

v1.10 再把续跑自身拆成两个资源阶段。自动与手动只在定位、剪贴板和键盘输入期间共享
`delivery_lock`；输入完成、剪贴板恢复后立即释放，最长 6 秒的 transcript 指纹核验在锁外按
session 并行。`pop_ready` 会绕过仍在核验的忙会话，让后面的会话先拿到投递机会；owned
`ResumeLease` 仍覆盖核验、记账和通知，因此同会话不会重入。动作在锁内重验 lifecycle、running、
开关、状态、策略、额度、冷却和记录指纹，`Resumer` 再验证 PID、进程启动时刻和命令行。
停止守护清队列并推进生命周期代数，所以 stop/start 也不能复活尚未输入的旧动作。

检测与投递现在可以重叠，`merge_resume_runtime` 因而成为必要的归约边界：扫描写回会话表时
保留状态锁内最新的累计次数、失败退避和冷却；只有本轮明确看到 `Running` 才清空自动连击。
否则旧扫描快照会把刚完成的续跑提交覆盖回去。

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
  自动更新要重做（见 [13.2](#132-v16--v17--v18-已交付与下一步候选)）。

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

整个 `detector` 模块的设计核心是一句话：**"这个会话现在是什么状态"和"要不要现在叫你过来"是两个问题，
不能用同一个阈值回答。**

| | `Verdict`（中断判定） | `AttentionLevel`（注意力分级） |
|---|---|---|
| 回答的问题 | 它现在**是什么状态**（够不够格续跑） | 要不要**现在叫人** |
| 取值 | `Running` / `Suspicious` / `ConfirmInterrupt` / `TaskCompleted` | `None` / `NeedsInput` / `Completed` / `RateLimited` / `Error` |
| 判错的代价 | **高**——往一个正在干活的会话里回车，会打断它、污染上下文 | **低**——多发一条通知，代价只是你瞥一眼 |
| 因此阈值 | 极严，只认两种确定情形 | 明显更松，宁可多叫一次 |

这条区分是被实际 bug 逼出来的：早期版本用同一套信号同时决定"通知"和"续跑"，
结果要么通知太少（漏掉真的在等你的会话），要么续跑太猛（在压缩上下文的间隙里敲字）。

还有一条同源但更容易被忽略的纪律：**判定层只回答"是什么"，不回答"该不该动手"。**
额度、冷却、总开关全部住在 `monitor` 的动作闸门里，`make_verdict` 一个都不看。
原因见 [7.10](#710-上限从判定层挪到动作闸门放弃动手的那一刻正是最该开口的一刻)——
把"该不该动手"混进判定，代价是应用会在放弃动手的同时把提醒也一起收掉。

### 6.1 四种信号（`SignalKind`）

| 信号 | 含义 | 来源 |
|---|---|---|
| `FileStale` | 会话文件停更 | `session_files` 的 mtime |
| `KeywordMatch` | 输出里命中了中断关键词 | `recent_output` |
| `ProcessExited` | 进程没了 | 进程快照 |
| `HeartbeatTimeout` | 心跳超时 | 会话文件时间线 |

`FileStale` 与 `HeartbeatTimeout` 来自同一个时间事实，判定时合并为 `transcript_idle`；
证据面板把它们当同源时间信号展示，不让用户误以为存在两票独立证据。

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

## 7. 续跑层：确定性分级与闭环核验

续跑要解决三个完全独立的难题：**投递什么**（7.1）、**投给谁**（7.2–7.6）、
以及 v1.5 补上的第三个——**投完之后怎么知道成没成**（7.8–7.12）。
外加一件横跨三者的事：**在真敲之前先看一遍会敲到哪**（7.13）。

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

| 机制 | 默认值 | 作用 | 住在哪 |
|---|---|---|---|
| `max_resume_count` | 5 | **连着**催几次没反应就停手（数 `resume_streak`，不是累计次数） | `monitor::has_nudges_left` |
| `resume_cooldown_secs` | 30 | 两次续跑之间的冷却；投递连续失败时线性放大（`effective_cooldown`，上限 5 倍） | `monitor::check_cooldown` |
| `auto_resume_enabled` | `true` | 总开关；关掉后只通知不动手 | `monitor::scan_once` |
| `auto_follow_latest` | `false` | 盲敲授权，见 7.2 | `Resumer::allow_blind` |

四条里前三条**都不在判定层**——这是 v1.5 特意搬的家，理由见 7.10。
（测试：`exhausted_streak_stops_typing_but_not_watching`、`failed_deliveries_never_exhaust_the_budget`、
`cooldown_*`、`verdict_ignores_every_resume_counter`）

`resumer` 是全仓库测试最密的模块（`cargo test resumer::` 数得到当下的条数）——因为它是唯一会"对外界产生副作用"的模块。

### 7.8 开环 → 闭环：敲完之后要回头看一眼

用户的原话是「自动续跑好像还是不行」，而且这已经是第三次报同一类问题。前两次都是照着症状打补丁；
这一次先问了一句"为什么每次都要等他来报"，答案是整条续跑链是**开环**的。

`Resumer::resume` 返回 `Ok` 的真实含义只是「AppleScript / PowerShell / xdotool 没报错」，
而不是「那句话进了那个会话」。这两件事之间横着一堆现实：

| 脚本成功但字没进去 | 为什么 |
|---|---|
| 焦点在最后一刻被别的窗口抢走 | 前台化和按键之间有时间差 |
| 粘贴进了同一个窗口的隔壁标签 | 多标签宿主只能靠标题匹配，标题会变 |
| 输入法把内容吃掉 | 走剪贴板已经大幅缓解，但组合键仍可能被 IME 拦 |
| pane 刚好被关掉 | tmux 通道里 pane id 会失效 |
| 辅助功能授权掉了 | macOS 上重签名、换版本都会掉，见 12.6 |

**发出动作却从不观察世界有没有变，这样的系统永远学不会自己坏了**，于是它的失败只能由用户来发现——
一次又一次。

闭环需要的信号本来就躺在磁盘上：agent 只要真的动起来，就会往自己的会话记录里写东西。

```rust
enum ResumeOutcome {
    Failed,        // 通道自己就报错了，字肯定没出去
    Landed,        // 会话记录长了 → 它动了
    Silent,        // 脚本成功，但记录一动不动 → 没落地
    Unverifiable,  // 这个 agent 没有可核验的记录文件
}
```

`resume_verified()` 的做法很朴素：投递**前**给 `session.session_file` 拍一个 `(长度, mtime)` 指纹，
投完盯 6 秒，长了就是落地了。没长就是没落地——**至于为什么没落地，这一层不必知道**，
也正因如此以后新增投递通道不需要再配一套失败识别逻辑。

一个刻意的分级：没有会话记录文件的 agent 记成 `Unverifiable`，**不算失败**。
宁可承认"这个核验不了"，也不假装核验过——把"不知道"记成"成功"是所有静默失败的起点。

### 7.9 一个被当三件事用的计数器，拆成三个

旧代码只有 `resume_count` 一个数字，同时被用来做三件互相冲突的事：给人看、撞上限、判断通道健康。
更糟的是它在**投递之前**就自增，且失败不回退。后果是一条完整的失效链：

> 辅助功能授权掉了 → 5 次投递全部失败，但 `resume_count` 照样从 0 数到 5 →
> 第 6 次起自动续跑对这个会话**永久沉默** → 而一个字都没真的敲进去过。

现在是三个各管一件事的计数器：

| 计数器 | 数什么 | 谁在用 | 什么时候清零 |
|---|---|---|---|
| `resume_count` | 一辈子催过多少次 | 只给人看（会话卡片上的"已续跑 N 次"） | 永不 |
| `resume_streak` | **连着**催了几次却没见它动 | `has_nudges_left` 对着 `max_resume_count` | 判定回到 `Verdict::Running`（会话真动了） |
| `resume_failures` | 连着几次**根本没送达** | `effective_cooldown` 退避 + 告警升级 + 卡片红标签 | 一次 `Landed` |

策略集中在一个纯函数 `apply_resume_outcome` 里，**不裹在锁里**——裹在锁里的策略没法单测，
而这三个数字的关系正是最需要被钉住的部分。

顺带修掉的第三个语义错误：上限的含义从"一辈子"改成"连着"。拿累计次数撞上限，等于给每个会话发一张
「一生只准被催 5 次」的配额卡——一个跑一整天、真的停顿过六次的会话，从第六次起就没人管了。
上限想拦的是"对着一个不响应的会话空转"，不是"一个会话一辈子只准被催 5 次"。
「它其实没干完活，每次都要我去发继续」有一半是这么来的。

### 7.10 上限从判定层挪到动作闸门（放弃动手的那一刻，正是最该开口的一刻）

上限原来是 `Detector::make_verdict` 里的一句提前返回，位置在任何证据被检查**之前**：

```rust
// 旧代码（已删除）
if session.resume_streak >= config.max_resume_count {
    return Verdict::Suspicious;
}
```

两个真实后果：

1. **单向门**。额度一光，判定永远给不出 `Running`，而清零条件正是 `Running`——
   于是会话自己恢复干活了，界面上还挂着"疑似中断"，而且再也回不去。
2. **悄悄放弃**（更严重的一个）。`grade_attention` 对 `Suspicious` 的处理是
   `(AttentionLevel::None, None)`，也就是不打扰。于是应用一边放弃自己动手，
   一边把该给人的提醒**也一起收了**——托盘上一片安静，用户还以为有人在守着。

修法不是给这个 `if` 加条件，而是承认它站错了地方：**判定层回答"它是什么状态"，
额度回答"我们还该不该动手"，这是两个问题。** 现在判定照实说 `ConfirmInterrupt`，
注意力分级照常升到 `NeedsInput`，托盘角标和通知照常叫人；只有敲字这一件事停下来，
并且在日志里说清楚是"催不动了"而不是"还在冷却"。

（测试：`a_session_we_stopped_nudging_still_calls_for_help` 走完整条链——
`ConfirmInterrupt` → `NeedsInput` → `attention.is_pending()`。这条链上任何一环退化，
应用就会重新学会安静地放弃。）

### 7.11 事件与状态：日志不能自我复述

把上限搬出判定层之后暴露了一个更早就存在的缺陷：一个**持续**处于中断状态的会话
（用户关了自动续跑，或者已经催不动了），每 10 秒就会重新落一条检测记录、写一条
"检测到中断"日志、发一条 webhook。

根子在于**事件和状态是两种东西，日志却只有一条流**：

| | 例子 | 该怎么说 |
|---|---|---|
| **事件** | "检测到中断" | 发生一次说一次——按**状态跃迁**发 |
| **状态** | "已经催不动了" | 它会一直成立——按**指纹变化**发 |

两种机制刻意分开实现：

- 事件走跃迁门：`if session.status != SessionStatus::Interrupted`，落库 / 日志 / webhook 三处跟着同一个条件。
  （这个写法本来就在用了——`Verdict::TaskCompleted` 一直是这么发的，只是中断这条漏了。）
- 状态走 `push_event_on_change(topic, fingerprint, event)`：`topic` 说"这条在讲哪件事"，
  `fingerprint` 说"那件事现在什么样"。指纹没变就闭嘴，变了（连击数从 5 涨到 8）才再说一次。
  情况解除时调用方要 `forget_topic`，否则同一个情况第二次发生就说不出口了。

顺手修正的一个不诚实的数字：`status.total_detections` 以前数的是"本轮有几个会话处于确认中断"，
于是一个开着的中断会话每轮都记一笔，界面上的检测数和 `detection_records` 的行数越差越远。
现在只数**新确认**的（`newly_confirmed`），它自己注释里承诺的那个不变式终于成立了。

（测试：`standing_conditions_are_only_announced_once` 直接测纯函数 `should_say`：
同一指纹说一次、指纹变了再说一次、忘掉之后能重新开口。）

### 7.12 静默的功能坏掉时必须变吵

自动续跑的全部价值在于"你感觉不到它"。代价是：**它坏掉的时候，长得和它正常工作一模一样**。
所以这一层的每个失败都有一个出口：

| 出口 | 触发 | 在哪看到 |
|---|---|---|
| 系统通知 | 连续 2 次没送达（≥600 秒节流） | 桌面通知 |
| 启动前体检 | 引擎 `start()` 前查 macOS 辅助功能授权 | 活动日志 + 通知 |
| 红标签「敲不进去 ×N」 | `resume_failures > 0` | 会话卡片 |
| 琥珀标签「已停手，等你」 | `resume_streak >= max_resume_count` | 会话卡片 |
| 「催不动了」日志 | 额度闸门拦下的那一轮（指纹变化时） | 活动日志 |
| 演练按钮 | 随时手动 | 会话卡片内展开（见 7.13） |

前两个是 v1.4 的，后四个是 v1.5 的。共同的判断标准只有一句：**用户不应该靠
"怎么一直没人帮我按继续"来发现这个功能坏了。**

### 7.13 演练（dry run）：把定位链路走一遍，但一个字都不敲

7.2 的策略是"定位不到就不敲"，可它对用户是**黑箱**：按下去之前没人知道会命中哪条分支。
而这个功能最大的心理负担恰恰在这里——尤其在 IDE 集成终端上，敲错标签就是把提示词
写进别人的代码。所以 `Resumer::probe` 走**完全相同的定位链路**，走到"该投递了"这一步
停手，把过程本身交回来：

```rust
pub struct ResumeProbe {
    pub session_id: String,
    pub certainty: String,          // exact | window | none —— 与真实投递同一套判定
    pub certainty_label: String,    // 已按当前语言翻好，前端不再拼字
    pub channel: String,            // tmux 面板 / iTerm2 标签 / 编辑器内置终端 …（已本地化）
    pub target: Option<String>,     // 命中的 pane id、tty、窗口标题或窗口 id
    pub detail: String,             // 卡在哪一环的人话说明（已本地化）
    pub would_deliver: bool,        // 按现在的配置，点"续跑"会不会真的发出字符
    pub terminal_app: Option<String>,
    pub tty: Option<String>,
    pub project_name: String,
    pub allow_blind: bool,          // 「盲敲最前窗口」开没开
    pub needs_permission_fix: bool, // macOS 辅助功能没给 → 前端出「去开权限」按钮
    pub tools: Vec<ToolStatus>,     // 这条通道依赖的外部工具在不在（含用途说明）
}
```

三个设计约束：

- **和真实路径共用判定，不另写一套。** 如果演练自己有一份"大概是这样"的逻辑，它就会
  在最需要可信的时候骗人。共用的代价是 `probe` 必须能在"要投递了"的位置干净地返回。
- **零副作用。** 不改剪贴板、不前台化窗口、不发任何按键；所以 tooltip 敢写
  "演练不会敲任何字，随便点"，用户才会真的去点。
- **结论要能直接行动，而且不靠认字符串。** `needs_permission_fix` 是个布尔而不是让前端
  去匹配工具名——认名字就得按语言比字符串，换成英文界面立刻失灵；它直接连到
  「去开权限」按钮（`openAccessibilitySettings`）。`tools` 把缺失的 `xdotool` / `wl-copy`
  连同**它是干什么用的**一起列出来，只报"缺失"等于把排查工作又推回给用户。

它同时是最省事的支持工具：用户报"续跑没反应"时，第一句话就是"点一下演练，把那一栏
念给我听"，因为它一次回答了 7.2 的确定性、7.4 的通道、以及依赖是否就位。

### 7.14 v1.9：续跑协调器的并发不变式

| 不变式 | 实现 |
|---|---|
| 同一会话最多一个在途续跑 | `ResumeRegistry::try_acquire` + RAII `ResumeLease` |
| 同会话排队只保留最新快照 | `ResumeQueue::upsert`（`VecDeque + HashMap`），在途时最多一条最新后继 |
| 跨会话真实投递串行 | 常驻 `resume_worker` + 全局 `delivery_lock` |
| 手动/自动不能抢剪贴板或窗口 | 两条入口共享 lease 与 `delivery_lock` |
| 停止后待处理自动动作失效 | queue clear + `lifecycle_epoch` |
| 停止后立即重启也不复活旧动作 | 动作代数必须与当前代数完全一致 |
| 会话自己恢复后旧动作取消 | 出队重验状态、策略与活动指纹 |
| PID 复用不能继承旧会话或误投 | 会话 id 与投递复核都包含进程启动时刻 |
| 扫描/投递重叠不丢计数 | `merge_resume_runtime` |
| 日志不猜次数 | `commit_resume_outcome -> ResumeCommit` |

这次重构的目标不是“多线程更快”，而是把安全边界显式化：检测可以持续刷新，真实输入仍严格
串行；排队只是意图，不是许可；任何旧事实到动手前都必须重新证明自己仍然成立。

### 7.15 v1.10：投递与核验按资源边界分离

| 不变式 | 实现 |
|---|---|
| 不可逆桌面操作仍全局串行 | 自动/手动只在 `Resumer::deliver` 外持有同一个 `delivery_lock` |
| 只读核验不占桌面锁 | `Resumer::verify_delivery` 在锁外轮询目标 session transcript |
| 不同会话可以并行核验 | worker 为取得 lease 的不同 session 派发独立任务 |
| 同会话核验期间不二次输入 | owned `ResumeLease` 一直持有到核验、记账、日志和通知完成 |
| 忙会话不挡住后面的会话 | `ResumeQueue::pop_ready` 轮转 leased session，最多检查一圈 |
| 所有会话忙时不自旋 | 一圈没有 ready 动作就等待 `Notify`；lease 释放后主动唤醒 |
| 界面阶段数字不靠猜 | Rust `snapshot()` 合并队列、`PhaseCounter` 与状态锁，前端只展示 |
| stop 不伪造撤销 | 尚未投递的动作被 epoch 重验取消；已经回车的动作仍完成核验与记账 |

如果 N 个会话都进入 6 秒静默核验，旧结构的核验尾延迟约为 `6N` 秒；新结构约为 6 秒加上
各自不可逆投递耗时。安全收益不是放宽定位，而是把“必须串行”和“没有理由串行”的资源分开。
完整设计见 `specs/v1.10_resume_pipeline_design.md`。

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
`App.tsx` 只保留应用壳与导航，Dashboard 编排下沉到 `DashboardPanel`。首次无会话时由
`OnboardingPanel` 给出“开始守护 → 用户自行运行 Agent → 立即扫描”三步，并明确 AgentPulse
不启动、不接管 Agent，定位不确定时不会输入。

`SessionList` 支持按项目、Agent、终端元数据搜索，以及“全部 / 等我 / 卡住 / 活跃”筛选；
策略集中在 `lib/sessions.ts` 的纯函数中并有 6 个测试。它只筛选 Rust 快照，不重算检测结论。
状态走 Zustand（`stores/useAppStore.ts`），事件靠后端 `emit("engine-events")` 推送而非前端轮询。

### 10.2 i18n 边界：谁渲染，谁持有文案

这是被明确要求过的一条规矩，也是唯一可行的分法：

| 侧 | 词条数 | 形态 | 覆盖 |
|---|---:|---|---|
| 后端 | 200 | `(key, zh, en)` | 托盘菜单、系统通知、活动日志、续跑结果文本、远程页面 |
| 前端 | 349 | `[zh, en]` | 所有界面文案 |

两边都由 `config.language` 驱动。后端 i18n 有 6 个测试，其中三个是防腐的：
`no_duplicate_keys`（重复 key 会静默覆盖）、`placeholders_are_all_documented`
（文案里的 `{}` 占位符必须都有出处），以及 v1.8 补上的 `every_enum_key_resolves_to_real_wording`。

最后那个补的是一个真实的洞：`t()` 查不到 key 时会**把 key 本身返回**，而
`InterruptReason` / `Tactic` / `AttentionLevel` / `ResumeOutcome` 四族的 `i18n_key()`
都会直接进活动日志和通知（`lib.rs`、`monitor/mod.rs`、`resumer/mod.rs`、`remote/mod.rs`）。
漏一条词条不会让任何测试变红，只会让用户在日志里看到 `reason.upstream_rejected` 这种键名。
现在这个测试遍历四族的每个变体，逐个确认它在两种语言下都查得到真话。

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
| `CustomAdapterConfig` | 自定义适配器的配置载体；设置页可增删改（v1.6 已交付） |
| `ModelPriceOverride` | 用户覆盖价目表 |

---

## 11. 工程实践：CI、打包、cfg 纪律

### 11.1 CI 现状（`.github/workflows/ci.yml`）

| Job | 触发 | 内容 |
|---|---|---|
| `check-rust` | 每次 push / PR | **三平台矩阵**（ubuntu / macos / windows，`fail-fast: false`）→ `cargo clippy --all-targets -- -D warnings` → `cargo test` |
| `check-frontend` | 每次 push / PR | pnpm 11 + Node 22 → `pnpm test`（vitest，38 个）→ `pnpm build`（`tsc && vite build`，类型错误即红） |
| `build-tauri` | **仅 `v*` 标签** | 4 个目标：`aarch64-apple-darwin`、`x86_64-apple-darwin`、`x86_64-unknown-linux-gnu`、`x86_64-pc-windows-msvc` |
| `release` | 仅 `v*` 标签 | 汇总产物 → `softprops/action-gh-release@v2` |

两个刻意的选择：

- **三个平台都跑 lint 和测试**。续跑层几乎全是 `#[cfg(target_os = …)]`，只在 ubuntu 上跑 clippy
  等于 Windows 和 macOS 分支从来没被编译过——上一版就是这么让一个 Windows 专属编译错误
  躺到打标签才暴露的。
- **`--all-targets`**。不加它，`#[cfg(test)]` 里的代码不会被编译；单元测试是跨平台脚本
  唯一的自动化保障，"能编译"和"测试也能编译"是两件事。

还有一处后补的：**`pnpm test` 单独一步，排在 `pnpm build` 前面**。此前 `check-frontend` 只跑
`pnpm build`，于是前端的 38 个测试在 CI 里根本没人跑——本地绿、远端从不验证，等于没有。
拆成两步还有个好处：测试挂了能一眼看出是测试挂了，而不是被埋进一次构建失败里。

最近一次绿灯：run `31039877510`（`b5d7269`，Frontend / Rust macOS / Rust Ubuntu / Rust Windows
四个全绿；`build-tauri` 与 `Create Release` 因为不是标签推送而 Skipped，符合预期）。

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

### 11.4 怎么触发打包（`build-tauri`）

**一句话：打包只认 `v*` 标签，推 `main` 不会打包。**

`build-tauri` 和 `release` 两个 job 都带 `if: startsWith(github.ref, 'refs/tags/v')`，
所以日常往 `main` 推代码只会跑 lint 和测试（这也是为什么 CI 一次只要几分钟）。
四个平台的产物 + GitHub Release 需要显式打一个标签。

**完整流程**

```bash
# 0. 先确认四处版本号一致（package.json 是唯一来源，其余三处由测试锁死）
pnpm test                          # src/version.test.ts 会替你查

# 1. 本地五道关全过再打标签——标签是对外的，不该用它来试错
cd src-tauri && cargo clippy --all-targets -- -D warnings && cargo test && cd ..
npx tsc --noEmit && pnpm test && pnpm build

# 2. 推代码，等 main 上的 CI 绿了
git push origin main
gh run watch "$(gh run list --limit 1 --json databaseId --jq '.[0].databaseId')" --exit-status

# 3. 打标签并推标签（标签必须以 v 开头，且和 package.json 的版本一致）
git tag v1.10.0
git push origin v1.10.0

# 4. 盯打包
gh run list --limit 3
gh run watch <run-id> --exit-status
```

**标签推上去之后会发生什么**

```
push tag v*
   │
   ├─► check-rust     ubuntu / macos / windows  ─┐   （标签推送也会重跑一次）
   ├─► check-frontend ubuntu                    ─┤
   │                                             │  needs: 两个都绿才继续
   └─► build-tauri  ────────────────────────────┘
         ├── macos-latest  · aarch64-apple-darwin      → .dmg（Apple Silicon）
         ├── macos-latest  · x86_64-apple-darwin       → .dmg（Intel）
         ├── ubuntu-22.04  · x86_64-unknown-linux-gnu  → .deb / .rpm / .AppImage
         └── windows-latest· x86_64-pc-windows-msvc    → .msi / .exe(NSIS)
                   │  每个目标 upload-artifact 一份
                   ▼
              release（Create Release）
                   softprops/action-gh-release@v2
                   generate_release_notes: true，产物全部挂到 Release 上
```

几个容易踩的点：

| 点 | 说明 |
|---|---|
| `fail-fast: false` | 一个平台挂了另外三个照样编完，一次就能看清所有平台的问题 |
| `needs: [check-rust, check-frontend]` | lint / 测试不绿**不会**开始打包，省掉四份白跑的构建 |
| `permissions: contents: write` | 创建 Release 需要，已经配好；不用手动开 |
| `ubuntu-22.04` 不是 `latest` | Linux 构建刻意钉在 22.04：glibc 版本决定产物的向下兼容范围，`latest` 漂移会让老发行版装不上 |
| 标签打错了 | 删标签重推：`git tag -d v1.5.0 && git push origin :refs/tags/v1.5.0`，然后重新打。已经生成的 Release 要手动删 |
| 未签名 | macOS 产物**没有 Apple 开发者签名**，下载后首次打开要右键 → 打开；另见 12.6 的签名与辅助功能授权的坑 |

**只想验打包链路、不想发版**：推一个预发布标签（如 `v1.5.0-rc.1`）也一样会触发——
`v*` 匹配的是前缀。跑完把 Release 和标签删掉即可。

### 11.5 图标流水线（`scripts/gen-icons.sh`）

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

### 11.6 版本号只有一个来源

版本号原来抄在四个地方：`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`，
以及 `src/App.tsx` 里一个手写的 `const APP_VERSION = "1.0.0"`。前三个是各自工具链的硬性要求，
第四个纯粹是手抄——结果就是页脚长期显示 `v1.0.0`，而应用其实已经是 v1.4。

**一个显示错版本号的应用，会让人怀疑它报的其他每一件事。** 这个项目的全部产出都是判断
（它卡住了没有、这句话敲进去了没有），信任成本比一般工具高得多，所以这类"小错"要按大错处理。

现在的做法：

| 环节 | 做法 |
|---|---|
| 唯一来源 | `package.json` 的 `version` |
| 注入前端 | `vite.config.ts` 读它，`define` 成编译期常量 `__APP_VERSION__`（`src/vite-env.d.ts` 声明类型） |
| 页脚 | `const APP_VERSION = __APP_VERSION__`，不再有任何手写字面量 |
| 另外两处 | `tauri.conf.json` / `Cargo.toml` 仍需各自写一份（工具链要求），但由 `src/version.test.ts` 断言三者相等——不一致就红 |

`version.test.ts` 用 `?raw` 导入 `Cargo.toml`、用 `resolveJsonModule` 读两个 JSON，
**刻意不 import `node:fs`**：`@types/node` 不在 devDependencies 里，`src/` 下任何测试
只要碰 `node:*` 就会让 `pnpm build` 报 `TS2307`（这个坑踩过一次）。

---

## 12. 已知欠账与未验证清单

这一节是全文最重要的部分。**代码写完 ≠ 功能可用**，尤其是"往别人终端里敲字"这种事。

### 12.1 从未在真机上跑通过（⚠️ 高优先级）

| 项 | 状态 | 怎么验 |
|---|---|---|
| **剪贴板续跑路径** | ⚠️ 三个平台**都没有实机验证过**。单测只保证生成的脚本语法正确、内容里走的是剪贴板而不是合成按键 | 在 macOS 的 iTerm2 / Terminal / VS Code 集成终端里各跑一次真实续跑，看中文有没有变成"啊啊啊" |
| **落地核验（`resume_verified`）** | ⚠️ 逻辑有单测，但"6 秒窗口够不够"这个数只在推理上成立 | 真机续跑一次，看日志里报的是 `Landed` 还是 `Silent`；若正常续跑常被判成 `Silent`，说明窗口要放宽 |
| **tmux / screen 通道** | ⚠️ 代码完整、`send-keys` 走 argv 数组不拼 shell，但**从没对着真实 pane 跑过** | `tmux new -s t`，在里面起一个 Claude Code，等它停下看会不会被续上 |
| **Windows 续跑** | ⚠️ 只有 CI 编译 + 纯字符串单测 | 在 cmd、Windows Terminal、VS Code 各验一次；重点看多标签宿主的标题匹配 |
| **Linux 续跑** | ⚠️ 同上，且 X11 / Wayland 两条路都没实测 | 各验一次；`ydotool` 需要 uinput 权限，估计会有坑 |
| **v1.10 两阶段多会话流水线** | ⚠️ 队列/lease/阶段计数有单测，真实桌面并发未验 | 按 `docs/manual-test.md` §13 验证 A 核验时 B 能投递、手动不被核验阻塞、stop 边界和阶段数字 |
| **手机看板** | ⚠️ 没有用真实浏览器打开过 | 手机连同一 WLAN，开 `bind_all`，扫码进页面 |
| **打包路径** | ⚠️ 4 个目标的 `build-tauri` 自这些改动以来**没有跑过**（它只在 `v*` 标签上触发，见 11.4） | 打一个 `v1.5.0`，或先推 `v1.5.0-rc.1` 只验链路 |

清单的正式版本在 `docs/manual-test.md`（每平台每终端一行，走一遍打勾）。
**演练按钮（7.13）把这张表的验证成本从"需要冒风险去试"降到了"零风险随时可查"**，
但它验的是"定位到哪儿"，不是"字真的进去了"——后者仍然只能真机跑。

### 12.2 文档与配置的陈旧项

| 项 | 状态 |
|---|---|
| `docs/architecture.md` | ✅ 已重写并对齐到 v1.10（两阶段流水线、并行核验、并发归约、进程代际身份） |
| `src-tauri/tauri.conf.json` 的 `icon` 数组 | ✅ 已补 `icons/icon.png`（512px） |
| 四处版本号漂移 | ✅ 已收成单一来源 + 测试锁死，见 11.6 |
| `README.md` 路线图 / 前置要求 / 配置说明 | ✅ 已对齐（Node 22 / pnpm 11，路线图到 v1.10，配置表指向本文档 10.3） |
| 本文档自身的计数 | ✅ 2026-08-07 已按当前工作区重数 |

### 12.3 结构性欠账

- **ConfigPanel 已完成第一轮拆分。** 主文件约 610 行，通用骨架、通知、成本、AI 分区已移到
  `src/components/config/`；Webhook、远程和适配器仍在主文件，下一轮可继续按相同边界拆出。
- **前端测试覆盖纯函数、store、版本一致性和跨语言枚举/i18n 门禁，共 99 个。**
  `SessionList` 的搜索、筛选和排序策略已下沉为纯函数并覆盖；组件渲染层仍缺
  `@testing-library/react` 覆盖。
- **自动更新缺失**。`045e571` 移除了 updater 插件（没配签名公钥会导致启动即崩），
  现在只能手动下载新版本。

### 12.4 AI 兜底判定已接入，但保持受限权限

`ai_judge/mod.rs` 仍保持供应商中立（OpenAI-compatible `api_url` + 可换模型），同时已经接入
自动检测回路。它只在唯一的弱证据缺口被调用，严格接受 `DONE` / `CONTINUE`：

- 每轮最多问一个，会话记录指纹不变就复用答案；
- `CONTINUE` 只能把 `Suspicious` 提升到 `ConfirmInterrupt`；
- `DONE` 只阻止重复提问，不撤销任何已成立判定；
- 请求失败、非标准回复、没启用或没配 Key，都等于没问过；
- `TurnState::is_busy()` 时永远不问，保护长时间上下文压缩和工具调用。

### 12.5 主动搁置（不是欠账，是决定）

- **v1.3 P2 远程审批**：手机上点一下就续跑。没做，因为它把只读服务变成可写服务，
  整个 9.1 的安全模型要重做。设计见 13.2。
- **v2.0 编排层 / v2.1+ 自治层**：与非侵入定位冲突，见 13.4。**动工前必须先确认。**

### 12.6 macOS：勾还在，权限已经没了（每次换版本都会踩）

这是本项目**最容易被误判成"功能有 bug"的一件事**，而它根本不在代码里，所以单独记一节。

macOS 的辅助功能授权由 TCC 数据库管理，而 TCC 记的**不是应用路径，是代码签名**
（designated requirement）。CI 出来的包是 ad-hoc 签名（没有 Developer ID，见 11.4 注意事项），
ad-hoc 签名**每次构建都不一样**。后果：

> 换一个版本的 AgentPulse，系统设置 › 隐私与安全性 › 辅助功能 里那个勾**还在**，
> 但对新的二进制**已经不生效**了。

于是表现是：窗口老老实实跳到前台，然后一个字都没敲进去，而用户看设置面板明明是开着的。
自签名/自己 `cargo tauri build` 出来的包、把 `.app` 覆盖更新、把它从"下载"拖到"应用程序"——
都会触发同一件事。

**怎么根治**：让签名身份别再变。`scripts/macos-signing-identity.sh`（`pnpm
macos:signing-identity`）造一张自签名的代码签名证书并在登录钥匙串里标为可信，之后
`APPLE_SIGNING_IDENTITY="AgentPulse Self-Signed" pnpm tauri:build`，指定要求就从
`cdhash H"…"` 变成 `identifier "com.agentpulse.app" and certificate leaf H"…"`
——认名字加证书，证书不换这串就不变，**勾一次一直有效**。装上新版本后还要再重勾
一次（让 TCC 把旧的哈希记录换成新的名字记录），那是最后一次。

自签名不能公证，**对外分发仍需真的 Developer ID**：证书导成 base64 存进仓库
secrets，CI 里给 `APPLE_CERTIFICATE` / `APPLE_CERTIFICATE_PASSWORD` /
`APPLE_SIGNING_IDENTITY`，`tauri build` 会自己认。**签名身份故意不写进
`tauri.conf.json`**——写死了，没有这张证书的人连构建都过不去。

**已经装了 adhoc 包、暂时不想重新构建**（三条都有效，按省事程度排）：

| 做法 | 命令 / 步骤 |
|---|---|
| 关掉再打开 | 设置面板里把 AgentPulse 的勾**取消再勾上**——这一步就是让 TCC 重新记录当前签名 |
| 删掉再加回 | 选中 AgentPulse 按 `−`，再按 `+` 从"应用程序"里重新添加 |
| 命令行重置 | `tccutil reset Accessibility com.agentpulse.app`（会清掉这一项的全部记录，然后重新授权） |

**它在产品里的四个出口**（不能指望用户读文档）：

- `accessibility_granted()` 用 `tell application "System Events" to return UI elements enabled`
  **只读探测**——不弹窗、不把人拽进设置面板，所以敢在引擎 `start()` 前和每次演练里各查一次。
- `is_accessibility_error()` 认 `-1719` / `-25211` / `not allowed to send keystrokes` 等，
  把这个"默认静默"的错误换成一句能照着做的话。它**故意不带 cfg**：纯字符串判断，
  三个平台的 CI 都能测（`accessibility_errors_are_recognised`）。
- `signature_is_stable()` 读 `codesign -d --requirements -`：出现 `anchor apple` 或
  `certificate` 说明认的是名字加证书，只有 `cdhash` 说明是临时签名。同样**故意不带 cfg**。
  查不出来就当稳定——这一项只用来加一句解释，拿不到证据时宁可少说，也不要凭猜测吓人。
- `accessibility_hint()` 据此在 `resume.needs_accessibility` / `probe.no_accessibility`
  和它们的 `_adhoc` 变体之间选一句。**分两句是因为要用户做的动作不一样**：没勾过的
  去勾上，勾过的得取消再勾一次、而且得知道这事每次更新都会重演、根治要靠稳定签名。
  一句笼统的"请去开启权限"会让已经勾过的人以为程序在骗他——用户的原话是"我明明勾选了"。

**结构性解法是绕开它**：`channel_needs_accessibility()` 里 tmux、screen、iTerm2 三条通道
返回 `false`——它们直接写伪终端，不碰 `System Events`，所以**根本不需要这个授权**。
在 tmux 里跑 agent 是唯一一劳永逸不受签名影响的路（这也是 v1.4 加 tmux 通道的主要动机）。

---

## 13. 未来规划与设计

规划的第一原则没变：**下一个版本的价值不在新功能，而在让已经写完的功能变得可信。**
v1.4 和 v1.5 就是照这条原则做的两版——一版让"动手之前"可见（演练、权限自检、免权限通道），
一版让"动手之后"可查（落地核验、三个计数器、失败会出声）。

剩下的最大风险仍然只有一条：**续跑这条主链路没有任何实机证据**（12.1）。

### 13.1 已交付：原 v1.4 / v1.5 候选清单的落点

| 原候选 | 结果 | 落在哪 |
|---|---|---|
| P0 续跑演练（dry-run）按钮 | ✅ v1.4 | `Resumer::probe` + `ResumeProbe`，见 7.13 |
| P0 三平台实机验证清单 | ✅ 清单已写 / ⚠️ **还没走完** | `docs/manual-test.md`，见 12.1 |
| P1 拆 `ConfigPanel.tsx` | ✅ v1.6（第一轮） | 通用骨架、通知、成本、AI 分区已拆，主文件降到约 610 行 |
| P1 前端 vitest | ✅ v1.4 | 落地时 38 个，现已 99 个，见 14.2 |
| P2 重写 `docs/architecture.md` / README / `icons/icon.png` | ✅ | 见 12.2 |
| P2 自动更新重做 | ❌ 仍未做 | 见 13.2 |
| P1 自定义适配器 UI | ✅ v1.6 | 设置页可增删改名称、进程匹配和会话文件模式 |
| tmux / screen 通道（原属 13.3"更远"） | ✅ v1.4 **提前做了** | 它是确定性最高、且**唯一不受 macOS 签名影响**的通道，见 12.6 |
| 健康自检（原属 13.3） | ✅ v1.4 大部分 | `ToolStatus` / `channel_health` 随演练一起给出 |
| 判定证据面板（原 v1.5 ②） | ✅ v1.6 | 后端 `DetectionEvidence` 快照 + 会话卡片展开，只渲染事实不重算策略 |
| 闭环核验（原清单里没有，是 v1.5 现场加的） | ✅ v1.5 | 见 7.8–7.12 |

最后一行值得单独说：它不在任何一版的候选清单里，是从"为什么每次都要等用户来报同一类问题"
这个问题倒推出来的。**清单能列出的都是想得到的功能；想不到的那一层，只能靠追问症状的成因找到。**

### 13.2 v1.6 / v1.7 / v1.8 / v1.9 / v1.10 已交付与下一步候选

按"先让已交付的东西可信"排序：

**P0 — 走完 `docs/manual-test.md`。** 这不是开发任务，是准入条件。12.1 那张表里有
七行还挂着 ⚠️，其中"落地核验的 6 秒窗口够不够"只有实机能回答——真机跑一次，
看日志报的是 `Landed` 还是 `Silent`。

**已交付 — 自定义适配器 UI。** `CustomAdapterConfig` 已在设置页提供名称、进程匹配、
会话文件模式的增删改表单；全部走组件库和 i18n。

**已交付 — 检测侧判定证据面板。** 后端保存 `DetectionEvidence` 事实快照：信号、进程存活、
`TurnState`、忙碌宽限、命中的中断关键词/完成标记和 AI 第二意见。前端只展示快照，
不复制 `make_verdict` 的策略，避免判定出现两个出处。

**已交付 — AI 兜底接进自动回路。** 只在「关键词命中、记录仍增长、结构证据用尽」
这一处提问，严格接受 `DONE` / `CONTINUE`，每轮最多问一个，并按记录指纹缓存。
权限是单向的：`CONTINUE` 可以把可疑提升为确认中断；`DONE`、请求失败或非标准回复
都不撤销已有结论。忙碌回合不问，避免重新引入上下文压缩误判。

**P1 — 继续拆 `ConfigPanel.tsx`。** 第一轮已把通用骨架、通知、成本、AI 分区拆出，主文件约 610 行；
Webhook、远程和适配器仍可按同一边界继续拆，但不再是阻塞发布的欠账。

**已交付（v1.9）— 续跑协调器。** 扫描与真实投递彻底解耦；按会话合并队列、常驻
worker、RAII 租约、stop 生命周期失效、出队全量重验、进程启动代际身份和并发状态归约共同
保证“检测持续刷新，但旧动作绝不补敲”。首次引导与会话聚焦是同版本的前端配套。

**已交付（v1.10）— 两阶段续跑流水线。** v1.9 仍让只读核验占用全局投递锁；现在窗口、
剪贴板和键盘阶段严格串行，输入完成立即释放，跨会话 transcript 核验并行。队列绕过 leased
session，owned lease 仍覆盖完整闭环；页脚直接展示 Rust 快照里的待投递与核验数量。

**P2 — 组件层测试。** 现在的 99 个前端测试只覆盖纯函数和 store 归约；`SessionList`
的排序、标签显隐这类逻辑值得补 `@testing-library/react`。

**已交付（v1.8）— 限流识别与保持窗口。** 需求原话是「按不同供应商选不同策略」，但代码读
下来，`RateLimited => Wait` 已经是全局默认（`detector/mod.rs`），最保守的那条路今天对所有
供应商都生效。真正的暴露面有两处，都不在「策略表」上：

**一、认不出来。** `default_rate_limit_keywords()` 只有 8 条，中转站把限流写成
「上游负载已饱和」或只回一个 `upstream_busy` 时，一条都不命中，原因落到 `RuntimeError`，
而它配的手段是 `Nudge`——于是应用按冷却一遍遍往里敲字，正好是会让号被封的那个行为。
所以只做一张「供应商 → 策略」表是无效的：表要先知道这是限流才谈得上查，而出事的前提
恰恰是没认出来。

**二、认出来了也只按住几十秒。** 这一条是动工时读代码才发现的，比第一条更要紧：适配器
只读记录尾部 40 行（`read_tail_lines(path, 40)`），而 agent 撞上限流后还会继续写重试日志。
等那行 `429` 被顶出这 40 行，判定就再也看不见它，原因掉回 `Stalled` → `Nudge`——**应用
正好在限流窗口还没过去的时候开始敲字**。不是没有 `Wait`，是 `Wait` 只维持到那行字滚走为止。

| 落点 | 位置 |
|---|---|
| 兜底形状识别（关键词全落空后看 HTTP 形状与中转站说法） | `detector::rate_limit::upstream_rejection` |
| 从消息里抠等待时间（`retrying in 34s` / `请在 30 秒后重试`） | `detector::rate_limit::parse_wait_hint` |
| 新原因 `UpstreamRejected`（与 `RateLimited` 分开，措辞不同） | `detector::InterruptReason` |
| 保持窗口（认出那轮记截止时刻，之后不看证据只看时刻） | `detector::RateLimitHold`、`Detector::apply_rate_limit_hold` |
| 看见它自己动了就放手（判定为运行中/已完成时窗口立刻作废） | 同上，`Verdict` 参与判断 |
| 跨轮存活 | `AgentSession::rate_limit_hold`，由 `scan_once` 逐轮合并 |
| 活动日志说出截止时刻 | `log.rate_limit_hold` |

四个刻意的取舍：`500` 不算限流形状（太常见，拿它当限流会让真故障没人叫）；存坏的时间戳
**故意不保守**（当成「一直按住」会让某个会话永久静默，比多敲一次糟得多）；用户配的关键词
永远排在兜底前面；`Suspicious` **不在**放手名单里（它的意思是证据不足，而这个功能的前提
就是认不出来时宁可多等）。

放手那条是落地之后补的，补的是一个真实的洞：合并回写那段不分判定结果都写
`interrupt_reason`，而前端直接照它画，于是一个已经恢复干活的会话会在剩下的窗口里同时
显示「运行中」和「撞上限流，不敲字」。触发概率不低——消息里带个 `retrying in 10m`
窗口就是十分钟，而限流实际常常一两分钟就过去。判据是**事实优先于估算**：截止时刻是估的，
「记录又开始长了」是事实。

**未做（下一个增量）— 按供应商 profile。** 真正因供应商而异的只有三项：限流后冷却下限、
同窗口撞第 N 次后彻底停手只叫人、是否信任消息里的 reset 时间。「敲字对限流没用」这类知识
对所有供应商一样，不做成开关——做成开关等于允许用户把自己配到危险的一侧。未识别的供应商
一律落最严格那档。

**已定 — 供应商身份由用户自己挂，不嗅探。** 代码里现在完全没有这个概念（grep `adapters/`、
`detector/`、`monitor/` 无任何 `ANTHROPIC_BASE_URL` / `base_url`）。读运行中进程的 environ
能直接拿到（`sysinfo` 的 `Process::environ()` 确实存在），但同一个 block 里就是
`ANTHROPIC_AUTH_TOKEN`，等于让本应用具备读取用户密钥的能力，一旦加进去 9.1 那张安全表
每一行都要重新论证；读 `~/.claude/settings.json` 同样碰凭证，且不代表运行中进程实际拿到的值。
第三条路是不识别，按项目/会话让用户自己挂 profile，零新增权限面，代价是要点一下。

取第三条，依据不只是「按非侵入定位倾向」——需求里点名的 `cc-switch`（`farion1231/cc-switch`，
12.4 万星，同样是 Tauri 2 桌面应用）就是这么做的：它的 `Provider` 结构体里是 `id` / `name` /
`settings_config` / `category` / `icon`，**每一项都是用户填的**，全库没有一处从进程环境或
凭证文件反推「你在用哪家」。市面上做得最大的同类工具都没去嗅探，这条路不是妥协。

**P2 — 自动更新重做。** 生成签名密钥对、配 `tauri-plugin-updater`、在 CI 的 `release` job 里
产出 `latest.json`。上次失败的原因很具体（没有公钥就崩），别重复。顺带能解掉 12.6 的一半——
有稳定的 Developer ID 签名，辅助功能授权就不会每次更新都失效。

**P2 — 多项目视图。** 按 `cwd` 项目名分组会话，成本页已经有按项目聚合，主面板还没有。

**待拍板 — 远程审批（原 v1.3 P2）。** 想要的体验：手机收到"会话在等你回话"的通知，
点一下就让它续跑，不用回到电脑前。难点在于这会把一个**结构性只读**的服务变成可写服务，
9.1 那张表里的每一条都要重新论证。建议的设计（还没实现）：

| 设计点 | 方案 |
|---|---|
| 动作白名单 | 只允许一个动作：`resume(session_id)`。没有"改配置"、没有"停止引擎"、没有自由文本 |
| 提示词不可远程指定 | 用的是本机配置里的 `resume_prompt` / `goal_resume_prompt`，**手机不能传任意文本**——否则远程看板就变成了远程命令注入面 |
| 一次性动作令牌 | 通知里带一个绑定 `session_id` + 过期时间的一次性令牌，用完即废；读令牌（看板）和写令牌（审批）分开 |
| 幂等 | 同一个动作令牌重复提交只生效一次 |
| 默认关闭 | 独立开关 `remote.allow_approval`，默认 `false`；开启时明确告知"手机上的人可以让你的 agent 继续跑" |
| 审计 | 每次远程审批落 `resume_records`，标注来源是远程；活动日志里可见 |
| 仍不放宽绑定 | `bind_all` 的告知文案不变，且远程审批建议只在 `127.0.0.1` + 反代/隧道场景下用 |

### 13.3 更远的候选（都仍在"非侵入"边界内）

| 想法 | 价值 | 备注 |
|---|---|---|
| **SSH 远端守护** | 守护跑在远程机器上的 agent（会话文件通过 ssh 读，续跑通过 `ssh + tmux send-keys` 投递） | tmux 通道已经在 v1.4 落地，这条路的前置条件已经具备 |
| **更多适配器** | Aider / Cline / Gemini CLI / Continue | 做完 13.2 的自定义适配器 UI 之后，这件事可以交给用户 |
| **Windows 用 UIAutomation 取代标题匹配** | 能直接枚举标签页，把"窗口级确定性"提升到"标签级确定性"，多标签宿主不再需要靠标题猜 | 工程量不小，但这是 Windows 侧唯一的确定性天花板突破口 |
| **从历史里学阈值** | 你每次手动续跑（说明它漏判了）和每次撤销（说明它误判了）都是标注数据，可以用来给这台机器调 `idle_timeout_secs`；v1.5 的 `ResumeOutcome` 让这件事第一次有了**自动**标注来源（`Silent` = 投递侧的问题，`Landed` 后马上又停 = 判定侧的问题） | 要小心：不能让学习结果绕过 6.4 的两条铁律 |
| **AI 兜底接进自动回路** | ✅ v1.6 | 唯一弱证据缺口自动提问，结构化 `DONE` / `CONTINUE`，见 12.4 |
| **成本报表导出** | ✅ v1.7 | 花费页三个维度（按天 / 按项目 / 按模型）都能导 CSV；写入面只有下载夹一处，没有新增授权项 |

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

### 13.5 v1.10 之后的核心计划

后续工作已单独固化在 [`docs/post-v1.10-plan.md`](docs/post-v1.10-plan.md)。优先级保持为：
先完成 v1.10 真实桌面验收，再根据实测选择 Attempt 状态机或自适应落地核验作为下一版唯一核心主题；
定位证据增强与并发故障注入随后推进。编排、自主决策和并发桌面输入继续不做。

### 13.6 读了一遍 cc-switch：它对 429 的答案我们抄不了

需求里让「参考 ccswitch」，所以把 `farion1231/cc-switch` 的代理层读了一遍。结论对
13.2 那条限流设计有直接影响，记在这里免得下次又从头猜。

**它撞 429 之后不是等，是换一家。** `categorize_proxy_error()` 里状态码分两桶：
`400/405/406/413/414/415/422/501` 是 `NonRetryable`，**其余全部 4xx 和 5xx 都是
`Retryable`**——429 落在「其余」里，于是走故障转移队列切下一个供应商。理由写在注释里，
是成立的：换一家可能持有不同的 key、配额、地域。

**但它有一样东西我们没有：它在请求路径上。** cc-switch 是个本地代理，agent 的流量从它
身上过，所以它能读到 HTTP 状态码、能读到 `x-ratelimit-*` 头、能中途改写目标地址。
AgentPulse 是非侵入的旁观者，只看得见终端**渲染出来的字**——拿不到状态码，也没有任何
「换一家」的手段可用。所以「参考 cc-switch」不能理解成照搬它的策略表：它的整套答案
（切换）在我们这儿不存在，我们全部的动作就是 `ResumeTactic` 那三个：`Nudge` / `Wait` /
`HandOff`。

**它也没有「按供应商配重试策略」这张表。** 它的 `Provider` 结构体整个读完，十二个字段里
**没有一个是策略**：`id` / `name` / `settings_config` / `website_url` / `category` /
`created_at` / `sort_index` / `notes` / `meta` / `icon` / `icon_color`，加一个布尔
`in_failover_queue`。熔断阈值（`failure_threshold` / `error_rate_threshold` /
`timeout_seconds`）来自 `AppProxyConfig`、经 `get_proxy_config_for_app(&app_type)` 取，
是**按 app 类型**配的，不是按供应商（代码搜索 `cooldown` 也零命中）；唯一两处按供应商分叉的判断是
「Codex 官方线路和 xAI OAuth 的 401/403 不许转移」，写死在代码里，不是给用户的开关，
而且理由是「转移会静默把对话挪到另一个账号上」——跟限流无关。它也不解析 `Retry-After`：
那个头只出现在日志脱敏白名单里，用来打日志，没有喂给任何等待逻辑。

对我们的三点结论：(1) 13.2 把重心放在**识别**而不是策略表，方向没错，做得最大的同类
工具也没有那张表；(2) 解析限流消息自带的等待时间（13.2 第二层）反而是我们相对它的优势
——它能读 `Retry-After` 却没喂给任何等待逻辑，我们读不到头，但 agent 会把「retrying in 34s」
直接打在终端上。而且 `"retrying in"` **已经在** `default_rate_limit_keywords()` 的八条里：
这句话今天就能让判定落到 `RateLimited => Wait`，缺的只是把后面那个数字取出来当冷却下限，
不是从零做一套识别；(3) 未识别的供应商落最保守档这条，在它那儿的对应物是「认不出来就当可重试」，
因为它换一家的代价只是慢一点；我们敲字的代价是号可能被封，所以这里**必须**比它保守。

### 13.7 当前决策记录

本轮已完成 v1.6 的可解释判定链路。远程审批、可写网络 API、编排层和自治层仍不实现；
它们会改变非侵入或只读安全边界，必须单独确认。ConfigPanel 的文件级拆分和组件渲染测试
仍是工程质量欠账，不阻塞当前判定链路发布。

1. **自动更新**：仍需稳定签名材料后再做。
2. **统计增强**：趋势对比、CSV 导出、模型分布和恢复耗时列入后续版本。

---

## 14. 附录

### 14.1 常用命令

```bash
pnpm install                     # 装前端依赖
pnpm tauri:dev                   # 开发（含 Rust 热重载）
pnpm tauri:build                 # 打包，产物在 src-tauri/target/release/bundle/
pnpm build                       # 仅前端：tsc && vite build（CI 用的就是这条）
pnpm test                        # 前端 vitest（含四处版本号一致性）

cd src-tauri
cargo clippy --all-targets -- -D warnings   # CI 的 lint，本地务必先跑
cargo test                                  # 后端单元测试
cargo test -- --list                        # 列出全部测试名

./scripts/gen-icons.sh           # 从 SVG 母版重出整套图标（需要 Chrome/Chromium）

git tag v1.10.0 && git push origin v1.10.0    # 触发 4 目标打包 + 建 Release，见 11.4
```

### 14.2 测试分布（Rust 262 + 前端 99）

Rust（`cargo test -- --list` 的模块级统计）：

| 模块 | 个数 | 守的是什么 |
|---|---:|---|
| `detector` | 68 | 双重校验、结构证据、注意力/策略、限流保持与形状识别 |
| `resumer` | 47 | 三平台脚本、剪贴板、定位拒绝、演练与落地核验 |
| `storage` | 35 | 六张表、游标去重、续跑记录、历史聚合 |
| `monitor` | 34 | 动作闸门、计数归约、队列绕行、RAII 租约/阶段计数、stop epoch、并发状态合并 |
| `adapters` | 25 | 记录解析、进程发现、PID 启动代际身份与历史键 |
| `export` | 18 | CSV 转义、上限与列齐全 |
| `remote` | 11 | 鉴权、定长比较、换绑与页面渲染 |
| `cost` | 7 | 价格匹配与聚合 |
| `i18n` | 6 | 重复 key、占位符和枚举词条门禁 |
| `webhook` | 6 | 五家载荷格式 |
| 其他（`ai_judge` / 顶层） | 5 | 第二意见契约与跨模块不变式 |

前端（vitest，`pnpm test`）：

| 文件 | 个数 | 守的是什么 |
|---|---:|---|
| `src/lib/utils.test.ts` | 25 | token / 金额 / 时间格式化、路径与 class 合并 |
| `src/lib/display.test.ts` | 19 | 状态、注意力、策略映射和 i18n key |
| `src/lib/trend.test.ts` | 19 | 趋势语义、0 与未知、涨跌颜色 |
| `src/lib/history.test.ts` | 14 | 本地日期分组、跨月跨年与夏令时 |
| `src/stores/useAppStore.test.ts` | 7 | IPC 结果与演练归约 |
| `src/components/ui/BarChart.test.ts` | 7 | 时间刻度与空数据 |
| `src/lib/sessions.test.ts` | 6 | 会话搜索、四种 scope、注意力优先排序 |
| `src/version.test.ts` | 2 | 四处版本一致与 SemVer 形状 |

值得单独一提的几个测试名（它们本身就是文档）：

- `long_compaction_pause_is_not_a_stall` —— 铁律 A
- `awaiting_user_plus_silence_confirms` —— 铁律 B
- `talking_about_an_error_is_not_having_one` —— 散文不是故障证据
- `keyword_hit_alone_never_types` —— 关键词单独出现不足以动手
- `verdict_ignores_every_resume_counter` —— 判定层不回答"该不该动手"（7.10）
- `a_session_we_stopped_nudging_still_calls_for_help` —— 放弃动手的那一刻正是最该出声的一刻
- `exhausted_streak_stops_typing_but_not_watching` —— 停手 ≠ 停看
- `failed_deliveries_never_exhaust_the_budget` —— 敲不进去不算"催过了"
- `the_users_clipboard_is_given_back` —— 不能吞掉你复制的东西
- `vscode_script_never_sends_ctrl_c` —— 焦点在编辑器里时 Ctrl-C 是复制
- `blind_typing_is_off_by_default` —— 定位不到就不敲
- `accessibility_errors_are_recognised` —— 静默的权限错误要换成人话（12.6）
- `pty_channels_need_no_accessibility` —— tmux / screen / iTerm2 不需要授权
- `tail_survives_multibyte_at_the_window_edge` —— 尾部读取不能把 UTF-8 字符切一半
- `a_scrolled_away_rate_limit_still_holds_the_line` —— v1.8 的存在理由：证据被顶出 40 行之后仍然不敲字
- `the_stall_we_built_this_for_still_gets_nudged` —— 保持窗口不能顺手把普通卡住也一起按住
- `an_unrecognized_relay_limit_is_not_nudged` —— 中转站换个说法也不能掉回"敲字"那条路
- `a_dead_process_outranks_a_rejection_shape` —— 进程都没了就不是在等限流
- `a_500_is_not_a_rejection_shape` —— 拿最常见的错误码当限流，会让真故障没人叫
- `every_listed_phrase_is_recognized_on_its_own` —— 逐条钉住，防的是"某条词根本命中不了却没人发现"
- `every_enum_key_resolves_to_real_wording` —— 漏一条词条不该让用户在日志里看见键名
- `resume_queue_coalesces_each_session_to_the_latest_snapshot` —— 扫描再快也不能堆旧动作
- `stop_then_restart_does_not_revive_an_old_action` —— stop/start 不能让旧动作复活
- `overlapping_scan_preserves_a_completed_resume_commit` —— 扫描和投递重叠不能丢账
- `a_reused_pid_with_a_different_start_time_is_rejected` —— 裸 PID 相同不代表还是原进程

### 14.3 技术栈

| 层 | 技术 |
|---|---|
| 桌面框架 | Tauri 2（`tray-icon` 特性；插件 shell / notification / autostart） |
| 后端 | Rust 2021 — tokio(full)、sysinfo 0.35、notify 8、rusqlite 0.32(bundled)、reqwest 0.12(json)、glob 0.3、chrono、tracing、dirs 6、uuid v4、serde/serde_json |
| 前端 | React 19.1 + TypeScript 5.8 + Vite 6.3 + TailwindCSS 3.4 |
| 组件 | Radix UI（tabs / select / switch / tooltip / slot）+ cva + clsx + tailwind-merge |
| 状态 | Zustand 5 |
| 桥接 | `@tauri-apps/api` 2.5 |
| 测试 | `cargo test`（Rust 内联 `#[cfg(test)]`）+ vitest 4 |
| release profile | `strip = true`、`lto = true` |

### 14.4 目录结构

```
agent-pulse/
├── PROJECT_STATUS.md          ← 本文档
├── README.md                  ← 面向使用者（路线图到 v1.10，含打包触发说明）
├── docs/architecture.md       ← 分层架构与数据流
├── docs/manual-test.md        ← 三平台实机验证清单（还没走完，见 12.1）
├── docs/post-v1.10-plan.md     ← v1.10 推送后的续跑核心计划与下一次恢复顺序
├── index.html                 ← 首屏底色写死 #fafafa
├── public/{icon.svg,favicon.png}
├── scripts/{gen-icons.sh,make_ico.py}
├── src/                       ← 前端
│   ├── App.tsx  main.tsx  types.ts  index.css
│   ├── version.test.ts        ← 四处版本号一致性（见 11.6）
│   ├── components/{DashboardPanel,OnboardingPanel,ConfigPanel,CostPanel,HistoryPanel,LogPanel,SessionList,StatsPanel,StatusCards}.tsx
│   ├── components/ui/         ← Radix 封装（Button/Card/Field/Select/Switch/Tabs/Tooltip）+ 自绘 BarChart
│   ├── i18n/index.ts          ← 前端中英双语文案
│   ├── lib/{display,chime,useNotice,utils,sessions}.ts + 对应纯函数测试
│   └── stores/useAppStore.ts + useAppStore.test.ts
└── src-tauri/
    ├── tauri.conf.json        ← 版本号由 src/version.test.ts 与 package.json 对齐
    ├── icons/{master.svg,master-macos.svg,…}
    └── src/
        ├── lib.rs             ← 装配：AppState / 托盘 / 事件泵 / 25 个命令
        ├── adapters/{mod,claude_code,codex,opencode}.rs
        ├── detector/mod.rs    ← Verdict + AttentionLevel（只回答"是什么"）
        ├── monitor/mod.rs     ← 主循环 + 动作闸门
        ├── resumer/mod.rs     ← 三平台续跑 + tmux/screen + 演练 + 落地核验（唯一带平台 cfg 的文件）
        ├── cost/mod.rs  storage/mod.rs  notify/mod.rs  webhook/mod.rs
        ├── remote/mod.rs      ← 只读看板
        ├── config/mod.rs  i18n/mod.rs  ai_judge/mod.rs
        └── main.rs
```

---

<p align="center">
  <sub>本文档随代码一起演进。如果你发现文档和代码对不上，<b>以代码为准，并顺手改这里</b>——<br>
  文档撒的谎比没有文档更贵。</sub>
</p>

