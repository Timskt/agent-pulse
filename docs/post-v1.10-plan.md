# v1.10 之后的续跑核心计划

> 日期：2026-08-07
> 状态：v1.10 文档收尾；以下项目除“真机验收”外均为后续计划
> 当前边界：自动续跑只允许“精确目标 + 后台通道 + transcript/协议可验证”；没有安全通道时延后，不抢焦点

这份计划只围绕 AgentPulse 的核心竞争力：**在不接管 Agent 的前提下，尽可能准确、静默、可验证地续跑。**
前端扩展、工作流编排和自治决策不进入近期核心优先级。

## 1. v1.10 已建立的基线

后续重构不得破坏以下约束：

1. Rust 是检测事实、动作许可、协调状态和记账结果的唯一事实来源；前端只展示快照。
2. 自动策略固定为 `BackgroundOnly`：目标必须是 `Exact`，通道必须是 `Background`，并且必须存在 transcript 或协议级核验。
3. 自动模式没有安全后台通道时返回 `deferred/no-safe-transport`；延后既不算成功，也不算失败，不得自动降级为前台键盘注入。
4. 只有用户明确点击手动续跑时，调用方才可使用 `AllowForeground`，并继续遵守目标定位和全局输入锁。
5. transport ACK 只说明系统调用接受了写入，不代表文本已经进入 Agent；v1.10 只在基线之后出现与本次提示词**精确相等的整个 user message** 时确认 `Landed`。数组内容必须只有一个 `text` / `input_text` block，不能用“某个 block 碰巧相等”掩盖额外文本或图片。
6. 同一份中断证据必须经过连续观测 reducer，从 `Observing` 进入 `Suspected`，满足阈值后才进入 `Eligible`；证据变化或恢复健康会重置时序状态。
7. Attempt Ledger 以 `session_generation + evidence_hash + prompt_hash` 为幂等键，区分 created、delivering、transport acknowledged、verified、deferred、unverifiable 和 failed。当前自动链已在投递前创建/复用 attempt；只有通过单实例仲裁的主实例会在 setup 阶段把遗留 delivering/acked 原子收敛为 unverifiable；明确未输入的 deferred 只有在 `next_retry_at` 到期后才能 CAS 重试；任意 prompt 下的危险状态都会按整个 generation 阻断重放，最终 `delivery_started` 也以 generation-wide 原子 claim 保证单赢家。
8. Codex 只有在 `codex resume <UUID>` 与对应 transcript metadata 精确匹配时才获得稳定逻辑会话 ID；Claude 只有 argv 显式 UUID 与 cwd 下唯一同名 transcript 一一对应时才关联。裸会话、`--continue` 和不能确认的数据使用进程代际并保留为独立运行记录，不得按 cwd 或前端 `Set` 猜测合并。Windows 目标定位仍需核验原始 creation FILETIME，且外部 console 不能因为已知 PID 就被提升为精确后台 transport。
9. 每次有效 `start()` 都绑定独立 lifecycle epoch；`stop()` 先失效 epoch、清队列，再穿过全局 delivery fence。`stop()` 返回后不得继续落下旧生命周期的自动输入，`stop → start` 也不能复活旧监控循环。

## 2. P0：完成 v1.10 真机验收

下一次恢复工作先执行 `docs/manual-test.md`，不继续堆功能。至少覆盖：

- tmux exact pane：自动后台投递，期间不抢焦点，并在 transcript 中出现本次精确 prompt；
- iTerm2 exact TTY：验证后台 `write text` 不 `activate`、不选中标签、不干扰当前工作；
- Terminal.app、Linux 普通终端、外部 Windows cmd/conhost、Windows Terminal/ConPTY、IDE 集成终端、screen 非精确 window：自动模式应稳定显示“延后”，不得偷偷走前台降级；
- 手动模式：用户点击后才允许前台定位/输入，且仍需拒绝未知目标；
- 同一会话不会重复投递，A 的只读核验不阻塞 B 的投递，stop 不复活旧动作；
- transcript 只发生 mtime、assistant 簿记或无关 prompt 变化时不得判定 `Landed`；
- 历史页默认显示逻辑会话，诊断投递记录保持折叠；不能确认身份的条目显示“旧运行记录”。

### Windows cmd.exe + Codex CLI 必测

这条尚未完成正式真机复测，不能用单元测试或 Windows CI 代替：

- [ ] 在经典 `cmd.exe` / conhost 中启动 Codex CLI；
- [ ] 自动续跑返回 `deferred/no-safe-transport`，不使用 `AttachConsole` / `WriteConsoleInputW` 冒充 PID 精确后台寻址；
- [ ] 自动路径不弹 PowerShell 窗口、不切换前台、不抢焦点、不输入；
- [ ] 用户明确点击“续跑”后才允许前台定位；窗口不唯一、目标退出或进程代际变化时拒绝；
- [ ] 手动路径用 Unicode `SendInput` 发送完整提示词，所有字符进入原会话，不是只多一个换行；
- [ ] 文本完成后独立发送 Enter，Codex 实际提交该提示词；
- [ ] 不出现 `Failed to paste image: no image on clipboard`；
- [ ] 不读取或覆盖剪贴板，不发送 `Ctrl+V` / `SendKeys`，不弹可见 PowerShell 窗口；
- [ ] transcript 基线之后能找到与本次提示词精确相等的 user message；
- [ ] Windows Terminal、VS Code/Cursor 等 ConPTY/集成终端在没有 owned endpoint 时同样自动延后。
验收记录必须包含实际机器、Windows 版本、终端宿主、Agent 版本、通道、提示词、结果和失败日志。

## 3. P1：Owned PTY / ConPTY transport

当前外部终端会话的最大限制是 AgentPulse 不拥有输入端点。后续最有价值的方向是提供可选的托管 transport：

- Unix 使用 owned PTY，Windows 使用 owned ConPTY；
- 启动时生成稳定 `session_generation` 和精确输入句柄，不再依赖窗口标题或前台焦点；
- 写入有明确字节数、关闭和进程代际语义；
- transport ACK 仍然不能替代 transcript/协议核验；
- 作为明确的“由 AgentPulse 启动/托管”模式提供，不能悄悄改变现有旁观模式的会话所有权。

准入标准：后台、无焦点、可寻址、可审计；崩溃恢复不重复提交。

## 4. P1：官方协议 / server transport

优先使用 Agent 自己提供的结构化入口，而不是模拟键盘：

- Codex app-server 或后续公开的官方协议；
- OpenCode server；
- 其他 Agent 的本地 RPC、session API 或受支持的 resume endpoint。

统一适配为：

```text
resolve exact session → submit prompt → transport ACK → DeliveryConfirmed → observe progress
```

协议适配器必须显式声明目标确定性、后台可见性、ACK 语义和可用的验证等级。没有官方保证的内部接口不得包装成“可靠 transport”。

## 5. P1：IDE / 终端插件端点

为 VS Code、Cursor、JetBrains、Windows Terminal 等宿主提供轻量插件或扩展桥接：

- 插件向 AgentPulse 注册稳定的 terminal/session endpoint；
- endpoint 绑定工作区、终端实例、进程代际和可撤销令牌；
- 插件在后台写入目标终端并返回结构化 ACK；
- 宿主关闭、终端重建或令牌失效后立即撤销能力；
- 没有插件时继续自动延后，不回退到盲敲当前窗口。

## 6. P1：Attempt Ledger 的 retry / backoff / quarantine

v1.10 已将 ledger 接入自动投递前置防重和结果更新，并完成保守的跨进程恢复：只有单实例仲裁后的主实例会在 setup 阶段把遗留 `delivering/transport_acked` 原子收敛为 `unverifiable`；`created` 可恢复 claim；`deferred` 只有 `next_retry_at` 到期后可重试；任意 prompt 下的危险状态都会按整个 generation 熔断；最终 `delivery_started` claim 也使用 generation-wide 原子条件更新，多个安全占位并存时仍只有一个能进入不可逆投递。下一步继续增强恢复质量，而不是放宽安全边界：

- 对 `transport_acked/unverifiable` 使用 baseline cursor + prompt hash + transcript 做只读补核验，能证明已落地时收敛为 verified；
- 将固定 deferred 冷却升级为 transport capability 感知的指数 backoff；
- failed 按 typed error taxonomy 区分可重试、不可重试和需要人工确认；
- 连续相同错误进入 quarantine，并提供明确的人工解除入口；
- 将 ledger finalize 与 resume history 写入进一步收敛到同一 SQLite 事务。

## 7. P1：历史模型正式拆分

当前稳定逻辑身份已经减少 Codex 跨进程重启的重复，legacy 数据仍需保守处理。后续将混在一张记录里的概念拆开：

```text
SessionIdentity   # 逻辑对话/任务身份
SessionRuntime    # 某次 PID + process_started_at 运行实例
CurrentSnapshot   # 当前检测快照
DetectionEvent    # 为什么判成中断/恢复
ResumeEvent       # 用户可读的最终续跑事件
ResumeAttempt     # 可恢复、幂等的动作账本
```

迁移只合并有可证明共同身份的记录。cwd、项目名、相近时间只能作为候选证据，不能单独触发合并；无法证明的旧数据继续显示为 legacy runtime。

## 8. P1：拆分 DeliveryConfirmed 与 ProgressObserved

v1.10 的精确 prompt 核验回答“本次文本是否进入目标 transcript”。下一步把两个问题正式拆开：

- `DeliveryConfirmed`：基线之后出现本次精确 user prompt，或官方协议返回等价的持久化确认；
- `ProgressObserved`：确认投递后，Agent 产生新的 assistant/tool/turn 状态，证明工作确实继续；
- 已确认投递但没有进展，不能归咎于 transport，也不能继续无限补敲；应进入等待、限流保持或人工接管策略；
- 统计分别记录投递延迟、首个进展延迟和无进展比例。

## 9. P1：Typed error taxonomy、fingerprint 熔断与 Goal budgets

参考 [`qxd-ljy/codex-goal-auto-retry-build`](https://github.com/qxd-ljy/codex-goal-auto-retry-build) 的关键启发：**单个 turn 失败不必等于 Goal 终止，跨 turn 续跑应发生在 terminal + idle 边界**。该仓库运行在 Codex 内部，可以直接持有 thread/goal 状态锁并调用 idle turn API；AgentPulse 是外部控制器，不能照搬其权限模型，也不采用“除 UsageLimit 外所有错误都保持 Active”的无界策略，而要把 Goal 生命周期、续跑资格和单次 Attempt 三层分开：

```text
Goal lifecycle       # 目标还能否继续、硬预算与断路
Resume eligibility   # 当前 terminal/idle 证据是否足够
Attempt Ledger       # 本次不可逆投递的 at-most-once 与核验
```

后续 P0/P1：

- 保留 Codex 结构化错误元数据，建立 `UsageLimited / TransientTransport / ServerOverloaded / PermanentAuth / InvalidRequest / PolicyBlocked / SandboxOrLocalConfig / ContextExhausted / SessionBudgetExceeded / Unknown` 分类；
- `UsageLimited` 进入带 `retry_at/reset_at` 的等待状态，不创建投递 attempt；
- transient error 只允许有上限、有退避和 jitter 的 continuation；永久错误直接 `HandOff`；
- 为同一错误 fingerprint 建立断路器，fingerprint 至少包含 session generation、错误类、code/status、归一化消息和阶段；
- 引入不会因普通 transcript activity 清零的 Goal 硬预算：总 continuation 次数、同错误次数、wall-clock/deadline，以及可得时的 token/cost；
- 增加真正的端到端序列测试：`turn_error → terminal → idle → policy → attempt claim → delivery → verified`，并覆盖 UsageLimit、永久 400/401/策略错误、重复 transient、context/budget exhausted 不会形成无限循环；
- 固定所适配的 Codex 协议/fixture 版本，每次上游升级重跑 typed error 分类矩阵。

另外吸收三个架构细节，但按外部控制器边界重新实现：

- 将“读取 Goal 状态 → 决定续跑 → claim/start”包在同一代际许可或持久化 CAS 中，不能读完状态后再无保护地启动下一轮；
- 把 continuation deferral 作为可恢复状态持久化，限流、人工暂停和安全 transport 缺失都不能靠易丢的内存布尔值表达；
- 对每个 terminal turn 使用稳定 event id 做幂等预算记账，普通 activity、进程重启或重复事件不得重置/重复扣减 Goal budget。

核心原则：Attempt Ledger 约束“一次动作不重复”，Goal budget 约束“一系列不同动作不无限产生”，两者不可互相替代。

## 10. 建议实施顺序

1. 完成 v1.10 三平台真机矩阵，尤其是 Windows 自动延后与手动前台 Unicode `SendInput`；
2. 建立 typed error taxonomy，并固定 Codex 协议/fixture 兼容矩阵；
3. 为重复错误加入 fingerprint 熔断与不会被普通 activity 清零的 Goal budgets；
4. 增强 ledger transcript 补核验、typed retry/backoff/quarantine 与原子历史记账；
5. 选一个结构化 transport 做纵向样板：优先官方/server 协议；
6. 设计 Owned PTY/ConPTY 的显式托管模式，再评估 IDE/终端插件端点；
7. 拆分历史模型并实施保守迁移；
8. 最后加入 `DeliveryConfirmed` / `ProgressObserved` 两级指标。

在这些基础设施完成前，不扩大自动前台注入范围，不把“脚本退出码为 0”包装成可靠续跑，也不以清理历史页面为由进行不可证明的数据合并。
