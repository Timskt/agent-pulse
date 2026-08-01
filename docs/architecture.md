# AgentPulse 架构设计

> 这份文档回答的是「**为什么这样拆**」和「**一次续跑到底经过了哪些判断**」。
> 配置项逐条说明、路线图、开发红线在 [PROJECT_STATUS.md](../PROJECT_STATUS.md)；
> 三平台手动验收清单在 [manual-test.md](./manual-test.md)。
>
> 最后一次与代码对齐：2026-08-01（`v1.5` 已交付）。

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
                            │ Tauri IPC：25 个命令 + 3 个事件
┌───────────────────────────▼──────────────────────────────────┐
│                      Rust 核心                                │
│                                                              │
│  ① 感知层  adapters/    进程表 → 会话；记录文件 → 输出/回合状态 │
│  ② 判定层  detector/    两条正交的轴：要不要续跑 / 要不要叫人   │
│  ③ 投递层  resumer/     复用器 → GUI 终端，「定位不到就不敲」    │
│  ④ 洞察层  cost/ storage/  token 计价、限流预测、SQLite 时间线  │
│  ⑤ 远程层  remote/      只读手机看板（默认关，令牌鉴权）        │
│                                                              │
│  横切：monitor/ 调度  notify/ 提醒  webhook/ 外推  i18n/ 文案   │
└──────────────────────────────────────────────────────────────┘
```

## 3. 目录地图

| 路径 | 行数 | 职责 |
|------|-----:|------|
| `src-tauri/src/resumer/mod.rs` | 3189 | 投递层全部：通道选择、三平台脚本、演练、落地核验、聚焦 |
| `src-tauri/src/monitor/mod.rs` | 1292 | 扫描循环、状态合并、**动作闸门**、事件环 |
| `src-tauri/src/adapters/` | 1024 | 进程快照 + Claude Code / Codex / OpenCode / 自定义 |
| `src-tauri/src/detector/mod.rs` | 883 | 信号融合、注意力分级、词边界匹配 |
| `src-tauri/src/remote/mod.rs` | 870 | 手写 HTTP 服务 + 内嵌看板页 |
| `src-tauri/src/storage/mod.rs` | 651 | SQLite 六张表：续跑/检测/日聚合/用量/游标/会话历史 |
| `src-tauri/src/lib.rs` | 627 | Tauri 装配：命令、托盘、事件泵、窗口行为 |
| `src-tauri/src/i18n/mod.rs` | 531 | 后端文案（通知、托盘、日志、报错） |
| `src-tauri/src/cost/mod.rs` | 512 | 模型价目表、增量读用量、限流预测 |
| `src/components/ConfigPanel.tsx` | 867 | 设置页十个分区（**待拆**） |
| `src/i18n/index.ts` | 520 | 前端文案（界面上的字） |

## 4. ① 感知层：会话是怎么被认出来的

`adapters::take_process_snapshot()` 每轮扫描**只枚举一次**系统进程，然后把同一份
快照交给所有适配器 —— 四个适配器各自遍历系统进程表的写法在会话多的机器上是
可观测的浪费。

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

| 信号 `SignalKind` | 触发条件 | 强度 |
|---|---|---|
| `ProcessExited` | 进程不在快照里 | 强（单独即可确认） |
| `KeywordMatch` | `recent_output` 命中中断关键词 | 强 |
| `HeartbeatTimeout` | `last_activity` 超过 `阈值 × 超时` | 强 |
| `FileStale` | 记录文件超过 `idle_timeout` 未更新 | 弱（需组合） |

```
命中完成标记                    → TaskCompleted（永不续跑）
进程已退出 且 无完成标记        → ConfirmInterrupt
有强信号 或 信号数 ≥ 2          → ConfirmInterrupt
只有弱信号                      → Suspicious（再看一轮）
没有信号                        → Running
```

### 两个救过命的细节

**忙时宽限（`BUSY_GRACE_MULTIPLIER = 10`）**：`TurnState::is_busy()` 为真时，
「多久不写文件算卡住」放大 10 倍（默认 60s → 600s）。这条专门用来躲开长时间的
上下文压缩 —— 但**只放大 `FileStale` 这类时间信号**，不会让一个真正停在
`AwaitingUser` 的会话不再产生续跑。两个方向的失败都被用户实际报过，所以两边
都不能松：压缩不能被判成卡住，真卡住也不能不管。

**词边界匹配（`contains_keyword`）**：裸 `contains` 会让 `"500"` 命中
`"1500 tokens"`、`"429"` 命中 `"14290"` —— 一句正常的用量统计就能把会话判成
服务器错误。规则是关键词首尾为 ASCII 单词字符时要求那一侧不紧贴另一个单词字符，
并特意放宽两处：`(y/n)` 这类符号打头结尾的不做要求；**含中文的关键词完全跳过
边界判断**，因为中文不用空格分词，`是否继续` 前后紧跟汉字是常态。

## 6. ③ 投递层：那串字符到底怎么落进终端的

整层的设计原则只有一句：**定位不到就不敲。**「没续跑」用户还能自己补一句，
「敲进别人的代码」是不可撤销的。

### 通道优先级

按确定性从高到低排，认得出哪条就用哪条：

| 通道 | 寻址方式 | 需要前台窗口 | 需要系统授权 | 过输入法 |
|------|----------|:---:|:---:|:---:|
| **tmux** | `send-keys -t %3 -l --`（pane id） | 否 | 否 | 否 |
| iTerm2 | `write text` 直接写 pty | 是 | 否 | 否 |
| Terminal / VS Code / Cursor / Warp | AppleScript + TTY 匹配 | 是 | **辅助功能** | 是 |
| Windows 控制台 / Terminal | PowerShell + `SetForegroundWindow` + SendKeys | 是 | 否 | 是 |
| Linux X11 / Wayland | `xdotool` / `ydotool` | 是 | 否 | 是 |
| screen | `-X stuff`（只到 session 当前选中的 window） | 否 | 否 | 否 |

**tmux 是唯一一条不需要任何系统授权、也不需要窗口在前台的路。** 按 pane id
寻址，参数走 argv 数组所以没有 shell 插值，还绕开了输入法。如果续跑总是不稳，
最有效的一步是把 agent 跑在 tmux 里。

`screen -X stuff` 只能投给该 session **当前选中**的 window，属于窗口级不确定，
所以和其它盲敲路径同一个门槛：必须显式打开 `auto_follow_latest`。

### 非 ASCII 必须走剪贴板

合成按键要过输入法。中文提示词用 `keystroke` 敲出来，拼音输入法会把它变成
一串「啊啊啊啊」（用户实测报过）。所以：非 ASCII 提示词先写进剪贴板，再合成
**一个 ASCII 粘贴组合键**（⌘V / `^v` / Ctrl+Shift+V）。iTerm2 走 `write text`
直写 pty，不经过按键合成，因此豁免。

自由文本**永不拼进 shell 字符串**：Linux 走 stdin 喂剪贴板，Windows 用单引号
PS 字面量，tmux/screen 用 argv 数组。

### 演练（`probe_resume`）

同一套定位流程走完，**一个字都不敲**，返回 `ResumeProbe`：确定性等级、通道、
具体目标（pane id / tty / 窗口标题）、一段解释、`would_deliver`、以及
`needs_permission_fix`（macOS 缺辅助功能授权时前端据此给「去开权限」按钮 ——
用布尔而不是让前端匹配工具名，否则换成英文界面立刻失灵）。

结论同时写一条活动日志。用户报「续跑没反应」时截的往往就是那面板，有这一行
就不用再问「你演练过吗、结果是什么」。

### 落地核验（`resume_verified`）——投递不是终点

`Resumer::resume` 返回 `Ok` 的真实含义只是「AppleScript / PowerShell / xdotool
没报错」，不是「那句话进了那个会话」。焦点被抢、粘进隔壁标签、IME 拦住组合键、
pane 刚被关掉、授权中途失效 —— 每一种都让脚本成功而字没进去。**只发动作不看世界
有没有变，系统就永远不知道自己坏了**，只能等用户来报。

核验用的信号本来就在磁盘上：agent 真动起来就会往自己的会话记录里写东西。
投递前记一次 `transcript_fingerprint`（会话文件的 `(长度, mtime)`），之后每
`VERIFY_POLL_MS = 300` 毫秒复查一次，最多等 `VERIFY_WINDOW_SECS = 6` 秒 ——
一看到长出新内容就立刻收工，不必等满：

| `ResumeOutcome` | 含义 | 计入 |
|---|---|---|
| `Failed` | 通道自己报错，字肯定没出去 | `resume_failures`（通道健康）|
| `Landed` | 记录长了 → 它动了 | 成功，清零失败计数 |
| `Silent` | 脚本成功但记录一动不动 → 没落地 | `resume_failures` |
| `Unverifiable` | 这个 agent 没有可核验的记录文件 | **刻意不算失败** |

`Unverifiable` 不算失败是个明确取舍：没有证据不等于有反证，把它记成失败会让
没有 transcript 的适配器永远显示「敲不进去」。

三个计数器因此各管一件事，不能再共用一个：`resume_count` 是终身累计（只用于显示）、
`resume_streak` 是连续没效果的次数（对 `max_resume_count`，`Verdict::Running` 时清零）、
`resume_failures` 是通道健康（驱动 `effective_cooldown` 的线性退避与升级告警）。
其中最要紧的一条：**敲不进去不算「催过了」**（`failed_deliveries_never_exhaust_the_budget`）
—— 否则一个坏掉的通道会在 5 轮里静静烧完额度，然后彻底沉默。

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

### 命令（前端 → Rust，共 25 个）

| 分组 | 命令 |
|------|------|
| 引擎 | `get_state` `get_status` `start_monitoring` `stop_monitoring` `scan_now` |
| 配置 | `get_config` `update_config` `get_platform_info` `get_translations` |
| 续跑 | `manual_resume` `probe_resume` `focus_terminal` `open_accessibility_settings` |
| 统计 | `get_stats` `get_resume_history` `get_totals` `get_session_history` |
| 花费 | `get_cost_daily` `get_cost_projects` `get_rate_forecast` |
| 提醒 | `test_notify` `test_webhook` `ai_analyze` |
| 看板 | `get_lan_ip` `generate_remote_token` |

### 事件（Rust → 前端，共 3 个）

| 事件 | 载荷 | 说明 |
|------|------|------|
| `engine-events` | `Vec<EngineEvent>` | 增量日志，800 ms 一批。事件泵**常驻在 `setup` 里**，不挂在 `start_monitoring` 命令上 —— 否则从托盘启动监控时前端一条日志都收不到 |
| `engine-stopped` | `()` | 引擎停了 |
| `attention-alert` | `AttentionAlert` | 该叫人了；前端据此响声、高亮会话、立刻补一次状态 |

前端的兜底轮询是**自适应**的，不是固定 3 秒：守护中取扫描周期的一半（夹在
2–8 秒），没在守护降到 10 秒，**窗口不可见时整轮跳过**，切回来立刻补一次。
原来那个无条件 `setInterval(3000)` 在窗口收进托盘时照样每 3 秒敲一次后端。

## 11. 一次自动续跑的完整路径

```
每轮扫描（tokio interval）
  take_process_snapshot()                       ← 全局只枚举一次进程
  ↓
  各适配器 discover_sessions(&snapshot)
  ↓
  与上一轮状态合并（保留三个计数器、首次见到时间）
  ↓
  逐会话：recent_output / error_output / turn_state
  ↓
  Detector::detect → Verdict + AttentionLevel + 命中的关键词/标记
  ↓                  （只回答「是什么」，不碰额度也不碰冷却）
  ├─ AttentionLevel 需要叫人 → Notifier（系统通知 + 声音 + 托盘角标）
  │                            → Webhook（Slack / Discord / ntfy / Bark）
  │
  └─ 动作闸门（monitor，判定层之外）
       Verdict == ConfirmInterrupt
       且 auto_resume_enabled
       且 has_nudges_left：resume_streak < max_resume_count   ← 连续无效次数，不是终身次数
       且 now - last_resume ≥ effective_cooldown              ← 随 resume_failures 线性退避
       ↓
       选提示词（命中 goal 关键词 → 目标恢复提示词，否则通用）
       ↓
       Resumer::resume_verified
         ⓵ 投递前记一次 transcript_fingerprint（长度, mtime）
         ⓶ Resumer::resume
              a. 认得出 tmux/screen？→ 复用器投递（最高确定性）
              b. 否则 GUI 定位：TTY → 终端应用 → 窗口/标签
                   定位不到 且 未开 auto_follow_latest → 放弃，写 blind_refused
                   macOS 先查辅助功能授权，没有就**不跳窗口**，直接说去哪儿点
              c. 非 ASCII → 剪贴板 + 一个 ASCII 粘贴键
         ⓷ 每 300ms 复查指纹，最多 6 秒 → ResumeOutcome
              Failed / Silent → resume_failures += 1，退避加长，连续失败升级告警
              Landed          → resume_streak = 0，resume_failures = 0
              Unverifiable    → 只累加 resume_count，不判成失败
       ↓
       storage.record_resume(...)  → success 存核验结果，不是脚本退出码
       push_event(...)             → 活动日志（前端 800ms 内看到）
```

**这张图里唯一不能挪的一段是「判定 → 闸门」那条横线。** 判定只说状态，
额度、冷却、总开关全在闸门里；把任何一条塞回 `make_verdict`，催满的会话就会被
钉在 `Suspicious`，于是既不敲也不叫人（见 §5）。

## 12. 四个花了很大代价才弄明白的事实

这一节是这份文档最该被读的部分。四个问题都表现为「功能好像没生效」，而且都
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

现在的处理：动手之前用 `osascript -e 'tell application "System Events" to return
UI elements enabled'` 查一次（只读、不弹窗、不会把用户拽进设置面板），没授权就
**不跳窗口**，直接返回一句「去哪儿点」，并在演练面板上给「去开权限」按钮。

**要根治得做正式签名**（Developer ID + notarize），那时授权才跟着 bundle id 走。

### 12.2 tmux 是唯一一条不需要授权的路

上面那条的直接推论：只要 agent 跑在 tmux 里，`send-keys -t %3 -l --` 按 pane id
寻址，**不需要辅助功能授权、不需要窗口在前台、不过输入法、没有 shell 插值**。
授权问题、IDE 内置终端定位不准、中文变「啊啊啊」这三类麻烦一次性全绕开。

所以「续跑总是不稳怎么办」的第一建议不是调参数，是把 agent 跑进 tmux。

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

所以 v1.5 把投递改成闭环（见 §6「落地核验」）：投递后回头看 agent 自己的记录
文件有没有长出新内容，`Landed / Silent / Failed / Unverifiable` 四态分开记账。
**一个不会检查自己有没有生效的守护进程，坏掉的时候和正常的时候长得一模一样。**
这条推论比这三个具体的坑更值钱：以后再加任何「替用户动手」的能力，都要连着
它的核验信号一起加，否则等于又造一个只会报喜的通道。

## 13. 测试与门禁

本地和 CI 跑的是同五道门：

| 门 | 命令 | 现状 |
|----|------|------|
| Rust lint | `cargo clippy --all-targets -- -D warnings` | 干净 |
| Rust 单测 | `cargo test` | 115 passed |
| 前端单测 | `pnpm test`（vitest） | 32 passed |
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

### cfg 纪律（红过两次）

私有项的 `#[cfg]` 必须和调用点**严格对齐** —— 多挂一个平台，那个平台的
`-D warnings` 就会因 dead_code 变红（`outcome_text`、`resumer_with` 各红过一次）。
`pub` 项豁免 dead_code，所以跨平台的**脚本生成器和纯解析函数刻意留成 `pub` 且
不加 cfg**，这样每个平台的 CI 都会编译并测试它们。

## 14. 已知边界（不要当成已验证）

- 剪贴板粘贴那条续跑路径只在 **macOS + Terminal.app** 上真机验证过；Windows 和
  Linux 依赖 CI 编译 + 不带 cfg 的单元测试，**没有真机跑过**。
- tmux/screen 通道编译通过、有 4 个单元测试，**没有对着真实 tmux pane 试过**。
- 落地核验的 6 秒窗口是**推理出来的，不是测出来的**：它假设 agent 收到输入后 6 秒内
  会往记录里写点什么。真机上如果某个 agent 反应更慢，核验会把 `Landed` 误判成
  `Silent` —— 后果是冷却变长、多一条失败日志，不会误伤额度（`Silent` 也不算「催过了」），
  但这个数值该由真机数据定，不该由我拍。
- 手机看板的修复是推理 + 单元测试（换绑同端口能成功），**没有用真手机端到端复现过**。
- AI 判断（`ai_judge`）走的是 OpenAI 兼容端点，刻意保持中立、不绑某一家；目前**只由
  用户手点触发**，没有接进自动回路。
- 环境自检目前长在 `collect_tools` 里，**没有覆盖会话目录可读性**，也还没有独立面板。
- 判定证据面板（把每条信号的具体值、`TurnState`、忙时宽限有没有生效、命中哪个
  关键词摊开给用户看）**还没做**。
- 前端测试只到纯函数层（工具函数、显示映射、store 归约、版本一致性，共 32 个），
  **组件渲染层没有任何测试**。

## 15. 版本轨迹

| 版本 | 内容 |
|------|------|
| v0.1 – v1.0 ✅ | 核心引擎、三平台续跑、托盘常驻、SQLite 统计、Webhook、AI 判断、i18n |
| v1.1 感知层 ✅ | `TurnState`、`error_output` 双通道、词边界匹配、注意力分级 |
| v1.2 洞察层 ✅ | token 计价、限流预测、项目排行、会话历史时间线 |
| v1.3 远程层 ✅ | 只读手机看板（令牌鉴权、默认 loopback） |
| v1.4 可信化 ✅ | tmux/screen 通道、续跑演练、前端 vitest、三平台验收清单 |
| v1.5 闭环 ✅ | **续跑落地核验**（`ResumeOutcome` + 指纹比对）、三个计数器分家、放弃动手时改为出声而非静默、看板换绑竞态修复、局域网地址自动推导、强令牌生成、判定层与动作闸门彻底分离 |
| v2.0 编排层 ⛔ | **与「非侵入」定位冲突，已搁置** —— 不经确认不动工 |
| v2.1+ 自治层 ⛔ | 同上 |

