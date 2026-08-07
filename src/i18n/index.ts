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
  "footer.resume_pipeline": [
    "续跑流水线：待投递 {pending} · 核验 {verifying}",
    "Resume pipeline: {pending} pending · {verifying} verifying",
  ],

  // ── 首次使用 ──
  "onboarding.title": ["三步开始守护", "Start monitoring in three steps"],
  "onboarding.desc": [
    "Agent 仍由你在项目终端里正常启动，AgentPulse 只负责观察与安全续跑",
    "You still start agents normally in project terminals; AgentPulse only observes and resumes safely",
  ],
  "onboarding.settings": ["前往设置", "Open settings"],
  "onboarding.monitor.title": ["开始守护", "Start monitoring"],
  "onboarding.monitor.body": [
    "开启周期扫描，发现本机正在运行的受支持 Agent",
    "Enable periodic scans to discover supported agents already running on this machine",
  ],
  "onboarding.monitor.action": ["开始守护", "Start monitoring"],
  "onboarding.monitor.ready": ["守护中", "Monitoring"],
  "onboarding.agent.title": ["在项目终端运行 Agent", "Run an agent in your project terminal"],
  "onboarding.agent.body": [
    "像平时一样自行运行其中一个命令；这里仅作提示，不会替你执行",
    "Run one of these commands yourself as usual; they are examples only and are never executed here",
  ],
  "onboarding.agent.commands": ["可用命令示例", "Example commands"],
  "onboarding.scan.title": ["立即扫描", "Scan now"],
  "onboarding.scan.body": [
    "Agent 启动后手动刷新一次，不必等待下一轮周期扫描",
    "Refresh once after the agent starts instead of waiting for the next scheduled scan",
  ],
  "onboarding.scan.action": ["立即扫描", "Scan now"],
  "onboarding.scan.ready": ["已扫描", "Scanned"],
  "onboarding.boundary.title": ["非侵入式守护", "Non-invasive monitoring"],
  "onboarding.boundary.body": [
    "不会启动或接管 Agent，只观察进程和会话记录；无法精确定位目标终端时不会盲目输入。",
    "AgentPulse never starts or takes over agents. It only observes processes and transcripts, and never types when the target terminal cannot be located precisely.",
  ],

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
  "session.search.placeholder": [
    "搜索项目、Agent 或终端…",
    "Search project, agent or terminal…",
  ],
  "session.search.label": ["搜索会话", "Search sessions"],
  "session.scope.label": ["会话范围", "Session scope"],
  "session.scope.all": ["全部 {count}", "All {count}"],
  "session.scope.attention": ["等我 {count}", "Needs me {count}"],
  "session.scope.stalled": ["卡住 {count}", "Stalled {count}"],
  "session.scope.active": ["活跃 {count}", "Active {count}"],
  "session.result_count": ["显示 {visible}/{total}", "Showing {visible}/{total}"],
  "session.clear_filters": ["清除", "Clear"],
  "session.no_matches": ["没有符合条件的会话", "No matching sessions"],
  "session.no_matches_hint": [
    "换个关键词或清除筛选，正在守护的会话没有消失",
    "Try another query or clear filters — monitored sessions are still here",
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

  // ── 判定证据 ──
  "evidence.button": ["看判据", "Evidence"],
  "evidence.hide": ["收起判据", "Hide evidence"],
  "evidence.title": ["为什么是这个结论", "Why this ruling"],
  "evidence.process": ["进程存活", "Process alive"],
  "evidence.turn": ["回合结构", "Turn state"],
  "evidence.signals": ["检测信号", "Signals"],
  "evidence.signal.file_stale": ["记录停更", "Transcript stale"],
  "evidence.signal.transcript_idle": ["记录时间信号", "Transcript time signal"],
  "evidence.signal.keyword_match": ["命中关键词", "Keyword match"],
  "evidence.signal.process_exited": ["进程已退出", "Process exited"],
  "evidence.signal.heartbeat_timeout": ["心跳超时", "Heartbeat timeout"],
  "evidence.grace": ["忙碌宽限", "Busy grace"],
  "evidence.keyword": ["中断关键词", "Stall keyword"],
  "evidence.completion": ["完成标记", "Completion marker"],
  "evidence.second_opinion": ["AI 第二意见", "AI second opinion"],
  "evidence.yes": ["是", "Yes"],
  "evidence.no": ["否", "No"],
  "evidence.none": ["没有", "None"],
  "evidence.turn.unknown": ["无法识别", "Unknown"],
  "evidence.turn.tool_running": ["工具仍在运行", "Tool still running"],
  "evidence.turn.busy": ["回合尚未收尾", "Turn still active"],
  "evidence.turn.awaiting_user": ["正在等人", "Waiting for you"],
  "evidence.opinion.finished": ["这一轮已完成", "Turn finished"],
  "evidence.opinion.unfinished": ["这一轮还没完成", "Turn unfinished"],

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

  // ── 中断原因 ──
  //
  // 这几条要回答的不是「它怎么了」（那是上面的级别在说），而是
  // 「所以这次我到底做了什么」。四个「按住手」的原因必须有措辞，
  // 否则界面上一片「已中断」，用户会以为守护神漏了一次。
  "reason.process_crashed": ["进程已退出", "Process gone"],
  "reason.rate_limited": ["等限流过去", "Waiting out the limit"],
  "reason.upstream_rejected": ["上游挡回来了", "Upstream rejected"],
  "reason.awaiting_input": ["在问你话", "Asking you"],
  "reason.runtime_error": ["运行时报错", "Runtime error"],
  "reason.stalled": ["活没干完", "Work unfinished"],
  "reason.unknown": ["原因不明", "Reason unclear"],

  // ── 这一轮打算怎么办 ──
  //
  // 措辞的落点是「所以你现在要不要动」，不是内部枚举名。两条都由后端算好的
  // `resume_tactic` 决定，界面不再自己从原因推——推错的样子是界面上凭空
  // 多出一句「这次没帮你按继续」，或者该说的时候不说。
  "tactic.wait": [
    "这次不催——{reason}，等它自己恢复就好。",
    "Not nudging — {reason}. It will recover on its own.",
  ],
  "tactic.hand_off": [
    "这次没有帮你按「继续」——{reason}，敲字帮不上忙，得你看一眼。",
    "No nudge sent — {reason}. Typing wouldn't help; this one needs you.",
  ],

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

  "stats.peak": ["峰值 {count}", "peak {count}"],
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

  // ── 续跑记录中心 ──
  //
  // 这四条是徽标上的**短标签**，跟后端 `resume.outcome_*` 那几句长话是两套：
  // 那边要在日志里把事情说清楚（「按键发出去了，但盯了几秒会话一点没动……」），
  // 这边只有一个徽标的宽度。同一个概念两种长度，不要互相复用。
  "outcome.landed": ["已落地", "Landed"],
  "outcome.silent": ["没反应", "No reaction"],
  "outcome.failed": ["没送达", "Not delivered"],
  "outcome.unverifiable": ["无法核验", "Unverifiable"],
  // 徽标只有两三个字，说不清「所以我该干什么」，所以把下一步动作放在悬浮解释里
  "outcome.landed_hint": [
    "字敲进去了，而且看见会话动了起来",
    "The text went in and the session was seen picking it up",
  ],
  "outcome.silent_hint": [
    "字敲出去了，但盯了几秒会话没动——大概进了别的窗口，去看看焦点和输入法",
    "Keystrokes went out but the session didn't budge — they likely hit another window; check focus and your input method",
  ],
  "outcome.failed_hint": [
    "一个字都没敲出去，通道自己就报错了——多半是权限或者定位不到窗口",
    "Nothing was typed at all — the channel itself errored out, usually permissions or a window it couldn't find",
  ],
  "outcome.unverifiable_hint": [
    "已发送。这类会话没有可读的记录文件，核验不了，不代表出了问题",
    "Sent. This kind of session keeps no readable transcript, so it can't be verified — that's not a fault",
  ],
  "outcome.legacy": ["旧记录", "Older record"],
  "outcome.legacy_hint": [
    "这条记录写在加入投递核验之前，当时只记了成功或失败",
    "Written before delivery verification existed — only success or failure was recorded back then",
  ],

  "records.title": ["续跑记录", "Resume records"],
  "records.desc": [
    "每一次续跑敲进去了没有，以及没进去的原因",
    "Whether each resume actually landed — and why it didn't",
  ],
  "records.search": [
    "搜索项目 / Agent / 原因…",
    "Search project, agent or reason…",
  ],
  "records.filter_outcome": ["投递结果", "Outcome"],
  "records.filter_type": ["提示词", "Prompt"],
  "records.all": ["全部", "All"],
  "records.empty": ["还没有续跑记录", "No resumes yet"],
  "records.empty_hint": [
    "开始守护后，每一次自动或手动续跑都会记在这里",
    "Once monitoring starts, every auto or manual resume is logged here",
  ],
  "records.no_match": ["没有匹配的记录", "Nothing matched"],
  "records.no_match_hint": [
    "换个关键词，或把筛选条件放回「全部」",
    "Try another keyword, or set the filters back to All",
  ],
  "records.reason": ["原因", "Reason"],
  "records.stuck": ["卡了 {dur}", "Stuck {dur}"],
  "records.stuck_hint": [
    "出手前它已经这么久没动了。这个数越大，说明发现得越晚",
    "It had been idle this long before we stepped in — the bigger it is, the later we caught it",
  ],

  // ── 时长 ──
  //
  // 只给 key，不在代码里拼 `"3 分 20 秒"`：那种写法在英文里语序和单位都不一样。
  "dur.secs": ["{n} 秒", "{n}s"],
  "dur.mins": ["{n} 分钟", "{n} min"],
  "dur.hours": ["{n} 小时", "{n} h"],

  // ── 趋势对比 ──
  "trend.title": ["跟上一期比", "Versus last period"],
  "trend.desc": [
    "同样长的两段时间放一起看，才知道是在变好还是变坏",
    "Two equal spans side by side — the only way to tell better from worse",
  ],
  "trend.window_1": ["今日 / 昨日", "Today vs yesterday"],
  "trend.window_7": ["近 7 天 / 前 7 天", "7 days vs previous 7"],
  "trend.interruptions": ["中断次数", "Interruptions"],
  "trend.resumes": ["续跑次数", "Resumes"],
  "trend.landed_rate": ["敲进去的比例", "Landed rate"],
  "trend.stuck_secs": ["平均卡多久", "Avg. stuck"],
  "trend.baseline": ["上期 {value}", "Was {value}"],
  "trend.flat": ["持平", "No change"],
  "trend.no_baseline": ["没有可比的上期", "No period to compare"],
  "trend.no_data": ["暂无", "—"],
  // 空窗提示分两种，因为该做的事完全不同：一个是等，一个是没开着
  "trend.too_new": [
    "上一期这个应用还没在跑，没有可比的数据。守护满 {days} 天后这里会自动出现",
    "The app wasn't running last period, so there's nothing to compare. This fills in after {days} day(s) of monitoring",
  ],
  "trend.stuck_unknown": [
    "这一期没有能算出卡了多久的记录——没有可读会话记录的 agent 算不出这个数",
    "No measurable stuck time this period — agents without a readable transcript can't report it",
  ],

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
  "cost.peak": ["峰值 ${cost}", "peak ${cost}"],
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
  "cost.period_spend": ["近 30 天花费", "Spend, last 30 days"],
  "cost.period_tokens": ["近 30 天 tokens", "Tokens, last 30 days"],
  "cost.cache_hit_rate": ["缓存读取占比", "Cache read share"],
  "cost.period_requests": ["近 30 天请求", "Requests, last 30 days"],
  "cost.models": ["模型排行", "Top models"],
  "cost.models_desc": ["近 {days} 天按模型聚合", "Grouped by model, last {days} days"],

  // ── 会话历史 ──
  "history.title": ["会话档案", "Session records"],
  "history.desc": [
    "每个会话一行，记着它活了多久、被续跑过几次、花了多少。点开看细节。",
    "One row per session: how long it ran, how often it was resumed, what it cost. Click for details.",
  ],
  "history.search": ["搜索项目 / 终端…", "Search project or terminal…"],
  "history.empty": ["还没有历史会话", "No past sessions yet"],
  "history.seen": ["{first} → {last}", "{first} → {last}"],
  "history.no_match": ["没有匹配的会话", "Nothing matched"],
  "history.records": ["共 {count} 条", "{count} total"],
  "history.previous": ["上一页", "Previous"],
  "history.next": ["下一页", "Next"],
  "history.page": ["第 {page} / {total} 页", "Page {page} of {total}"],

  // 汇总条
  "history.sum_total": ["会话总数", "Sessions"],
  "history.sum_live": ["仍在运行", "Still running"],
  "history.sum_resumes": ["续跑次数", "Resumes"],
  "history.sum_cost": ["累计花费", "Total spend"],

  // 状态筛选与状态标记
  "history.filter_all": ["全部", "All"],
  "history.filter_live": ["运行中", "Running"],
  "history.filter_ended": ["已结束", "Ended"],
  "history.live": ["运行中", "Running"],
  "history.ended": ["已结束", "Ended"],
  "history.last_seen_as": ["最后一眼：{status}", "Last seen: {status}"],
  "history.ended_at": ["{time} 结束", "ended {time}"],
  "history.lasted": ["持续 {duration}", "ran for {duration}"],
  "history.today": ["今天", "Today"],
  "history.yesterday": ["昨天", "Yesterday"],

  // 档案抽屉
  "history.detail_title": ["会话档案", "Session record"],
  "history.detail_loading": ["正在读取档案…", "Loading record…"],
  "history.detail_missing": [
    "这个会话的档案已经不在库里了",
    "This session is no longer in the database",
  ],
  "history.lifecycle": ["生命周期", "Lifecycle"],
  "history.first_seen_at": ["首次发现", "First seen"],
  "history.last_seen_at": ["最后发现", "Last seen"],
  "history.duration": ["持续时长", "Duration"],
  "history.final_status": ["最后状态", "Final status"],
  "history.agent": ["Agent", "Agent"],
  "history.terminal": ["终端", "Terminal"],
  "history.detail_usage": ["用量", "Usage"],
  "history.detail_tokens": ["Tokens", "Tokens"],
  "history.interruptions": ["中断 {count} 次", "{count} interruption(s)"],
  "history.no_interruptions": ["没有被判定过中断", "Never flagged as interrupted"],
  "history.resume_timeline": ["续跑时间线", "Resume timeline"],
  "history.no_resumes": ["没有续跑过", "Never resumed"],
  "history.detection_timeline": ["中断记录", "Interruptions"],
  "history.stuck_for": ["卡了 {duration}", "stuck {duration}"],
  "history.copy_transcript": ["复制会话记录路径", "Copy transcript path"],
  "history.copy_dir": ["复制工作目录", "Copy working directory"],
  "history.no_transcript": [
    "这个 agent 没有可读的会话记录文件",
    "This agent has no readable transcript file",
  ],

  // ── 导出 ──
  //
  // 「导到哪儿去了」必须说出来：这个应用不弹系统保存对话框（那要多引一个插件
  // 和一条权限），文件直接写到下载夹。只说「成功」而不说位置，用户找不到文件时
  // 比失败更难受。所以成功那句话本身就是个可点的链接，点了在文件管理器里亮出来。
  "export.csv": ["导出 CSV", "Export CSV"],
  "export.running": ["正在导出…", "Exporting…"],
  "export.done": ["已导出 {rows} 行，点这里查看", "{rows} rows exported — reveal"],
  "export.failed": ["导出失败", "Export failed"],
  // 撞上限时必须换一套措辞，不能只把行数写小。用户看到「已导出 100000 行」
  // 会以为那就是全部——一份自己不说自己不全的导出，比导出失败更坏
  "export.truncated": [
    "只导出了最近 {rows} 行（还有更多，先筛一下再导）",
    "Only the latest {rows} rows (more exist — narrow the filter)",
  ],
  "export.cost_daily": ["导出每日花费", "Export daily spend"],
  "export.cost_projects": ["导出项目花费", "Export by project"],
  "export.cost_models": ["导出模型花费", "Export by model"],
  "export.stats": ["导出统计摘要", "Export summary"],

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
  // 只给读屏软件听的名字。图标按钮里那个叉是 aria-hidden 的装饰，
  // 不补这一条读屏就只念一声「按钮」，用户不知道按下去会发生什么。
  // 屏幕上看不见也算用户可见文案，所以照样进这张表、照样跟着语言切
  "common.close": ["关闭", "Close"],
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
