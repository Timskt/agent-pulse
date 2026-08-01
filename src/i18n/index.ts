/**
 * 界面文案的国际化
 *
 * 边界约定和后端一致：**谁渲染，谁持有文案**。
 * - 这里只管界面上的字：导航、按钮、字段标签、空状态。
 * - 系统通知、托盘菜单、引擎日志由后端 `src-tauri/src/i18n/mod.rs` 持有。
 *
 * 两份字典都由 `config.language` 驱动，所以不会出现「界面中文、通知英文」
 * 那种土不土洋不洋的搭配。
 *
 * 表格和后端一样用 `[中文, English]` 并排：漏翻一眼就能看出来。
 */

import { useMemo } from "react";
import { useAppStore } from "../stores/useAppStore";

export type LangCode = "zh" | "en";

/** `[中文, English]` */
type Entry = readonly [zh: string, en: string];

const TABLE = {
  // ── 外壳 ──
  "app.subtitle": [
    "AI Agent 守护与自动续跑",
    "AI agent guardian & auto-resume",
  ],
  "app.running": ["守护中", "Watching"],
  "app.stopped": ["未启动", "Idle"],
  "nav.dashboard": ["总览", "Overview"],
  "nav.stats": ["统计", "Statistics"],
  "nav.cost": ["花费", "Spending"],
  "nav.history": ["历史", "History"],
  "nav.config": ["设置", "Settings"],
  "btn.scan_now": ["立即扫描", "Scan now"],
  "btn.start": ["开始守护", "Start"],
  "btn.stop": ["停止守护", "Stop"],
  "footer.sessions": ["会话 {count} 个", "{count} session(s)"],
  "footer.interval": ["每 {secs} 秒扫描", "scanning every {secs}s"],
  "footer.last_scan": ["最近扫描 {time}", "last scan {time}"],
  "footer.never_scanned": ["尚未扫描", "not scanned yet"],

  // ── 指标卡 ──
  "metric.sessions": ["会话总数", "Sessions"],
  "metric.active": ["活跃", "Active"],
  "metric.interrupted": ["中断", "Interrupted"],
  "metric.resumes": ["续跑次数", "Resumes"],
  "metric.pending": ["等你回应", "Waiting on you"],
  "metric.cost_today": ["今日花费", "Spent today"],

  // ── 会话列表 ──
  "session.title": ["会话", "Sessions"],
  "session.empty": ["现在没有正在跑的 Agent", "No agents running right now"],
  "session.empty_hint": [
    "启动 Claude Code / Codex CLI / OpenCode，会话会自动出现在这里",
    "Start Claude Code, Codex CLI or OpenCode — sessions show up here on their own",
  ],
  "session.resumed": ["已续跑 {count} 次", "resumed {count}×"],
  // 这两条是把「静默失败」摆到台面上。自动续跑的价值全在无感，代价是它坏掉的
  // 时候跟正常工作长得一模一样——所以坏了必须在会话卡片上一眼看得见。
  "session.resume_failing": ["敲不进去 ×{count}", "not landing ×{count}"],
  "session.stood_down": ["已停手，等你", "stood down — needs you"],
  "session.usage": ["{tokens} tokens · ${cost}", "{tokens} tokens · ${cost}"],
  "session.resume": ["续跑", "Resume"],
  "session.resume_goal": ["带目标续跑", "Resume with goal"],
  "session.focus": ["跳到终端", "Jump to terminal"],
  "session.analyze": ["AI 看一眼", "Ask AI"],

  // ── 续跑演练 ──
  //
  // 「按下去才知道会发生什么」是这个功能最大的心理负担，尤其在 IDE 内置
  // 终端上——敲错窗口就是把提示词打进别人的代码。演练把它变成零风险。
  "probe.button": ["演练", "Dry run"],
  "probe.running": ["定位中…", "Locating…"],
  "probe.title": ["续跑演练", "Resume dry run"],
  "probe.close": ["收起", "Close"],
  "probe.would_deliver": ["会敲进去", "Will type"],
  "probe.would_not_deliver": ["不会敲", "Won't type"],
  "probe.channel": ["通道", "Channel"],
  "probe.target": ["目标", "Target"],
  "probe.deps": ["环境依赖", "Dependencies"],
  "probe.dep_ok": ["就绪", "ready"],
  "probe.dep_missing": ["缺失", "missing"],
  "probe.fix_accessibility": ["去开权限", "Grant permission"],
  "probe.nothing_typed": [
    "演练不会敲任何字，随便点。",
    "A dry run never types anything — click freely.",
  ],
  "session.pid": ["进程 {pid}", "PID {pid}"],

  // ── 状态与注意力级别 ──
  "status.active": ["运行中", "Running"],
  "status.suspended": ["疑似中断", "Maybe stuck"],
  "status.interrupted": ["已中断", "Interrupted"],
  "status.completed": ["已完成", "Completed"],
  "status.exited": ["已退出", "Exited"],
  "attention.needs_input": ["等你输入", "Needs you"],
  "attention.completed": ["已完成", "Completed"],
  "attention.rate_limited": ["限流等待", "Rate limited"],
  "attention.error": ["出错", "Error"],

  // ── 日志 ──
  "log.title": ["活动日志", "Activity log"],
  "log.count": ["{count} 条", "{count} entries"],
  "log.empty": [
    "还没有日志，开始守护后这里会实时刷新",
    "Nothing yet — logs stream in once monitoring starts",
  ],

  // ── 统计 ──
  "stats.detections": ["中断检测", "Detections"],
  "stats.resumes": ["续跑次数", "Resumes"],
  "stats.successful": ["续跑成功", "Succeeded"],
  "stats.success_rate": ["成功率", "Success rate"],
  "stats.activity": ["近 {days} 天活动", "Last {days} days"],
  "stats.activity_desc": [
    "每天的中断检测与续跑次数",
    "Detections and resumes per day",
  ],
  "stats.legend_total": ["检测 + 续跑", "Detected + resumed"],
  "stats.legend_success": ["续跑成功", "Succeeded"],
  "stats.no_activity": [
    "还没有历史数据，开始守护后会自动记录",
    "No history yet — it fills in once monitoring starts",
  ],

  "stats.bar_tooltip": [
    "{date}：检测 {detections} 次 · 续跑 {resumes} 次 · 成功 {successful} 次",
    "{date}: {detections} detected · {resumes} resumed · {successful} succeeded",
  ],
  "stats.history": ["续跑记录", "Resume history"],
  "stats.history_desc": ["最近 {limit} 次续跑", "The last {limit} resumes"],
  "stats.records": ["{count} 条", "{count} records"],
  "stats.no_history": ["还没有续跑记录", "No resumes yet"],
  "stats.prompt_goal": ["目标续跑", "Goal"],
  "stats.prompt_generic": ["通用续跑", "Generic"],

  // ── 花费 ──
  "cost.today": ["今日花费", "Spent today"],
  "cost.trend": ["每日花费", "Daily spend"],
  "cost.trend_desc": [
    "近 {days} 天的 token 花费",
    "Token spend over the last {days} days",
  ],
  "cost.projects": ["项目排行", "Top projects"],
  "cost.projects_desc": [
    "近 {days} 天按项目聚合",
    "Grouped by project, last {days} days",
  ],
  "cost.no_data": [
    "还没有用量数据，Claude Code 跑起来后会自动统计",
    "No usage yet — it appears once Claude Code runs",
  ],
  "cost.forecast": ["限流预测", "Rate limit forecast"],
  "cost.forecast_desc": [
    "按最近一小时的速度推算窗口余量",
    "Projected from the last hour's pace",
  ],
  "cost.window_used": [
    "{hours} 小时窗口已用 {percent}%",
    "{percent}% of the {hours}h window used",
  ],
  "cost.minutes_left": [
    "约 {minutes} 分钟后触发限流",
    "about {minutes} min to the limit",
  ],
  "cost.no_limit": ["按当前速度不会触发限流", "no limit expected at this pace"],
  "cost.budget_unset": [
    "没设窗口额度，填上之后才能预测",
    "Set a window budget to get a forecast",
  ],
  "cost.requests": ["{count} 次请求", "{count} requests"],
  "cost.tokens": ["{tokens} tokens", "{tokens} tokens"],
  "cost.bar_tooltip": [
    "{date}：${cost} · {tokens} tokens · {requests} 次请求",
    "{date}: ${cost} · {tokens} tokens · {requests} requests",
  ],
  "cost.range_total": [
    "近 {days} 天合计 ${cost}",
    "${cost} over the last {days} days",
  ],
  "cost.window_tokens": [
    "{used} / {budget} tokens",
    "{used} / {budget} tokens",
  ],

  // ── 会话历史时间线 ──
  "history.title": ["会话历史", "Session history"],
  "history.desc": [
    "按项目或终端找回以前的会话",
    "Find past sessions by project or terminal",
  ],
  "history.search": ["搜索项目 / 终端…", "Search project or terminal…"],
  "history.empty": ["还没有历史会话", "No past sessions yet"],
  "history.seen": ["{first} → {last}", "{first} → {last}"],
  "history.no_match": ["没有匹配的会话", "Nothing matched"],

  // ── 设置：检测 ──
  "cfg.detection": ["检测", "Detection"],
  "cfg.detection.desc": [
    "多久看一眼、多久算卡住",
    "How often we look, and when we call it stuck",
  ],
  "cfg.poll_interval": ["扫描间隔（秒）", "Scan interval (seconds)"],
  "cfg.idle_timeout": ["无活动超时（秒）", "Idle timeout (seconds)"],
  "cfg.idle_threshold": ["连续无活动次数", "Consecutive idle checks"],
  "cfg.max_resume": ["最多自动续跑次数", "Max auto-resumes"],
  "cfg.cooldown": ["两次续跑间隔（秒）", "Cooldown between resumes (seconds)"],

  // ── 设置：行为 ──
  "cfg.behavior": ["行为", "Behavior"],
  "cfg.behavior.desc": [
    "判定中断之后做什么",
    "What happens once a stall is called",
  ],
  "cfg.auto_resume": ["自动续跑", "Auto-resume"],
  "cfg.auto_resume.desc": [
    "检测到中断就替你敲续跑指令；关掉则只提醒不动手",
    "Type the resume prompt for you; turn it off to get alerts only",
  ],
  "cfg.startup_scan": ["启动即守护", "Start watching on launch"],
  "cfg.startup_scan.desc": [
    "打开应用后自动开始监控",
    "Begin monitoring as soon as the app opens",
  ],
  "cfg.follow_latest": ["跟随最新会话", "Follow the newest session"],
  "cfg.follow_latest.desc": [
    "多窗口时慎用，可能对着非目标会话动手",
    "Risky with several windows — it may act on the wrong session",
  ],
  "cfg.heartbeat": ["心跳日志", "Heartbeat log"],
  "cfg.heartbeat.desc": [
    "每轮扫描写一条，排查检测问题时打开",
    "One line per scan — turn it on when detection misbehaves",
  ],

  // ── 设置：提醒 ──
  "cfg.notify": ["提醒", "Alerts"],
  "cfg.notify.desc": [
    "会话需要你的时候怎么叫你",
    "How you get pulled in when a session needs you",
  ],
  "cfg.notify_enabled": ["桌面通知", "Desktop notifications"],
  "cfg.notify_enabled.desc": [
    "等待输入 / 完成 / 限流 / 出错时弹系统通知",
    "System notification on input needed, completion, rate limits, and errors",
  ],
  "cfg.notify_needs_input": ["等你输入", "Needs input"],
  "cfg.notify_completed": ["任务完成", "Completed"],
  "cfg.notify_rate_limited": ["限流等待", "Rate limited"],
  "cfg.notify_error": ["会话出错", "Errors"],
  "cfg.notify_resumed": ["续跑成功", "Resumed"],
  "cfg.notify_sound": ["声音提醒", "Play a sound"],
  "cfg.notify_sound.desc": [
    "提醒时顺带响一下",
    "Chime alongside the notification",
  ],

  "cfg.notify_volume": ["音量 {value}%", "Volume {value}%"],
  "cfg.notify_throttle": [
    "同一会话最短提醒间隔（秒）",
    "Minimum gap between alerts (seconds)",
  ],
  "cfg.notify_badge": ["托盘角标", "Tray badge"],
  "cfg.notify_badge.desc": [
    "托盘图标上显示还有几个会话在等你",
    "Show how many sessions are waiting, right on the tray icon",
  ],
  "cfg.notify_test": ["发一条测试通知", "Send a test notification"],

  // ── 设置：花费 ──
  "cfg.cost": ["花费", "Spending"],
  "cfg.cost.desc": [
    "token 统计与预算告警",
    "Token accounting and budget alerts",
  ],
  "cfg.cost_enabled": ["统计 token 花费", "Track token spend"],
  "cfg.cost_enabled.desc": [
    "读取 Claude Code 的用量记录并按模型计价",
    "Read Claude Code usage logs and price them per model",
  ],
  "cfg.daily_budget": [
    "每日预算（美元，0 = 不限）",
    "Daily budget (USD, 0 = off)",
  ],
  "cfg.session_budget": [
    "单会话预算（美元，0 = 不限）",
    "Per-session budget (USD, 0 = off)",
  ],
  "cfg.alert_percent": ["用到预算的多少 % 告警", "Alert at this % of budget"],
  "cfg.rate_window": ["限流窗口（小时）", "Rate limit window (hours)"],
  "cfg.rate_budget": [
    "窗口内 token 额度（0 = 不预测）",
    "Tokens per window (0 = no forecast)",
  ],

  // ── 设置：提示词 ──
  "cfg.prompts": ["续跑提示词", "Resume prompts"],
  "cfg.prompts.desc": [
    "中断后替你敲进去的那句话",
    "What we type into the agent when it stalls",
  ],
  "cfg.generic_prompt": ["通用续跑提示词", "Generic resume prompt"],
  "cfg.goal_prompt": ["目标恢复提示词", "Goal recovery prompt"],
  "cfg.goal_badge": [
    "检测到活跃目标时自动使用",
    "Used when a live goal is detected",
  ],
  "cfg.goal_prompt.hint": [
    "输出里出现 goal / objective / turn_budget 之类的词，就判定任务是目标驱动的，改用这条把目标显式带回来",
    "When the output mentions goal / objective / turn_budget we treat the task as goal-driven and use this prompt to restate it",
  ],

  // ── 设置：关键词 ──
  "cfg.keywords": ["关键词", "Keywords"],
  "cfg.keywords.desc": [
    "逗号分隔；决定什么算中断、什么算完成",
    "Comma-separated; they decide what counts as stalled or done",
  ],
  "cfg.kw_interrupt": ["中断关键词", "Stall keywords"],
  "cfg.kw_interrupt.hint": [
    "出现这些词又没有完成标记，才判定为中断",
    "A stall needs one of these and no completion marker",
  ],
  "cfg.kw_completion": ["完成标记", "Completion markers"],
  "cfg.kw_completion.hint": [
    "出现完成标记就不再续跑，免得把做完的活儿又跑一遍",
    "These stop a resume, so finished work isn't run twice",
  ],

  "cfg.kw_goal": ["目标关键词", "Goal keywords"],
  "cfg.kw_goal.hint": [
    "命中这些词就改用目标恢复提示词",
    "Hitting one of these switches to the goal recovery prompt",
  ],
  "cfg.kw_input": ["等你输入的关键词", "Needs-input keywords"],
  "cfg.kw_rate_limit": ["限流关键词", "Rate limit keywords"],
  "cfg.kw_error": ["出错关键词", "Error keywords"],

  // ── 设置：外部推送 ──
  "cfg.webhook": ["外部推送", "Outbound push"],
  "cfg.webhook.desc": [
    "把提醒转发到 Slack / Discord / ntfy / Bark，人不在电脑前也知道",
    "Forward alerts to Slack, Discord, ntfy or Bark so you hear it away from the desk",
  ],
  "cfg.webhook_enabled": ["启用外部推送", "Enable outbound push"],
  "cfg.webhook_enabled.desc": [
    "中断 / 续跑 / 完成时发一条 HTTP 请求",
    "One HTTP call on stall, resume, or completion",
  ],
  "cfg.webhook_provider": ["推送渠道", "Channel"],
  "cfg.webhook_custom": ["自定义", "Custom"],
  "cfg.webhook_url": ["推送地址", "Endpoint"],
  "cfg.webhook_topic": ["主题 / 设备 Key", "Topic / device key"],
  "cfg.webhook_topic.hint": [
    "ntfy 填主题名，Bark 填设备 Key；留空则直接用上面的完整地址",
    "ntfy takes a topic, Bark takes a device key; leave blank to post to the URL as-is",
  ],
  "cfg.webhook_template": ["消息模板", "Message template"],
  "cfg.webhook_template.hint": [
    "可用占位符：{agent_name}、{session_id}、{verdict}、{message}",
    "Placeholders: {agent_name}, {session_id}, {verdict}, {message}",
  ],
  "cfg.webhook_on_interrupt": ["中断时推", "On stall"],
  "cfg.webhook_on_resume": ["续跑时推", "On resume"],
  "cfg.webhook_on_complete": ["完成时推", "On completion"],
  "cfg.webhook_test": ["发送测试", "Send a test"],

  // ── 设置：AI 判断 ──
  "cfg.ai": ["AI 辅助判断", "AI second opinion"],
  "cfg.ai.desc": [
    "让模型看一眼输出，减少误判",
    "Let a model read the output to cut false alarms",
  ],
  "cfg.ai_enabled": ["启用 AI 判断", "Enable AI judging"],
  "cfg.ai_enabled.desc": [
    "需要填 API Key；只在关键词判不准时才调用",
    "Needs an API key; only called when keywords are inconclusive",
  ],
  "cfg.ai_endpoint": ["API 端点", "API endpoint"],
  "cfg.ai_model": ["模型", "Model"],
  "cfg.ai_key": ["API Key", "API key"],

  "cfg.ai_confidence": ["置信度阈值 {value}%", "Confidence threshold {value}%"],
  "cfg.ai_confidence.hint": [
    "模型判定中断的概率高过这个数才动手，越高越保守",
    "We only act above this confidence — higher is more conservative",
  ],

  // ── 设置：自定义适配器 ──
  "cfg.adapters": ["自定义适配器", "Custom adapters"],
  "cfg.adapters.desc": [
    "内置支持 Claude Code / Codex CLI / OpenCode，别的 CLI 在这里加",
    "Claude Code, Codex CLI and OpenCode ship built in — add other CLIs here",
  ],
  "cfg.adapters.empty": ["还没有自定义适配器", "No custom adapters yet"],
  "cfg.adapter_name": ["名称", "Name"],
  "cfg.adapter_process": ["进程匹配（如 aider）", "Process match (e.g. aider)"],
  "cfg.adapter_session": [
    "会话文件模式（可选）",
    "Session file pattern (optional)",
  ],
  "cfg.adapter_add": ["添加适配器", "Add adapter"],

  // ── 设置：本地看板 ──
  "cfg.remote": ["手机看板", "Phone dashboard"],
  "cfg.remote.desc": [
    "在手机浏览器上只读查看会话状态",
    "Read-only session view from your phone's browser",
  ],
  "cfg.remote_enabled": ["启用本地看板", "Enable the dashboard"],
  "cfg.remote_enabled.desc": [
    "在本机起一个只读 HTTP 服务，带令牌才能看",
    "Serves a read-only page on this machine, token required",
  ],
  "cfg.remote_port": ["端口", "Port"],
  "cfg.remote_token": ["访问令牌", "Access token"],
  "cfg.remote_token.hint": [
    "留空则启动时自动生成",
    "Generated automatically if left blank",
  ],
  "cfg.remote_token_generate": ["生成", "Generate"],
  "cfg.remote_token_weak": [
    "开到局域网上，令牌就是唯一那道门——现在这个太短了，几秒就能试出来。建议至少 16 位，点「生成」直接来一个。",
    "On the LAN the token is the only lock on the door, and this one is short enough to guess in seconds. Use at least 16 characters — “Generate” makes one for you.",
  ],
  "cfg.remote_bind_all": ["允许局域网访问", "Allow LAN access"],
  "cfg.remote_bind_all.desc": [
    "从 127.0.0.1 改成 0.0.0.0：同一个网络里的人只要拿到令牌就能看你的会话，请只在可信网络里开",
    "Switches from 127.0.0.1 to 0.0.0.0 — anyone on the network with the token can read your sessions. Trusted networks only.",
  ],
  "cfg.remote_url": ["看板地址", "Dashboard URL"],
  "cfg.remote_url.lan_found": [
    "这就是手机上要打开的地址，不用自己换 IP",
    "This is the address to open on your phone — no IP swapping needed",
  ],
  "cfg.remote_url.lan_unknown": [
    "没能算出这台电脑的局域网 IP（可能没连网），手机上要把 127.0.0.1 换成它",
    "Couldn't determine this machine's LAN IP (offline?) — swap 127.0.0.1 for it on your phone",
  ],
  "cfg.remote_copy_link": ["复制带令牌的链接", "Copy link with token"],

  // ── 设置：系统 ──
  "cfg.system": ["系统", "System"],
  "cfg.system.desc": [
    "托盘、开机自启与语言",
    "Tray, launch at login, and language",
  ],
  "cfg.tray": ["常驻托盘", "Live in the tray"],
  "cfg.tray.desc": [
    "关窗口只是收起来，右键托盘图标就能控制守护",
    "Closing the window only hides it; right-click the tray icon to control monitoring",
  ],

  "cfg.autostart": ["开机自启", "Launch at login"],
  "cfg.autostart.desc": ["由系统的登录项管理", "Handled by the OS login items"],
  "cfg.language": ["界面语言", "Language"],
  "cfg.language.desc": [
    "界面、通知、托盘一起切换",
    "Switches the UI, notifications and tray together",
  ],
  "cfg.on": ["已开启", "On"],
  "cfg.os_managed": ["系统设置", "OS settings"],

  // ── 通用 ──
  "common.save": ["保存修改", "Save changes"],
  "common.saved": ["已保存", "Saved"],
  "common.loading": ["加载中…", "Loading…"],
  "common.remove": ["删除", "Remove"],
  "common.test": ["测试", "Test"],
  "common.error": ["出错了：{detail}", "Something went wrong: {detail}"],
  "common.copy": ["复制", "Copy"],
  "common.copied": ["已复制", "Copied"],
} satisfies Record<string, Entry>;

export type I18nKey = keyof typeof TABLE;

/** 插值变量：`t("footer.sessions", { count: 3 })` */
export type Vars = Record<string, string | number>;

/**
 * 取翻译并按 `{名字}` 插值
 *
 * 只替换调用方真的传了的变量，所以模板文案里写死的 `{agent_name}`
 * 这类「要给用户看的占位符」不会被误伤。
 */
export function translate(lang: LangCode, key: I18nKey, vars?: Vars): string {
  const entry = TABLE[key] as Entry | undefined;
  // 漏收录的 key 原样返回，比显示空白好排查
  if (!entry) return key;
  let out = lang === "en" ? entry[1] : entry[0];
  if (vars) {
    for (const [name, value] of Object.entries(vars)) {
      out = out.split(`{${name}}`).join(String(value));
    }
  }
  return out;
}

/** 当前语言下的查表器；语言没变时返回同一个对象，不会连累下游重渲染 */
export function useI18n() {
  const lang = useAppStore(selectLang);
  return useMemo(
    () => ({
      lang,
      t: (key: I18nKey, vars?: Vars) => translate(lang, key, vars),
    }),
    [lang],
  );
}

export type Translator = ReturnType<typeof useI18n>;

/** 单独抽出来，避免每次渲染都新建选择器函数 */
function selectLang(state: { config: { language: string } | null }): LangCode {
  return state.config?.language === "en" ? "en" : "zh";
}

/** 表格的全部 key，i18n 覆盖测试用 */
export const ALL_KEYS = Object.keys(TABLE) as I18nKey[];
