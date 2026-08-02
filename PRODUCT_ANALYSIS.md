# AgentPulse 产品分析与演进建议

> 基于 v1.5.0（commit `5e2e793`）的完整代码审读 + 2026 年市场调研
> 文档日期：2026-07-30（深度修订：补充统计界面设计、核心算法审查、遗漏功能）

---

## 目录

1. [当前状态评估](#1-当前状态评估)
2. [竞品全景与差异化定位](#2-竞品全景与差异化定位)
3. [用户痛点挖掘（来自社区/论坛/Issue）](#3-用户痛点挖掘)
4. [优化建议：性能与架构](#4-优化建议性能与架构)
5. [优化建议：用户体验](#5-优化建议用户体验)
6. [功能扩展：短期高价值（v1.6–v1.7）](#6-功能扩展短期高价值)
7. [功能新增：中期差异化（v1.8–v2.0）](#7-功能新增中期差异化)
8. [需求挖掘：长期生态位（v2.x+）](#8-需求挖掘长期生态位)
9. [商业化路径思考](#9-商业化路径思考)
10. [优先级矩阵与推荐路线图](#10-优先级矩阵与推荐路线图)
11. [**深度审查：统计与数据可视化界面**](#11-深度审查统计与数据可视化界面)
12. [**深度审查：核心判断算法**](#12-深度审查核心判断算法)
13. [**深度审查：遗漏功能与盲区**](#13-深度审查遗漏功能与盲区)

---

## 1. 当前状态评估

### 1.1 工程成熟度

| 维度 | 评分 | 说明 |
|---|---|---|
| 核心功能完整度 | ★★★★★ | 检测→判定→续跑→核验闭环已打通，三平台覆盖 |
| 代码质量 | ★★★★★ | 115+32 测试、clippy -D warnings、cfg 纪律严格 |
| 文档质量 | ★★★★★ | PROJECT_STATUS.md 1341 行，是少见的全景式工程文档 |
| 安全性 | ★★★★☆ | 只读看板+令牌+CSP nonce，但缺自动更新签名 |
| 可扩展性 | ★★★☆☆ | 适配器 trait 已抽象，但 UI 侧自定义入口未开放 |
| 用户触达 | ★★☆☆☆ | 无自动更新、无 onboarding 引导、无使用数据回流 |

### 1.2 核心护城河

AgentPulse 的**非侵入式守护**定位在 2026 年的 AI Agent 工具生态中是独一无二的：

- **Claude Squad**（14k+ stars）：侵入式，必须从它启动 agent，用 tmux + git worktree 隔离
- **cross_agent_session_resumer**：只做会话迁移（A→B），不做守护
- **cli-continues**：只做会话恢复，无检测/通知/成本
- **CodeAgentSwarm**：编排型，要接管终端

没有任何竞品做到「**不改变你的工作流，装上就守护已在跑的会话**」。这是真正的差异化。

### 1.3 当前最大风险

1. **实机验证空白**：12.1 的 7 行 ⚠️ 仍未消除，续跑主链路没有真机证据
2. **用户增长引擎缺失**：没有自动更新、没有 onboarding、没有社区入口
3. **单一用户群**：只覆盖"跑终端 agent 的独立开发者"，团队场景未触及

---

## 2. 竞品全景与差异化定位

### 2.1 竞品矩阵

| 工具 | 类型 | 核心能力 | 侵入性 | 平台 | 定价 |
|---|---|---|---|---|---|
| **Claude Squad** | 多 Agent 管理器 | tmux 隔离 + git worktree + TUI | 高（必须从它启动） | macOS/Linux | 免费 AGPL |
| **CodeAgentSwarm** | 编排器 | 并行任务分配 + 进度追踪 | 高 | 跨平台 | 免费 |
| **cross_agent_session_resumer** | 会话迁移 | 跨 provider 恢复上下文 | 中 | CLI | 免费 |
| **cli-continues** | 会话恢复 | 跨工具 resume | 低 | CLI (npx) | 免费 |
| **Claude Code 内置** | 原生 | `--continue`、并行会话侧栏 | 无（但只服务自己） | 内置 | — |
| **Cursor Agent** | IDE 内置 | 后台 agent + Bugbot | 无（但锁在 IDE 里） | IDE | $20-40/mo |
| **Devin** | 云端自治 | 全自主 + 浏览器 + 终端 | 完全接管 | Cloud | $500/mo |
| **AgentPulse** | 非侵入守护 | 检测+续跑+核验+成本+通知 | **零** | macOS/Win/Linux | 免费 |

### 2.2 市场空白

```
侵入程度轴：
  零侵入 ←────────────────────────────→ 完全接管
  AgentPulse    cli-continues    Claude Squad    Devin
  (守护)        (恢复)          (管理)         (自治)
```

AgentPulse 占据的是**最左侧**——唯一一个"你不需要改变任何习惯"的工具。
这个位置的优势是：用户无需迁移成本，卸载无残留。
劣势是：能力天花板受限于"从外部观测"的信息量。

### 2.3 竞品功能对标（AgentPulse 有/无）

| 能力 | AgentPulse | Claude Squad | Cursor | Devin |
|---|---|---|---|---|
| 中断检测 | ✅ 四信号融合 | ❌ | ❌ | ❌（自己管自己） |
| 自动续跑 | ✅ 三平台+tmux | ⚠️ 只 auto-accept | ❌ | N/A |
| 续跑核验 | ✅ 闭环 | ❌ | ❌ | N/A |
| 成本追踪 | ✅ 21 模型 | ❌ | ⚠️ 只显示用量 | ❌ |
| 限流预测 | ✅ | ❌ | ❌ | ❌ |
| 手机看板 | ✅ | ❌ | ❌ | ✅（Web） |
| 多 Agent 并行管理 | ❌ | ✅ 核心能力 | ✅ 侧栏 | ✅ |
| Git 隔离 | ❌ | ✅ worktree | ✅ branch | ✅ sandbox |
| 任务编排 | ❌（刻意不做） | ⚠️ 简单 | ✅ Agent Mode | ✅ 核心 |
| 桌面通知 | ✅ 原生+声音 | ❌ | ✅ IDE 内 | ✅ |
| Webhook 集成 | ✅ 5 家 | ❌ | ❌ | ✅ Slack |

---

## 3. 用户痛点挖掘

### 3.1 来自 Reddit / GitHub Issues / HN 的真实声音

| 痛点 | 来源 | 频率 | AgentPulse 覆盖 |
|---|---|---|---|
| **Rate limit 卡住不知道** | Reddit r/ClaudeAI 多帖、GitHub #26699 | 极高 | ✅ 检测+通知+续跑 |
| **5 小时限额到了要手动 continue** | Reddit "I built a tool that auto-retries" (2k+ upvotes) | 极高 | ✅ 自动续跑 |
| **跑着跑着静默退出了不知道** | HN、Medium 多篇 | 高 | ✅ 进程退出检测 |
| **多个终端不知道哪个在等我** | TDS "How to Run Many Sessions" | 高 | ⚠️ 有注意力排序，缺全局视图 |
| **不知道花了多少钱** | 多平台讨论 | 中高 | ✅ 成本面板 |
| **续跑了但不知道成没成功** | 本项目用户反馈 | 中 | ✅ v1.5 闭环核验 |
| **换个工具上下文就丢了** | cross_agent_session_resumer (3k+ stars) | 中 | ❌ 不在定位内 |
| **并行 agent 互相冲突** | Claude Squad 存在的理由 | 中 | ❌ 不在定位内 |
| **Context window 快满了 agent 变蠢** | LinkedIn / 学术论文 | 中 | ❌ 未覆盖 |
| **团队里不知道谁的 agent 在跑** | 企业场景 | 低（当前） | ❌ 未覆盖 |

### 3.2 痛点优先级矩阵

```
        高频率
          │
    ①    │    ②
  rate limit  多会话
  自动续跑    全局视图
          │
 ─────────┼───────── 高痛感
          │
    ③    │    ④
  成本失控  context
  团队可见  退化预警
          │
        低频率
```

AgentPulse 已经很好地覆盖了 ①，部分覆盖了 ② 的"谁在等我"，
但 ② 的"全局鸟瞰"和 ③④ 还有空间。

---

## 4. 优化建议：性能与架构

### 4.1 后端性能

| 项 | 现状 | 建议 | 收益 |
|---|---|---|---|
| 进程扫描 | 每轮 `sysinfo` 全量刷新 | 增量扫描：记录上轮 PID 集合，只对新 PID 做完整查询 | CPU -60%（会话多时） |
| 会话文件读取 | 尾部 8KB 窗口 | 已有，但可加 `inotify`/`FSEvents` 驱动（notify crate 已引入） | 从轮询变异步 |
| SQLite 写入 | 每事件一次 INSERT | 批量写入（攒 100ms 或 10 条）+ WAL 模式 | 磁盘 IO -80% |
| osascript 调用 | 每次续跑 spawn 一次 | 复用 `osascript -i` 交互模式（macOS） | 延迟 -200ms |
| 成本计算 | 每轮重算所有会话 | 增量：只处理有新 usage 行的会话 | O(n)→O(Δ) |

### 4.2 架构改进

| 项 | 建议 | 理由 |
|---|---|---|
| **事件总线** | 引入轻量级内部 event bus（`tokio::broadcast`） | 当前 monitor→notify/webhook/storage 是硬编码调用链，加新消费者要改主循环 |
| **适配器热加载** | 自定义适配器编译为 WASM 或 Lua 脚本 | 让用户自己写适配器而不需要 Rust 编译 |
| **配置 schema 导出** | 自动生成 JSON Schema | 为将来的配置 UI 校验和外部工具集成铺路 |
| **插件化通知通道** | 通知通道 trait 化（当前 webhook 是硬编码 5 家） | 用户想加企业微信/飞书/钉钉不需要改源码 |

### 4.3 前端架构

| 项 | 现状 | 建议 |
|---|---|---|
| ConfigPanel.tsx | 867 行单文件 | 按 Tab 分组拆 6-7 个子组件（PROJECT_STATUS 已规划） |
| 状态管理 | 单一 Zustand store | 按领域拆分：useSessionStore / useCostStore / useConfigStore |
| 数据获取 | 手动 invoke + 轮询 | 引入 TanStack Query（或 Zustand + subscribe 模式）做缓存+重试 |
| 组件测试 | 0 个 | 补 @testing-library/react，至少覆盖 SessionList 排序逻辑 |
| 错误边界 | 无 | 加 React ErrorBoundary，IPC 失败不至于白屏 |

---

## 5. 优化建议：用户体验

### 5.1 Onboarding 流程（当前完全缺失）

**问题**：用户下载后打开 app，看到空荡荡的 Dashboard，不知道下一步该做什么。

**建议**：

```
首次启动向导（3 步）：
┌─────────────────────────────────────────┐
│ ① 欢迎 + 一句话解释                      │
│   "AgentPulse 在后台守护你的 AI Agent，   │
│    卡住了自动续跑，有事叫你。"             │
│                                          │
│ ② 权限检查（macOS）                      │
│   辅助功能权限：[已授权 ✅] / [去授权 →]   │
│   通知权限：    [已授权 ✅] / [去授权 →]   │
│                                          │
│ ③ 开始守护                               │
│   [启动监控] 按钮                         │
│   "启动后，你照常用 Claude Code 就行。"    │
└─────────────────────────────────────────┘
```

### 5.2 会话卡片信息密度优化

**当前**：每行显示 agent 名 + 状态 + 注意力 + 项目 + PID + TTY + token + 按钮×4-5

**问题**：信息过载，5+ 个会话时扫描成本高。

**建议**：

- **紧凑/展开双模式**：默认只显示「项目名 + 状态灯 + 一句话」，点击展开详情
- **分组视图**：按项目分组（多项目视图已在 13.2 规划）
- **键盘快捷键**：`↑↓` 选会话，`r` 续跑，`l` 演练，`f` 跳终端
- **全局搜索**：`⌘K` 快速定位会话

### 5.3 通知体验增强

| 现状 | 建议 |
|---|---|
| 通知内容固定 | 通知里带上下文：「agent-pulse 项目的 Claude Code 在等你回话，已等 3 分钟」 |
| 点击通知只跳窗口 | 点击通知直接展开对应会话卡片 + 高亮 |
| 无"免打扰"模式 | 加「专注模式」：只推送 needs_input，其余静默 |
| 无通知历史 | 通知中心：最近 24h 的通知时间线 |

### 5.4 演练结果可视化

当前演练结果是纯文本一行。建议改为结构化卡片：

```
┌─ 演练结果 ─────────────────────────────┐
│ ✅ 精确定位                             │
│                                         │
│ 终端：iTerm2                            │
│ TTY：/dev/ttys003                       │
│ 项目：agent-pulse                       │
│ 通道：write text (直接写 PTY)           │
│ 需要辅助功能权限：否                     │
│                                         │
│ [真跑一次续跑]  [关闭]                   │
└─────────────────────────────────────────┘
```

---

## 6. 功能扩展：短期高价值（v1.6–v1.7）

### 6.1 多会话鸟瞰视图（Dashboard 2.0）

**痛点**：跑 5-10 个 agent 时，需要一眼看出"谁在等我、谁在跑、谁卡了"。

**设计**：

```
┌─────────────────────────────────────────────────────┐
│  ● 3 运行中   ● 2 等你回话   ● 1 限流   ● 1 已完成  │
├─────────────────────────────────────────────────────┤
│                                                     │
│  ┌─ agent-pulse ──────────────────────────────────┐ │
│  │ 🟢 Claude Code  ttys003  运行中  12min  $0.42 │ │
│  │ 🔴 Claude Code  ttys005  等你    3min   $1.23 │ │
│  └────────────────────────────────────────────────┘ │
│                                                     │
│  ┌─ my-saas-app ────────────────────────────────┐  │
│  │ 🟡 Codex CLI    ttys007  限流    ETA 4min    │  │
│  │ 🟢 OpenCode     ttys009  运行中  8min  $0.15 │  │
│  └──────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
```

**实现成本**：中。数据已有（sessions + attention + cost），纯前端重组。

### 6.2 智能续跑策略引擎

**痛点**：不同中断原因需要不同的续跑策略。

| 中断原因 | 当前行为 | 建议行为 |
|---|---|---|
| Rate limit | 统一提示词 | 等待 ETA 后再续 + 专用提示词「限流已恢复，请继续」 |
| 网络断开 | 统一提示词 | 先检测网络恢复，再续 |
| 等用户输入 | 通知 | 通知 + 可选自动回复「是的，继续」 |
| 进程崩溃 | 通知 | 通知 + 可选重启命令（用户配置） |
| Context 满 | 无 | 检测 compaction 事件 + 提示「建议开新会话」 |

**实现**：在 `detector` 的 `Verdict` 里加 `InterruptReason` 枚举，`monitor` 根据 reason 选择策略。

### 6.3 会话时间线（Activity Feed）

**痛点**：回来时不知道 agent 这段时间经历了什么。

**设计**：每个会话一条时间线：

```
14:03  🟢 开始运行
14:15  🟡 触发 rate limit，等待中
14:19  🟢 限流恢复，自动续跑 → ✅ 落地
14:32  🔴 等待用户输入（已通知）
14:35  🟢 用户回复，继续运行
14:52  🔵 完成
```

**实现成本**：低。`EngineEvent` 已有 session_id + timestamp + message，前端加一个时间线组件。

### 6.4 快捷键系统

| 快捷键 | 动作 |
|---|---|
| `⌘K` / `Ctrl+K` | 快速搜索会话 |
| `⌘R` | 续跑选中会话 |
| `⌘L` | 演练定位 |
| `⌘F` | 跳到终端 |
| `⌘,` | 打开设置 |
| `Space` | 展开/折叠会话详情 |
| `1-5` | 切换 Tab |

### 6.5 自定义适配器 UI（已在 13.2 规划）

补充具体设计：

```
┌─ 添加自定义 Agent ─────────────────────────────┐
│                                                 │
│ 名称：[Gemini CLI        ]                      │
│ 进程匹配：[gemini          ] (正则)             │
│ 会话文件：[~/.gemini/sessions/*.jsonl] (glob)    │
│                                                 │
│ 回合判定：                                      │
│   忙碌标记：[tool_call|generating]              │
│   完成标记：[task_complete|done]                │
│   中断关键词：[rate_limit|error|timeout]        │
│                                                 │
│ 续跑提示词：[请继续完成当前任务]                  │
│                                                 │
│ [测试连接]  [保存]  [取消]                       │
└─────────────────────────────────────────────────┘
```

---

## 7. 功能新增：中期差异化（v1.8–v2.0）

### 7.1 Context Window 退化预警

**市场信号**：2026 年大量讨论"agent 跑久了变蠢"——context window 接近上限时，
模型开始遗忘早期指令、重复操作、质量下降。

**AgentPulse 能做什么**（非侵入）：

1. 从会话文件里读取 token 用量（Claude Code 的 JSONL 里有 `usage.input_tokens`）
2. 对比模型的 context window 上限（价目表里已有）
3. 当使用率 > 75% 时发出预警：「context 已用 78%，建议开新会话或触发 compaction」
4. 检测 compaction 事件（Claude Code 会输出 `[Compacting conversation...]`）

**价值**：这是目前**没有任何工具在做**的事。Claude Code 自己只显示一个不显眼的百分比。

### 7.2 智能限流调度器

**痛点**：多个 agent 同时跑，同时撞限流墙。

**设计**：

```
限流预测引擎（已有 forecast_rate_limit）
        ↓
多会话协调：
  - 会话 A 预计 4min 后撞墙 → 建议暂停
  - 会话 B 正在限流中 → ETA 2min
  - 会话 C 刚启动 → 还有 45min 余量
        ↓
用户可见的"限流时间表"：
  14:00 ─── A 运行 ─── A 限流 ─── A 恢复 ───
  14:00 ─── B 运行 ──────────── B 限流 ──────
  14:00 ─── C 运行 ──────────────────────────
```

### 7.3 团队模式（Team Dashboard）

**市场信号**：企业团队开始多人同时跑 agent，需要：
- 谁的 agent 在跑、花了多少钱
- 全局 token 预算管控
- 限流池共享感知

**非侵入实现路径**：

```
每个开发者的 AgentPulse 实例
        ↓ (opt-in 上报)
团队聚合服务（轻量 HTTP）
        ↓
团队看板（只读 Web）
  - 全员今日花费
  - 活跃会话数
  - 限流状态
  - 预算告警
```

**关键约束**：上报是 opt-in 的、只读的、不含代码内容。

### 7.4 SSH 远端守护（已在 13.3 规划）

补充实现细节：

```yaml
# config.json 新增
"remote_machines": [
  {
    "name": "dev-server",
    "ssh": "user@192.168.1.100",
    "agent_paths": ["~/.claude/projects"],
    "tmux_socket": "/tmp/tmux-1000/default"
  }
]
```

续跑通道：`ssh user@host tmux send-keys -t session:window "继续" Enter`

前置条件已具备：v1.4 的 tmux 通道 + 会话文件读取逻辑。

### 7.5 本地 API / CLI（编排器接口）

**来自 13.4 的折中路径**：不做编排，但开放接口让外部编排器调用。

```bash
# CLI
agentpulse status              # JSON 输出所有会话状态
agentpulse resume <session-id> # 触发续跑
agentpulse probe <session-id>  # 演练定位
agentpulse cost --today        # 今日花费

# HTTP API (localhost:19527)
GET  /api/sessions             # 会话列表
POST /api/sessions/:id/resume  # 续跑
GET  /api/cost?range=7d        # 成本
WS   /api/events               # 实时事件流
```

**价值**：让 AgentPulse 成为 AI 开发工作流的**可组合基础设施**，
而不是一个孤立的桌面 app。n8n / Zapier / 自定义脚本都能接入。

### 7.6 会话录制与回放

**痛点**：agent 跑了 2 小时，回来想知道它到底做了什么。

**设计**：
- 从会话文件提取关键事件（工具调用、文件修改、错误）
- 生成"执行摘要"时间线
- 可选：接入 AI 生成自然语言总结

---

## 8. 需求挖掘：长期生态位（v2.x+）

### 8.1 Agent 健康评分

**概念**：给每个会话一个实时"健康分"（0-100），综合：

| 因子 | 权重 | 说明 |
|---|---|---|
| 响应延迟趋势 | 25% | 越来越慢 = 可能快满了 |
| Context 使用率 | 25% | > 80% 扣分 |
| 错误频率 | 20% | 最近 5min 的错误密度 |
| 续跑成功率 | 15% | 历史核验结果 |
| 限流距离 | 15% | 离下一次限流还有多远 |

**价值**：一个数字告诉用户"这个会话还能不能继续跑，还是该开新的"。

### 8.2 跨会话知识传递

**场景**：会话 A 探索了 30 分钟发现"这个 API 要用 v2 endpoint"，
会话 B 在同一个项目里又走了同样的弯路。

**非侵入方案**：
- 监控所有同项目会话的 `CLAUDE.md` / `.cursorrules` 变更
- 当一个会话产出了"经验"（写入 memory/rules 文件），
  提醒其他同项目会话的注意
- 不直接修改文件（红线），只通知

### 8.3 预算守卫（Budget Guardian）

**痛点**：跑着跑着发现这个月花了 $500。

**设计**：
- 设置日/周/月预算上限
- 到达 80% 时通知
- 到达 100% 时可选：暂停所有自动续跑（不续跑 = 不花新钱）
- 按项目/按 agent 分配子预算

### 8.4 Agent 性能基准

**长期愿景**：积累足够数据后，能回答：
- "Claude Code 在这类任务上平均跑多久？"
- "哪个项目的 token 效率最高？"
- "限流最常发生在几点？"

这是**只有守护型工具才能收集到的数据**——侵入式工具只能看到自己启动的会话。

### 8.5 插件市场

```
AgentPulse Plugin Registry
├── adapters/
│   ├── aider-adapter
│   ├── gemini-cli-adapter
│   ├── cline-adapter
│   └── continue-adapter
├── notifications/
│   ├── feishu-webhook
│   ├── dingtalk-webhook
│   └── telegram-bot
├── strategies/
│   ├── smart-rate-limit-scheduler
│   └── context-aware-resume
└── integrations/
    ├── n8n-connector
    └── raycast-extension
```

---

## 9. 商业化路径思考

### 9.1 定价模型选项

| 模型 | 适用阶段 | 说明 |
|---|---|---|
| **完全免费 + 开源** | 现在 | 建立社区、积累用户 |
| **Free + Pro** | 用户 > 1000 | Pro：团队看板、SSH 远端、预算守卫、优先支持 |
| **Team License** | 有企业需求 | 按席位收费、集中管控、审计日志 |

### 9.2 增长引擎

| 渠道 | 动作 | 优先级 |
|---|---|---|
| GitHub | README 加 GIF 演示、Awesome 列表投稿 | 高 |
| Reddit | r/ClaudeAI、r/ChatGPTCoding 发帖（痛点帖+解决方案） | 高 |
| Hacker News | Show HN（"I built a non-invasive guardian for AI coding agents"） | 高 |
| Product Hunt | 准备 launch（需要自动更新 + onboarding 先就位） | 中 |
| Twitter/X | 开发日志 thread、rate limit 痛点 meme | 中 |
| 博客 | "Why non-invasive monitoring beats agent orchestration" | 中 |

### 9.3 开源策略

- 核心引擎保持 MIT（当前）
- 团队功能可以作为 "Enterprise Edition" 闭源
- 适配器/插件生态开放贡献

---

## 10. 优先级矩阵与推荐路线图

### 10.1 价值/成本矩阵

```
高价值
  │
  │  ① Onboarding    ② 多会话鸟瞰
  │  ③ 自动更新      ④ 会话时间线
  │  ⑤ 快捷键       ⑥ Context 预警
  │
  │  ⑦ 适配器UI     ⑧ 本地 API/CLI
  │  ⑨ 限流调度     ⑩ 团队模式
  │
  │  ⑪ SSH远端      ⑫ 插件市场
  │  ⑬ 预算守卫     ⑭ 健康评分
  │
低价值 ─────────────────────── 高成本
         低成本
```

### 10.2 推荐路线图

#### v1.6（2-3 周）— 可信化收尾 + 体验基础

| 优先级 | 项 | 估时 |
|---|---|---|
| P0 | 走完 manual-test.md 实机验证 | 2d |
| P0 | Onboarding 首次启动向导 | 3d |
| P1 | 拆 ConfigPanel.tsx | 1d |
| P1 | 自定义适配器 UI | 3d |
| P1 | 会话时间线（Activity Feed） | 2d |
| P2 | 快捷键系统 | 2d |
| P2 | 检测侧判定证据面板 | 2d |

#### v1.7（3-4 周）— 差异化功能

| 优先级 | 项 | 估时 |
|---|---|---|
| P0 | 自动更新（签名 + updater + latest.json） | 3d |
| P0 | 多会话鸟瞰视图（按项目分组） | 3d |
| P1 | Context Window 退化预警 | 3d |
| P1 | 智能续跑策略（按中断原因分派） | 4d |
| P1 | 本地 API / CLI | 4d |
| P2 | 通知增强（免打扰 + 上下文 + 历史） | 2d |

#### v1.8（4-6 周）— 生态扩展

| 优先级 | 项 | 估时 |
|---|---|---|
| P1 | SSH 远端守护 | 5d |
| P1 | 限流调度器（多会话协调） | 4d |
| P1 | 预算守卫 | 3d |
| P2 | 通知通道插件化（飞书/钉钉/Telegram） | 3d |
| P2 | 组件层测试补齐 | 3d |

#### v2.0（远期）— 平台化

| 项 | 说明 |
|---|---|
| 团队模式 | opt-in 上报 + 聚合看板 |
| 插件市场 | 适配器/通知/策略的注册与分发 |
| Agent 健康评分 | 综合指标 + 趋势 |
| 跨会话知识传递 | 同项目经验共享提醒 |

### 10.3 立即可做的 5 件事（本周）

1. **录一个 30 秒 GIF**：rate limit → 自动续跑 → 核验成功。放 README 顶部。
2. **发一个 Reddit 帖**：标题 "I built a tool that watches your AI coding agents and auto-resumes them when they get stuck"，附 gif。
3. **加一个 `--version` CLI 参数**：让 `agentpulse --version` 能输出版本号（为 CLI 铺路）。
4. **在会话卡片加 context 使用率进度条**：数据已有（`usage.input_tokens` / 模型上限）。
5. **把 manual-test.md 的 macOS 部分走一遍**：消除最大的"它到底能不能用"疑问。

---

## 附录 A：竞品链接

| 工具 | 链接 | Stars |
|---|---|---|
| Claude Squad | https://github.com/smtg-ai/claude-squad | 14k+ |
| cross_agent_session_resumer | https://github.com/Dicklesworthstone/cross_agent_session_resumer | 3k+ |
| cli-continues | https://github.com/yigitkonur/cli-continues | 1k+ |
| CodeAgentSwarm | https://www.codeagentswarm.com | — |
| amux | https://github.com/（minimal TUI for parallel agents） | — |

## 附录 B：用户痛点原始来源

| 来源 | 链接/描述 |
|---|---|
| Reddit 自动续跑帖 | r/ClaudeAI "I built a tool that auto-retries Claude Code when you hit the limit" (2k+ upvotes) |
| GitHub 限流卡死 | anthropics/claude-code#26699 "Session permanently stuck on Rate limit reached" |
| HN 会话恢复 | news.ycombinator.com/item?id=47075089 "npx continues – resume same session" |
| TDS 并行指南 | towardsdatascience.com "How to Effectively Run Many Claude Code Sessions in Parallel" |
| Context 退化 | zylos.ai "Context Window Management and Session Lifecycle for Long-Running Agents" |
| 成本管控 | cockroachlabs.com "The Bill Arrives: How to Manage Agentic AI Costs at Scale" |

## 附录 C：技术趋势信号（2026 H2）

| 趋势 | 对 AgentPulse 的影响 |
|---|---|
| Claude Code 内置并行会话侧栏 | 减少了"多终端管理"痛点，但**不解决中断检测** |
| Cursor 后台 Agent + Bugbot | IDE 内闭环，但**不覆盖终端用户** |
| Codex CLI 登顶 Terminal-Bench | 用户量增长 → AgentPulse 潜在用户增长 |
| 企业 AI 预算管控成刚需 | 成本追踪从"nice to have"变"must have" |
| Tauri 2 生态成熟（5MB 包体） | 技术选型正确，继续深耕 |
| Agent 可观测性成独立赛道 | 验证了"监控 agent"这个方向的价值 |

---

## 11. 深度审查：统计与数据可视化界面

> 审读对象：`StatsPanel.tsx`(168行)、`CostPanel.tsx`(208行)、`StatusCards.tsx`(48行)、
> `HistoryPanel.tsx`(141行)、`storage/mod.rs` 的 SQL 层、`cost/mod.rs` 的聚合层

### 11.1 当前统计界面的问题诊断

#### A. 数据维度单一，缺乏“效率”指标

当前 StatsPanel 只有四个数字：检测数、续跑数、成功数、成功率。
这些是“守护行为”的统计，但用户真正关心的是：

| 用户真正想知道的 | 当前是否回答 | 数据来源 |
|---|---|---|
| 今天 agent 总共跑了多久？ | ❌ | `session_history.first_seen / last_seen` 可算 |
| 平均每次中断到恢复要多久？ | ❌ | `detection_records.created_at` → `resume_records.created_at` 可算 |
| 哪个项目最“费人”（中断最频繁）？ | ❌ | `detection_records.session_id` → `working_dir` 可关联 |
| 续跑成功率趋势（是在变好还是变差）？ | ⚠️ 只有柱状图，无趋势线 | `daily_stats` 可算 |
| 今天比昨天多花了还是少花了？ | ❌ | `usage_records.date` 可算 |
| 哪个模型性价比最高？ | ❌ | `usage_records.model` + `cost_usd` 可算 |
| 限流最常发生在几点？ | ❌ | `detection_records.signals` 含 rate_limit 关键词 + `created_at` |
| 会话平均存活时长？ | ❌ | `session_history.first_seen / last_seen` |

#### B. 可视化形式原始

| 组件 | 现状 | 问题 |
|---|---|---|
| StatusCards | 6 个纯数字卡片 | 无趋势、无对比、无上下文。数字孤立存在，用户看不出「好不好」 |
| StatsPanel 柱状图 | 30 天活动量 | 只有总量柱，没有按类型分色（检测/续跑/失败），看不出结构 |
| CostPanel 趋势 | 14 天花费柱状 | 无均值线、无异常标记、无同比 |
| CostPanel 项目 | 横条归一化 | 无时间维度（这个项目是今天突然费钱还是一直费） |
| Forecast | 单一进度条 | 无历史对比（昨天同一时刻用了多少） |
| 续跑历史 | 纯列表 | 无筛选（按项目/按结果）、无统计摘要 |

#### C. 缺少“可行动”的洞察

好的统计不是“给你看数字”，而是“告诉你该做什么”。当前所有统计都是被动展示，
没有任何一处主动说：“你的 X 项目中断频率异常高，建议检查提示词质量”。

### 11.2 统计界面重设计方案

#### 设计原则

1. **分层展示**：概览 → 趋势 → 明细，不要一屏塞所有东西
2. **对比产生意义**：数字永远要有参照系（昨天 / 上周 / 均值）
3. **可行动**：每个异常都要有“下一步”建议
4. **时间为主轴**：所有数据都要能沿时间线展开

#### 重设计后的统计页结构

```
┌───────────────────────────────────────────────────────────┐
│  ═══ 概览卡片（带趋势箭头） ═══                          │
│                                                           │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐  │
│  │ $4.32   │ │ 7 次    │ │ 86%     │ │ 42min   │  │
│  │ 今日花费 │ │ 中断检测 │ │ 续跑成功 │ │ 平均恢复 │  │
│  │ ↑ 23%   │ │ ↓ 2    │ │ ↑ 4%    │ │ ↓ 8min  │  │
│  └─────────┘ └─────────┘ └─────────┘ └─────────┘  │
├───────────────────────────────────────────────────────────┤
│  ═══ 活动时间线（可切换 7d / 30d / 90d） ═══              │
│                                                           │
│  花费趋势（面积图 + 均值虚线 + 异常点标记）              │
│  ─────────────────────────────────────────────────  │
│  中断/续跑热力图（每小时 × 每天，颜色深浅 = 次数）       │
│  ─────────────────────────────────────────────────  │
│  Token 消耗堆叠图（按模型分色）                          │
├───────────────────────────────────────────────────────────┤
│  ═══ 洞察卡片（主动生成） ═══                              │
│                                                           │
│  💡 “agent-pulse 项目本周中断 12 次，比上周多 3 倍，       │
│      其中 9 次是 rate limit。建议错开高峰时段。”          │
│                                                           │
│  💡 “claude-opus-5 占今日花费的 78%，但只完成了 2 个      │
│      任务。考虑对简单任务用 sonnet。”                    │
├───────────────────────────────────────────────────────────┤
│  ═══ 项目排行（按花费 / 按中断次数 / 按效率） ═══          │
│                                                           │
│  项目          花费      中断   续跑成功  平均恢复   │
│  agent-pulse   $12.40    18次   94%      3min       │
│  my-saas       $8.20     7次    71%      12min      │
│  side-project  $2.10     2次    100%     1min       │
└───────────────────────────────────────────────────────────┘
```

### 11.3 花费页重设计

当前 CostPanel 的三个问题：

1. **无模型维度**：用户不知道钱花在哪个模型上。数据已有（`usage_records.model`），
   但前端从未展示。
2. **无“效率”指标**：每美元完成了多少次续跑？每个 token 产出了什么？
   这是“花费”和“价值”的关联，当前完全缺失。
3. **预测太粗**：`forecast_rate_limit` 只用“最近 1 小时”的速率线性外推，
   没有考虑“每天下午 3 点总会撞墙”这种周期性模式。

#### 花费页新增内容

| 新增模块 | 内容 | 数据来源 |
|---|---|---|
| 模型花费占比 | 饼图 / 堆叠柱：opus vs sonnet vs haiku | `usage_records.model` GROUP BY |
| 缓存效率 | 缓存命中率 = cache_read / (input + cache_read) | `usage_records` 已有字段 |
| 每会话成本 | 单次会话平均花费 + 最贵会话 TOP5 | `usage_records.session_file` GROUP BY |
| 日/周/月报表 | 可导出的花费摘要 | SQL 聚合 |
| 预算进度 | 日预算 + 周预算 + 月预算三条进度条 | `config.cost.daily_budget_usd` 扩展 |

### 11.4 数据层缺失的 SQL 视图

当前 `storage/mod.rs` 只有基础 CRUD，缺少以下聚合查询：

```sql
-- 1. 每小时中断分布（用于热力图）
SELECT strftime('%H', created_at) AS hour, COUNT(*) AS cnt
FROM detection_records
WHERE created_at > datetime('now', '-7 days')
GROUP BY hour;

-- 2. 按模型的花费分布
SELECT model, SUM(cost_usd) AS total_cost, COUNT(*) AS requests,
       SUM(input_tokens + output_tokens) AS total_tokens
FROM usage_records
WHERE date > date('now', '-30 days')
GROUP BY model;

-- 3. 平均恢复时间（中断→续跑的时间差）
SELECT AVG(
  (julianday(r.created_at) - julianday(d.created_at)) * 86400
) AS avg_recovery_secs
FROM detection_records d
JOIN resume_records r ON r.session_id = d.session_id
  AND r.created_at > d.created_at
  AND r.created_at < datetime(d.created_at, '+30 minutes');

-- 4. 缓存命中率
SELECT date,
  SUM(cache_read_tokens) * 100.0 / NULLIF(SUM(input_tokens + cache_read_tokens), 0) AS hit_rate
FROM usage_records
GROUP BY date;

-- 5. 项目效率排行
SELECT project,
  SUM(cost_usd) AS total_cost,
  COUNT(DISTINCT session_file) AS sessions,
  SUM(cost_usd) / COUNT(DISTINCT session_file) AS cost_per_session
FROM usage_records
WHERE date > date('now', '-7 days')
GROUP BY project
ORDER BY total_cost DESC;
```

### 11.5 竞品统计功能对标

| 功能 | AgentPulse | ccusage (CLI) | Claude Usage Analytics (VSCode) | Marc Nuri Dashboard |
|---|---|---|---|---|
| 实时花费 | ✅ | ✅ | ✅ | ❌ |
| 按模型分布 | ❌ | ✅ | ✅ | ❌ |
| 按项目分布 | ✅ | ✅ | ❌ | ❌ |
| 缓存命中率 | ❌ | ✅ | ❌ | ❌ |
| 限流预测 | ✅ | ❌ | ❌ | ❌ |
| 中断统计 | ✅ | ❌ | ❌ | ❌ |
| 续跑成功率 | ✅ | ❌ | ❌ | ❌ |
| 恢复时间 | ❌ | ❌ | ❌ | ❌ |
| 热力图 | ❌ | ❌ | ❌ | ❌ |
| 导出报表 | ❌ | ✅ (CSV) | ❌ | ❌ |
| 多设备聚合 | ❌ | ❌ | ❌ | ✅ |
| Context 使用率 | ❌ | ❌ | ✅ | ✅ |
| 会话时长 | ❌ | ✅ | ❌ | ✅ |

**结论**：AgentPulse 在“守护行为统计”上独一无二（中断/续跑/核验），
但在“花费分析”维度上落后于 ccusage 和 VSCode 插件。
两者应该互补：把 ccusage 的花费深度 + AgentPulse 的守护统计 合为一体。

---

## 12. 深度审查：核心判断算法

> 审读对象：`detector/mod.rs`(884行)、`adapters/mod.rs`(270行)、
> `adapters/claude_code.rs`(603行)、`monitor/mod.rs`(1293行)、`resumer/mod.rs`(3190行)

### 12.1 算法架构总览

当前检测引擎是一个 **四信号融合 + 双层判定** 的架构：

```
信号采集层（适配器）
├── 进程存活（ProcessExited）
├── 文件新鲜度（FileStale）
├── 关键词匹配（KeywordMatch）
└── 心跳超时（HeartbeatTimeout）
        ↓
综合判定层（make_verdict）
├── Running        → 什么都不做
├── Suspicious     → 只观察，不动手
├── ConfirmInterrupt → 触发续跑流程
└── TaskCompleted  → 知会用户
        ↓
注意力分级层（grade_attention）—— 与判定正交
├── None         → 不打扰
├── NeedsInput   → 🔴 叫人
├── Completed    → 🟢 知会
├── RateLimited  → 🟡 告知
└── Error        → ⚫ 警报
        ↓
行动闸门层（monitor 三道闸）
├── 冷却检查（check_cooldown + 线性退避）
├── 额度检查（has_nudges_left，数连击不数累计）
└── 总开关（auto_resume_enabled）
        ↓
投递 + 核验层（resumer）
├── 定位（tmux/screen > iTerm2 API > TTY匹配 > 盲敲）
├── 投递（AppleScript / PowerShell / xdotool）
└── 核验（盯6s记录文件是否长出新内容）
```

### 12.2 算法优势（已经做对的）

| 设计决策 | 为什么是对的 |
|---|---|
| 判定与注意力正交 | “要不要动手”和“要不要叫人”是两个完全不同的问题，混在一起就会出现“放弃动手的同时也放弃叫人” |
| 散文 vs 结构分离 | `recent_output` 和 `error_output` 分开，避免「agent 谈论 500」被当成「发生了 500」 |
| TurnState 结构判定 | 不只看 mtime，而是看记录结构（工具调用未返回 = 在忙），解决了压缩上下文的误判 |
| 词边界匹配 | `contains_keyword` 避免 "1500 tokens" 命中 "500"，中文跳过边界检查 |
| 额度数连击不数累计 | 会话恢复就清零，避免“一辈子只准被催 5 次” |
| 失败不消耗额度 | 敲不进去不算催过，避免权限失效时自己把自己关掉 |
| 闭环核验 | 不只“脚本没报错”，而是“看见会话动了”，抓住“敲进隔壁窗口”的静默失败 |
| BUSY_GRACE_MULTIPLIER | 回合未收尾时阈值放宽 10 倍，避免压缩上下文时的误判 |

### 12.3 算法缺陷与改进建议

#### 缺陷 1：FileStale 和 HeartbeatTimeout 同源但双重计数

**现状**：代码注释已明确指出两者都来自 mtime，是「同一个事实的两种说法」。
但信号列表里仍然可能同时出现两者，前端展示时会让人以为有两个独立证据。

**建议**：合并为一个信号 `TranscriptIdle`，只报一次，附带两个阈值的信息。

#### 缺陷 2：无“中断原因”分类

**现状**：`Verdict::ConfirmInterrupt` 是一个统一结论，不区分“为什么中断”。
但续跑策略应该因原因而异：

| 中断原因 | 最优续跑策略 | 当前行为 |
|---|---|---|
| Rate limit | 等 ETA 后再续 + 专用提示词 | 统一提示词立即续 |
| 等待用户输入 | 通知用户，不自动续 | 通知 + 可能自动续 |
| 进程崩溃 | 不续，只通知 | 尝试续跑（但进程已死，敲了也没用） |
| 网络断开 | 等网络恢复再续 | 统一提示词立即续 |
| Context 满 | 建议开新会话 | 无感知 |

**建议**：在 `DetectionResult` 中加 `interrupt_reason: InterruptReason` 枚举：

```rust
pub enum InterruptReason {
    RateLimited,       // error_output 命中 rate_limit_keywords
    AwaitingInput,     // recent_output 命中 input_keywords
    ProcessCrashed,    // !process_alive
    NetworkError,      // error_output 含 ECONNRESET/ETIMEDOUT
    ContextExhausted,  // 检测到 compaction + 之后无新输出
    Unknown,           // 纯超时，无关键词
}
```

#### 缺陷 3：无“置信度”概念

**现状**：判定是二元的——要么 ConfirmInterrupt，要么不是。
但现实中存在“很可能中断但不完全确定”的灰色地带。

**场景**：
- 文件 5 分钟没动 + 进程活着 + 回合状态 Unknown（Codex）→ 确定中断？
- 文件 2 分钟没动 + 进程活着 + AwaitingUser → 可能只是在等用户思考

**建议**：加一个 `confidence: f32`（0.0–1.0），由信号强度加权：

```
进程死亡:                  confidence = 0.99
AwaitingUser + 停更 10min:  confidence = 0.95
AwaitingUser + 停更 2min:   confidence = 0.70
Unknown + 停更 10min:       confidence = 0.80
Unknown + 停更 2min:        confidence = 0.50
关键词命中 + 无停更:        confidence = 0.30
```

用途：
- confidence > 0.8 → 自动续跑
- 0.5–0.8 → 通知用户，用户决定
- < 0.5 → 只记录，不打扰

这也可以与已有的 `ai_judge` 配置对接：AI 判定作为 confidence 的一个加权因子。

#### 缺陷 4：无“中断模式学习”

**现状**：每轮扫描都是独立判定，不记得“这个会话上次中断是什么样子”。

**问题**：
- 某个会话每次 rate limit 后都会自己恢复，不需要续跑——但当前每次都会尝试
- 某个会话每次中断都是真的卡死，应该更快续跑——但当前要等同样的冷却时间

**建议**：在 `session_history` 中记录每个会话的中断模式：

```rust
struct SessionPattern {
    /// 历史中断次数
    total_interrupts: u32,
    /// 其中自己恢复的比例（不需要续跑）
    self_recovery_rate: f32,
    /// 平均中断持续时间（秒）
    avg_interrupt_duration: u64,
    /// 最常见的中断原因
    dominant_reason: InterruptReason,
    /// 续跑后平均多久恢复活动
    avg_resume_to_activity: u64,
}
```

用途：
- `self_recovery_rate > 0.7` → 延迟续跑，先等一会看它能不能自己恢复
- `dominant_reason == RateLimited` → 用限流专用提示词
- `avg_resume_to_activity` → 核验窗口可以缩短（不用等 6s）

#### 缺陷 5：TurnState 对非 Claude Code 适配器无效

**现状**：Codex 和 OpenCode 适配器的 `turn_state()` 始终返回 `Unknown`。
这意味着对这两个 agent，判定回退到纯超时逻辑，面监 BUSY_GRACE 放宽也没有。

**影响**：
- Codex CLI 在跑长命令时可能被误判为中断
- OpenCode 在压缩上下文时可能被误判

**建议**：
- Codex：读取 `~/.codex/sessions/*.jsonl` 的结构（如果有）
- OpenCode：检查其 API 或日志格式
- 通用回退：如果进程 CPU > 5%，则认为在忙（用 `sysinfo` 的 CPU 数据）

#### 缺陷 6：无“全局上下文”感知

**现状**：每个会话独立判定，不知道“整个系统”的状态。

**场景**：
- 3 个会话同时被判为中断——很可能是网络断了，不是 3 个 agent 都卡了
- 所有会话同时限流——说明是账户级别的限制，续跑也没用

**建议**：在 `scan_once` 中加一个“全局健康”判定：

```rust
// 如果超过 50% 的会话同时被判为中断，很可能是系统级问题
let interrupt_ratio = interrupted as f64 / total as f64;
if interrupt_ratio > 0.5 && total > 2 {
    // 不逐个续跑，而是发一条“可能网络断了”的通知
    // 等网络恢复后再统一续跑
}
```

### 12.4 算法演进路线图

```
v1.5（当前）          v1.7               v2.0
四信号融合          + 中断原因分类      + 模式学习
二元判定            + 置信度            + 自适应阈值
固定阈值            + 全局上下文感知    + AI 判定融合
统一续跑策略        + 按原因分派策略    + 按历史优化策略
```

### 12.5 与 AI Judge 的融合点

当前 `ai_judge` 配置已存在但未实现。它应该是 confidence 计算的一个因子：

```
最终 confidence = 规则引擎 confidence × 0.6 + AI 判定 confidence × 0.4

规则引擎：快、确定性强、但无法理解语义
AI 判定：慢、能理解“agent 在说什么”、但有延迟和成本

融合策略：
- 规则引擎 confidence > 0.9 → 直接用，不问 AI（进程死了没什么可问的）
- 规则引擎 confidence 0.5–0.9 → 问 AI，综合判定
- 规则引擎 confidence < 0.5 → 不续跑，但可以用 AI 决定是否通知
```

---

## 13. 深度审查：遗漏功能与盲区

> 以下是在完整审读 15,000+ 行代码后发现的“应该做但没做”的功能点

### 13.1 会话生命周期指标（完全缺失）

当前只有“当前状态”的快照，没有任何“过程”指标：

| 指标 | 价值 | 实现难度 |
|---|---|---|
| 会话存活时长 | 知道 agent 一般跑多久 | 低（first_seen → last_seen） |
| 有效工作时间 vs 等待时间 | 知道多少时间被浪费了 | 中（需记录状态转换时间戳） |
| 中断→恢复延迟 | 续跑系统的核心 KPI | 低（detection → resume 时间差） |
| 续跑后多久真正恢复活动 | 核验的补充：敲进去了但 agent 可能还在想 | 中（resume → 下一次文件变化） |
| 会话“产出”（文件修改数） | 判断 agent 是不是在“空转” | 高（需解析工具调用） |

### 13.2 导出与报表（完全缺失）

**痛点**：用户想给老板报告“这个月 AI 花了多少钱”，或者自己复盘“哪天花得最多”。

当前没有任何导出功能。建议：

| 格式 | 内容 | 触发方式 |
|---|---|---|
| CSV | 每日花费 / 每项目花费 / 续跑记录 | 统计页“导出”按钮 |
| JSON | 全量数据（给开发者 / 外部工具） | CLI `agentpulse export` |
| Markdown | 周报 / 月报摘要 | 自动生成或手动触发 |

### 13.3 会话“产出”感知（完全缺失）

**问题**：当前只知道 agent “在跑”或“停了”，不知道它“做了什么”。

Claude Code 的 JSONL 里其实有这些信息：
- `tool_use` 事件：Bash、Read、Write、Search 等
- 文件修改：`file-history-snapshot` / `file-history-delta`
- 工具调用次数和类型

可以非侵入地提取：

```
会话摘要卡片：
┌─────────────────────────────────────────┐
│ agent-pulse · Claude Code · 42min        │
│                                         │
│ 工具调用：Bash ×12, Read ×8, Write ×5  │
│ 文件修改：7 个文件                       │
│ Token：45.2k in / 12.8k out            │
│ 花费：$0.42                             │
│ 状态：🟢 运行中（正在跑测试）          │
└─────────────────────────────────────────┘
```

### 13.4 多显示器 / 多桌面感知（macOS 特有）

**问题**：续跑用 AppleScript 激活窗口时，如果终端在另一个桌面（Mission Control），
`activate` 会切换桌面，打断用户当前工作。

**建议**：
- 检测目标窗口是否在当前桌面
- 如果不在，优先用 `write text`（iTerm2）或 tmux send-keys，不切换桌面
- 只有不得不用合成按键时才切换

### 13.5 续跑提示词智能化（当前是固定字符串）

**现状**：`resume_prompt` 和 `goal_resume_prompt` 是用户配置的固定字符串。

**问题**：
- 如果中断原因是 rate limit，提示词应该是“限流已恢复，请继续”
- 如果是等待输入，应该是“是的，继续执行”
- 如果是崩溃重启，应该是“请从上次中断的地方继续”

**建议**：提示词模板化 + 变量替换：

```
模板："{reason_context}。请继续完成当前任务。"

reason_context 按原因填充：
- RateLimited: "限流已恢复"
- AwaitingInput: "确认继续"
- ProcessRestarted: "会话已重启"
- Unknown: ""
```

### 13.6 守护“健康报告”（完全缺失）

**问题**：用户不知道 AgentPulse 自己工作得怎么样。

建议加一个“守护健康报告”：

```
┌─ 守护健康 ─────────────────────────────┐
│                                         │
│ 运行时间：3 天 14 小时                  │
│ 扫描次数：12,847                        │
│ 发现中断：23 次                         │
│ 成功续跑：19 次 (83%)                   │
│ 续跑失败：4 次                          │
│   └─ 原因：权限失效 ×2, 焦点被抢 ×2    │
│ 核验结果：Landed 17, Silent 2           │
│ 通道健康：✅ 正常                       │
│                                         │
│ ⚠️ 建议：辅助功能权限将在重新构建后     │
│    失效，请提前检查。                    │
└─────────────────────────────────────────┘
```

### 13.7 “专注模式”与智能免打扰（完全缺失）

**现状**：通知节流是固定 `throttle_secs`，不区分场景。

**应该做的**：

| 场景 | 行为 |
|---|---|
| 用户正在打字（前台应用活跃） | 只推送 needs_input，其余延迟 |
| 用户锁屏 / 离开 | 全部静默，回来后一次性摘要 |
| 深夜（可配置时段） | 只记录，不弹通知 |
| 连续多个会话同时中断 | 合并为一条通知，不刷屏 |

### 13.8 会话“标签”与分组（完全缺失）

**痛点**：跑 8 个会话时，纯按注意力排序不够——用户想按“工作项目”和“副业项目”分开看。

**建议**：
- 自动按 `working_dir` 的父目录分组（已有基础）
- 允许用户手动打标签（存在 SQLite）
- 支持“只看某个项目”的过滤

### 13.9 与 Marc Nuri Dashboard 的差异化思考

Marc Nuri 的 AI Coding Agent Dashboard（2026 年 6 月发布）做了：
- 跨设备会话聚合（多机器 → 一个 Web 看板）
- 浏览器内终端附加（点击会话 → 嵌入 xterm.js）
- 远程启动会话（从手机 spawn 新 agent）
- 工作流模板（Implement Issue / Review PR）

**AgentPulse 不应该做的**：
- 不做会话启动（红线：不启动 agent）
- 不做终端嵌入（那是另一个产品形态）
- 不做任务编排（红线：不改变所有权）

**AgentPulse 应该借鉴的**：
- 跨设备可见性（已有 remote 模块，但只是本机看板的网络映射）
- 会话卡片的“一眼可读”设计（项目 + 分支 + 状态 + 上下文使用率）
- “ stale 检测”的可视化（多久没动了，用颜色渐变而不是二元状态）

### 13.10 安全与隐私盲区

| 项 | 现状 | 风险 |
|---|---|---|
| 会话文件内容 | 只读尾部，不存储 | ✅ 安全 |
| remote token | 明文存在 config.json | ⚠️ 应存入系统 keychain |
| webhook URL | 明文存在 config.json | ⚠️ 含 token 的 URL 应加密 |
| SQLite 数据库 | 无加密 | ⚠️ 含项目路径、花费数据 |
| 日志 | 可能含会话 ID、项目名 | 低风险，但应可配置详细级别 |

### 13.11 可访问性（a11y）盲区

当前前端完全没有考虑可访问性：
- 无 ARIA 标签（状态灯只用颜色区分，色盲用户无法分辨）
- 无键盘导航（Tab 顺序未定义）
- 无屏幕阅读器支持
- 字体大小固定（10px/11px 对视力不好的用户不友好）

### 13.12 综合优先级建议（补充到路线图）

| 优先级 | 项 | 章节 | 估时 |
|---|---|---|---|
| **P0** | 中断原因分类 + 按原因分派续跑策略 | 12.3-缺陷2 | 3d |
| **P0** | 统计页加“趋势对比”（今天 vs 昨天） | 11.2 | 2d |
| **P1** | 花费按模型分布 + 缓存命中率 | 11.3 | 2d |
| **P1** | 会话时长 + 恢复时间指标 | 13.1 | 2d |
| **P1** | CSV 导出 | 13.2 | 1d |
| **P1** | 全局上下文感知（多会话同时中断 = 系统问题） | 12.3-缺陷6 | 2d |
| **P2** | 置信度 + 自适应阈值 | 12.3-缺陷3 | 4d |
| **P2** | 中断模式学习 | 12.3-缺陷4 | 4d |
| **P2** | 会话产出感知（工具调用统计） | 13.3 | 3d |
| **P2** | 智能免打扰 | 13.7 | 2d |
| **P3** | 热力图（中断时间分布） | 11.2 | 2d |
| **P3** | 守护健康报告 | 13.6 | 1d |
| **P3** | 会话标签与分组 | 13.8 | 2d |

---

<p align="center">
<sub>本文档是产品思考而非技术规格。具体实现细节请参考 PROJECT_STATUS.md。<br>
所有建议均需经过“非侵入三红线”过滤：不启动 agent、不确定不动手、不改变所有权。</sub>
</p>
