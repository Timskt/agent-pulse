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
- **判定与动手分开** — 判定层只回答「现在是什么状态」，额度/冷却/总开关住在动作闸门里；催满次数只停手不闭嘴，改成叫人接管，不会静默放弃
- **安全边界内的静默自动续跑** — 自动模式只允许“精确目标 + 后台通道 + transcript/协议可验证”；当前已认可的安全后台通道是 tmux exact pane 与 iTerm2 exact TTY。外部 Windows console 无法按 PID 获得精确输入端点，因此自动路径安全延后，绝不抢焦点或盲敲当前窗口
- **手动才允许前台降级** — Terminal.app、Linux 普通终端、外部 Windows cmd/conhost、Windows Terminal/ConPTY、IDE 集成终端和 screen 非精确 window 不会被自动模式偷偷激活；只有用户明确点击续跑时才允许受控的前台定位/输入，未知目标仍拒绝
- **精确 prompt 闭环核验** — transport/API 返回成功 ≠ 字进去了。只有 transcript 基线之后新增的**整个 user message**与本次提示词逐字符精确相等才算 `Landed`（首尾空格/换行也不能被忽略）；数组内容必须恰好只有一个纯文本块，文本旁带图片或额外 block 也不会误判成功
- **两阶段续跑流水线** — 扫描只做发现、取证和生成动作；协调队列按精确运行时代际合并最新快照。窗口/剪贴板/键盘的不可逆输入严格串行，输入完成立即释放全局锁，不同会话最长 6 秒的只读落地核验并行进行：一个静默会话既不会拖住下一轮扫描，也不会堵住其他会话或用户手动续跑
- **纯 reducer + Attempt Ledger** — Rust 用连续观测 reducer 将 `Observing → Suspected → Eligible`，证据变化立即重置；SQLite attempt 账本以运行时代际、证据和提示词做幂等键，严格区分开始投递、transport ACK、verified、deferred 与 failed。危险未决状态按整个运行时代际阻断，改 prompt 也不能绕过；崩溃恢复只在单实例仲裁后的主实例执行并始终 fail closed
- **稳定逻辑会话历史** — Codex 仅在 `codex resume <UUID>` 与 transcript metadata 精确匹配时复用逻辑会话 ID；Claude 仅在 argv 中存在显式 `--session-id/--resume <UUID>` 且 cwd 对应目录唯一命中同名 transcript 时关联。裸会话、`--continue` 和无法证明身份的数据一律使用进程代际，不按 cwd 或前端去重猜测合并
- **续跑全链路防重与失效检查** — 同一运行时代际用 RAII 租约保证最多一个在途动作，手动/自动共用投递锁；每条监控循环绑定独立 lifecycle epoch，停止会清队列、推进代数并穿过不可逆投递 fence，保证 `stop()` 返回后旧生命周期不再落字；手动按钮回传 Rust 生成的不透明 `runtime_generation`，旧界面行不能在 PID 复用后重新绑定到新进程；真正敲字前重验开关、状态、额度、冷却、记录版本与进程代际
- **续跑演练（dry-run）** — 把定位链路完整走一遍但**一个字都不敲**，先告诉你会敲到哪个窗口、缺哪个依赖、要不要去开权限
- **Windows 外部控制台安全边界** — `AttachConsole` / `WriteConsoleInputW` 面向共享 console 输入缓冲，不能把输入精确绑定到某个外部 PID，因此不作为自动后台 transport。自动续跑返回安全延后；用户明确点击“续跑”后，才允许前台定位并用 Unicode `SendInput` 发送完整文本和独立 Enter。该手动路径不碰剪贴板、不发送 `Ctrl+V`、不弹 PowerShell 窗口，仍待 Windows 真机复测
- **定位不到就不敲** — 认不出会话属于哪个窗口时宁可放弃续跑，也不往别人的窗口里回车（想要兜底可在设置里打开「跟随最新会话」）
- **花费与限流洞察** — 增量读用量记录算 token 与美元花费，按天/按项目排行，还能预测多久撞到窗口限流
- **撞上限流就按住不敲** — 官方 429 之外，中转站的「上游负载已饱和」「`upstream_busy`」和 HTTP 形状也认；认出之后记下截止时刻（会读消息里自带的 `retrying in 34s` / 「30 秒后重试」），**之后不看证据只看时刻**——因为记录尾部只读 40 行，那行 429 迟早滚走，而限流窗口还没过去。在限流窗口里反复敲字是会让账号被封的行为
- **只读手机看板** — 默认**关闭**，开了也只听 `127.0.0.1`；需要手机看时才显式勾「允许局域网访问」（换成 `0.0.0.0`：同一个网络里的人拿到令牌就能看你的会话，请只在可信网络里开）。令牌必填、空令牌直接拒绝服务、比较用固定时间，结构上没有任何写路径
- **判定证据面板** — 会话卡片可展开查看本轮事实：信号、进程存活、回合结构、忙碌宽限、命中词和第二意见；前端只展示，不重算判定
- **结构化 AI 第二意见** — 只有关键词命中但证据不足时才问一次，严格接受 `DONE` / `CONTINUE`；失败或 `DONE` 不改变原判，忙碌回合永不询问
- **插件式适配器** — Claude Code / Codex CLI / OpenCode / 自定义，可扩展支持更多 Agent
- **安全机制** — 最大续跑连击数、冷却时间（随失败次数退避）、手动/自动模式切换
- **实时可观测** — 结构化日志流、状态面板、事件时间线、中英双语界面
- **桌面窗口自适应** — 主窗口最小宽度已降至 360px；顶栏、导航、历史筛选和续跑诊断会随窗口宽度重排，长路径/session ID/错误/prompt 可选择、复制或折行。360×700 正式窗口验收仍以 `docs/manual-test.md` 的未勾选清单为准
- **首次引导与多会话聚焦** — 空看板用三步说明如何开始守护，并明确“不启动/接管 Agent”；会话可按项目、Agent、终端搜索，并筛选等我、卡住或活跃状态

## 技术栈

| 层 | 技术 |
|---|---|
| 桌面框架 | Tauri 2.0 |
| 后端引擎 | Rust 2021 (sysinfo / notify / tokio / rusqlite) |
| 前端 | React 19 + TypeScript 5.8 + TailwindCSS 3 + Vite 6 |
| 组件层 | Radix UI + class-variance-authority |
| 状态管理 | Zustand |
| 测试 | Rust `cargo test`：325 passed；前端 vitest：103 passed（9 files）；`pnpm build` 通过 |

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

产物位于 `src-tauri/target/release/bundle/`。四平台一起出包走 CI，见下面
[打包与发布](#打包与发布)。

## 工作原理

```
┌──────────────┐    ┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│ 稳定会话身份  │ →  │ 连续观测归约  │ →  │ 安全通道许可  │ →  │ 精确提示词核验 │
│ 进程+记录元数据│    │ 证据 hash/阈值 │    │ Attempt 幂等  │    │ outcome/记账  │
└──────────────┘    └──────────────┘    └──────────────┘    └──────────────┘
```

1. **发现** — 适配器扫描系统中的 AI Agent 进程（Claude Code / Codex / OpenCode / 自定义）
2. **检测** — 轮询检查：会话文件是否停止更新、输出中是否有中断关键词、进程是否退出、回合有没有收尾
3. **决策** — 双重校验：有中断信号 + 无完成标记 → 确认中断（判定只说状态，动不动手由闸门决定）
4. **稳定性归约** — Rust 纯 reducer 对同一份结构证据做连续观测；只有从 `Observing` 经 `Suspected` 进入 `Eligible` 才生成自动动作
5. **能力许可与防重** — 自动策略检查目标确定性、后台可见性和验证能力；Attempt Ledger 用运行时代际 + 证据 + prompt 幂等键阻止重复投递，危险状态按整个运行时代际形成防重放栅栏；恢复只由单实例仲裁后的主实例执行
6. **协调与投递** — 动作按精确运行时代际合并；真实输入全局串行，只读核验跨会话并行。自动只走安全后台通道；无通道按 `next_retry_at` 延后，手动点击才允许前台降级
7. **精确核验** — 投递前记录 transcript 基线，之后只接受与本次 prompt 精确相等的新 user message；transport ACK、mtime 或无关记录增长都不算成功
8. **保护与历史** — 冷却、连击上限、lifecycle epoch 和 PID 启动代际防止旧动作复活；稳定逻辑会话进入档案，无法证明身份的旧数据保守保留为 legacy runtime

更详细的分层、判定依据和踩坑记录见 [docs/architecture.md](./docs/architecture.md)。

## 配置说明

配置文件位于 `~/Library/Application Support/agent-pulse/config.json`（macOS）。

常用配置项：

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| `poll_interval_secs` | 10 | 轮询间隔 |
| `idle_timeout_secs` | 60 | 空闲超时判定 |
| `max_resume_count` | 5 | 连续几次没效果就停手（会话一动就清零，不是终身上限）|
| `resume_cooldown_secs` | 30 | 续跑冷却时间（按连续失败次数线性退避，最多 6 倍）|
| `auto_follow_latest` | false | 定位不到时是否盲敲前台窗口 |
| `custom_keywords` | rate limit, overloaded... | 中断触发关键词 |
| `resume_prompt` | 请继续完成... | 续跑提示词 |

完整配置参考（20+ 主配置 + 7 个子配置）见 [PROJECT_STATUS.md § 10.3](./PROJECT_STATUS.md)。

## 路线图

- [x] **v1.0** — 核心引擎 + Claude Code / Codex CLI / OpenCode 适配器 + 三平台续跑 + Dashboard UI + 系统托盘
- [x] **v1.1** — 剪贴板投递 + 定位不到就不敲 + 三平台 CI 矩阵 + 实机验证清单
- [x] **v1.2** — 感知层（成本追踪 + 限流预测）+ 洞察层（AI 判定 + 统计面板）
- [x] **v1.3** — 远程层（手机看板 + Webhook）+ 会话历史 + i18n 中英双语
- [x] **v1.4** — tmux / screen 直投通道 + 续跑演练（dry-run）按钮 + 前端 vitest + 三平台验收清单
- [x] **v1.5** — 续跑闭环（落地核验 + 四态记账）+ 三个计数器分家 + 判定层与动作闸门分离 + 看板换绑竞态修复 + 局域网地址自动推导
- [x] **v1.6** — 判定证据面板 + 自定义适配器 UI + AI 第二意见仲裁 + 跨语言枚举/i18n 门禁
- [x] **v1.7** — 单实例守护 + 续跑记录中心 + 统计趋势对比 + 会话档案页 + 图表时间刻度 + CSV 导出 + 会话生命周期收拢（「关了还显示运行中」）
- [x] **v1.8** — 限流认得出来也按得住：中转站的说法与 HTTP 形状兜底识别 + 从消息里读等待时间 + **保持窗口**（证据滚出尾部 40 行之后仍然不敲字）
- [x] **v1.9** — 续跑协调器重构（扫描/投递解耦 + 按会话合并队列 + worker 串行投递 + RAII 租约 + stop 生命周期失效 + 并发状态合并 + PID 启动代际身份）+ 首次三步引导 + 多会话搜索与筛选
- [x] **v1.10** — 两阶段续跑流水线 + 连续观测 reducer + 安全 transport 能力门槛 + 无安全通道自动延后 + transcript 精确 prompt 核验 + Attempt Ledger + 稳定逻辑会话历史；外部 Windows console 自动路径延后，手动前台 Unicode `SendInput` 路径仍待真机复测

阶段性取舍、每一条的来由和候选清单见 [PROJECT_STATUS.md § 13](./PROJECT_STATUS.md)。
v1.10 推送后的续跑核心优先级与恢复顺序见 [docs/post-v1.10-plan.md](./docs/post-v1.10-plan.md)。
**v2.0 编排层 / v2.1+ 自治层与「非侵入」定位冲突，已明确搁置。**

## 打包与发布

**打包只认 `v*` 标签，推 `main` 不会打包。** `build-tauri` 和 `release` 两个 job 都带
`if: startsWith(github.ref, 'refs/tags/v')`，所以日常推代码只跑 lint 和测试（几分钟），
四平台产物 + GitHub Release 需要显式打一个标签。

```bash
# 1. 本地五道关全过再打标签——标签是对外的，不该拿它试错
cd src-tauri && cargo clippy --all-targets -- -D warnings && cargo test && cd ..
npx tsc --noEmit && pnpm test && pnpm build

# 2. 推代码，等 main 上的 CI 绿
git push origin main
gh run watch "$(gh run list --limit 1 --json databaseId --jq '.[0].databaseId')" --exit-status

# 3. 打标签并推标签（必须以 v 开头，且与 package.json 的版本一致——src/version.test.ts 会替你查）
git tag v1.10.0
git push origin v1.10.0

# 4. 盯打包
gh run list --limit 3
gh run watch <run-id> --exit-status
```

标签推上去之后：`check-rust`（ubuntu / macOS / Windows）和 `check-frontend` 全绿才会开始
`build-tauri`，四个目标各自产出并上传产物，最后由 `release` 汇总建 Release：

| 目标 | 产物 |
|---|---|
| `aarch64-apple-darwin` | `.dmg`（Apple Silicon） |
| `x86_64-apple-darwin` | `.dmg`（Intel） |
| `x86_64-unknown-linux-gnu` | `.deb` / `.rpm` / `.AppImage`（钉在 ubuntu-22.04，glibc 向下兼容） |
| `x86_64-pc-windows-msvc` | `.msi` / `.exe`（NSIS） |

标签打错了：`git tag -d v1.5.0 && git push origin :refs/tags/v1.5.0`，重新打；已生成的
Release 要手动删。macOS 产物**没有 Apple 开发者签名**，首次打开要右键 → 打开。

### macOS：让「辅助功能」授权不再每次构建就失效

macOS 把辅助功能授权挂在**代码签名**上，不是路径也不是 bundle id。没有证书时
Tauri 只能临时签名（adhoc），授权实际绑在那一个二进制的哈希上——改一行代码重新
构建，哈希就变了，系统设置里那个勾还在（记的是旧哈希）、实际已经不生效。
这就是「我明明勾选了却还是敲不进去」的真正原因（详见
[architecture.md § 12.1](./docs/architecture.md)）。

本地开发用一张自签名证书就能解决，跑一次：

```bash
pnpm macos:signing-identity                       # 造证书，只需一次
export APPLE_SIGNING_IDENTITY="AgentPulse Self-Signed"
pnpm tauri:build
```

装好之后去「系统设置 › 隐私与安全性 › 辅助功能」**取消再勾一次**——这是最后一次，
之后重新构建都不会再掉。自签名只解决「授权能不能留住」，不能公证，**对外分发仍需
真的 Developer ID**：把证书导成 base64 存进仓库 secrets，CI 里设
`APPLE_CERTIFICATE` / `APPLE_CERTIFICATE_PASSWORD` / `APPLE_SIGNING_IDENTITY`
三个环境变量，`tauri build` 会自己认——**签名身份故意不写进 `tauri.conf.json`**，
写死了没有这张证书的人就构建不了。

完整说明见 § 11.4。

## License

MIT
