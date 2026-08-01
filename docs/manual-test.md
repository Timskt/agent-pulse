# 三平台实机验证清单

> **准入规则**：每次发 `v*` tag 之前，至少把当前改动涉及的平台走一遍。
> 打勾 = 在真机上跑通并确认结果正确；不是"编译过了"就算。

## 0. 通用前置

- [ ] `cargo test` 全绿（含 `every_locate_script_compiles` 的 osacompile 实编译）
- [ ] `cargo clippy --all-targets -- -D warnings` 无警告
- [ ] `pnpm build`（tsc + vite）无错误
- [ ] 三平台 CI 矩阵（ubuntu / windows / macos）全绿

## 1. macOS

### 1.1 演练定位（零风险，先走这步）

对每个终端各开一个 agent 会话（Claude Code / Codex CLI / OpenCode），在会话卡片点「演练定位」：

| 终端 | 预期 level | 预期 message 关键词 | 通过 |
|---|---|---|---|
| iTerm2 | `exact` | `精确匹配 iTerm2 · tty /dev/ttys0xx` | ☐ |
| Terminal.app | `exact` | `精确匹配 Terminal · tty /dev/ttys0xx` | ☐ |
| VS Code 集成终端 | `window` | `窗口级匹配 Code · 标题含「项目名」` | ☐ |
| Cursor | `window` | `窗口级匹配 Cursor · 标题含「项目名」` | ☐ |
| Windsurf | `window` | `窗口级匹配 Windsurf · 标题含「项目名」` | ☐ |
| Warp | `refused`（默认） | `定位不到 Warp`（无脚本接口） | ☐ |
| 未安装 / 已退出的 App | `refused` | `应用未在运行` 或 `定位不到` | ☐ |

### 1.2 真实续跑（有风险，确认演练结果后再做）

| 终端 | 验证点 | 通过 |
|---|---|---|
| iTerm2 | 中文提示词完整落入正确标签，不变"啊啊啊" | ☐ |
| Terminal.app | 同上；多标签时只落到 TTY 对应的那个 | ☐ |
| VS Code | 提示词落入集成终端而非编辑器；焦点在编辑器时应拒绝 | ☐ |
| Warp（开盲敲） | 提示词落入前台 Warp 窗口 | ☐ |
| 盲敲=关 + 定位不到 | 不敲任何字，返回"定位不到"错误 | ☐ |

### 1.3 辅助功能权限

- [ ] 首次运行弹出"辅助功能"授权提示
- [ ] 授权后续跑正常；未授权时返回明确错误而不是静默失败

## 2. Windows

### 2.1 演练定位

| 终端 | 预期 level | 预期 message 关键词 | 通过 |
|---|---|---|---|
| cmd / conhost | `exact` | `精确匹配` + 窗口标题 | ☐ |
| Windows Terminal（单标签） | `exact` | 同上 | ☐ |
| Windows Terminal（多标签） | `window` | `窗口级匹配` + 标题含项目名 | ☐ |
| VS Code 集成终端 | `window` | `窗口级匹配 Code` | ☐ |
| Hyper / Tabby / ConEmu | `window` | 多标签宿主标题匹配 | ☐ |
| 找不到窗口 | `refused` | `定位不到` | ☐ |

### 2.2 真实续跑

| 终端 | 验证点 | 通过 |
|---|---|---|
| cmd | 中文提示词完整送入，无乱码 | ☐ |
| Windows Terminal 多标签 | 只送到标题匹配的那个标签 | ☐ |
| VS Code | 送入集成终端而非编辑器 | ☐ |
| 盲敲=关 + 找不到窗口 | 不发送，返回拒绝 | ☐ |

### 2.3 特殊环境

- [ ] Git Bash / MSYS2 下 TTY 为 None，走窗口标题路径
- [ ] 管理员权限终端 vs 普通权限终端的 UIPI 隔离提示

## 3. Linux

### 3.1 演练定位

| 终端 / 桌面 | 预期 level | 预期 message 关键词 | 通过 |
|---|---|---|---|
| GNOME Terminal (X11) | `window` | `窗口级匹配` + 窗口 ID | ☐ |
| Kitty / Alacritty (X11) | `window` | 同上 | ☐ |
| VS Code 集成终端 (X11) | `window` | 同上 | ☐ |
| 任何终端 (Wayland) | `refused` | `定位不到`（xdotool 不可用） | ☐ |

### 3.2 真实续跑

| 终端 / 桌面 | 验证点 | 通过 |
|---|---|---|
| GNOME Terminal (X11) | xdotool 聚焦 + 剪贴板粘贴成功 | ☐ |
| ydotool 路径 (Wayland) | uinput 权限就绪后可用；无权限时报明确错误 | ☐ |
| 盲敲=关 + 无窗口 | 不操作，返回拒绝 | ☐ |

### 3.3 权限

- [ ] `xdotool` 已安装（`apt install xdotool`）
- [ ] ydotool 的 uinput 权限（`/dev/uinput` 660 + 用户在 `input` 组）

## 4. 通知 & 手机看板

| 项 | 验证点 | 通过 |
|---|---|---|
| macOS 原生通知 | 中断时弹通知，点击跳到窗口 | ☐ |
| 声音 | 提示音播放、音量跟随配置 | ☐ |
| 手机看板 | 手机连同一 WLAN → `bind_all` → 浏览器打开 → 数据实时刷新 | ☐ |

## 5. 打包 & 发布

| 项 | 验证点 | 通过 |
|---|---|---|
| `v*` tag 触发 build-tauri | 4 个目标（aarch64/x86_64 macOS, x64 Windows, x64 Linux）全绿 | ☐ |
| Release 产物 | .dmg / .msi / .AppImage / .deb 均可下载安装 | ☐ |
| 首次启动 | 无崩溃、无白屏、权限提示正常 | ☐ |

---

*最后更新：v1.4.0 — 新增「演练定位」列（零风险验证定位链路）*
