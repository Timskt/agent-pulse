//! 后端文案的国际化
//!
//! 边界约定：**谁渲染，谁持有文案**。
//! - 本模块只管后端直接呈现给用户的文字：系统通知、托盘菜单、注意力级别名称。
//! - 界面上的文字由前端 `src/i18n/index.ts` 持有（那边有类型安全的 key 和插值）。
//!
//! 两份字典都由同一个 `config.language` 驱动，所以不会出现「界面中文、通知英文」
//! 那种土不土洋不洋的搭配。
//!
//! 表格用 `(key, zh, en)` 三元组：两种语言写在同一行，漏翻一眼就能看出来。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 支持的语言
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lang {
    Zh,
    En,
}

impl Lang {
    pub fn as_str(&self) -> &'static str {
        match self {
            Lang::Zh => "zh",
            Lang::En => "en",
        }
    }

    pub fn from_code(s: &str) -> Self {
        match s {
            "en" => Lang::En,
            _ => Lang::Zh,
        }
    }
}

/// `(key, 中文, English)`
const TABLE: &[(&str, &str, &str)] = &[
    // ── 通知标题 ──
    ("notify.needs_input.title", "Agent 在等你回应", "Agent is waiting for you"),
    ("notify.completed.title", "任务已完成", "Task completed"),
    ("notify.rate_limited.title", "已触发限流，正在等待", "Rate limited — waiting it out"),
    ("notify.error.title", "会话出错", "Session error"),
    ("notify.resumed.title", "已自动续跑", "Auto-resumed"),
    ("notify.budget.title", "成本预警", "Budget alert"),
    ("notify.rate_forecast.title", "限流预警", "Rate limit warning"),
    ("notify.resume_broken.title", "自动续跑敲不进去了", "Auto-resume can't type anymore"),
    ("notify.test.title", "AgentPulse 通知测试", "AgentPulse test notification"),
    ("notify.test.body", "如果你看到这条通知，说明提醒通道已经打通。", "If you can see this, notifications are working."),
    // ── 注意力级别 ──
    ("attention.none", "正常", "Normal"),
    ("attention.needs_input", "等待输入", "Needs input"),
    ("attention.completed", "已完成", "Completed"),
    ("attention.rate_limited", "限流等待", "Rate limited"),
    ("attention.error", "出错", "Error"),
    // ── 托盘菜单 ──
    ("tray.show", "显示主窗口", "Show window"),
    ("tray.start", "开始监控", "Start monitoring"),
    ("tray.stop", "停止监控", "Stop monitoring"),
    ("tray.scan", "立即扫描", "Scan now"),
    ("tray.quit", "退出 AgentPulse", "Quit AgentPulse"),
    ("tray.tooltip", "AgentPulse · AI Agent 守护", "AgentPulse · AI agent guardian"),
    ("tray.pending", "个会话在等你", "session(s) need you"),
    // ── 通知正文（带 {占位符}，经 tf() 插值）──
    ("notify.budget.daily_body", "今日已花费 ${spent}，是预算 ${budget} 的 {percent}%", "Spent ${spent} today — {percent}% of the ${budget} daily budget"),
    ("notify.budget.session_body", "{label} 已花费 ${spent}，超过单会话预算 ${budget}", "{label} has spent ${spent}, past the ${budget} per-session budget"),
    ("notify.rate_forecast.body", "{window} 小时窗口已用 {percent}%，按当前速度约 {minutes} 分钟后触发限流", "{percent}% of the {window}h window used — about {minutes} min to the limit at this rate"),
    // 这条是「静默功能坏掉时必须吵一次」那条纪律的落点：自动续跑连着失败，
    // 用户是看不出来的——屏幕上「没人替我按继续」和「它正常在守着」长得一样
    (
        "notify.resume_broken.body",
        "{label} 连续 {count} 次没能把提示词敲进去，自动续跑现在等于没在工作：{detail}",
        "{label} failed to receive the prompt {count} times in a row — auto-resume is effectively off: {detail}",
    ),
    // ── 引擎日志（前端 Activity Log 直接展示）──
    ("log.engine_started", "监控引擎已启动，开始守护 AI Agent 会话", "Monitoring started — your agent sessions are being watched"),
    ("log.engine_stopped", "监控引擎已停止", "Monitoring stopped"),
    ("log.interrupt_detected", "[{agent}] 检测到中断信号：{signals}", "[{agent}] Interruption detected: {signals}"),
    ("log.cooldown_skip", "续跑冷却中，跳过本次触发", "Still cooling down — skipping this resume"),
    // 额度用光。刻意跟冷却分成两条：冷却是「等一会儿就好」，这条是「等也没用，
    // 得人去看一眼」，两句话对用户的要求完全不同，合成一句就等于什么也没说
    (
        "log.nudges_exhausted",
        "[{agent}] 连着催了 {count} 次都没见它动，先不敲了——这一个交给你看一眼",
        "[{agent}] {count} nudges in a row with no movement — standing down; this one needs you",
    ),
    ("log.suspicious", "疑似中断，继续观察", "Possibly interrupted — keeping watch"),
    (
        "log.arbitration_answered",
        "[{agent}] AI 第二意见：{verdict}",
        "[{agent}] AI second opinion: {verdict}",
    ),
    (
        "log.arbitration_failed",
        "[{agent}] AI 第二意见没拿到，维持原判：{detail}",
        "[{agent}] AI second opinion unavailable; keeping the original ruling: {detail}",
    ),
    ("arbitration.finished", "这一轮已完成", "this turn is finished"),
    ("arbitration.unfinished", "这一轮还没完成", "this turn is unfinished"),
    ("log.resume_sent", "已触发续跑（{mode}，第 {count} 次）：{detail}", "Resume sent ({mode}, attempt {count}): {detail}"),
    ("log.resume_failed", "续跑失败：{detail}", "Resume failed: {detail}"),
    ("log.resume_manual", "手动续跑：{detail}", "Manual resume: {detail}"),
    // 开工前那次体检的结论。只写日志不弹窗：tmux / screen / iTerm2 三条通道
    // 不需要辅助功能授权，对用那几条路的人来说弹窗就是误报
    (
        "log.channel_unhealthy",
        "投递通道体检没过，自动续跑现在可能敲不进去：{detail}",
        "The delivery channel failed its check — auto-resume may not be able to type: {detail}",
    ),
    ("log.mode_goal", "Goal 恢复", "goal recovery"),
    ("log.mode_generic", "通用", "generic"),
    ("log.alerted", "已提醒：{label}（{level}）", "Alerted: {label} ({level})"),
    ("log.usage_added", "新增 {count} 条用量记录，今日累计 ${cost}", "{count} new usage record(s) — ${cost} spent today"),
    ("log.heartbeat", "心跳：会话 {total} 个，活跃 {active}，中断 {interrupted}", "Heartbeat: {total} session(s), {active} active, {interrupted} interrupted"),
    ("log.focused", "已跳转到会话所在终端：{detail}", "Jumped to the session's terminal: {detail}"),
    // ── 外部推送（Slack / Discord / ntfy / Bark）──
    ("push.title", "AgentPulse · {verdict}", "AgentPulse · {verdict}"),
    ("push.verdict_interrupt", "检测到中断", "Interrupted"),
    ("push.verdict_resume", "已自动续跑", "Auto-resumed"),
    ("push.verdict_complete", "任务完成", "Task completed"),
    ("push.complete_body", "Agent 已经把活儿干完了", "The agent finished its task"),
    ("push.test_title", "AgentPulse 推送测试", "AgentPulse push test"),
    ("push.test_body", "看到这条就说明推送通道是通的。", "If this reached you, the push channel works."),
    ("push.test_ok", "发送成功（HTTP {status}）", "Sent (HTTP {status})"),
    // ── 命令返回的错误（前端直接弹给用户）──
    ("err.already_running", "监控已经在运行了", "Monitoring is already running"),
    ("err.session_not_found", "找不到这个会话，可能已经退出了", "That session is gone — it may have exited"),
    ("err.push_url_missing", "还没填推送地址或主题", "No endpoint or topic configured yet"),
    ("err.push_request", "请求失败：{detail}", "Request failed: {detail}"),
    ("err.push_status", "服务端返回 HTTP {status}", "The server returned HTTP {status}"),
    ("err.notify_failed", "系统通知发不出去：{detail}", "The system notification failed: {detail}"),
    ("err.remote_bind", "看板地址 {addr} 打不开：{detail}", "Could not bind the dashboard to {addr}: {detail}"),
    // ── 会话状态（手机看板自己渲染，所以文案在后端）──
    ("status.active", "运行中", "Running"),
    ("status.suspended", "疑似中断", "Maybe stuck"),
    ("status.interrupted", "已中断", "Interrupted"),
    ("status.completed", "已完成", "Completed"),
    ("status.exited", "已退出", "Exited"),
    // ── 手机看板（v1.3 远程层）──
    ("remote.title", "AgentPulse 看板", "AgentPulse dashboard"),
    ("remote.readonly", "只读", "Read-only"),
    ("remote.watching", "守护中", "Watching"),
    ("remote.idle", "未启动", "Idle"),
    ("remote.metric_sessions", "会话", "Sessions"),
    ("remote.metric_pending", "等你回应", "Waiting on you"),
    ("remote.metric_cost", "今日花费", "Spent today"),
    ("remote.empty", "现在没有正在跑的 Agent", "No agents running right now"),
    ("remote.resumed", "已续跑 {count} 次", "resumed {count}×"),
    ("remote.updated", "{time} 更新", "updated {time}"),
    ("remote.offline", "连不上 AgentPulse，电脑上的应用可能已经退出", "Can't reach AgentPulse — the desktop app may have quit"),
    ("log.remote_started", "手机看板已启动：{url}", "Phone dashboard is up: {url}"),
    ("log.remote_lan", "看板已开放到局域网：同一网络里拿到令牌的人都能读你的会话", "The dashboard is open to the LAN — anyone on it with the token can read your sessions"),
    // 手机要连的是这个地址：0.0.0.0 只是「听所有网卡」，手机上敲它连不上，
    // 敲 127.0.0.1 连的是手机自己。少了这一行，用户只能去系统设置里翻 IP。
    ("log.remote_lan_url", "手机上打开这个地址：{url}（令牌请用设置页的「复制带令牌的链接」）", "Open this on your phone: {url} (use “Copy link with token” in Settings for the token)"),
    ("log.remote_weak_token", "看板开到了局域网，但令牌只有几个字符：这时候令牌就是唯一那道门，建议至少 {min} 位，设置页可以一键生成", "The dashboard is on the LAN but the token is very short — it is the only lock on the door. Use at least {min} characters; Settings can generate one."),
    ("log.remote_stopped", "手机看板已停止", "Phone dashboard stopped"),
    // ── 续跑与聚焦的结果文案 ──
    //
    // 这些字符串会被塞进 `log.resume_sent` / `log.resume_failed` 的 `{detail}`，
    // 是实打实要给用户看的，所以不能留在 `resumer` 里硬编码中文。
    (
        "resume.sent",
        "已通过 {terminal} 发送续跑指令（{outcome}，TTY {tty}）",
        "Resume prompt sent via {terminal} ({outcome}, TTY {tty})",
    ),
    ("resume.matched", "已精确匹配到窗口", "matched the exact window"),
    ("resume.followed", "回退到当前窗口", "fell back to the current window"),
    ("resume.outcome_other", "结果 {raw}", "result {raw}"),
    // ── 投递核验的四种结论 ──
    //
    // 这四条是 v1.5 那个架构改动露到界面上的部分：以前日志只能说「脚本没报错」，
    // 现在能说清「字到底进去了没有」。区分「盯完没动」和「没法核验」很要紧——
    // 前者是真出问题了，后者只是这类 agent 不落盘，别把两者混成一句话。
    ("resume.outcome_landed", "已确认会话动起来了", "confirmed the session picked it up"),
    (
        "resume.outcome_silent",
        "按键发出去了，但盯了几秒会话一点没动，很可能敲进了别的窗口",
        "keystrokes went out but the session didn't budge — they likely landed in another window",
    ),
    (
        "resume.outcome_unverified",
        "已发送，这类会话没有可读的记录文件，核验不了",
        "sent — this kind of session keeps no readable transcript, so it can't be verified",
    ),
    ("resume.outcome_failed", "没能送达", "could not be delivered"),
    (
        "resume.no_terminal",
        "认不出这个会话在哪个终端里，没有动手",
        "Could not tell which terminal this session lives in — nothing was typed",
    ),
    // 定位不到窗口时宁可不做：往错误的窗口里敲一句中文再回车，比不续跑糟糕得多
    (
        "resume.blind_refused",
        "没能定位到这个会话的窗口，为免打错地方已放弃续跑；确实想让它兜底的话，去设置里打开「跟随最新会话」",
        "Couldn't locate this session's window, so nothing was typed — enable \"Follow the newest session\" in settings if you want a blind fallback",
    ),
    ("resume.script_failed", "终端脚本执行出错：{detail}", "The terminal script failed: {detail}"),
    // Windows 的 SetForegroundWindow 在后台进程里经常被系统拒掉，
    // 而按键打的是「当时的前台窗口」——切不过去就必须放弃，不能赌
    (
        "resume.focus_failed",
        "没能把这个会话的窗口切到前台（系统拒绝了），为免把提示词敲进别的窗口已放弃",
        "Couldn't bring this session's window to the front (the OS refused), so nothing was typed — it would have gone to whatever window was focused",
    ),
    (
        "resume.app_not_running",
        "这个会话所在的应用已经不在运行了",
        "The app hosting this session is no longer running",
    ),
    ("resume.unsupported", "当前平台不支持自动续跑", "Auto-resume isn't supported on this platform"),
    // TTY / 窗口是 macOS 才有的定位手段，Windows 和 Wayland 上只能给个笼统结果
    ("resume.sent_simple", "已发送续跑指令（{outcome}）", "Resume prompt sent ({outcome})"),
    (
        "resume.sent_window",
        "已通过 {tool} 发送续跑指令（窗口 {window}）",
        "Resume prompt sent via {tool} (window {window})",
    ),
    ("resume.frontmost_app", "前台终端", "the frontmost terminal"),
    ("resume.tty_unknown", "未知", "unknown"),
    (
        "resume.tool_missing",
        "没找到 xdotool 或 ydotool，装一个再试：sudo apt install xdotool",
        "Neither xdotool nor ydotool is installed — try: sudo apt install xdotool",
    ),
    ("resume.tool_failed", "{tool} 输入失败", "{tool} could not type the text"),
    (
        "resume.clipboard_missing",
        "中文提示词需要借剪贴板投递，但没找到 wl-copy / xclip / xsel，装一个再试：sudo apt install wl-clipboard",
        "Non-ASCII prompts are delivered via the clipboard, but none of wl-copy / xclip / xsel is installed — try: sudo apt install wl-clipboard",
    ),
    ("resume.no_window", "找不到 PID {pid} 对应的终端窗口", "No terminal window found for PID {pid}"),
    // ── tmux / screen 通道 ──
    //
    // 这是确定性最高的一条路：按 pane id 寻址，不需要窗口在前台，
    // 也完全绕开输入法——中文提示词不会再被拼音候选拆成「啊啊啊」。
    (
        "resume.sent_mux",
        "已通过 {tool} 直接写入 {target}（不经输入法）",
        "Typed straight into {target} via {tool} (bypassing the IME)",
    ),
    ("resume.mux_failed", "{tool} 投递失败：{detail}", "{tool} could not deliver the text: {detail}"),
    // macOS 的合成按键要「辅助功能」权限。没授权时脚本前半段（切窗口）照样成功，
    // 后半段 keystroke 静默失败——用户看到的就是「跳过去了，然后什么都没发生」。
    (
        "resume.needs_accessibility",
        "窗口已经切过去了，但系统不允许本应用模拟键盘，所以一个字都没敲进去。请到「系统设置 › 隐私与安全性 › 辅助功能」里勾上 AgentPulse（如果已经勾了，取消再勾一次——应用更新过后这条授权会失效）。或者在 tmux 里跑 Agent，那条路不需要任何权限。",
        "The window came forward, but the OS blocked this app from simulating the keyboard, so nothing was typed. Enable AgentPulse under System Settings › Privacy & Security › Accessibility (if it is already on, toggle it off and back on — the grant breaks whenever the app is updated). Alternatively, run your agents inside tmux: that path needs no permission at all.",
    ),
    // ── 续跑演练（dry-run）──
    //
    // 走完全部定位流程但一个字都不敲，把「要冒险按一次才知道」变成「随时可查」。
    ("probe.certainty_exact", "能精确定位", "Exact target"),
    ("probe.certainty_window", "只能定位到窗口", "Window-level only"),
    ("probe.certainty_none", "定位不到", "Can't locate it"),
    (
        "probe.detail_exact",
        "会通过{channel}投递到 {target}，这是能拿到的最强证据，敲错地方的可能性基本没有。",
        "Will deliver to {target} via {channel}. That's the strongest evidence available — there's essentially no chance of typing into the wrong place.",
    ),
    (
        "probe.detail_window",
        "会通过{channel}投递到窗口 {target}。窗口是对的，但窗口里当前是哪个标签/面板无从得知——如果你在这个窗口里开了多个标签，可能敲到隔壁那个。",
        "Will deliver to window {target} via {channel}. The window is right, but which tab or pane is currently selected inside it can't be known — if you have several tabs open there, the text may land in the neighbouring one.",
    ),
    (
        "probe.detail_none",
        "认不出这个会话的窗口，所以续跑会直接放弃，不会乱敲。想让它兜底投到前台终端的话，去设置里打开「跟随最新会话」。",
        "This session's window can't be identified, so a resume would give up rather than type blindly. Turn on \"Follow the newest session\" in settings if you want it to fall back to the frontmost terminal.",
    ),
    (
        "probe.detail_none_blind",
        "认不出这个会话的窗口。你已经打开了「跟随最新会话」，所以续跑会投到当时的前台终端——那可能不是这个会话。",
        "This session's window can't be identified. Since \"Follow the newest session\" is on, a resume will type into whatever terminal is frontmost at that moment — which may not be this session.",
    ),
    (
        "probe.no_accessibility",
        "另外：本应用还没拿到「辅助功能」权限，所以合成按键会静默失效（表现就是窗口跳过来了、字没敲进去）。到「系统设置 › 隐私与安全性 › 辅助功能」里勾上 AgentPulse；已经勾了的话取消再勾一次。",
        "Also: this app hasn't been granted Accessibility permission, so synthetic keystrokes fail silently — the window comes forward and nothing gets typed. Enable AgentPulse under System Settings › Privacy & Security › Accessibility; if it's already on, toggle it off and back on.",
    ),
    ("probe.channel_tmux", "tmux 面板", "the tmux pane"),
    ("probe.channel_screen", "screen 会话", "the screen session"),
    ("probe.channel_iterm2", "iTerm2 标签", "the iTerm2 tab"),
    ("probe.channel_terminal", "终端标签", "the Terminal tab"),
    ("probe.channel_ide", "IDE 窗口", "the IDE window"),
    ("probe.channel_x11", "X11 窗口", "the X11 window"),
    ("probe.channel_console", "控制台窗口", "the console window"),
    ("probe.channel_frontmost", "前台终端", "the frontmost terminal"),
    ("probe.channel_unknown", "未知通道", "an unknown channel"),
    ("probe.tool_tmux", "按面板 id 精确投递，绕开输入法", "Delivers by pane id, bypassing the IME"),
    ("probe.tool_screen", "投递到会话当前选中的窗口", "Delivers to the session's currently selected window"),
    ("probe.tool_accessibility_name", "辅助功能权限", "Accessibility permission"),
    (
        "probe.tool_accessibility",
        "模拟键盘的前提；没有它，除 tmux 和 iTerm2 之外的通道全部失效",
        "Required to simulate the keyboard; without it every channel except tmux and iTerm2 fails",
    ),
    ("probe.tool_xdotool", "X11 下定位窗口并输入", "Locates windows and types under X11"),
    ("probe.tool_ydotool", "Wayland 下输入的兜底方案", "The fallback for typing under Wayland"),
    ("probe.tool_clipboard", "投递非 ASCII 提示词要借剪贴板", "Non-ASCII prompts are delivered via the clipboard"),
    ("probe.tool_powershell", "定位控制台窗口并输入", "Locates the console window and types into it"),
    (
        "probe.settings_opened",
        "已打开「辅助功能」设置页。请在列表里找到 AgentPulse：没有就点 + 号加进去，已经勾上的话取消再勾一次——应用更新过后旧的授权不再生效。",
        "Opened the Accessibility settings pane. Find AgentPulse in the list: add it with + if it isn't there, and if it is already checked, uncheck it and check it again — the old grant stops applying once the app has been updated.",
    ),
    (
        "probe.settings_unsupported",
        "只有 macOS 需要这个权限",
        "Only macOS needs this permission",
    ),
    ("focus.done", "已跳到 {terminal}（{outcome}）", "Jumped to {terminal} ({outcome})"),
    ("focus.app_only", "只激活了应用", "brought the app forward only"),
    ("focus.failed", "跳转失败：{detail}", "Couldn't jump: {detail}"),
    (
        "focus.no_window",
        "找不到这个会话对应的窗口（Wayland 下没法聚焦）",
        "No window found for this session (focusing isn't possible on Wayland)",
    ),
    ("focus.unsupported", "当前平台不支持跳转", "Jumping to a terminal isn't supported on this platform"),
    (
        "focus.no_terminal",
        "认不出这个会话在哪个终端里",
        "Could not tell which terminal this session lives in",
    ),
    ("focus.done_simple", "已跳到会话窗口（{outcome}）", "Jumped to the session window ({outcome})"),
    // ── 续跑演练（dry-run）的活动日志 ──
    //
    // 面板上那一大段多行结论留给面板；日志只留一行摘要，否则一次演练就把日志刷满。
    // 用户报「续跑没反应」时截图的往往就是日志，有这一行就不用再问「你演练过吗」。
    (
        "log.probe",
        "续跑演练：{certainty} · 通道 {channel}",
        "Resume dry run: {certainty} · channel {channel}",
    ),
    // ── 外部命令（osascript / xdotool / PowerShell）的失败原因 ──
    //
    // 这些字符串会经 Err 一路冒到 Activity Log 里，同样是用户可见文案。
    ("cmd.spawn_failed", "启动 {program} 失败：{detail}", "Could not start {program}: {detail}"),
    ("cmd.failed", "{program} 执行失败：{detail}", "{program} failed: {detail}"),
    (
        "cmd.timeout",
        "{program} 执行超时（{secs}s），已强制终止",
        "{program} timed out after {secs}s and was killed",
    ),
    // ── 检测信号的说明 ──
    //
    // 这些句子会拼进 `log.interrupt_detected` 的 `{signals}`，是用户在
    // Activity Log 里唯一能看到的「凭什么这么判」，必须跟着界面语言走。
    (
        "signal.process_exited",
        "进程 {pid} 已经退出了",
        "Process {pid} has exited",
    ),
    (
        "signal.file_stale",
        "会话记录已经 {elapsed}s 没有更新（阈值 {threshold}s）",
        "The transcript hasn't changed for {elapsed}s (threshold {threshold}s)",
    ),
    (
        "signal.keyword_match",
        "输出里出现了关键词「{keyword}」",
        "The output mentions \"{keyword}\"",
    ),
    (
        "signal.heartbeat_timeout",
        "最后一次活动距今 {elapsed}s（阈值 {threshold}s）",
        "Last activity was {elapsed}s ago (threshold {threshold}s)",
    ),
    // ── 注意力分级的依据（进通知正文）──
    (
        "attention.detail.completed",
        "匹配到完成标记「{marker}」",
        "Matched the completion marker \"{marker}\"",
    ),
    (
        "attention.detail.keyword",
        "输出里出现了「{keyword}」",
        "The output mentions \"{keyword}\"",
    ),
    (
        "attention.detail.awaiting_keyword",
        "输出里出现了「{keyword}」，Agent 正在等你回应",
        "The output mentions \"{keyword}\" — the agent is waiting for you",
    ),
    (
        "attention.detail.process_exited",
        "进程已经退出，也没看到完成标记",
        "The process exited without any completion marker",
    ),
    // 这一条对应「它其实没有干完活，每次都要我去发继续」：回合收尾了、
    // 记录也不动了、又没有完成标记，就是活没干完自己停了。
    (
        "attention.detail.stalled",
        "这一轮已经收尾，但活儿看着没干完",
        "The turn is finished, but the work looks unfinished",
    ),
    (
        "attention.detail.silent",
        "会话长时间没有输出，疑似卡住了",
        "The session has been silent for a long time — it may be stuck",
    ),
    // ── 中断原因（进日志，也进通知的「为什么」一行）──
    //
    // 这些句子直接对应 `InterruptReason`，措辞的落点是「接下来会发生什么」，
    // 而不是内部枚举名——用户关心的是「它会自己好，还是要我去看一眼」。
    ("reason.none", "没有中断", "Not interrupted"),
    (
        "reason.process_crashed",
        "进程已经不在了",
        "The process is gone",
    ),
    (
        "reason.rate_limited",
        "撞上限流，等窗口过去就会自己恢复",
        "Rate limited — it will recover once the window passes",
    ),
    (
        "reason.awaiting_input",
        "它在问你一个具体的问题",
        "It is asking you a specific question",
    ),
    (
        "reason.runtime_error",
        "运行时报了故障",
        "The runtime reported a failure",
    ),
    (
        "reason.stalled",
        "活儿没干完就自己停了",
        "It stopped with the work unfinished",
    ),
    (
        "reason.unknown",
        "确实停了，但说不出为什么",
        "It has stopped, but the reason is unclear",
    ),
    // ── 按原因分派手段之后，闸门要说清楚「为什么这次不敲字」──
    //
    // 这两条是新增的「不动手」路径。上一版只有「动手」和「冷却中」两种说法，
    // 于是不该敲字的场合只能靠不敲来表达，用户在日志里什么也看不到。
    (
        "log.resume_hand_off",
        "{agent}：{reason}，敲「继续」帮不上忙，交给你处理",
        "{agent}: {reason} — typing \"continue\" won't help, handing it to you",
    ),
    (
        "log.resume_wait",
        "{agent}：{reason}，这次不催，等它自己恢复",
        "{agent}: {reason} — not nudging, waiting for it to recover",
    ),
];

/// 国际化查表器
pub struct I18n {
    lang: Lang,
}

impl I18n {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    pub fn from_code(code: &str) -> Self {
        Self::new(Lang::from_code(code))
    }

    pub fn lang(&self) -> Lang {
        self.lang
    }

    pub fn set_lang(&mut self, lang: Lang) {
        self.lang = lang;
    }

    /// 取翻译；未收录的 key 原样返回，便于排查而不至于显示 `key_not_found`
    pub fn t(&self, key: &'static str) -> &'static str {
        TABLE
            .iter()
            .find(|(k, _, _)| *k == key)
            .map(|(_, zh, en)| match self.lang {
                Lang::Zh => *zh,
                Lang::En => *en,
            })
            .unwrap_or(key)
    }

    /// 取翻译并拿走所有权
    ///
    /// 纯粹是为了让「查表结果直接填进结构体字段」的地方少一串 `.to_string()`。
    pub fn t_owned(&self, key: &'static str) -> String {
        self.t(key).to_string()
    }

    /// 取翻译并按 `{名字}` 占位符插值
    ///
    /// 后端文案只有几十条，引模板引擎不值得；但插值必须集中在这里，
    /// 否则又会退化成到处 `format!` 手拼中文——那正是「土不土洋不洋」的来源。
    pub fn tf(&self, key: &'static str, vars: &[(&str, &str)]) -> String {
        let mut out = self.t(key).to_string();
        for (name, value) in vars {
            out = out.replace(&format!("{{{name}}}"), value);
        }
        out
    }

    /// 导出当前语言的全部条目（前端调试用；界面文案在前端自己的字典里）
    pub fn all(&self) -> HashMap<&'static str, &'static str> {
        TABLE
            .iter()
            .map(|(k, zh, en)| {
                (
                    *k,
                    match self.lang {
                        Lang::Zh => *zh,
                        Lang::En => *en,
                    },
                )
            })
            .collect()
    }
}

impl Default for I18n {
    fn default() -> Self {
        Self::new(Lang::Zh)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_both_languages() {
        assert_eq!(I18n::new(Lang::Zh).t("tray.quit"), "退出 AgentPulse");
        assert_eq!(I18n::new(Lang::En).t("tray.quit"), "Quit AgentPulse");
    }

    #[test]
    fn unknown_key_returns_itself() {
        assert_eq!(I18n::new(Lang::Zh).t("nope.nope"), "nope.nope");
    }

    #[test]
    fn interpolates_placeholders() {
        let text = I18n::new(Lang::En).tf(
            "log.heartbeat",
            &[("total", "3"), ("active", "2"), ("interrupted", "1")],
        );
        assert_eq!(text, "Heartbeat: 3 session(s), 2 active, 1 interrupted");
    }

    #[test]
    fn placeholders_are_all_documented() {
        // 占位符漏填会原样显示 `{name}`，比翻译错更难看，所以两种语言的
        // 占位符集合必须一致——漏一个这里就会红。
        for (key, zh, en) in TABLE {
            let mut zh_vars: Vec<&str> = placeholders(zh);
            let mut en_vars: Vec<&str> = placeholders(en);
            zh_vars.sort_unstable();
            en_vars.sort_unstable();
            assert_eq!(zh_vars, en_vars, "{key} 两种语言的占位符不一致");
        }
    }

    fn placeholders(s: &str) -> Vec<&str> {
        let mut out = Vec::new();
        let mut rest = s;
        while let Some(start) = rest.find('{') {
            let after = &rest[start + 1..];
            match after.find('}') {
                Some(end) => {
                    out.push(&after[..end]);
                    rest = &after[end + 1..];
                }
                None => break,
            }
        }
        out
    }

    #[test]
    fn no_duplicate_keys() {
        let mut keys: Vec<&str> = TABLE.iter().map(|(k, _, _)| *k).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(before, keys.len(), "i18n 表里有重复 key");
    }
}
