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
7. [续跑层：竞争力来自静默、精确与可证明](#7-续跑层竞争力来自静默--精确--可证明)
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
| 后端 | Rust；`monitor/mod.rs` 负责协调，`resume_core.rs` 负责平台无关许可与连续观测 reducer |
| 前端 | TypeScript + React 19，49 个文件 |
| 门禁状态 | v1.10 收尾门禁已通过：`fmt` / `clippy -D warnings` / Rust 325 tests / 前端 103 tests（9 files）/ production build / `git diff --check` |
| Tauri 命令 | 38 个 `#[tauri::command]` |
| 支持的 Agent | Claude Code / Codex CLI / OpenCode（`all_adapters()`） |
| 自动后台通道 | tmux exact pane、iTerm2 exact TTY；全部要求 transcript/协议验证。外部 Windows console 无法按 PID 精确寻址，自动延后，手动点击后才可前台降级 |
| i18n 词条 | 后端 200 条（`(key, zh, en)`），前端 349 条（`[zh, en]`） |
| 持久化 | SQLite 会话/统计表 + v1.10 `resume_attempts` Attempt Ledger |
| 功能层次 | v1.9 协调器 ✅ · **v1.10 连续观测 reducer / 安全 transport 门槛 / 精确 prompt 核验 / Attempt Ledger / 保守历史身份 / 主窗口 360px 配置与窄屏样式 ✅** · 360×700 正式验收与 Windows 手动前台路径真机复测待完成 |

**这个版本能做到的事**：后台观察 Claude Code / Codex / OpenCode 的会话文件与进程，用连续观测
reducer 抑制一次性抖动；只有“精确目标 + 后台通道 + transcript/协议可验证”同时成立时才自动投递。
没有安全通道就安静延后，不激活 Terminal、IDE 或当前工作窗口；用户明确点击手动续跑时才允许前台降级。
投递后只在 transcript 基线之后出现与本次提示词精确相等的整个 user message 时确认成功；数组 content 必须是唯一纯文本 block，额外文本或图片不会误报成功。

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

### 后端（`src-tauri/src/`）

| 文件 | 行数 | 职责 | 关键符号 |
|---|---:|---|---|
| `monitor/mod.rs` | 2498 | 扫描调度、两阶段续跑流水线、动作闸门、并发状态归约 | `ResumeQueue::pop_ready`、`ResumeRegistry`、`ResumeLease`、`PhaseCounter`、`resume_worker`、`run_auto_resume`、`snapshot`、`merge_resume_runtime` |
| `resume_core.rs` | 纯核心 | transport 能力许可与连续观测 reducer | `DeliveryPolicy`、`TransportCapability`、`ResumeDecisionState`、`reduce_decision` |
| `resumer/mod.rs` | 3420 | 三平台/tmux/screen 投递、定位演练、两阶段落地核验 | `Resumer::{deliver,verify_delivery,resume_verified,probe}`、`ResumeDelivery`、`ResumeOutcome` |
| `storage/mod.rs` | 持久化核心 | 会话历史、最终续跑事件与 Attempt Ledger | `begin_attempt`、`mark_attempt_*`、`upsert_session_history` |
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

### v1.10 自动续跑主链

```text
Adapter discovery / stable identity
        ↓
Detector verdict + stable evidence hash
        ↓
resume_core reducer: Observing → Suspected → Eligible
        ↓
Transport capability gate: Exact + Background + Verification
        ↓
Attempt Ledger idempotency key
        ↓
ResumeQueue / session lease / lifecycle revalidation
        ↓
background transport or Deferred
        ↓
exact prompt verification after transcript baseline
        ↓
Rust outcome + attempt + counters + events
```

关键边界：

- **身份**：Codex 只有 `codex resume <UUID>` 与 transcript metadata 精确一致时使用 `cx-<UUID>`；Claude 只有 argv 中显式 `--session-id/--resume <UUID>` 且 cwd 项目目录唯一命中同名 JSONL 时关联 transcript。其余情况使用 PID + 进程启动代际，绝不按 cwd 猜“最新文件”。
- **时序**：`idle_threshold` 是连续观测次数。证据变化、恢复健康或只有 Suspicious 会重置/停止累计。
- **许可**：自动入口固定 `BackgroundOnly`；能力不满足时返回 `deferred/no-safe-transport`，不碰前台。
- **幂等**：`resume_attempts` 以 `session_generation + evidence_hash + prompt_hash` 唯一约束动作身份；危险状态对整个 `session_generation` 生效，不能靠修改 prompt 绕过。只有确认未投递的 `created/deferred` 可参加原子 claim，最终 `delivery_started` 更新仍按整个 generation 检查单赢家，其他 Existing 状态不重放。
- **资源**：真实输入全局串行；输入结束即释放全局锁，不同 session 的只读核验并行。
- **核验**：只有基线之后本次精确 user prompt 出现才是 `Landed`；transport ACK 不是 verified。
- **事实源**：决策状态、transport 能力、attempt、pending/verifying 和 outcome 全在 Rust 归约；前端只展示。

### 进程内装配

`MonitorEngine` 持有扫描状态、按 session 合并的动作队列、session lease registry、全局 delivery lock、
lifecycle epoch、连续观测状态和阶段计数。`resume_core.rs` 保持纯函数，不访问桌面、数据库或前端；
`resumer/mod.rs` 只负责平台 transport 和核验；`storage/mod.rs` 保存最终事件与 Attempt Ledger。

### SQLite 的两类记录

- `resume_records`：给用户看的最终续跑事件；
- `resume_attempts`：不可逆动作账本，区分 created、delivering、transport_acked、verified、deferred、unverifiable、failed；
- `session_history`：逻辑会话/legacy runtime 档案；
- 统计、成本与日志表继续承担原职责。

当前自动投递链已在不可逆输入前 `begin_attempt` / `mark_attempt_delivery_started`，并按结果更新 transport ACK、verified、deferred、unverifiable 或 failed；同进程内 `deferred` 会在相同证据再次出现且安全能力恢复后重试，应用重启后的完整 retry/backoff/quarantine 恢复执行器仍是后续计划。

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

## 7. 续跑层：竞争力来自“静默 + 精确 + 可证明”

### 7.1 自动与手动是两种不同权限

| 策略 | 目标门槛 | 可见性 | 核验 | 无能力时 |
|---|---|---|---|---|
| `BackgroundOnly`（自动默认） | `Exact` | `Background` | transcript / protocol 必须存在 | `Deferred`，绝不前台降级 |
| `AllowForeground`（用户手动） | 不能是 `Unknown` | 可抢焦点 | 无记录时只能 `Unverifiable` | 明确拒绝/失败 |

`auto_follow_latest` 不能把自动模式变成盲敲模式；screen 的当前 window、Terminal.app、Linux 普通终端、
外部 Windows cmd/conhost、Windows Terminal/ConPTY、VS Code/Cursor 等 IDE 集成终端都没有足够的自动后台能力，默认延后。

### 7.2 真正支持的后台通道

1. **tmux exact pane**：按 pane id `send-keys`，不经过焦点和输入法；
2. **iTerm2 exact TTY**：AppleScript `write text` 写目标 session，自动脚本不 `activate`、不 `select`。

两条通道都要求 transcript/协议验证。外部 Windows console 不在列表中：`AttachConsole` /
`WriteConsoleInputW` 只能访问 console 级共享输入缓冲，不能按 PID 获得“只写给目标 CLI”的精确端点。
因此 classic cmd/conhost、Windows Terminal/ConPTY 和 IDE terminal 的自动模式均安全延后。

### 7.3 Windows bug 的边界与手动修复路径

旧 `Ctrl+V` 可能被 Codex 当成粘贴图片；仅发送 Enter 又会表现为“文本没进入，只多一个换行”。
v1.10 不再把外部 console 共享输入缓冲包装成自动后台通道。只有用户明确点击“续跑”后，才允许
重新核验进程代际与唯一窗口目标，前台定位后用 Unicode `SendInput` 发送完整 prompt，再独立发送
Enter。该路径不访问或覆盖剪贴板、不发送 `Ctrl+V` / `SendKeys`，也不弹可见 PowerShell 窗口；
目标不唯一、已退出或代际变化时必须拒绝。

真机必须确认：自动路径确实延后；手动路径文本完整进入、Enter 生效、不只产生换行、不弹
PowerShell、没有图片粘贴错误、不串到其他窗口，并在 transcript 基线之后出现本次精确 prompt。
未复测前不得写“Windows 已验证”。

### 7.4 连续观测 reducer

`ResumeDecisionState` 为 `Observing / Suspected / Eligible`。同一份结构证据必须连续达到阈值；证据 hash
不包含每轮变化的空闲秒数。这样既避免一次扫描就出手，也避免 hash 每轮变化导致永远无法 eligible。

### 7.5 Attempt Ledger

`resume_attempts` 将 decision、session generation、evidence、prompt 和 transcript baseline 绑在一个
attempt 上。唯一约束防止同一证据重复投递；`delivery_started_at` 在不可逆 transport 前落库；
`transport_acked_at` 与 `verified_at` 分离。只有通过单实例仲裁的主实例会在 setup 阶段把遗留
`delivering/transport_acked` 事务收敛为 `unverifiable`，绝不重放；第二实例不会触碰首实例活跃账本。
明确未输入的 `deferred` 只有在 `next_retry_at` 到期后才能重新 CAS claim。任意 prompt 下的危险状态
都会按整个 generation 阻断新 attempt，账本 ACK/finalize 失败时不更新内存计数、不写“已送达”记录、
不发成功通知。typed backoff/quarantine 仍属于后续计划。

### 7.6 精确 prompt 核验

基线之后只有结构化 user message 与本次 prompt 精确相等才 `Landed`。mtime、文件增长、assistant/tool
事件、不同 prompt、基线之前的旧 prompt 都不算成功。

| outcome | 成功计数 | 失败计数 | 连击额度 |
|---|:---:|:---:|:---:|
| `Landed` | 是 | 否 | 消耗一次“催过” |
| `Silent` / `Failed` | 否 | 是 | 不冒充成功 |
| `Unverifiable` | 否 | 否 | 不冒充已确认 |
| `Deferred` | 否 | 否 | 不消耗 |

### 7.7 稳定逻辑会话与 legacy history

Codex 精确 resume UUID 映射为稳定逻辑 ID；Claude 只接受 argv 中显式 UUID 与 cwd 下唯一同名 transcript 的一一关联。普通 Claude 会话和 `--continue` 无法证明映射时保留为独立进程代际，两个同 cwd 进程不会再冒充同一份“最新 transcript”。历史页默认展示会话档案，投递诊断默认折叠；有稳定身份显示“逻辑会话”，无法证明身份的旧数据显示“旧运行记录”。迁移只合并可证明的旧碎片，不按 cwd、项目名、`Set` 或 `DISTINCT` 做危险清理。

### 7.8 两阶段并发不变式

- 同 session 最多一个在途闭环；
- 不可逆输入全局串行；
- 不同 session 的 transcript 核验可并行；
- 排队动作出手前重验 lifecycle、状态、额度、冷却、记录版本和 PID 启动代际；
- stop 取消未输入动作，但不抹掉已发生输入的核验与记账；
- pending/verifying 由 Rust RAII 状态产生，前端不重算。

完整规范见 `specs/v1.10_resume_pipeline_design.md`。

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

- `daily_stats` 支撑统计页，但只有 `Landed` 能计为成功；Deferred/Unverifiable 不得抬高成功率；
- `session_history` 保存逻辑会话与保守的 legacy runtime，历史页默认不混入逐次投递诊断；
- `resume_records` 是用户可读的最终事件，`resume_attempts` 是内部幂等动作账本，两者不能混为一张列表；
- 成本页有按天柱状图和按项目聚合。

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
| `check-frontend` | 每次 push / PR | pnpm 11 + Node 22 → `pnpm test`（vitest，103 个 / 9 files）→ `pnpm build`（`tsc && vite build`，类型错误即红） |
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

### 12.1 真机未验证与 P0 清单（⚠️ 高优先级）

| 项 | 当前事实 | 怎么验 |
|---|---|---|
| **Windows cmd/conhost + Codex** | 外部 console 自动路径应延后；手动前台 Unicode `SendInput` 尚无正式真机结论 | 先验自动不输入；再点“续跑”验完整文本、独立 Enter、无剪贴板/PowerShell、无串线、transcript 精确 prompt |
| **iTerm2 exact TTY 自动后台** | 自动脚本设计为不 activate/select，尚需真实多窗口验证 | 在其他应用工作时触发，焦点不变且目标 transcript 出现 prompt |
| **tmux exact pane** | 精确后台通道有单测，尚需真实 pane 验收 | 多 pane 同时运行，确认只进入目标 pane |
| **自动延后矩阵** | Terminal.app、Linux 普通终端、外部 Windows cmd/conhost、Windows Terminal/ConPTY、IDE、screen 非精确 window 应延后 | 逐项确认无窗口切换、无输入、无成功计数 |
| **手动前台降级** | 代码路径与自动策略分离，仍需三平台验证 | 自动先延后，手动点击后才允许前台；未知目标仍拒绝 |
| **精确 prompt 核验** | 解析逻辑有测试，真实 Agent 写盘时序未完整采样 | 验证 mtime/assistant 变化不算成功，本次 user prompt 才 Landed |
| **两阶段多会话流水线** | 队列/lease/阶段计数有单测，真实桌面并发未验 | 按 `docs/manual-test.md` §13 验 A 核验不堵 B、stop 边界和阶段数字 |
| **历史身份迁移** | 稳定 Codex UUID 与保守 legacy 显示已实现 | 用真实旧库确认逻辑会话降噪且无错误合并/数据丢失 |

单元测试、脚本字符串和 Windows CI 不能代替上表的真机结论。未执行项保持未勾选。

### 12.2 文档与配置的陈旧项

| 项 | 状态 |
|---|---|
| `docs/architecture.md` | ✅ 已对齐到 v1.10 安全能力门槛、自动延后、精确 prompt 核验、reducer、Attempt Ledger 与稳定历史身份 |
| `src-tauri/tauri.conf.json` 的 `icon` 数组 | ✅ 已补 `icons/icon.png`（512px） |
| 四处版本号漂移 | ✅ 已收成单一来源 + 测试锁死，见 11.6 |
| `README.md` 路线图 / 前置要求 / 配置说明 | ✅ 已对齐（Node 22 / pnpm 11，路线图到 v1.10，配置表指向本文档 10.3） |
| 本文档自身的计数 | ✅ 2026-08-07 已按当前工作区重数 |

### 12.3 结构性欠账

- **ConfigPanel 已完成第一轮拆分。** 主文件约 610 行，通用骨架、通知、成本、AI 分区已移到
  `src/components/config/`；Webhook、远程和适配器仍在主文件，下一轮可继续按相同边界拆出。
- **前端测试覆盖纯函数、store、版本一致性、跨语言枚举/i18n 与窄屏静态契约门禁，共 103 个。**
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
| P1 前端 vitest | ✅ v1.4 | 落地时 38 个，现已 103 个，见 14.2 |
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

**P0 — 走完 `docs/manual-test.md`。** 这不是开发任务，是准入条件。重点是 Windows
`cmd.exe + Codex CLI` 的自动安全延后、手动前台 Unicode 文本 + 独立 Enter、无安全通道延后、
精确 prompt 核验和稳定逻辑历史；未勾选项不得用单元测试代替。

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

**P2 — 组件层测试。** 现在的 103 个前端测试覆盖纯函数、store 归约和窄屏静态契约；`SessionList`
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

本轮并发收尾额外补齐：监控循环绑定 start epoch；stop 清队列/推进 epoch 后穿过不可逆投递 fence；deferred claim 使用 SQLite CAS；Windows 目标定位使用原始 creation FILETIME 排除裸 PID 复用，但外部 console 不因此获得精确后台输入能力；transcript 只接受整个 user message 的唯一纯文本 block。

后续工作已固化在 [`docs/post-v1.10-plan.md`](docs/post-v1.10-plan.md)：先完成 v1.10 真机矩阵，
再推进 Owned PTY/ConPTY、Codex/OpenCode 官方或 server transport、typed error taxonomy、错误 fingerprint
熔断、Goal budgets、IDE/终端插件端点、Attempt Ledger 的 retry/backoff/quarantine、`SessionIdentity /
SessionRuntime / CurrentSnapshot / DetectionEvent / ResumeEvent` 历史模型拆分，以及
`DeliveryConfirmed` 与 `ProgressObserved` 分级。

不扩大自动前台输入范围，不把 transport ACK 当 verified，不用“清理重复历史”为理由做不可证明的合并。

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

### 14.2 测试分布（数量以最终 v1.10 门禁为准）

下表保留测试职责地图；工作区仍在收尾，精确数量必须由最终 `cargo test -- --list` 重新生成，不能沿用旧数字作为发布结论。

| 模块 | 个数 | 守的是什么 |
|---|---:|---|
| `detector` | 68 | 双重校验、结构证据、注意力/策略、限流保持与形状识别 |
| `resume_core` | 已覆盖 | 自动 transport 硬门槛与 `Observing / Suspected / Eligible` 纯 reducer |
| `resumer` | 已覆盖 | 后台/前台策略、精确 prompt 核验、Windows 自动延后与手动前台 Unicode 输入 |
| `storage` | 已覆盖 | Attempt Ledger 幂等与状态转换、续跑记录、稳定/legacy 历史聚合 |
| `monitor` | 已覆盖 | reducer 时序集成、ledger 调用链、自动/手动策略、队列/lease/stop epoch 与 outcome 归约 |
| `adapters` | 已覆盖 | transcript 解析、Codex UUID 精确关联、进程代际身份与历史键 |
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
| `src/responsive.test.ts` | 4 | 360px 窗口下限、长文本折行/选择与复制入口的静态契约 |

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

