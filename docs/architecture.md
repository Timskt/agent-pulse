# AgentPulse 架构设计

> 这份文档回答的是「**为什么这样拆**」和「**一次续跑到底经过了哪些判断**」。
> 配置项逐条说明、路线图、开发红线在 [PROJECT_STATUS.md](../PROJECT_STATUS.md)；
> 三平台手动验收清单在 [manual-test.md](./manual-test.md)。
>
> 最后一次与代码对齐：2026-08-07（`v1.10` 开发完成，待发布）。

## 1. 一条不能越过的线

AgentPulse 是 **AI Agent 的守护神，不是它的容器**。整套架构都压在这一条约束上：

- **不启动** agent，**不代理** agent 的输入输出，**不替换** 用户的终端；
- 只做两件事：在**外部**观察，以及在需要时**像用户一样敲一次键盘**。

所以你在代码里看不到 pty、看不到子进程托管、看不到自己实现的终端。会话是从
系统进程表里**认出来**的，输出是从 agent 自己的记录文件里**读出来**的，续跑是
往用户那个真实的终端窗口里**敲进去**的。

这条线也决定了什么功能不做：编排层（v2.0，替用户拆解任务、派发给多个 agent）和
自治层（v2.1+，让 agent 自己长出目标）都与「非侵入」直接冲突，**已明确搁置**，
不经确认不动工。

## 2. 分层总览

```
┌──────────────────────────────────────────────────────────────┐
│  React 19 + TypeScript + Tailwind 3 + Zustand                │
│  总览 / 统计 / 花费 / 历史 / 设置        ← 界面文案 i18n 字典   │
└───────────────────────────┬──────────────────────────────────┘
                            │ Tauri IPC：38 个命令 + 3 个事件
┌───────────────────────────▼──────────────────────────────────┐
│                      Rust 核心                                │
│                                                              │
│  ① 感知层  adapters/    进程表 → 会话；记录文件 → 输出/回合状态 │
│  ② 判定层  detector/    两条正交的轴：要不要续跑 / 要不要叫人   │
│  ③ 投递层  resumer/     复用器 → GUI 终端，「定位不到就不敲」    │
│  ④ 洞察层  cost/ storage/  token 计价、限流预测、SQLite 时间线  │
│  ⑤ 远程层  remote/      只读手机看板（默认关，令牌鉴权）        │
│  ⑥ 导出层  export/      CSV 转义边界（内容 vs 数值分开处理）     │
│                                                              │
│  横切：monitor/ 调度  notify/ 提醒  webhook/ 外推  i18n/ 文案   │
└──────────────────────────────────────────────────────────────┘
```

## 3. 目录地图

| 路径 | 行数 | 职责 |
|------|-----:|------|
| `src-tauri/src/resumer/mod.rs` | 3420 | 投递层全部：通道选择、三平台脚本、两阶段投递/核验、演练、聚焦 |
| `src-tauri/src/storage/mod.rs` | 2392 | SQLite 六张表：续跑/检测/日聚合/用量/游标/会话历史 |
| `src-tauri/src/monitor/mod.rs` | 2498 | 扫描循环、两阶段续跑流水线、动作闸门、并发状态归约 |
| `src-tauri/src/detector/mod.rs` | 2098 | 信号融合、注意力分级、词边界匹配、限流保持窗口 |
| `src-tauri/src/adapters/` | 1388 | 进程快照、进程代际身份、Claude Code / Codex / OpenCode |
| `src-tauri/src/lib.rs` | 884 | Tauri 装配：命令、托盘、事件泵、窗口行为 |
| `src-tauri/src/remote/mod.rs` | 873 | 手写 HTTP 服务 + 内嵌看板页 |
| `src-tauri/src/i18n/mod.rs` | 751 | 后端文案（通知、托盘、日志、报错、CSV 表头） |
| `src-tauri/src/export/mod.rs` | 684 | CSV 渲染与转义边界（`Cell::Text` 中和公式 / `Cell::Value` 保持可求和） |
| `src-tauri/src/cost/mod.rs` | 536 | 模型价目表、增量读用量、限流预测 |
| `src-tauri/src/detector/rate_limit.rs` | 438 | 限流形状识别与等待时间解析（纯函数，不碰时钟/网络/配置） |
| `src/i18n/index.ts` | 800 | 前端文案（界面上的字） |
| `src/components/ConfigPanel.tsx` | 610 | 设置页主编排 |
| `src/components/SessionList.tsx` | 549 | 会话列表、搜索、筛选、状态与动作入口 |
| `src/components/OnboardingPanel.tsx` | 140 | 首次三步引导与非侵入边界说明 |
| `src/lib/sessions.ts` | 90 | 会话筛选和排序纯函数；不重算 Rust 判定 |

## 4. ① 感知层：会话是怎么被认出来的

`adapters::take_process_snapshot()` 每轮扫描**只枚举一次**系统进程，然后把同一份
快照交给所有适配器 —— 四个适配器各自遍历系统进程表的写法在会话多的机器上是
可观测的浪费。

进程身份不是裸 PID，而是 **PID + 进程启动时刻**。`process_session_id` 把启动代际写入
会话 id，防止系统复用 PID 时让新 Agent 继承旧会话的冷却、失败退避和自动续跑额度。
投递前的 `process_matches_session` 只定向刷新目标 PID（不再为每条动作重扫整张进程表），
并同时复核启动时刻与可读命令行；同 PID、不同代际一律拒绝投递。

每个适配器实现 `AgentAdapter`，其中三个方法是后面所有判断的原料：

| 方法 | 读什么 | 为什么单独存在 |
|------|--------|----------------|
| `recent_output` | 记录尾部的散文 | agent 说的话，用于关键词匹配 |
| `error_output` | 记录里被运行时标成故障的行 | agent **谈论**一个错误和它**遇到**一个错误，在散文里长得一模一样。实测栽在「不再撞上错误关键词 500」被判成会话报了 500 —— 词边界也拦不住，那确实是个独立的 `500`。所以「出错 / 限流」两级注意力只认这个通道 |
| `turn_state` | 记录的**结构** | 见下 |

`TurnState` 是整套检测里最关键的一个类型：

```
Unknown      认不出来（Codex / OpenCode 没有可读的回合结构）
ToolRunning  工具调用发出去了，结果还没回来 —— 正在跑命令，别打扰
Busy         回合没收尾（刚收到提示词、刚拿到工具结果、正在思考）
AwaitingUser 回合收尾了，确实停在等人
```

它存在的理由：**压缩上下文、跑一条几分钟的构建、拉一个大仓库，记录文件都不落盘。**
只看 mtime 会把这些全判成卡住，然后往一个正在干活的会话里敲字。`Unknown` 故意
不算忙 —— 认不出结构时保留超时兜底，否则不写记录的 agent 就彻底测不出中断了。

## 5. ② 判定层：两条正交的轴

这是最容易被合成一条的地方，但它们回答的是不同的问题：

| 轴 | 类型 | 问的问题 | 错判的代价 |
|----|------|----------|-----------|
| 中断检测 | `Verdict` | **要不要替他敲一次续跑** | 往正在干活的会话里插字 |
| 注意力分级 | `AttentionLevel` | **要不要现在就叫人过来** | 半夜弹一个假警报 |

一个会话完全可以「不该续跑」（在等人确认）同时「该叫人」（等你输入）；
也可以「该续跑」同时「不必叫人」（自动续跑会处理掉）。合成一条就必然牺牲一边。

**但这两根轴都不回答「该不该动手」。** 判定层只报「现在是什么状态」，
额度用没用完、冷却过没过、总开关开没开，全部住在 `monitor` 的动作闸门里
（`has_nudges_left` / `effective_cooldown` / `auto_resume_enabled`）。
这条边界是踩过坑才划出来的：续跑上限原先写成 `make_verdict` 里的提前返回，
于是催满次数的会话被永久钉在 `Suspicious`，而 `Suspicious` 映射出的注意力级别是
`None` —— **不敲了，也不叫人了**，一个静默放弃。同一个提前返回还让 `Verdict::Running`
再也不可能出现，连「它自己动起来了」这个重置条件都被堵死。测试
`verdict_ignores_every_resume_counter` 把这条边界钉住。

### 信号与判定

| 信号 `SignalKind` | 触发条件 | 证据性质 |
|---|---|---|
| `ProcessExited` | 进程不在快照里 | 系统事实，单独即可确认 |
| `HeartbeatTimeout` / `FileStale` | 记录时间线超阈值；两者是同源事实，展示时合并为时间信号 | 时间线 / 文件 mtime |
| `KeywordMatch` | `recent_output` 命中中断关键词 | 输出文本，单独只到可疑 |

判定层不靠信号数量“凑票”：进程存活、回合结构和完成标记先处理，时间信号只作为停更事实。
当记录仍在增长、只命中关键词时，判定进入唯一的弱证据缺口：`Suspicious`，并置位
`wants_second_opinion`。监控层下一轮把记录尾部交给配置的 OpenAI-compatible 端点，严格要求
`DONE` / `CONTINUE` 两个词；只有 `CONTINUE` 能把结论提升为 `ConfirmInterrupt`，失败、超时或
`DONE` 都不改变原结论。忙碌回合永远不问，避免把上下文压缩误判重新打开。

`DetectionEvidence` 是后端发出的事实快照：信号种类、进程存活、`TurnState`、忙碌宽限、命中
关键词/完成标记和第二意见。前端证据面板只渲染它，不复制判定策略。

```
命中完成标记                    → TaskCompleted（永不续跑）
进程已退出 且 无完成标记        → ConfirmInterrupt
回合忙碌且停更                  → Suspicious（宽限后继续观察）
只有关键词 + 记录仍在增长       → Suspicious + 请求第二意见
关键词 + 第二意见 CONTINUE      → ConfirmInterrupt
没有信号                        → Running
```

**忙时宽限（`BUSY_GRACE_MULTIPLIER = 10`）**：`TurnState::is_busy()` 为真时，
「多久不写文件算卡住」放大 10 倍（默认 60s → 600s）。这条专门用来躲开长时间的
上下文压缩 —— 但**只放大 `FileStale` 这类时间信号**，不会让一个真正停在
`AwaitingUser` 的会话不再产生续跑。两个方向的失败都被用户实际报过，所以两边
都不能松：压缩不能被判成卡住，真卡住也不能不管。

数据库演进采用形状驱动迁移：启动时 `migrate()` 通过 `pragma_table_info` 检查列，
`ensure_column()` 只补缺失列。旧安装不会因为 `CREATE TABLE IF NOT EXISTS` 对已存在表
无效而永远缺字段；重复启动也保持幂等。

**词边界匹配（`contains_keyword`）**：裸 `contains` 会让 `"500"` 命中
`"1500 tokens"`、`"429"` 命中 `"14290"` —— 一句正常的用量统计就能把会话判成
服务器错误。规则是关键词首尾为 ASCII 单词字符时要求那一侧不紧贴另一个单词字符，
并特意放宽两处：`(y/n)` 这类符号打头结尾的不做要求；**含中文的关键词完全跳过
边界判断**，因为中文不用空格分词，`是否继续` 前后紧跟汉字是常态。

这条规则有个反面：**词干在这个匹配器里永远不会命中**。写 `throttl` 想同时覆盖
`throttled` / `throttling`，实际是一条都命中不了——`throttl` 结尾是单词字符，
后面紧跟 `ed` 就被边界规则挡掉。这个错误在 v1.8 真的写进去过，243 个绿灯测试
没有一个变红，是变异检查逼出来的（见 13 章）。要覆盖多种词形就把词形逐个列出。

### 限流保持窗口（v1.8）

`RateLimited => Wait` 一直是默认，问题不在策略而在**证据的寿命**：适配器只读记录
尾部 40 行，而 agent 撞上限流后还会继续写重试日志。那行 `429` 被顶出窗口之后，
判定就再也看不见它，原因掉回 `Stalled => Nudge`——**应用正好在限流窗口还没过去的
时候开始敲字**，而这正是会让账号被封的行为。不是没有 `Wait`，是 `Wait` 只维持到
那行字滚走为止。

所以判定层多了一个不看证据、只看时刻的短路：

```
认出限流/上游拒绝  → 记下截止时刻（max(冷却, 60s, 消息里的等待时间)）
                     并把当轮原因一起存进 RateLimitHold
判定为运行中/已完成 → 立刻放手：它自己动了，说明限流已经过去
后续每一轮         → 截止时刻没到就重放原来的原因，直接 Wait，不再要求证据还在
                     截止时刻过了就放开，回到正常判定
```

中间那条是**事实优先于估算**：截止时刻是估出来的（`retrying in 10m` 是上游说的、
冷却下限是我们配的，都可能比真实窗口长得多），而「记录又开始长了」是事实。不放手
的话，一个已经恢复干活的会话会在剩下的窗口里一直挂着「撞上限流，不敲字」，旁边的
状态徽标却写着「运行中」——两句话自相矛盾，用户只能猜哪句真。这跟 `classify_reason`
里「进程存活位优先于日志行」是同一条原则。

放手名单里**故意没有 `Suspicious`**：它的意思是证据不足，而这个功能的整个前提就是
认不出来时宁可多等。拿「说不清」当恢复信号，等于在最不确定的时刻松开手。

`RateLimitHold` 挂在 `AgentSession` 上、由 `scan_once` 逐轮合并，所以它能活过证据
滚走的那一刻。存的是原因而不是「按住」这一个布尔，用户看到的才是「限流」还是
「上游拒绝」，而不是一个没有解释的沉默。日志里必须说出截止时刻——「不催」如果只说
「在等」，跟守护神漏了一次分不出来。

## 6. ③ 投递层：自动静默不是“把窗口藏起来”

v1.10 的原则是：**自动动作只有在精确寻址、后台执行、可验证三项同时成立时才获准投递。**
脚本没有报错、窗口可以被激活、键盘事件被系统接受，都不是安全自动续跑的充分条件。

### 6.1 平台无关的能力许可

`resume_core.rs` 把 transport 能力拆成三条正交轴：

```text
TargetCertainty = Exact | Window | Unknown
Visibility      = Background | StealsFocus
Verification    = Transcript | Protocol | None
DeliveryPolicy  = BackgroundOnly | AllowForeground
```

自动入口默认 `BackgroundOnly`，许可条件固定为：

```text
target == Exact && visibility == Background && verification != None
```

没有安全能力时返回 `deferred/no-safe-transport`。`Deferred` 不是失败，也不是成功：不增加成功数、
不消耗连击额度、不刷错误通知，更不能自动降级成前台注入。只有用户明确点击手动续跑时，调用方才
能显式传入 `AllowForeground`；即使如此，`Unknown` 目标仍然拒绝。

### 6.2 当前真实通道边界

| 通道 / 宿主 | 自动模式 | 手动模式 | 原因 |
|---|---|---|---|
| tmux exact pane | 后台允许 | 允许 | pane id 精确寻址，不需要焦点 |
| screen 当前 window | 延后 | 可在显式盲跟随下使用 | 只能定位当前 window，不是 exact pane |
| iTerm2 exact TTY | 后台允许 | 允许 | `write text` 直写目标 session；自动脚本不 `activate`、不 `select` |
| Terminal.app / macOS IDE | 延后 | 可前台定位后输入 | 需要选中窗口/标签或模拟键盘 |
| Windows classic cmd/conhost | 延后 | 可前台定位后用 Unicode `SendInput` 输入 | 外部 console 的输入缓冲是共享资源，不能按 PID 精确寻址 |
| Windows Terminal / ConPTY / IDE | 延后 | 可前台定位后用 Unicode `SendInput` 输入 | 外部既有会话没有精确后台输入端点 |
| Linux X11 / Wayland 普通终端 | 延后 | 可显式走 xdotool/ydotool | 依赖激活窗口和键盘注入 |
| 无 transcript/协议确认 | 延后 | 可投递但只能是 `Unverifiable` | transport ACK 不能证明 Agent 收到 |

因此“跨平台支持”不等于“所有终端都可自动静默”。自动安全延后是正确结果，不是功能失败。

### 6.3 Windows 外部 console：自动延后，手动才前台输入

Windows 旧实现通过剪贴板发送 `Ctrl+V`，在 cmd/conhost + Codex CLI 中可能触发 Codex 的
“粘贴图片”快捷键。后续尝试过 `AttachConsole` / `WriteConsoleInputW`，但这组 API 面向的是 console
级共享输入缓冲：知道目标 PID 属于某个 console，并不等于获得了“只投递给这个 PID/CLI”的精确输入
端点。对于不由 AgentPulse 创建和持有的外部 console，不能把它宣称为安全后台 transport。

因此 v1.10 的 Windows 边界是：

```text
自动 BackgroundOnly
→ 外部 cmd/conhost、Windows Terminal/ConPTY、IDE terminal 均无精确后台端点
→ deferred/no-safe-transport，不激活窗口、不输入

用户明确点击手动续跑 AllowForeground
→ 重新核验进程代际与唯一窗口目标
→ 前台定位目标窗口
→ Unicode SendInput 发送完整 prompt
→ 独立发送 Enter
```

手动路径不访问或覆盖剪贴板，不发送 `Ctrl+V` / `SendKeys`，也不通过可见 PowerShell 窗口执行。
窗口无法唯一定位、目标已退出或代际变化时必须拒绝，不能盲敲当前窗口。完整 Unicode 文本、独立
Enter、无 PowerShell 弹窗、无图片粘贴错误以及 transcript 精确 prompt **仍需 Windows 真机复测**；
代码与静态检查不能替代该结论，清单见 `docs/manual-test.md`。

### 6.4 不可逆输入与只读核验的资源边界

窗口、剪贴板和键盘属于桌面级共享资源，真实输入继续受全局 `delivery_lock` 保护；同一 session
由 lease 排他。输入完成后立即释放全局锁，后续只读取目标 transcript，所以不同 session 的核验
可以并行。每条 `start()` 监控循环绑定自己的 lifecycle epoch；stop 会先清空未投递动作并推进 epoch，再获取并释放 `delivery_lock` 作为不可逆投递 fence。已经越过检查的输入会在 stop 返回前完成，等待中的旧动作拿锁后因 epoch 失效退出，因此 stop 返回后不会再落入旧生命周期的自动输入；只读核验和记账仍可继续。

排队不授予投递许可。真正出手前再次重验：运行开关、session 状态、额度、冷却、记录版本、
PID + `process_started_at`、Windows 原始 creation `FILETIME` tick 和 lifecycle epoch。任何一个事实变化都取消旧动作；Windows 手动前台路径在不可逆输入前重新核验目标代际与唯一窗口，不能因为持有 PID/process handle 就把外部 console 提升为精确后台端点。

### 6.5 transcript 精确 prompt 核验

旧的 `(mtime, len)` 指纹只能证明文件发生了变化，不能证明这次提示词进入目标会话。v1.10 在投递前
记录文件长度基线，只扫描基线之后新增的结构化 user message：Codex 的 `event_msg/user_message`、
`response_item/message(role=user)`，以及 Claude 的 `type=user/message.content`。

只有提取出的**整个 user message**与本次 prompt **逐字符精确相等**才是 `Landed`，不会 `trim` 首尾空格或换行。数组形式必须恰好只有一个 `text` / `input_text` block；匹配文本旁带额外文本、图片或其他 block 均拒绝。
assistant/tool 簿记、mtime 变化、不同 prompt、基线之前已有的相同 prompt 都不能算成功。

| `ResumeOutcome` | 含义 | 记账 |
|---|---|---|
| `Landed` | 本次精确 prompt 已被 transcript/协议确认 | 唯一计入成功和“催过”的状态 |
| `Silent` | transport 接受后未找到本次 prompt | 失败 |
| `Failed` | transport/定位明确失败 | 失败 |
| `Unverifiable` | 手动通道没有可验证记录 | 不算成功，不冒充已确认 |
| `Deferred` | 自动模式没有安全后台通道 | 不算成功，也不算失败 |

### 6.6 演练仍然只是定位诊断

`probe_resume` 走定位链路但不输入，适合提前发现缺少依赖、权限或精确目标。它不能证明字符真的进入
Agent，也不能替代 transcript 核验和真机验收。自动模式的最终许可仍由 Rust 的能力模型决定，
前端不根据 probe 文案自行放宽。

## 7. ④ 洞察层：花费与时间线

`CostTracker` **增量**读 Claude Code 的用量记录：每个文件记一个字节游标，下次只
读新增部分，游标存在 SQLite 里跨重启保留。计价用内置价目表 + 用户覆盖项
（`ModelPriceOverride`），按日期取当时的价格。

`Storage`（SQLite）六张表，每张回答一个问题：

| 表 | 回答 |
|----|------|
| `resume_records` | 什么时候替谁敲了什么，**落地没落地**（`success` 存的是核验结果，不是脚本退出码）|
| `detection_records` | 每次判定的依据（供成功率统计） |
| `daily_stats` | 按天汇总：扫了几轮、判了几次、敲了几次、成功几次 |
| `usage_records` | 按天 / 按项目 / 按窗口聚合 token 与花费 |
| `usage_cursors` | 每个用量文件读到哪个字节了（跨重启保留） |
| `session_history` | 一个项目/终端上次见到是什么时候（关掉的会话也留着） |

限流预测（`forecast_rate_limit`）拿「窗口内已用」和「最近一小时的速度」推算还剩
多久撞线。没设窗口额度就明说「填上才能预测」，不假装算得出来。

## 8. ⑤ 远程层：只读手机看板

一个手写的 HTTP 服务（没引 web 框架），只有两个路由：`/` 返回内嵌的单页，
`/api/state` 返回会话状态 JSON。**结构上只读** —— 没有任何写路径，看板拿不到
续跑、改配置、停引擎这些能力。

安全模型（每一条都是刻意的）：

- 默认**关**；开了也只听 `127.0.0.1`；
- 勾「允许局域网访问」才换成 `0.0.0.0`，界面和日志同时说明「同一网络里的人
  拿到令牌就能看你的会话」；
- 令牌**必须**有，空令牌直接拒绝服务（fail-closed），比较用固定时间；
- 没有 CORS，每个请求一个 CSP nonce；
- 令牌只进剪贴板（「复制带令牌的链接」），**不进活动日志**；
- 开到局域网时令牌短于 16 位会同时在日志和设置页告警，并给一键生成 32 位强令牌
  的按钮 —— 这里选的是**大声警告而不是拒绝绑定**，因为拒绝绑定会再造出一次
  「手机连上来被拒绝」，正是用户报过的那个症状。

## 9. i18n 边界：谁渲染，谁持有文案

两份字典，同一个 `config.language` 驱动：

| 字典 | 持有 |
|------|------|
| `src/i18n/index.ts`（`[zh, en]`） | 界面上的字：导航、按钮、字段、空状态 |
| `src-tauri/src/i18n/mod.rs`（`(key, zh, en)`） | 系统通知、托盘菜单、引擎日志、命令返回的报错 |

后端返回的报错**已经是当前语言的成品文案**，前端原样显示，不再拼「错误：」之类
的前缀 —— 那正是中英混杂的来源。测试强制两件事：没有重复 key，且**同一个 key 的
中英文占位符集合完全一致**（把 `{port}` 改成 `{addr}` 必须两种语言一起改）。

## 10. IPC 契约

### 命令（前端 → Rust，共 38 个）

| 分组 | 命令 |
|------|------|
| 引擎 | `get_state` `get_status` `start_monitoring` `stop_monitoring` `scan_now` |
| 配置 | `get_config` `update_config` `get_platform_info` `get_translations` |
| 续跑 | `manual_resume` `probe_resume` `focus_terminal` `open_accessibility_settings` |
| 统计 | `get_stats` `get_resume_history` `get_resume_page` `get_stats_overview` `get_totals` `get_stats_trend` |
| 花费 | `get_cost_daily` `get_cost_projects` `get_cost_models` `get_usage_summary` `get_rate_forecast` |
| 历史 | `get_session_history` `get_session_history_page` `get_session_history_summary` `get_session_detail` |
| 导出 | `export_resumes` `export_sessions` `export_cost` `export_stats` `reveal_export` |
| 提醒 | `test_notify` `test_webhook` `ai_analyze` |
| 看板 | `get_lan_ip` `generate_remote_token` |

带 `_page` 后缀的四个是分页版，老的不分页命令保留给远程看板和托盘用。
**导出跟的是当前筛选，不是当前页** —— 用户点导出时想的是「我筛出来的这些行」，
只给可见的 20 行是一种没人会发现的静默数据丢失。

### 事件（Rust → 前端，共 3 个）

| 事件 | 载荷 | 说明 |
|------|------|------|
| `engine-events` | `Vec<EngineEvent>` | 增量日志，800 ms 一批。事件泵**常驻在 `setup` 里**，不挂在 `start_monitoring` 命令上 —— 否则从托盘启动监控时前端一条日志都收不到 |
| `engine-stopped` | `()` | 引擎停了 |
| `attention-alert` | `AttentionAlert` | 该叫人了；前端据此响声、高亮会话、立刻补一次状态 |

桌面外壳本身也按窗口宽度自适应：顶栏使用可换行 Grid，导航只在自身区域横向滚动；主内容、
历史筛选、会话行与续跑诊断统一设置 `min-width: 0` 和长文本断行策略，诊断错误和完整工作目录可选择并提供明确复制操作。Tauri 主窗口 `minWidth` 已降至 360，配置契约测试锁定 360×700 可达；页面级 `scrollWidth`、真实字体/缩放和长文本组合仍须按 `docs/manual-test.md` 完成正式窗口验收。历史诊断默认不挂载，避免为了响应式重新引入重复数据。

前端的兜底轮询是**自适应**的，不是固定 3 秒：守护中取扫描周期的一半（夹在
2–8 秒），没在守护降到 10 秒，**窗口不可见时整轮跳过**，切回来立刻补一次。
原来那个无条件 `setInterval(3000)` 在窗口收进托盘时照样每 3 秒敲一次后端。

## 11. 一次自动续跑的完整路径（v1.10）

### 11.1 稳定身份

适配器先确定逻辑会话和运行代际。Codex 只接受精确命令形状 `codex resume <UUID>`，随后递归寻找对应 rollout JSONL，并核验 `session_meta.payload.id` / `session_id`。Claude 从保留参数边界的 argv 中只接受显式 `--session-id/--resume <UUID>`，并要求 cwd 对应 project 目录唯一命中同名 JSONL；裸会话与 `--continue` 不关联 transcript。无法确认时统一退回 PID + process generation 的运行实例 ID，不按 cwd 猜“最新文件”。

历史页默认显示会话档案：有稳定身份的条目标为“逻辑会话”，无法证明身份的旧数据标为“旧运行记录”。
旧数据迁移只合并可证明属于同一旧 session ID + cwd 的碎片，不使用前端 `Set`、SQL `DISTINCT`
或 cwd 单独强行合并；逐次投递记录放在默认折叠的诊断区。

### 11.2 连续观测 reducer

检测快照进入纯 reducer：

```text
Observing → Suspected { evidence_hash, observations }
          → Eligible  { decision_id, evidence_hash }
```

同一份稳定结构证据必须连续出现达到 `idle_threshold` 才进入 `Eligible`。恢复健康、只有可疑证据或
证据 hash 变化都会重置/重开观察窗口。hash 不包含每轮变化的“已空闲 N 秒”文案。

### 11.3 Attempt Ledger 与动作许可

Eligible 动作带 `decision_id` 和 `evidence_hash`。SQLite `resume_attempts` 使用：

```text
UNIQUE(session_generation, evidence_hash, prompt_hash)
```

作为不可逆动作的幂等键，并区分 `created → delivering → transport_acked → verified`、`deferred`、
`unverifiable`、`failed`。transport ACK 只是 transport 接受写入，不是 verified。只有单实例仲裁成功后的
主实例会在 setup 阶段把遗留 `delivering/transport_acked` 以 `IMMEDIATE` 事务收敛为 `unverifiable`；
第二实例不会改写首实例的活跃 attempt。`Existing(deferred)` 只有设置了有效且已到期的
`next_retry_at` 才能重新 CAS 到 `delivering`。最终 `delivery_started` claim 会在同一 SQLite 原子更新中按 generation 重新确认单赢家；任意 prompt 下的危险状态都会按整个 generation 阻断
新 attempt，提示词热更新不能绕过未决外部副作用。任何 ACK 或最终状态转换失败，
都不会更新内存计数、写“已送达”历史或发送成功通知。更细的 typed backoff/quarantine 仍是后续计划。

### 11.4 两阶段协调

1. 扫描生成候选动作并按 runtime generation 合并最新快照；
2. worker 绕过正在核验的忙 runtime generation；
3. 拿到 runtime-generation lease 与全局 delivery lock 后重验所有可变事实；
4. `BackgroundOnly` 能力不满足就 `Deferred`，不碰前台；
5. 允许的 transport 完成不可逆输入后立即释放全局锁；
6. transcript 精确 prompt 核验在锁外执行，不同 session 可并行；
7. outcome、attempt、会话计数、日志和通知在 Rust 中统一归约；前端只展示快照。

### 11.5 Rust 是唯一事实源

`ResumeDecisionState`、transport capability、attempt 状态、`resume_pending`、`resume_verifying` 和
最终 outcome 都由 Rust 产生。前端不能通过按钮状态、日志字符串或数组长度重算“是否允许自动续跑”。

详细状态和验收矩阵见
[`specs/v1.10_resume_pipeline_design.md`](../specs/v1.10_resume_pipeline_design.md)。

## 12. 八个花了很大代价才弄明白的事实

这一节是这份文档最该被读的部分。八个问题都表现为「功能好像没生效」，而且都
**不在报错里**。

### 12.1 重新构建过的应用，辅助功能授权会静默失效

症状：点续跑，窗口**跳过来了**，然后一个字都没敲。

macOS 上 `System Events` 的 `keystroke` 归「隐私与安全性 › 辅助功能」管，
而脚本前半段（`activate`、选标签）**不需要任何授权**。所以没授权时用户看到的
是干净的一次跳转加一片安静，日志里也只有 osascript 的 -1719。

更阴的是第二层：**TCC 记的是代码签名。** ad-hoc / 链接器签名的应用没有稳定的
designated requirement，授权实际绑在二进制的 cdhash 上 —— **每次重新构建都换一个
新 cdhash，系统设置里那个勾看着还在，实际已经不生效了。** 开发期每 `pnpm tauri
build` 一次就要去设置里删掉再重新添加一次。

现在的处理分三层：

**① 动手之前先查。** `osascript -e 'tell application "System Events" to return UI
elements enabled'`（只读、不弹窗、不会把用户拽进设置面板），没授权就**不跳窗口**，
直接返回一句「去哪儿点」，并在演练面板上给「去开权限」按钮。

**② 说清是哪一种缺。** 缺权限有两种成因，要用户做的动作不一样：没勾过的去勾上，
勾过的得**取消再勾一次**。所以 `signature_is_stable()` 读一次
`codesign -d --requirements -`：出现 `anchor apple` 或 `certificate` 说明认的是
名字加证书，只有 `cdhash` 说明是临时签名。据此在 `resume.needs_accessibility` /
`probe.no_accessibility` 和它们的 `_adhoc` 变体之间选一句。查不出来就当稳定
——这一项只用来加一句解释，拿不到证据时宁可少说，也不要凭猜测吓人。

**③ 根治靠固定签名。** `scripts/macos-signing-identity.sh` 造一张自签名的代码签名
证书（`extendedKeyUsage=codeSigning` + 登录钥匙串信任 `trustRoot`），之后
`APPLE_SIGNING_IDENTITY=…` 构建，指定要求就从 `cdhash H"…"` 变成
`identifier "com.agentpulse.app" and certificate leaf H"…"`，证书不换这串就不变，
勾一次一直有效。对外分发仍需真的 Developer ID（签名 + notarize）。

**签名身份故意不写进 `tauri.conf.json`。** 写死了，没有这张证书的人连构建都过不去；
放在环境变量里，本地用自签名、CI 用仓库 secrets 里的 Developer ID，两边都不用改配置。

### 12.2 只有可拥有或可精确寻址的 endpoint 才能后台自动化

只要 agent 跑在 tmux exact pane 里，`send-keys -t %3 -l --` 就能按 pane id 寻址，
不需要窗口在前台、不过输入法、没有 shell 插值。iTerm2 exact TTY 的 `write text` 同样能够
绑定到明确 session。两者仍必须有 transcript/协议核验；“API 可调用”不能跳过验证层。

外部 Windows classic cmd/conhost 不同：console input buffer 是共享资源，`AttachConsole` /
`WriteConsoleInputW` 不能提供只属于目标 PID/CLI 的 endpoint，因此自动路径必须延后。后续只有
AgentPulse 自己持有的 Owned ConPTY，或提供稳定 session endpoint 的官方/server 协议，才可按同一
标准评估为 Windows 后台 transport。

### 12.3 `abort()` 只是提交取消请求 —— 换绑地址会撞上自己

症状：勾上「允许局域网访问」，手机带着令牌访问，**连接被拒绝**。看着像鉴权，
其实根本没人在听。

`RemoteService::sync()` 原来是 `handle.abort()` 之后立刻 `TcpListener::bind()`
同一个端口。`JoinHandle::abort()` 是**异步**的：函数返回时那个任务可能还卡在
`accept().await` 上握着监听 socket。于是新的 `0.0.0.0` 撞上 EADDRINUSE，bind 失败后
`running` 留在 `None` —— 旧的 `127.0.0.1` 也已经没了，**两头都不在**。而「换绑」正是
勾那个开关触发的唯一动作，症状和操作严丝合缝。

修法：`abort()` 之后 **`await` 那个 handle**。取消后的 await 返回
`JoinError::Cancelled`，这正是「任务已落地、socket 已 drop」的信号。外加 3 次
120 ms 的退让重试，兜住内核回收端口的那个短窗口。

顺带一条同类的：「IP 换错了」和「服务没起来」在手机上表现完全一样（都是连接
被拒绝）。所以设置页不再硬写 `127.0.0.1` 让用户自己找局域网 IP，而是用
`UdpSocket::connect("8.8.8.8:80")` + `local_addr()` 算出来 —— UDP 的 `connect`
只让内核**选一条路由**，不发任何数据包，离线也能用、什么都不外传。

### 12.4 「脚本成功」和「字进去了」是两件事

症状：日志一片绿，用户还是在问「怎么一直没人帮我按继续」。

前三条讲的是**具体的三种**静默失败。这一条讲的是为什么它们能静默这么久：
系统只检查自己**发出去的动作**有没有报错，从不检查**世界有没有变**。
`osascript` 退出码 0 只证明脚本跑完了，不证明那句话落进了那个会话 —— 于是
「授权失效」、「粘到隔壁标签」、「IME 吃掉组合键」这些全部长成同一个样子：
一条成功日志，加一个没动的 agent。

v1.5 先用“记录是否增长”建立闭环；v1.10 又把门槛提高为：只检查 transcript 基线之后
新增的结构化 user message，并要求它与本次 prompt 精确相等。`Landed / Silent / Failed /
Unverifiable / Deferred` 分开记账；transport ACK、mtime 或 assistant 簿记都不能报喜。
**一个不会检查自己有没有生效的守护进程，坏掉的时候和正常的时候长得一模一样。**
以后再加任何「替用户动手」的能力，都必须同时定义 DeliveryConfirmed 证据。

### 12.5 减 86400000 毫秒不等于「前一天」

症状：历史页昨天那组的标题，一年里有两天会从「昨天」退回一个裸日期。

`history.ts` 里判断某一天该叫今天、昨天还是写日期，要先算出「昨天是几号」。
写成减一天的毫秒数在大部分日子里没问题，换季那两天两头都会错，而且**方向相反**：

| 情形 | 例子 | 减 24 小时得到 | 后果 |
|------|------|----------------|------|
| 春天少一小时 | 洛杉矶 2026-03-09 00:30 | 03-07 | **整个 03-08 被跳过**，昨天那组永远匹配不上 |
| 秋天多一小时 | 洛杉矶 2026-11-01 23:30 | 还是 11-01 | 算出来的「昨天」和今天同一天，昨天那组同样匹配不上 |

`setDate(getDate() - 1)` 让运行时按**本地日历**退一格，两种情况都不发生。

真正值钱的是第二层：**这个 bug 在开发机上不可能被发现。** 按整年逐小时扫过
五个时区，`Asia/Shanghai` 的分歧数是 0 —— 中国不实行夏令时，本地怎么测都是绿的。
既有的 11 个历史测试全部在错误实现下通过，因为没有一个落在换季那天。

所以新增的三个测试用 `vi.stubEnv("TZ", "America/Los_Angeles")` **自己钉住时区**，
外加一条守卫断言 `getTimezoneOffset() === 420` —— 万一哪天 TZ 覆盖失效，
另外两条测试会变成真空通过，而守卫会先红。凡是碰日期算术的测试都该这么写：
不钉时区的日期测试，只证明了写测试的人在哪个时区。

### 12.6 CSV 的防注入和可机读是**互斥**的，不能一起要

症状：导出的表格用 Excel 打开一切正常，`pandas.read_csv` 读出来数字全变字符串。

以 `= + - @` 开头的字段会被电子表格当公式执行，标准缓解手段是前面加一个单引号。
问题在于那个引号对电子表格是**标记**，对别的一切都是**数据**——`'-1` 在 pandas
里就是字符串 `"'-1"`。所以一律加前缀会把两类东西同时弄坏：负数不能求和了，
`@scope/pkg` 这种包名多了个引号。

`export/mod.rs` 因此不提供「转义一个字段」这种函数，只提供两个变体：

```rust
pub enum Cell {
    /// 内容不可控（项目名、工作目录、报错原文）→ 该防注入
    Text(String),
    /// 形状已知（数字、时间戳、yes/no、枚举）→ 只做 RFC 4180，保持可求和
    Value(String),
}
```

选哪个是**建表的人**在建表的时候决定的，不是转义函数猜的——因为只有调用点知道
这一列的内容从哪来。同一份代码里 `project` 是 `Text`，`cost` 是 `Value`。

另外三条 RFC 4180 的细节，缺一条就有一类用户打不开文件：行尾必须是 `\r\n`
（Windows Excel 双击只认这个）；含 `, " \n \r` 的字段要包引号且内部引号翻倍；
文件头必须写 UTF-8 BOM，否则简体中文 Windows 的 Excel 按 GBK 解，中文项目名全成乱码。

**表头跟界面语言走，所以表头不是稳定的机器接口。** 这是明知的取舍：混着中英文
的表头正是用户点名反感的东西，所以选了跟随 `config.language`；代价是脚本必须
按列的位置读，不能按表头的名字读，这一条写在 `i18n/mod.rs` 的 `csv.*` 段落上方。

### 12.7 拿数组长度当游标，遇上定长环就等于没有游标

症状：挂机几个钟头回来，活动日志停在某一行不动了。重启一下又好了。

推送泵每 800ms 比一次「现在有多少条」和「上次推到多少条」，不一样就把多出来的
发给前端。而 `state.events` 是个定长环，满 `EVENT_RING_CAP`（500）就裁掉最老的。
两件事各自都对，凑在一起就坏了：**长度到 500 之后再也不变**，于是
`total == last_len` 这个「没有新事件」的判断从此**永远成立**，后端还在记，
界面上再也不出现新的一行。

```rust
// 坏的：len() 是「还留着多少」，不是「推过多少」
let mut last_len = 0usize;
let total = state.events.len();
if total == last_len { continue; }   // 满环之后恒真
```

修法是给状态加一个**只增不减**的 `events_pushed`，游标比这个数。两个计数的分工
就是全部要点：`events_pushed` 跟着推过多少走，`events.len()` 跟着留着多少走
——一旦被写成同一个数，泵就又瞎了。切片长度还要**封顶在环长**：落后超过 500 条时
差值比环里现有的还大，硬按差值切会越界 panic，这种情况下丢掉最老的几条
（日志面板本来只看最近的）比崩掉好。

三条推论，按值钱程度排：

**① 定长缓冲的游标必须是单调量。** 环、`VecDeque` 上限、`drain` 到 N ——
凡是会把旧数据丢掉的容器，它的 `len()` 就不能用来回答「有没有新东西」。
这条 bug 潜伏到 500 条之后才发作，所以短时间的手工测试和 CI 全都是绿的：
攒够 500 条要几个钟头，而**几个钟头无人看管正是这个产品存在的理由**。

**② 同一件事写了两遍，就会有一遍忘了改。** 修的时候发现推送和裁剪存在两份实现
（引擎上一份、另一处 `push_event_public` 又抄了一份，各自硬编码 `500`）。
只有一份加了计数，另一份没加——它会让游标和实际推送量对不上，症状和原 bug 一样。
两份都收进 `MonitorState::push_event` 这个唯一入口了。

**③ 这是 §12.4 的同一个毛病，换了个身体。** 那条讲「不检查自己有没有生效的守护
进程，坏掉时和正常时长得一模一样」；这条是「不报告自己已经饱和的缓冲」。
静默停更没有任何错误码——没有 panic、没有 warn，前端也不知道自己该收到东西。
凡是有上限的东西都得能说出自己撞上了上限，这条在 v1.7 的导出上限
（`hit_cap` → 琥珀色提示）上刚花过一次代价，这里是第二次。

### 12.8 只读尾部 40 行，意味着一个正确的判定会自己过期

症状：撞上限流，日志里确实出现过「限流，不催」，但过一会儿应用**又开始敲字了**，
而限流窗口根本没过去。

`error_output()` 读的是记录尾部 40 行，而 agent 撞上限流后不会停下——它会继续写
重试日志。那行 `429` 被后面的输出顶出这 40 行之后，判定就再也看不见它，原因从
`RateLimited` 掉回 `Stalled`，手段跟着从 `Wait` 变回 `Nudge`。策略表从头到尾都是
对的，**是证据先过期了**。

这条的普适形式，比限流本身值钱：**从一个滑动窗口里读证据，得到的结论寿命不会超过
那个窗口。** 只要「该不该动手」依赖的事实会滚出视野，就必须把结论本身存下来，
而不是每轮重新推导。v1.8 存的是 `RateLimitHold`（截止时刻 + 当轮原因），挂在
`AgentSession` 上跨轮合并。

三条推论：

**① 存原因，不存布尔。** 只存「按住了」的话，用户看到的是一段没有解释的沉默；
存下原来的原因，日志才能说出是「限流」还是「上游拒绝」。

**② 一个存坏的截止时刻必须解释成「没有窗口」。** 这是全项目唯一一处**故意不选
保守侧**的地方：把解析不了的时间戳当成「一直按住」，会让某个会话再也不被续跑，
而且没有任何出口——永久且静默，比多敲一次糟得多。

**③ 存下来的结论必须有一条比时刻更硬的放手条件。** 只按时刻放手的第一版有个洞：
会话恢复干活之后，界面会在剩下的窗口里同时说「运行中」和「撞上限流，不敲字」。
因为合并回写那段不分判定结果都写 `interrupt_reason`，而窗口还在就照原样返回。
结论存下来了，就得同时想清楚**什么事实能让它提前作废**——否则一个估算出来的
时刻会盖住后来出现的真事实。

**④ 这类 bug 单元测试抓不到。** 时刻比较、形状识别、放手条件都能测（68 个测试），
但「日志一直在长、证据滚走了、它还按着」需要一个真实增长的记录文件，只能实机验
（`docs/manual-test.md` 第 12 节）。

## 13. 测试与门禁

本地和 CI 跑的是同五道门：

| 门 | 命令 | 现状 |
|----|------|------|
| Rust lint | `cargo clippy --all-targets -- -D warnings` | 干净 |
| Rust 单测 | `cargo test` | 325 passed（本轮 macOS 门禁）；Windows CI 另执行 PowerShell helper 的真实编译与 Win32 结构布局测试 |
| 前端单测 | `pnpm test`（vitest） | 103 passed（9 files） |
| 类型检查 | `npx tsc --noEmit` | 干净 |
| 前端构建 | `pnpm build` | 通过 |

CI（`.github/workflows/ci.yml`）的两个关键决定：

- **`check-rust` 三个平台都跑。** 投递层几乎全是 `#[cfg(target_os = ...)]`，只在
  ubuntu 上跑 clippy 等于 Windows 和 macOS 分支从来没被编译过 —— 上一版就是这么
  让一个 Windows 专属的编译错误躺到打标签才暴露的。`--all-targets` 才会把
  `#[cfg(test)]` 编进来。
- **`pnpm test` 是独立一步。** `pnpm build` 只保证编得过，不保证 store 的归约和
  显示映射没被改坏；测试挂了要能一眼看出是测试挂了。

`build-tauri` 和 `Create Release` 由 `refs/tags/v*` 触发，四目标矩阵：
macOS arm64 / macOS x64 / Linux x64 / Windows x64。

### 变异检查：新测试的验收标准，不在 CI 里

「加了 8 个测试」本身不说明任何事——一个从来不会红的测试和没有测试是一回事。
所以这个项目对新测试的验收标准是**改坏实现，确认它会红**。

现在有两个脚本把这件事固化下来，都是逐个把实现改坏、跑测试、记下谁红了、恢复；
跑一次就能回答「这些测试里有没有摆设」：

| 脚本 | 变异体 | 覆盖 |
|---|---:|---|
| `scripts/mutation-check-event-pump.sh` | 8 | 事件推送泵的游标与裁剪（见 12.7） |
| `scripts/mutation-check-rate-limit.sh` | 19 | 限流形状识别、等待时间解析（见 12.8） |
| `scripts/mutation-check-hold-release.sh` | 5 | 保持窗口的放手条件（见 12.8） |

**故意不进 CI。** 它要反复编译整个 crate，比五道门加起来还慢，而它验的是
*测试本身写得好不好*——那是写测试那次就该验完的事，不是每次 push 都要重跑的。
留成手动脚本，改动那块逻辑时跑一次。

两条从这个脚本里学到的：

- **有些变异体应该活下来，要明确标出来。** `EVENT_RING_CAP` 从 500 改成 499
  不会有任何测试红，这是对的：那个数是调参不是正确性约束，测试全都拿常量本身
  断言而不抄字面量。为了「杀死」它去写 `assert_eq!(cap, 500)` 等于把调参钉死。
  脚本里这类标成 `[等价]` 并反过来判——不区分的话，「有变异体活着」这个信号
  迟早被当成噪音。
- **改坏源码的脚本必须用 `trap` 恢复。** 第一版把恢复写在循环末尾，脚本中途
  崩了一次，仓库里就留下一个故意改坏的实现。`trap ... EXIT INT TERM` 对崩溃、
  Ctrl-C、被 kill 一样有效。

`scripts/mutation-check-rate-limit.sh`（v1.8，19 个变异体）是第二次，它当场逼出两个
真问题，都在刚写完、且已经有 243 个绿灯盖着的代码里：

- 一条**永远命中不了的关键词**：`throttl` 当词干写，而 `contains_keyword` 要求
  ASCII 词边界，所以它连 `throttled` 都匹配不上（见 5 章）。
- 一条**假绿的测试**：中文限流说法根本没有测试覆盖，改坏它没人红。

还学到第三条：**变异检查脚本自己也会假绿，而且有两种假法，都要单独报出来。**

- *变异没生效*：sed / perl 表达式跟真实源码对不上，改了个空气然后报「测试通过」。
  第一版 `mutation-check-rate-limit.sh` 有 8 条是照记忆写的，全中这个。
- *变异生效了但判定瞎了*：`mutation-check-hold-release.sh` 第一版按
  `^ *[a-z_]+ ` 去抓 cargo 的失败清单，而那清单是 `    detector::tests::xxx`
  ——带 `::`、行尾没空格，一条都匹配不上。五个变异体全被报成「没人红」，
  手动跑同一个变异体那两条测试确确实实红了。

两种假法对应的是两种断言强度，各有代价：前两个脚本只看 `cargo test` 的退出码，
结构上就不可能犯第二种错，但它也只能说「有人红了」，说不出是谁红的——
一个变异体碰巧让**别的**测试红了也算通过。`hold-release` 那个要求指名道姓
（「期望 `an_unsure_verdict_keeps_holding` 红」），断言更强，代价就是多了一处
能自己坏掉的解析。选后者时必须补一道：测试确实失败、却一个名字都没解析出来，
要报「脚本坏了」而不是安静地当成「没人红」。

共同的那一道谁都不能少：**替换到底有没有生效。** 没生效就报「这条检查是假的」，
不然它长得跟绿灯一模一样。

### cfg 纪律（红过两次）

私有项的 `#[cfg]` 必须和调用点**严格对齐** —— 多挂一个平台，那个平台的
`-D warnings` 就会因 dead_code 变红（`outcome_text`、`resumer_with` 各红过一次）。
`pub` 项豁免 dead_code，所以跨平台的**脚本生成器和纯解析函数刻意留成 `pub` 且
不加 cfg**，这样每个平台的 CI 都会编译并测试它们。

### 时钟纪律：两次 `now()` 之间可以跨过整秒

碰时间的断言不能写等号。`a_just_touched_session_is_a_number_not_unknown` 就是这么
红的：测试把「现在」格式化成秒级字符串，`stuck_secs()` 又重新取一次 `Local::now()`，
两次调用之间跨过一个整秒，差值就从 0 变成 1。

窗口有多窄：在本机紧循环跑 28 万次，只有 2 次拿到 1（约 1/14 万）。所以它在开发机上
过了几十次，然后在 Windows runner 上红了一次——CI 机器慢，那两次调用之间的间隔更宽。
**这不是「偶发抖动」，是断言写错了**：被测的性质是「给得出一个数，而不是说不知道」
（`Some` vs `None`），0 还是 1 从来不是重点。

所以：凡是拿两次时钟读数做差的断言，写成范围（`matches!(got, Some(0..=1))`）或
带余量（旁边 `stuck_secs_reads_the_transcript_mtime` 用的 `595..=605`），
并且**必须保留它真正要守的那条区别**——上面那条改完之后仍然会因为 `None` 变红，
变异验证过。

这条和 §12.5「不钉时区的日期测试只证明了写测试的人在哪个时区」是同一类：
时间相关的测试在开发机上全绿，只说明开发机的时钟和时区恰好合适。

## 14. 已知边界（不要当成已验证）

- 三平台真实续跑仍没有本轮可引用的正式真机验收记录；代码、脚本语法与单元测试通过
  不等于字符一定落进了正确终端。所有平台继续按 `docs/manual-test.md` 保持未勾选。
- 当前自动后台能力只承诺 tmux exact pane 与 iTerm2 exact TTY，且都要求 transcript/协议核验；
  Terminal.app、Linux 普通终端、外部 Windows cmd/conhost、Windows Terminal/ConPTY、IDE 集成终端、
  screen 非精确 window 自动延后，只有手动才允许前台降级。
- Windows 外部 console 不能按 PID 精确寻址；必须先确认自动路径安全延后，再在真实
  `cmd.exe + Codex CLI` 上复测用户手动点击后的 Unicode `SendInput`：完整文本、独立 Enter、
  无 PowerShell 弹窗、无剪贴板/图片粘贴错误、不串线，并在 transcript 基线之后看到本次精确 prompt。
- tmux/screen 通道有单元测试，但**没有对着真实 tmux pane / screen window 走完本轮验收**。
- v1.9 的队列合并、生命周期失效、RAII 租约、进程代际复核和并发状态归约都有单测，
  但**真实多会话排队投递**仍需手工验收：特别是“6 秒 × N 不阻塞扫描”、stop/start 取消、
  手动与自动同刻触发、PID 复用这几条只能在真实桌面环境确认端到端行为。
- 精确 prompt 核验的 6 秒窗口仍是**推理出来的，不是测出来的**：它要求 Agent 在窗口内把本次
  user prompt 持久化到 transcript。真机若写盘更慢会被判为 `Silent`；不会误报成功，但窗口应由
  真实投递延迟分布校准。
- 手机看板的修复是推理 + 单元测试（换绑同端口能成功），**没有用真手机端到端复现过**。
- AI 判断（`ai_judge`）走的是 OpenAI 兼容端点，刻意保持中立、不绑某一家。v1.6 起接进了
  自动回路，但只在「关键词命中、记录仍增长、结构证据用尽」这一处提问（`monitor/mod.rs`
  的 `wants_second_opinion`），默认关闭，且授权是单向的：`CONTINUE` 能把可疑升为确认，
  `DONE` 或请求失败都不撤销已有结论。**这条路会把记录尾部发到用户自己配的端点**，
  所以它必须由用户显式开启。
- 环境自检目前长在 `collect_tools` 里，**没有覆盖会话目录可读性**，也还没有独立面板。
- 导出的 CSV 有 18 个单元测试盯着转义边界，但**没有真的用 Excel / Numbers /
  pandas 各打开过一次**。BOM 和 `\r\n` 这两条是照标准写的，不是实测出来的。
- 前端测试只到纯函数层（工具函数、显示映射、store 归约、历史分组、趋势、
  图表刻度、会话搜索筛选、版本一致性和窄屏静态契约，共 103 个），**组件渲染层仍没有真实 DOM 测试**。
- **供应商身份在代码里不存在**，是刻意的：读运行中进程的 environ 能拿到 `base_url`，
  但同一个 block 里就是 `ANTHROPIC_AUTH_TOKEN`，等于让本应用具备读密钥的能力。
  v1.8 改成不靠身份也能兜住：关键词全落空时看 HTTP 形状和中转站说法
  （`detector::rate_limit::upstream_rejection`），认出来就进保持窗口。仍未做的是
  按供应商 profile 调三个旋钮（冷却下限、撞第 N 次就只叫人、是否信任消息里的 reset
  时间），未识别的一律落最严格那档。
- **保持窗口只活在内存里**（`AgentSession::rate_limit_hold`，逐轮合并）。重启应用会丢，
  再扫到那行 `429` 才会重新进入——这是刻意的取舍：落盘意味着一个存坏的截止时刻能让某个
  会话永久静默。同理，解析不了的时间戳一律当「没有窗口」，宁可多敲一次也不要永久沉默。
- 保持窗口的截止时刻存的是**朴素本地时间字符串**（`%Y-%m-%d %H:%M:%S`，没有时区），
  因为这个值要原样进日志给人看。代价是夏令时切换那一小时里它是有歧义的（秋天
  01:30 会出现两次）。**没有改成 UTC 是权衡后的决定，不是没想到**：窗口上限一小时、
  一年只有两次切换，撞上的前提是窗口正好跨过那一刻；后果最多是多按一小时或早放一次，
  都不会往限流窗口里敲字。真要修就得把存储和展示拆成两个值，而这个功能刚落地，
  为 0.02% 的场景动它的形状不划算。12.5 那条夏令时的账已经付过一次，这里是明知故犯。
- 保持窗口、判定与限流形状合计有 68 个 `detector` 模块单元测试
  盯着，**但「证据被顶出 40 行之后它还按着」这条只有实机能验**，见 `docs/manual-test.md`
  第 12 节；那一节还没走过。

## 15. 版本轨迹

| 版本 | 内容 |
|------|------|
| v0.1 – v1.0 ✅ | 核心引擎、三平台续跑、托盘常驻、SQLite 统计、Webhook、AI 判断、i18n |
| v1.1 感知层 ✅ | `TurnState`、`error_output` 双通道、词边界匹配、注意力分级 |
| v1.2 洞察层 ✅ | token 计价、限流预测、项目排行、会话历史时间线 |
| v1.3 远程层 ✅ | 只读手机看板（令牌鉴权、默认 loopback） |
| v1.4 可信化 ✅ | tmux/screen 通道、续跑演练、前端 vitest、三平台验收清单 |
| v1.5 闭环 ✅ | **续跑落地核验**（`ResumeOutcome` + 指纹比对）、三个计数器分家、放弃动手时改为出声而非静默、看板换绑竞态修复、局域网地址自动推导、强令牌生成、判定层与动作闸门彻底分离 |
| v1.6 可解释判定 ✅ | `InterruptReason` / `ResumeTactic` 单一策略源、`DetectionEvidence` 判据面板、结构化 AI 第二意见（单向授权）、自定义适配器 UI、跨语言枚举与 i18n 门禁 |
| v1.7 记录与导出 ✅ | 会话生命周期收拢（关掉的会话不再显示「运行中」）、续跑记录中心、统计趋势对比、会话档案抽屉、图表补时间刻度、**CSV 导出**（`Text` / `Value` 双变体转义）、跨夏令时的日期分组 |
| v1.8 限流保持 ✅ | 关键词落空后的兜底形状识别、从消息里抠等待时间（中英）、新原因 `UpstreamRejected`、**保持窗口**（证据被顶出 40 行之后仍然不敲字）、四族枚举 i18n 门禁、变异检查脚本 |
| v1.9 续跑协调器 ✅ | 扫描/投递解耦、按会话合并队列、常驻 worker、RAII 会话租约、stop 生命周期代数、出队全量重验、并发状态归约、PID + 启动代际身份；首次三步引导与多会话搜索筛选 |
| v1.10 两阶段续跑流水线（代码层 ✅，真机待验） | 不可逆桌面投递严格串行、跨会话只读核验并行、忙会话绕行避免队头阻塞、owned lease 覆盖完整闭环、Rust pipeline 快照与前端状态可视化；360px 与 Windows 手动前台路径仍按验收清单执行 |
| v2.0 编排层 ⛔ | **与「非侵入」定位冲突，已搁置** —— 不经确认不动工 |
| v2.1+ 自治层 ⛔ | 同上 |

