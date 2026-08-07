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
- **跨平台静默续跑** — macOS (AppleScript) / Windows (PowerShell + Win32) / Linux (xdotool·ydotool)，覆盖 iTerm2、Terminal、Windows Terminal、cmd·conhost，以及 VS Code / Cursor / Windsurf / JetBrains 全家桶里的内置终端
- **tmux / screen 直投** — 跑在复用器里的会话按 pane id 寻址，不需要辅助功能授权、不需要窗口在前台、不过输入法（续跑不稳时最有效的一招）
- **敲完要核验** — 投递成功 ≠ 字进去了。续跑后回头比对会话记录的指纹，`落地 / 静默 / 失败 / 无法核验` 四态分开记账，通道坏了当场出声而不是默默烧完额度
- **两阶段续跑流水线** — 扫描只做发现、取证和生成动作；协调队列按会话合并最新快照。窗口/剪贴板/键盘的不可逆输入严格串行，输入完成立即释放全局锁，不同会话最长 6 秒的只读落地核验并行进行：一个静默会话既不会拖住下一轮扫描，也不会堵住其他会话或用户手动续跑
- **续跑全链路防重与失效检查** — 同会话用 RAII 租约保证最多一个在途动作，手动/自动共用投递锁；停止会清队列并推进生命周期代数，旧动作即使 stop/start 也不能复活；真正敲字前重验开关、状态、额度、冷却、记录版本以及 PID + 进程启动代际，旧动作宁可取消也不补敲
- **续跑演练（dry-run）** — 把定位链路完整走一遍但**一个字都不敲**，先告诉你会敲到哪个窗口、缺哪个依赖、要不要去开权限
- **提示词走安全文本通道** — macOS/Linux 用可还原的剪贴板粘贴，Windows 用 Win32 Unicode `SendInput`；绕开输入法，也避免 Codex 把 `Ctrl+V` 误判为粘贴图片
- **定位不到就不敲** — 认不出会话属于哪个窗口时宁可放弃续跑，也不往别人的窗口里回车（想要兜底可在设置里打开「跟随最新会话」）
- **花费与限流洞察** — 增量读用量记录算 token 与美元花费，按天/按项目排行，还能预测多久撞到窗口限流
- **撞上限流就按住不敲** — 官方 429 之外，中转站的「上游负载已饱和」「`upstream_busy`」和 HTTP 形状也认；认出之后记下截止时刻（会读消息里自带的 `retrying in 34s` / 「30 秒后重试」），**之后不看证据只看时刻**——因为记录尾部只读 40 行，那行 429 迟早滚走，而限流窗口还没过去。在限流窗口里反复敲字是会让账号被封的行为
- **只读手机看板** — 默认**关闭**，开了也只听 `127.0.0.1`；需要手机看时才显式勾「允许局域网访问」（换成 `0.0.0.0`：同一个网络里的人拿到令牌就能看你的会话，请只在可信网络里开）。令牌必填、空令牌直接拒绝服务、比较用固定时间，结构上没有任何写路径
- **判定证据面板** — 会话卡片可展开查看本轮事实：信号、进程存活、回合结构、忙碌宽限、命中词和第二意见；前端只展示，不重算判定
- **结构化 AI 第二意见** — 只有关键词命中但证据不足时才问一次，严格接受 `DONE` / `CONTINUE`；失败或 `DONE` 不改变原判，忙碌回合永不询问
- **插件式适配器** — Claude Code / Codex CLI / OpenCode / 自定义，可扩展支持更多 Agent
- **安全机制** — 最大续跑连击数、冷却时间（随失败次数退避）、手动/自动模式切换
- **实时可观测** — 结构化日志流、状态面板、事件时间线、中英双语界面
- **首次引导与多会话聚焦** — 空看板用三步说明如何开始守护，并明确“不启动/接管 Agent”；会话可按项目、Agent、终端搜索，并筛选等我、卡住或活跃状态

## 技术栈

| 层 | 技术 |
|---|---|
| 桌面框架 | Tauri 2.0 |
| 后端引擎 | Rust 2021 (sysinfo / notify / tokio / rusqlite) |
| 前端 | React 19 + TypeScript 5.8 + TailwindCSS 3 + Vite 6 |
| 组件层 | Radix UI + class-variance-authority |
| 状态管理 | Zustand |
| 测试 | `cargo test`（macOS / Linux 263；Windows 264）+ vitest（99） |

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
│  适配器发现   │ →  │  检测策略引擎 │ →  │  续跑执行器   │ →  │  落地核验     │
│  进程扫描     │    │  多信号融合   │    │  通道优先级   │    │  记录指纹     │
│  会话文件     │    │  动作闸门     │    │  剪贴板投递   │    │  四态记账     │
└──────────────┘    └──────────────┘    └──────────────┘    └──────────────┘
```

1. **发现** — 适配器扫描系统中的 AI Agent 进程（Claude Code / Codex / OpenCode / 自定义）
2. **检测** — 轮询检查：会话文件是否停止更新、输出中是否有中断关键词、进程是否退出、回合有没有收尾
3. **决策** — 双重校验：有中断信号 + 无完成标记 → 确认中断（判定只说状态，动不动手由闸门决定）
4. **协调** — 动作进入按会话合并的队列；扫描立即结束。worker 绕过仍在核验的忙会话，只有窗口/剪贴板/键盘的真实投递全局串行；停止守护会清空待处理动作，生命周期代数阻止旧动作在重启后复活
5. **续跑** — 出队后重新确认记录、状态和进程启动代际仍与检测时一致，再优先 tmux/screen 直投；否则跨平台定位投递：macOS (AppleScript) / Windows（隐藏 PowerShell 定位器 + Win32 Unicode `SendInput`）/ Linux (xdotool·ydotool)；Windows 不借剪贴板、不发送 `Ctrl+V`，避免 Codex 把续跑误判为粘贴图片
6. **核验** — 回头看 agent 的会话记录有没有长出新内容，确认那句话真的落地了
7. **保护** — 冷却退避 + 连击上限 + 定位不到就不敲，防止无限循环和误敲；停手时改为叫人，不静默放弃

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
- [x] **v1.10** — 两阶段续跑流水线（不可逆投递严格串行 + 跨会话只读核验并行 + 忙会话绕行队列 + owned RAII 租约）+ Rust 单一来源的待投递/核验状态可视化

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
