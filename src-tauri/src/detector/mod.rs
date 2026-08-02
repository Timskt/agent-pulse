use crate::adapters::{AgentSession, SessionStatus, TurnState};
use crate::config::AppConfig;
use crate::i18n::I18n;
use chrono::{Local, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// 检测结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionResult {
    /// 会话 ID
    pub session_id: String,
    /// 是否检测到中断
    pub interrupted: bool,
    /// 检测到的信号列表
    pub signals: Vec<DetectionSignal>,
    /// 是否发现完成标记（有完成标记则不触发续跑）
    pub has_completion_marker: bool,
    /// 匹配到的完成标记
    pub matched_marker: Option<String>,
    /// 是否检测到活跃 Goal（用于智能选择续跑提示词）
    pub has_active_goal: bool,
    /// 判定结论
    pub verdict: Verdict,
    /// 注意力分级：这件事要不要现在打扰用户，以及用什么颜色打扰
    pub attention: AttentionLevel,
    /// 触发该注意力级别的具体依据（用于通知正文）
    pub attention_detail: Option<String>,
    /// 检测侧的结构性证据快照；前端只展示，不重新参与判定
    pub evidence: DetectionEvidence,
    /// 它为什么停下来 —— 决定该用什么手段（见 [`InterruptReason::tactic`]）
    pub interrupt_reason: InterruptReason,
    /// 结构性证据到这儿就用尽了：去问一句 [`Arbitration`] 有可能改变结论
    ///
    /// 由判定层置位，动作层照着做——**不在动作层重新推一遍「哪里算用尽」**。
    /// 只在「答案真的能改变结果」的地方为真：已经在动手的会话不问，
    /// 因为仲裁没有权力叫停，问了也只是花钱。
    #[serde(default)]
    pub wants_second_opinion: bool,
    /// 检测时间
    pub detected_at: String,
}

/// 注意力分级（v1.1 感知层）
///
/// 中断检测回答的是「要不要续跑」，注意力分级回答的是「要不要现在叫人」——
/// 两者正交：限流等待不需要人，但值得知道；等待授权必须叫人。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionLevel {
    /// 一切正常，不打扰
    #[default]
    None,
    /// 🔴 需要我输入 / 授权 —— 最高优先级，Agent 停下来等人
    NeedsInput,
    /// 🟢 任务已完成
    Completed,
    /// 🟡 限流等待中 —— 会自己恢复，只需知会
    RateLimited,
    /// ⚫ 出错 / 异常退出
    Error,
}

impl AttentionLevel {
    /// 稳定字符串键（用于节流去重与前端映射）
    pub fn key(&self) -> &'static str {
        match self {
            AttentionLevel::None => "none",
            AttentionLevel::NeedsInput => "needs_input",
            AttentionLevel::Completed => "completed",
            AttentionLevel::RateLimited => "rate_limited",
            AttentionLevel::Error => "error",
        }
    }

    /// 是否需要计入「待处理」角标
    pub fn is_pending(&self) -> bool {
        matches!(
            self,
            AttentionLevel::NeedsInput | AttentionLevel::RateLimited | AttentionLevel::Error
        )
    }

    /// i18n 表里的键；级别名字要出现在通知和日志里，不能硬编码中文
    pub fn i18n_key(&self) -> &'static str {
        match self {
            AttentionLevel::None => "attention.none",
            AttentionLevel::NeedsInput => "attention.needs_input",
            AttentionLevel::Completed => "attention.completed",
            AttentionLevel::RateLimited => "attention.rate_limited",
            AttentionLevel::Error => "attention.error",
        }
    }
}

/// 检测信号
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionSignal {
    pub kind: SignalKind,
    pub description: String,
}

/// 给人看的判定证据快照
///
/// 这里存**事实**，不存解释后的策略：进程是否活着、记录结构是什么、命中了什么。
/// UI 可以把它摊开回答「为什么是这个结论」，但不能拿它再推一次 verdict。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionEvidence {
    pub process_alive: bool,
    pub turn_state: TurnState,
    pub busy_grace_multiplier: u64,
    pub signal_kinds: Vec<SignalKind>,
    pub matched_interrupt_keyword: Option<String>,
    pub matched_completion_marker: Option<String>,
    pub second_opinion: Option<Arbitration>,
}

/// 信号类型
///
/// 这里曾经有一个 `ProcessIdle`（"CPU 无活动"），从来没有任何代码构造过它，
/// 现已删掉。不是忘了实现，而是这条路走不通：`claude` 在等 API 回包的时候
/// CPU 就是 0%，跟停在那儿等人按回车长得一模一样。区分这两者要看记录的
/// **结构**（见 `TurnState`），不是看 CPU。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    /// 会话文件长时间未更新
    FileStale,
    /// 匹配到中断关键词
    KeywordMatch,
    /// 进程已退出
    ProcessExited,
    /// 心跳超时
    HeartbeatTimeout,
}

/// 判定结论
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// 正常运行
    Running,
    /// 疑似中断，继续观察
    Suspicious,
    /// 确认中断，应触发续跑
    ConfirmInterrupt,
    /// 任务已完成，无需续跑
    TaskCompleted,
}

/// 它为什么停下来
///
/// 加这个枚举是为了修一处**动作层的错**：在此之前，判定层只能说出
/// 「确认中断」这一句话，于是所有停顿共用同一个手段——往终端里敲一句「继续」。
/// 可「进程已经死了」和「活没干完自己停了」需要的不是同一件事：
/// 对着一个死掉的进程敲字，字会落到它身后的 shell 里，
/// 我们却把这次投递记成「催过了」。
///
/// 分类的判据全部来自本轮已经采到的证据，**没有新增任何探测手段**：
/// 进程存活位、记录里标成故障的行、散文里的问句、回合结构。
/// 刻意没有的两个分类：
/// - `ContextExhausted`（上下文用尽）—— 现在没有任何证据源能认出它，
///   加一个永远构造不出来的分支，就是重犯 `SignalKind::ProcessIdle` 那个错。
/// - `NetworkError` 单列 —— 判据只有 `error_keywords`，而连接错误和
///   500 混在同一张表里分不开；就算分开了，两者的下一步动作也一样。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterruptReason {
    /// 没有中断，或者判定还没到「确认」那一档
    #[default]
    None,
    /// 进程没了
    ProcessCrashed,
    /// 撞上限流，会自己恢复
    RateLimited,
    /// 它在问一个具体的问题（要授权、要选 y/n）
    AwaitingInput,
    /// 运行时把某一行标成了故障
    RuntimeError,
    /// 回合收尾了、记录也不动了、又没有完成标记 —— 活没干完自己停了
    Stalled,
    /// 停是真的，但说不出为什么
    Unknown,
}

/// 知道原因之后该拿它怎么办
///
/// 这个枚举**要发到前端**，而不是让界面照着 `tactic()` 的分支再抄一份名单。
/// 抄一份的代价很具体：下次加一个原因，界面上要么凭空多出一句
/// 「这次故意没帮你按继续」，要么该说的时候不说，而且两处都编译通过。
/// 手段只有一个出处——判定层说了算，界面只负责把它画出来。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResumeTactic {
    /// 敲字：这一类停顿就是「少一句继续」
    ///
    /// 默认值故意选它而不是「什么都不做」：这个产品存在的理由就是
    /// 「它停了就替我按一下继续」，缺省到不作为等于默认把功能关掉。
    #[default]
    Nudge,
    /// 等着：它会自己恢复，敲字只会在同一个限流窗口里再撞一次
    Wait,
    /// 交回人手上：敲字帮不上忙，甚至会帮倒忙
    HandOff,
}

/// 第二意见：结构性证据用尽时，去问一个模型「这一轮活干完了没有」
///
/// 只有两个取值，而且**只有两个**——这是这条设计的全部价值。问出去的是一道
/// 是非题，回来的必须是一个可判定的信号，而不是一段还要再解析的散文。
/// 旁边那个 [`crate::ai_judge::AiJudge::analyze`] 就是散文版：它让模型回一段
/// JSON，解析失败时只能靠 `contains("true")` 猜——同一份记录这轮说中断、
/// 下轮说没中断，而且没人看得出它是猜的。
///
/// # 权限是单向的
///
/// 仲裁**只能把「不敢下结论」变成「下结论」，不能反过来**。理由是两边的代价
/// 不对称：多敲一句「继续」最坏是浪费一次对话，而少敲一句就是用户又得自己
/// 去发一遍——那正是这个产品存在的理由。所以：
///
/// - 拿不到答案（没开、没配、网络不通、回了别的字）一律当没问过，
///   判定退回今天的样子。**缺一个第二意见，永远不改变任何结论。**
/// - 拿到 `Finished` 也不会去撤销任何已经成立的判定。
///
/// [`Arbitration::Finished`] 的用处只有一个：记住这段记录已经问过了，
/// 别每轮再问一次。它不参与任何决定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arbitration {
    /// 活干完了 —— 只用来记「问过了」，不做决定
    Finished,
    /// 活没干完 —— 就是「它其实没干完，每次都要我去发继续」那一类
    Unfinished,
}

/// 一次判定的产出
///
/// 比单独一个 [`Verdict`] 多一句话：**结构性证据是不是到这儿就用尽了**。
/// 这一位由判定层给，而不是让调用方照着信号列表自己推——「哪里算证据用尽」
/// 跟「怎么下结论」是同一条逻辑的两面，分给两个地方写，下次改判定条件时
/// 就会有一处漏改，而且两边都编译通过。
struct Ruling {
    verdict: Verdict,
    /// 再问一句有可能改变上面那个结论
    wants_second_opinion: bool,
}

impl InterruptReason {
    /// 稳定字符串键（落库、前端映射、日志去重都用它）
    pub fn key(&self) -> &'static str {
        match self {
            InterruptReason::None => "none",
            InterruptReason::ProcessCrashed => "process_crashed",
            InterruptReason::RateLimited => "rate_limited",
            InterruptReason::AwaitingInput => "awaiting_input",
            InterruptReason::RuntimeError => "runtime_error",
            InterruptReason::Stalled => "stalled",
            InterruptReason::Unknown => "unknown",
        }
    }

    /// i18n 表里的键；原因要出现在日志和通知里，不能硬编码中文
    pub fn i18n_key(&self) -> &'static str {
        match self {
            InterruptReason::None => "reason.none",
            InterruptReason::ProcessCrashed => "reason.process_crashed",
            InterruptReason::RateLimited => "reason.rate_limited",
            InterruptReason::AwaitingInput => "reason.awaiting_input",
            InterruptReason::RuntimeError => "reason.runtime_error",
            InterruptReason::Stalled => "reason.stalled",
            InterruptReason::Unknown => "reason.unknown",
        }
    }

    /// 这个原因该配什么手段
    ///
    /// 只有三个原因**不**敲字，每一个都有具体的坏处可说：
    ///
    /// - `ProcessCrashed`：进程没了，那个终端里现在是 shell。敲进去的
    ///   「继续」会变成一条命令，而我们还会把它记成一次成功的续跑。
    /// - `RateLimited`：限流会自己过去。这时候敲字既不能让它提前恢复，
    ///   又会在冷却里白烧一次额度——真正该做的是把这条知会给人，然后等。
    /// - `AwaitingInput`：它在问一个具体的问题。「继续」不是那个问题的答案；
    ///   往一个 `(y/n)` 提示里敲回车，等于替用户批准了一件他没看过的事。
    ///   **这是权限边界问题，不只是效果问题。**
    ///
    /// 其余一律 `Nudge`，包括 `Unknown`——说不清为什么停，但「停了就催一下」
    /// 本来就是这个产品的默认动作，不该因为分不出原因就变成不作为。
    /// 尤其 `Stalled` 必须敲：那正是「它其实没有干完活，每次都要我去发继续」。
    pub fn tactic(&self) -> ResumeTactic {
        match self {
            InterruptReason::ProcessCrashed => ResumeTactic::HandOff,
            InterruptReason::AwaitingInput => ResumeTactic::HandOff,
            InterruptReason::RateLimited => ResumeTactic::Wait,
            InterruptReason::RuntimeError
            | InterruptReason::Stalled
            | InterruptReason::Unknown
            | InterruptReason::None => ResumeTactic::Nudge,
        }
    }
}

/// 回合还没收尾时，把「多久算卡住」的阈值放大多少倍
///
/// 压缩上下文、跑一条几分钟的构建、拉一个大仓库，记录文件都不会落盘。
/// 这些情况下 agent 明明在干活，只看 mtime 却会判成卡住。
/// 放大到 10 倍（默认 60s → 600s）既躲开了正常的长耗时，
/// 又不至于给真挂住的会话留一个永久盲区。
const BUSY_GRACE_MULTIPLIER: u64 = 10;

/// 多策略检测引擎
pub struct Detector {
    config: AppConfig,
    i18n: I18n,
}

/// 分级要看的全部证据
///
/// 打包成一个结构体而不是七个参数：这些值全是「同一轮扫描看到的东西」，
/// 而且有两个 `Option<&str>`、两个 `bool`，散着传特别容易在调用点串位——
/// 把 `recent_output` 和 `error_output` 传反了编译器一句话都不会说，
/// 但散文就会重新变成报错的证据。
///
/// `Copy` 是为了让同一份证据能同时喂给 [`Detector::grade_attention`] 和
/// [`Detector::classify_reason`]：两者看的是同一轮扫描，凑第二份必然走偏。
#[derive(Clone, Copy)]
struct AttentionInput<'a> {
    /// 记录尾部的散文（agent 说的话）
    recent_output: Option<&'a str>,
    /// 记录里被运行时自己标成故障的行
    error_output: Option<&'a str>,
    /// 命中的完成标记（`None` = 没命中）
    completion_marker: Option<&'a str>,
    process_alive: bool,
    verdict: &'a Verdict,
    turn_state: TurnState,
    /// 上一轮问来的第二意见；`None` = 没问过，或者拿不到答案
    ///
    /// 放进这个包里而不是单独传，是为了让分级和定因看到的是**同一份**证据：
    /// 只有一处知道「模型说活没干完」，界面上就不会出现分级说不清、
    /// 原因却说得出来这种自相矛盾的组合。
    second_opinion: Option<Arbitration>,
}

/// ASCII 单词字符（判断词边界用；多字节字符一律算边界）
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// 带词边界的包含判断
///
/// 裸 `contains` 会让 `"500"` 命中 `"1500 tokens"`、`"429"` 命中 `"14290"`，
/// 于是一句正常的用量统计就能把会话判成服务器错误。
///
/// 规则：关键词**首尾是 ASCII 单词字符时**，要求它在原文里的对应一侧
/// 不紧贴另一个单词字符。两个地方特意放宽：
/// - `(y/n)`、`[y/N]` 这类符号打头结尾的，对应那一侧不做要求；
/// - 关键词含中文时完全跳过边界判断——中文本来不用空格分词，
///   `是否继续` 前后紧跟汉字是常态，硬套边界规则会让它永远匹配不上。
fn contains_keyword(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    if !needle.is_ascii() {
        return haystack.contains(needle);
    }

    let hb = haystack.as_bytes();
    let nb = needle.as_bytes();
    let left_needed = is_word_byte(nb[0]);
    let right_needed = is_word_byte(nb[nb.len() - 1]);

    let mut from = 0usize;
    while let Some(pos) = haystack[from..].find(needle) {
        let start = from + pos;
        let end = start + needle.len();
        let left_ok = !left_needed || start == 0 || !is_word_byte(hb[start - 1]);
        let right_ok = !right_needed || end >= hb.len() || !is_word_byte(hb[end]);
        if left_ok && right_ok {
            return true;
        }
        // needle 是 ASCII，start 必然落在字符边界上，+1 安全
        from = start + 1;
    }
    false
}

/// 在已小写化的文本中找出第一个命中的关键词，返回原始关键词
fn first_match(lower_haystack: &str, keywords: &[String]) -> Option<String> {
    keywords
        .iter()
        .find(|kw| contains_keyword(lower_haystack, &kw.to_lowercase()))
        .cloned()
}

impl Detector {
    pub fn new(config: AppConfig) -> Self {
        let i18n = I18n::from_code(&config.language);
        Self { config, i18n }
    }

    /// 对单个会话执行全策略检测
    ///
    /// `process_alive` 由调用方从进程快照中判定，避免每次检测重复枚举系统进程。
    /// `turn_state` 是回合的结构性状态（见 [`TurnState`]）：它决定「多久不写文件
    /// 才算卡住」，因为正在压缩上下文或正在跑长命令的会话本来就不写文件。
    /// `error_output` 只包含记录里明确标成故障的行（见
    /// [`AgentAdapter::error_output`](crate::adapters::AgentAdapter::error_output)）：
    /// 「出错 / 限流」两级只看它，散文里提到 500 不算出错。
    ///
    /// `second_opinion` 是**上一轮**问来的答案（见 [`Arbitration`]）：这一层是纯的，
    /// 不会自己去发网络请求。判定要用第二意见时先把 `wants_second_opinion` 立起来，
    /// 由动作层去问、把答案缓存住，下一轮再喂回来。多等一个轮询周期，
    /// 换来的是这条流水线仍然是同步、可测、网络再慢也拖不住的。
    pub fn detect(
        &self,
        session: &AgentSession,
        recent_output: Option<&str>,
        error_output: Option<&str>,
        process_alive: bool,
        turn_state: TurnState,
        second_opinion: Option<Arbitration>,
    ) -> DetectionResult {
        let now = Local::now();
        let mut signals = Vec::new();
        let mut has_completion_marker = false;
        let mut matched_marker: Option<String> = None;
        let mut matched_interrupt_keyword: Option<String> = None;
        let mut has_active_goal = false;

        // 回合没收尾时把「不动」的容忍度放宽：压缩上下文、跑几分钟的构建、
        // 拉一个大仓库，期间记录文件都不会落盘，按原阈值全都会被判成卡住。
        let stale_grace = if turn_state.is_busy() {
            BUSY_GRACE_MULTIPLIER
        } else {
            1
        };

        // 策略 1: 进程存活检测
        if !process_alive {
            signals.push(DetectionSignal {
                kind: SignalKind::ProcessExited,
                description: self.i18n.tf(
                    "signal.process_exited",
                    &[("pid", &session.pid.to_string())],
                ),
            });
        }

        // 策略 2: 会话文件新鲜度检测
        if let Some(ref file_path) = session.session_file {
            if let Some(stale_secs) = self.check_file_staleness(file_path) {
                let threshold = self.config.idle_timeout_secs * stale_grace;
                if stale_secs > threshold {
                    signals.push(DetectionSignal {
                        kind: SignalKind::FileStale,
                        description: self.i18n.tf(
                            "signal.file_stale",
                            &[
                                ("elapsed", &stale_secs.to_string()),
                                ("threshold", &threshold.to_string()),
                            ],
                        ),
                    });
                }
            }
        }

        // 策略 3: 关键词匹配（中断信号 + 完成标记双重校验）
        if let Some(output) = recent_output {
            // 检查完成标记
            for marker in &self.config.completion_markers {
                if output.contains(marker.as_str()) {
                    has_completion_marker = true;
                    matched_marker = Some(marker.clone());
                    break;
                }
            }

            // 检查中断关键词
            if !has_completion_marker {
                let lower = output.to_lowercase();
                if let Some(keyword) = first_match(&lower, &self.config.custom_keywords) {
                    matched_interrupt_keyword = Some(keyword.clone());
                    signals.push(DetectionSignal {
                        kind: SignalKind::KeywordMatch,
                        description: self
                            .i18n
                            .tf("signal.keyword_match", &[("keyword", &keyword)]),
                    });
                }

                // 检测活跃 Goal 状态（用于智能续跑提示词选择）
                for goal_kw in &self.config.goal_keywords {
                    if output.contains(goal_kw.as_str()) {
                        has_active_goal = true;
                        break;
                    }
                }
            }
        }

        // 策略 4: 心跳超时（基于 last_activity）
        //
        // 注意：`last_activity` 本身就是记录文件的 mtime，所以这条跟策略 2
        // 说的是同一件事，只是阈值更宽。判定时不能把它们当成两个独立证据。
        if let Ok(last) = NaiveDateTime::parse_from_str(&session.last_activity, "%Y-%m-%d %H:%M:%S")
        {
            let elapsed = now.naive_local() - last;
            let timeout =
                self.config.idle_timeout_secs * self.config.idle_threshold as u64 * stale_grace;
            if elapsed.num_seconds() as u64 > timeout {
                signals.push(DetectionSignal {
                    kind: SignalKind::HeartbeatTimeout,
                    description: self.i18n.tf(
                        "signal.heartbeat_timeout",
                        &[
                            ("elapsed", &elapsed.num_seconds().to_string()),
                            ("threshold", &timeout.to_string()),
                        ],
                    ),
                });
            }
        }

        // 综合判定
        let Ruling {
            verdict,
            wants_second_opinion,
        } = self.make_verdict(
            process_alive,
            &signals,
            has_completion_marker,
            session,
            turn_state,
            second_opinion,
        );

        // 注意力分级（与续跑判定正交）
        let evidence = AttentionInput {
            recent_output,
            error_output,
            completion_marker: matched_marker.as_deref(),
            process_alive,
            verdict: &verdict,
            turn_state,
            second_opinion,
        };
        let (attention, attention_detail) = self.grade_attention(evidence);
        let interrupt_reason = self.classify_reason(evidence);
        let signal_kinds = signals.iter().map(|signal| signal.kind.clone()).collect();

        DetectionResult {
            session_id: session.id.clone(),
            interrupted: verdict == Verdict::ConfirmInterrupt,
            signals,
            has_completion_marker,
            matched_marker: matched_marker.clone(),
            has_active_goal,
            verdict,
            attention,
            attention_detail,
            evidence: DetectionEvidence {
                process_alive,
                turn_state,
                busy_grace_multiplier: stale_grace,
                signal_kinds,
                matched_interrupt_keyword,
                matched_completion_marker: matched_marker.clone(),
                second_opinion,
            },
            interrupt_reason,
            wants_second_opinion,
            detected_at: now.format("%Y-%m-%d %H:%M:%S").to_string(),
        }
    }

    /// 它为什么停下来 —— 只在判定为「确认中断」时才有意义
    ///
    /// 与 [`Self::grade_attention`] 用同一份证据，但**顺序不同，而且必须不同**。
    /// 分级把「记录里标成故障的行」排在「进程没了」前面，那是对的：两者都是
    /// ⚫出错，谁先命中都不影响颜色。可原因这里反过来——一个崩掉的进程往往
    /// 正好留下最后一行错误，照分级的顺序走就会被认成 `RuntimeError`，
    /// 于是又开始往一个死进程里敲字。**进程存活位是事实，散文和日志行是线索，
    /// 事实优先。**
    ///
    /// 判定不到「确认中断」就一律 `None`：这个字段是给动作层用的，
    /// 而动作层只在确认时才动手。给一个还在正常跑的会话贴上原因标签，
    /// 只会让日志里出现「原因：说不清」这种没有信息量的噪音。
    fn classify_reason(&self, input: AttentionInput<'_>) -> InterruptReason {
        let AttentionInput {
            recent_output,
            error_output,
            completion_marker,
            process_alive,
            verdict,
            turn_state,
            second_opinion,
        } = input;

        if completion_marker.is_some() || *verdict != Verdict::ConfirmInterrupt {
            return InterruptReason::None;
        }

        // 事实优先：进程没了就是没了，哪怕它临死前还留了一行错误
        if !process_alive {
            return InterruptReason::ProcessCrashed;
        }

        if let Some(errors) = error_output {
            let lower = errors.to_lowercase();
            // 限流排在故障前面：限流行本身常被运行时标成故障，
            // 而两者的手段相反（等 vs 敲），认错了就会在限流窗口里白撞一次。
            if first_match(&lower, &self.config.rate_limit_keywords).is_some() {
                return InterruptReason::RateLimited;
            }
            return InterruptReason::RuntimeError;
        }

        if let Some(output) = recent_output {
            let lower = output.to_lowercase();
            if first_match(&lower, &self.config.input_keywords).is_some() {
                return InterruptReason::AwaitingInput;
            }
        }

        // 回合结构说「这一轮已经收尾」，记录却还是不动，又没有完成标记：
        // 活没干完自己停了，这才是该敲一句「继续」的那一类。
        //
        // 第二个条件走的是另一条路到同一个结论：读不出回合结构的适配器
        // （Codex / OpenCode 这类不落盘的）只能一路落到「说不出为什么」，
        // 而这恰好是那道问题问出来的东西——模型说的就是「活没干完」。
        // 注意它只会把 `Unknown` 换成 `Stalled`，上面每一条都比它先走，
        // 事实和日志行永远排在一个模型的意见前面。
        if turn_state == TurnState::AwaitingUser || second_opinion == Some(Arbitration::Unfinished)
        {
            return InterruptReason::Stalled;
        }

        InterruptReason::Unknown
    }

    /// 注意力分级：决定「要不要现在叫人 + 用什么颜色叫」
    ///
    /// 优先级：完成 > 出错 > 限流 > 等待输入 > 卡住
    /// 出错排在限流之前，因为限流会自己恢复而报错不会。
    ///
    /// 这里比 [`Self::make_verdict`] 宽松是故意的：弹一条通知的代价只是「看一眼」，
    /// 往终端里敲字的代价是搞乱一个正在干活的会话。所以关键词命中足够叫人，
    /// 但不足以动手。
    ///
    /// 但「宽松」不等于「可以拿散文当证据」。「出错」和「限流」只看 `error_output`
    /// ——也就是记录里自己标成故障的行。实测踩过：agent 写下一句
    /// 「不再撞上错误关键词 500」，会话立刻被标成 ⚫出错，词边界拦不住，
    /// 因为那真是个独立的 `500`。散文里能算证据的只剩「在问你话」这一类
    /// （`input_keywords`），因为那句话本身就是它在等人。
    fn grade_attention(&self, input: AttentionInput<'_>) -> (AttentionLevel, Option<String>) {
        let AttentionInput {
            recent_output,
            error_output,
            completion_marker,
            process_alive,
            verdict,
            turn_state,
            second_opinion,
        } = input;

        if let Some(marker) = completion_marker {
            return (
                AttentionLevel::Completed,
                Some(
                    self.i18n
                        .tf("attention.detail.completed", &[("marker", marker)]),
                ),
            );
        }

        if let Some(errors) = error_output {
            let lower = errors.to_lowercase();
            if let Some(kw) = first_match(&lower, &self.config.error_keywords) {
                return (
                    AttentionLevel::Error,
                    Some(
                        self.i18n
                            .tf("attention.detail.keyword", &[("keyword", &kw)]),
                    ),
                );
            }
            if let Some(kw) = first_match(&lower, &self.config.rate_limit_keywords) {
                return (
                    AttentionLevel::RateLimited,
                    Some(
                        self.i18n
                            .tf("attention.detail.keyword", &[("keyword", &kw)]),
                    ),
                );
            }
            // 标成故障但一条关键词都没对上：仍然是故障，别把它咽下去
            return (
                AttentionLevel::Error,
                Some(self.i18n.tf(
                    "attention.detail.keyword",
                    &[("keyword", errors.lines().next().unwrap_or_default())],
                )),
            );
        }

        if let Some(output) = recent_output {
            let lower = output.to_lowercase();
            if let Some(kw) = first_match(&lower, &self.config.input_keywords) {
                return (
                    AttentionLevel::NeedsInput,
                    Some(
                        self.i18n
                            .tf("attention.detail.awaiting_keyword", &[("keyword", &kw)]),
                    ),
                );
            }
        }

        if !process_alive {
            return (
                AttentionLevel::Error,
                Some(self.i18n.t("attention.detail.process_exited").to_string()),
            );
        }

        if *verdict == Verdict::ConfirmInterrupt {
            // 回合收尾了却还是被判成中断，说明是「活没干完自己停了」——
            // 这正是用户每次得手动去发「继续」的那种情况，措辞要说清楚。
            // 问过一句并且得到「活没干完」时，走的是同一句措辞——那句话
            // 本来就是这个意思，没必要为「模型说的」再造一个说法。
            let key = if turn_state == TurnState::AwaitingUser
                || second_opinion == Some(Arbitration::Unfinished)
            {
                "attention.detail.stalled"
            } else {
                "attention.detail.silent"
            };
            return (
                AttentionLevel::NeedsInput,
                Some(self.i18n.t(key).to_string()),
            );
        }

        (AttentionLevel::None, None)
    }

    /// 检查文件距上次修改的秒数
    fn check_file_staleness(&self, path: &str) -> Option<u64> {
        let path_buf = PathBuf::from(path);
        let metadata = fs::metadata(&path_buf).ok()?;
        let modified = metadata.modified().ok()?;
        let elapsed = Local::now()
            .naive_local()
            .signed_duration_since(chrono::DateTime::<Local>::from(modified).naive_local());
        Some(elapsed.num_seconds().max(0) as u64)
    }

    /// 综合判定逻辑：这个会话到不到「可以往里敲字」的程度
    ///
    /// 判定的门槛比分级高得多，因为动手的代价不对称：漏判只是少续一次跑，
    /// 误判是往一个正在干活的会话里插一句话。所以这里只认两种确凿情形：
    ///
    /// 1. **进程没了**且没有完成标记 —— 事实明确，没什么可推测的。
    /// 2. **回合已经收尾**（[`TurnState::AwaitingUser`]）**且记录不动了** ——
    ///    agent 自己停在那儿等人，也就是「它其实没有干完活，每次都要我去发继续」。
    ///
    /// 被明确排除在外的：
    /// - **只有关键词命中**。输出里提到 "rate limit" 只说明它在谈这件事，
    ///   不代表它被限流了；这种最多够 [`Verdict::Suspicious`]（弹通知，不动手）。
    /// - **回合还没收尾**（[`TurnState::ToolRunning`] / [`TurnState::Busy`]）。
    ///   压缩上下文、跑长命令期间记录本来就不写，此时的「不动」不是证据。
    ///
    /// 还有一处旧逻辑的坑要点明：`FileStale` 和 `HeartbeatTimeout` 都来自记录文件的
    /// mtime，是**同一个事实的两种说法**。老代码的「至少两个信号才确认」永远会被
    /// 这两个信号自动满足，等于没有校验——所以现在不再数信号个数。
    fn make_verdict(
        &self,
        process_alive: bool,
        signals: &[DetectionSignal],
        has_completion_marker: bool,
        session: &AgentSession,
        turn_state: TurnState,
        second_opinion: Option<Arbitration>,
    ) -> Ruling {
        // 除了下面唯一那一处，所有出口都不需要第二意见
        let settled = |verdict| Ruling {
            verdict,
            wants_second_opinion: false,
        };

        // 已完成 → 不续跑
        if has_completion_marker {
            return settled(Verdict::TaskCompleted);
        }

        // 会话已标记完成或退出
        if session.status == SessionStatus::Completed || session.status == SessionStatus::Exited {
            return settled(Verdict::Running);
        }

        // 进程已退出且无完成标记 → 确认中断
        if !process_alive {
            return settled(Verdict::ConfirmInterrupt);
        }

        // 这里**故意不看续跑额度**。
        //
        // 旧代码在这个位置有一句「额度用光就返回 Suspicious」，那是把两个不同的
        // 问题揉在一起了：判定层回答的是「这个会话现在什么状态」，额度回答的是
        // 「我们还该不该动手」。揉在一起有两个真实后果：
        //
        // 1. 额度一旦用光，判定永远给不出 `Running`，于是清零条件也永远不成立
        //    ——会话自己恢复干活了，界面上还挂着「疑似中断」。
        // 2. 更糟的是它**悄悄放弃**：状态从「确认中断」降级成「疑似」，
        //    注意力分级就不再叫人了。应用不打算自己动手的时候，恰恰是最该
        //    把事情交回人手上的时候，而不是装作还在守着。
        //
        // 所以额度改成只拦「敲字」这个动作，拦在 `monitor::has_nudges_left`。
        // 判定层只管说实话。

        // 记录文件停更（这两个信号同源，取其一即可）
        let transcript_idle = signals
            .iter()
            .any(|s| matches!(s.kind, SignalKind::FileStale | SignalKind::HeartbeatTimeout));
        let keyword_hit = signals
            .iter()
            .any(|s| matches!(s.kind, SignalKind::KeywordMatch));

        // 正在干活 → 什么都不做。这时的停更是压缩上下文或长命令造成的，
        // 阈值本身已经放宽到 BUSY_GRACE_MULTIPLIER 倍；仍然超时说明真的可疑，
        // 但也只到「继续观察」，不至于动手。
        //
        // 这一处的「可疑」**故意不去问第二意见**。回合结构说它正在跑一个工具
        // 调用，那是事实，不是猜测；拿一个模型的意见去盖掉它，等于把
        // `long_compaction_pause_is_not_a_stall` 钉住的那件事重新放开——
        // 压缩上下文的会话又会被当成卡住，然后被敲一句「继续」。
        if turn_state.is_busy() {
            return settled(if transcript_idle {
                Verdict::Suspicious
            } else {
                Verdict::Running
            });
        }

        match (turn_state, transcript_idle) {
            // 回合收尾 + 记录停更 + 没有完成标记 → 它确实停在那儿等人了
            (TurnState::AwaitingUser, true) => settled(Verdict::ConfirmInterrupt),
            // 刚收尾还没到阈值：正常的一问一答间隙，别催
            (TurnState::AwaitingUser, false) => settled(Verdict::Running),
            // 读不出回合结构（Codex / OpenCode 这类不落盘的）：沿用超时兜底
            (_, true) => settled(Verdict::ConfirmInterrupt),
            // 没有停更信号时，光靠关键词只能到「可疑」
            //
            // **这是整个判定里唯一一处「结构性证据用尽」。** 关键词是弱证据：
            // 记录还在长（所以它没停更），可里面又确实出现了一句像在等人的话。
            // 单看这两样谁都不够，于是今天的结果是「可疑」——不动手、也不出声，
            // 一直挂到记录真的停更为止。那段等待就是用户自己去发「继续」的窗口。
            //
            // 所以第二意见只加在这儿：拿到「活没干完」就把这一票凑上去，
            // 提前几分钟动手；拿不到就一个字不改，退回上面那段等待。
            (_, false) => {
                if !keyword_hit {
                    return settled(Verdict::Running);
                }
                match second_opinion {
                    Some(Arbitration::Unfinished) => settled(Verdict::ConfirmInterrupt),
                    // 问过了说干完了：不改结论（仲裁没有叫停的权力），也不再问
                    Some(Arbitration::Finished) => settled(Verdict::Suspicious),
                    None => Ruling {
                        verdict: Verdict::Suspicious,
                        wants_second_opinion: true,
                    },
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::SessionStatus;

    fn detector() -> Detector {
        Detector::new(AppConfig::default())
    }

    /// 一个刚刚活动过的会话：不带任何停更信号
    fn session() -> AgentSession {
        AgentSession {
            id: "cc-1".to_string(),
            pid: 4242,
            last_activity: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            status: SessionStatus::Active,
            ..Default::default()
        }
    }

    fn signal(kind: SignalKind) -> Vec<DetectionSignal> {
        vec![DetectionSignal {
            kind,
            description: String::new(),
        }]
    }

    fn ago(secs: i64) -> String {
        (Local::now() - chrono::Duration::seconds(secs))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
    }
    // TESTS_PLACEHOLDER_DETECTOR

    // ── 中断原因与手段 ──

    /// 造一份「确认中断」的证据，只有需要改的那几项要写出来
    fn evidence<'a>(
        recent_output: Option<&'a str>,
        error_output: Option<&'a str>,
        process_alive: bool,
        turn_state: TurnState,
        verdict: &'a Verdict,
    ) -> AttentionInput<'a> {
        AttentionInput {
            recent_output,
            error_output,
            completion_marker: None,
            process_alive,
            verdict,
            turn_state,
            second_opinion: None,
        }
    }

    /// 这条是加 `InterruptReason` 的**全部理由**。
    ///
    /// 一个崩掉的进程往往正好留下最后一行错误。分级层把「标成故障的行」排在
    /// 「进程没了」前面（那里无所谓，两者都是 ⚫出错），照那个顺序分类原因
    /// 就会认成 `RuntimeError` → `Nudge`，于是继续往一个死进程里敲字：
    /// 字落进它身后的 shell，我们还把这次投递记成一次成功的续跑。
    #[test]
    fn a_dead_process_that_also_logged_an_error_is_still_dead() {
        let verdict = Verdict::ConfirmInterrupt;
        let reason = detector().classify_reason(evidence(
            None,
            Some("fatal error: connection error"),
            false,
            TurnState::AwaitingUser,
            &verdict,
        ));
        assert_eq!(reason, InterruptReason::ProcessCrashed);
        assert_eq!(reason.tactic(), ResumeTactic::HandOff);
    }

    /// 限流行本身常被运行时标成故障。认成 `RuntimeError` 就会去敲字，
    /// 而敲字既不能让它提前恢复，又在同一个限流窗口里白烧一次额度。
    #[test]
    fn rate_limits_are_waited_out_not_nudged() {
        let verdict = Verdict::ConfirmInterrupt;
        let reason = detector().classify_reason(evidence(
            None,
            Some("429 rate limit reached, retrying in 30s"),
            true,
            TurnState::AwaitingUser,
            &verdict,
        ));
        assert_eq!(reason, InterruptReason::RateLimited);
        assert_eq!(reason.tactic(), ResumeTactic::Wait);
    }

    /// 往一个 `(y/n)` 提示里敲「继续」，等于替用户批准了一件他没看过的事。
    /// 这是权限边界，不只是效果问题。
    #[test]
    fn a_pending_approval_is_never_answered_for_the_user() {
        let verdict = Verdict::ConfirmInterrupt;
        let reason = detector().classify_reason(evidence(
            Some("Do you want to delete these files? (y/n)"),
            None,
            true,
            TurnState::AwaitingUser,
            &verdict,
        ));
        assert_eq!(reason, InterruptReason::AwaitingInput);
        assert_eq!(reason.tactic(), ResumeTactic::HandOff);
    }

    /// 「它其实没有干完活，每次都要我去发继续」——这一类必须照敲，
    /// 分派逻辑不能把它一起挡掉。
    #[test]
    fn the_stall_we_built_this_for_still_gets_nudged() {
        let verdict = Verdict::ConfirmInterrupt;
        let reason = detector().classify_reason(evidence(
            Some("我先看一下这个文件。"),
            None,
            true,
            TurnState::AwaitingUser,
            &verdict,
        ));
        assert_eq!(reason, InterruptReason::Stalled);
        assert_eq!(reason.tactic(), ResumeTactic::Nudge);
    }

    /// 读不出回合结构（Codex / OpenCode 这类）时走超时兜底：说不清为什么停，
    /// 但「停了就催一下」本来就是默认动作，不该因为分不出原因就变成不作为。
    #[test]
    fn an_unexplained_stop_still_gets_nudged() {
        let verdict = Verdict::ConfirmInterrupt;
        let reason =
            detector().classify_reason(evidence(None, None, true, TurnState::Unknown, &verdict));
        assert_eq!(reason, InterruptReason::Unknown);
        assert_eq!(reason.tactic(), ResumeTactic::Nudge);
    }

    /// 原因是给动作层用的，而动作层只在「确认中断」时才动手。
    /// 给一个还在正常跑的会话贴原因标签，只会往日志里灌噪音。
    #[test]
    fn a_running_session_has_no_reason_to_explain() {
        for verdict in [
            Verdict::Running,
            Verdict::Suspicious,
            Verdict::TaskCompleted,
        ] {
            let reason = detector().classify_reason(evidence(
                Some("Do you want to continue? (y/n)"),
                Some("fatal error"),
                false,
                TurnState::AwaitingUser,
                &verdict,
            ));
            assert_eq!(reason, InterruptReason::None, "verdict={verdict:?}");
        }
    }

    /// 完成标记在场时不谈原因：活干完了，没有「为什么停」这个问题。
    #[test]
    fn a_finished_task_has_no_reason_to_explain() {
        let verdict = Verdict::ConfirmInterrupt;
        let reason = detector().classify_reason(AttentionInput {
            recent_output: None,
            error_output: None,
            completion_marker: Some("任务完成"),
            process_alive: true,
            verdict: &verdict,
            turn_state: TurnState::AwaitingUser,
            second_opinion: None,
        });
        assert_eq!(reason, InterruptReason::None);
    }

    /// 散文里提到 500 不算出错，这条规则在原因层也得成立——
    /// 否则一句正常的用量统计就能把「该催」变成「交给你」。
    #[test]
    fn prose_about_errors_does_not_change_the_reason() {
        let verdict = Verdict::ConfirmInterrupt;
        let reason = detector().classify_reason(evidence(
            Some("这次不会再撞上 500 了"),
            None,
            true,
            TurnState::AwaitingUser,
            &verdict,
        ));
        assert_eq!(reason, InterruptReason::Stalled);
    }

    /// 只有三个原因不敲字，而且每一个都有具体的坏处可说。
    /// 这条锁住那份名单：以后新增原因，默认必须落在 `Nudge` 一侧。
    #[test]
    fn only_three_reasons_hold_their_fire() {
        let silent = [
            InterruptReason::ProcessCrashed,
            InterruptReason::AwaitingInput,
            InterruptReason::RateLimited,
        ];
        for reason in silent {
            assert_ne!(reason.tactic(), ResumeTactic::Nudge, "{}", reason.key());
        }
        for reason in [
            InterruptReason::None,
            InterruptReason::RuntimeError,
            InterruptReason::Stalled,
            InterruptReason::Unknown,
        ] {
            assert_eq!(reason.tactic(), ResumeTactic::Nudge, "{}", reason.key());
        }
    }

    // ── 关键词边界 ──

    #[test]
    fn ascii_keywords_need_word_boundaries() {
        // 这是那次事故的直接成因：输出里随便一个「1500 tokens」
        // 就撞上了错误关键词 "500"，会话被判成 500 错误
        assert!(!contains_keyword("used 1500 input tokens", "500"));
        assert!(contains_keyword("http 500 internal", "500"));
        assert!(!contains_keyword("request id 14290", "429"));
        assert!(contains_keyword("got 429 from the api", "429"));
        // 标点开头/结尾的关键词不该被边界规则挡掉
        assert!(contains_keyword("continue? (y/n)", "(y/n)"));
        assert!(contains_keyword("timed out after 30s", "timed out"));
    }

    #[test]
    fn chinese_keywords_skip_boundary_checks() {
        // 中文没有词边界可言，硬套 ASCII 规则会一条都匹配不上
        assert!(contains_keyword("请确认是否继续执行", "是否继续"));
        assert!(contains_keyword("需要确认这一步", "需要确认"));
    }

    #[test]
    fn token_counts_do_not_look_like_errors() {
        let out = "Wrote 1500 lines, used 14290 output tokens in total.";
        let r = detector().detect(
            &session(),
            Some(out),
            None,
            true,
            TurnState::AwaitingUser,
            None,
        );
        assert_eq!(r.attention, AttentionLevel::None);
        assert_eq!(r.verdict, Verdict::Running);
    }

    // ── 散文 vs 结构：谁有资格把会话标成「出错」 ──

    #[test]
    fn talking_about_an_error_is_not_having_one() {
        // 实测栽过的那句：agent 自己写下「不再撞上错误关键词 500」，
        // 会话立刻被标成 ⚫出错。词边界拦不住——那确实是个独立的 500。
        let d = detector();
        let out = "修完之后就不会再撞上错误关键词 500 了，429 同理。";
        let r = d.detect(
            &session(),
            Some(out),
            None,
            true,
            TurnState::AwaitingUser,
            None,
        );
        assert_eq!(
            r.attention,
            AttentionLevel::None,
            "散文里提到状态码不算出错"
        );
    }

    #[test]
    fn runtime_marked_failures_do_alert() {
        // 反过来，真出错必须报：证据来自记录里标成故障的行
        let d = detector();
        let r = d.detect(
            &session(),
            Some("正在重试…"),
            Some("API Error: 500"),
            true,
            TurnState::AwaitingUser,
            None,
        );
        assert_eq!(r.attention, AttentionLevel::Error);

        let limited = d.detect(
            &session(),
            None,
            Some("API Error: 429 rate limit exceeded"),
            true,
            TurnState::AwaitingUser,
            None,
        );
        assert_eq!(limited.attention, AttentionLevel::RateLimited);
    }

    #[test]
    fn unrecognized_failure_lines_still_alert() {
        // 标成故障但没对上任何关键词：宁可报一个「出错」，也不要静静咽下去
        let d = detector();
        let r = d.detect(
            &session(),
            None,
            Some("upstream connect failure"),
            true,
            TurnState::AwaitingUser,
            None,
        );
        assert_eq!(r.attention, AttentionLevel::Error);
        assert!(r
            .attention_detail
            .as_deref()
            .unwrap_or_default()
            .contains("upstream connect failure"));
    }

    // ── 判定矩阵：要不要往终端里敲字 ──

    #[test]
    fn keyword_hit_alone_never_types() {
        // 会话在问话（散文里确实是它在等人）：可以叫人，但不能动手
        let d = detector();
        let out = "需要确认这一步再继续，是否继续执行？";
        let r = d.detect(
            &session(),
            Some(out),
            None,
            true,
            TurnState::AwaitingUser,
            None,
        );
        assert_eq!(r.attention, AttentionLevel::NeedsInput);
        assert!(!r.interrupted, "关键词命中不足以确认中断");
    }

    #[test]
    fn awaiting_user_plus_silence_confirms() {
        let d = detector();
        let s = session();
        assert_eq!(
            d.make_verdict(
                true,
                &signal(SignalKind::FileStale),
                false,
                &s,
                TurnState::AwaitingUser,
                None
            )
            .verdict,
            Verdict::ConfirmInterrupt,
            "回合收尾 + 记录停更 = 用户每次得去发「继续」的那种停"
        );
        assert_eq!(
            d.make_verdict(true, &[], false, &s, TurnState::AwaitingUser, None)
                .verdict,
            Verdict::Running,
            "刚收尾还没到阈值只是一问一答的间隙"
        );
    }

    #[test]
    fn busy_turn_is_never_confirmed_even_when_stale() {
        let d = detector();
        let s = session();
        for turn in [TurnState::ToolRunning, TurnState::Busy] {
            assert_eq!(
                d.make_verdict(true, &signal(SignalKind::FileStale), false, &s, turn, None)
                    .verdict,
                Verdict::Suspicious,
                "{turn:?}：放宽后的阈值仍超时，最多到「继续观察」"
            );
            assert_eq!(
                d.make_verdict(true, &[], false, &s, turn, None).verdict,
                Verdict::Running
            );
        }
    }

    #[test]
    fn unknown_turn_still_falls_back_to_timeout() {
        // Codex / OpenCode 读不出回合结构，只能靠超时兜底；
        // 一旦这里也不确认，那两个适配器就彻底不会续跑了
        let d = detector();
        assert_eq!(
            d.make_verdict(
                true,
                &signal(SignalKind::HeartbeatTimeout),
                false,
                &session(),
                TurnState::Unknown,
                None
            )
            .verdict,
            Verdict::ConfirmInterrupt
        );
    }

    #[test]
    fn completion_marker_and_dead_process_take_precedence() {
        let d = detector();
        let s = session();
        assert_eq!(
            d.make_verdict(true, &[], true, &s, TurnState::AwaitingUser, None)
                .verdict,
            Verdict::TaskCompleted
        );
        assert_eq!(
            d.make_verdict(false, &[], false, &s, TurnState::ToolRunning, None)
                .verdict,
            Verdict::ConfirmInterrupt,
            "进程没了就是事实，不用再看回合结构"
        );
    }

    #[test]
    fn verdict_ignores_every_resume_counter() {
        // 判定层只回答「这个会话现在什么状态」。额度用光是「我们不打算动手」，
        // 不是「它没有停在那儿等人」——把额度写进判定，等于让应用在放弃的同时
        // 顺手把状态也降级，于是连提醒都不发了。额度现在拦在 `monitor` 那一侧。
        let d = detector();
        let exhausted = AgentSession {
            resume_count: 100,
            resume_streak: AppConfig::default().max_resume_count + 3,
            resume_failures: 7,
            ..session()
        };
        assert_eq!(
            d.make_verdict(
                true,
                &signal(SignalKind::FileStale),
                false,
                &exhausted,
                TurnState::AwaitingUser,
                None
            )
            .verdict,
            Verdict::ConfirmInterrupt,
            "催不动了也要照实说它停着——不然注意力分级就不叫人了"
        );
    }

    #[test]
    fn lifetime_resume_count_never_caps() {
        // 回归：上限管的是「连着催都没反应」，不是「一辈子只准被催 5 次」。
        // 一个跑一整天、真停顿过很多次但每次都被成功唤醒的会话，
        // 累计次数早就破百了，照样该继续守着它。
        let d = detector();
        let veteran = AgentSession {
            resume_count: 100,
            resume_streak: 0,
            ..session()
        };
        assert_eq!(
            d.make_verdict(
                true,
                &signal(SignalKind::FileStale),
                false,
                &veteran,
                TurnState::AwaitingUser,
                None
            )
            .verdict,
            Verdict::ConfirmInterrupt
        );
    }

    #[test]
    fn arbiter_only_votes_at_the_weak_evidence_gap() {
        let d = detector();
        let s = session();
        let weak = signal(SignalKind::KeywordMatch);

        let unanswered = d.make_verdict(true, &weak, false, &s, TurnState::Unknown, None);
        assert_eq!(unanswered.verdict, Verdict::Suspicious);
        assert!(unanswered.wants_second_opinion);

        let unfinished = d.make_verdict(
            true,
            &weak,
            false,
            &s,
            TurnState::Unknown,
            Some(Arbitration::Unfinished),
        );
        assert_eq!(unfinished.verdict, Verdict::ConfirmInterrupt);
        assert!(!unfinished.wants_second_opinion);

        let finished = d.make_verdict(
            true,
            &weak,
            false,
            &s,
            TurnState::Unknown,
            Some(Arbitration::Finished),
        );
        assert_eq!(finished.verdict, Verdict::Suspicious);
        assert!(!finished.wants_second_opinion, "同一版记录不能反复花钱问");
    }

    #[test]
    fn busy_turn_never_asks_the_arbiter() {
        let ruling = detector().make_verdict(
            true,
            &signal(SignalKind::KeywordMatch),
            false,
            &session(),
            TurnState::Busy,
            None,
        );
        assert_eq!(ruling.verdict, Verdict::Running);
        assert!(!ruling.wants_second_opinion);
    }

    // TESTS_PLACEHOLDER_DETECTOR

    #[test]
    fn long_compaction_pause_is_not_a_stall() {
        // 用户的原问题：「如果工具在压缩会话上下文耗时比较久是不是也会被错误识别呢？」
        // 压缩期间记录文件一个字都不写，按原阈值（60×3=180s）400s 必然被判中断。
        let d = detector();
        let quiet = AgentSession {
            last_activity: ago(400),
            ..session()
        };

        let judged = d.detect(&quiet, None, None, true, TurnState::AwaitingUser, None);
        assert!(judged
            .signals
            .iter()
            .any(|s| matches!(s.kind, SignalKind::HeartbeatTimeout)));
        assert_eq!(judged.verdict, Verdict::ConfirmInterrupt);

        let busy = d.detect(&quiet, None, None, true, TurnState::Busy, None);
        assert!(
            busy.signals.is_empty(),
            "回合没收尾时阈值放宽 10 倍，400s 还够不上"
        );
        assert_eq!(busy.verdict, Verdict::Running);
        assert_eq!(busy.attention, AttentionLevel::None, "更不该弹通知打扰");
    }

    #[test]
    fn stalled_and_silent_are_worded_differently() {
        // 「活没干完自己停了」和「长时间没输出」是两回事，通知正文得说清是哪种
        let d = detector();
        let quiet = AgentSession {
            last_activity: ago(400),
            ..session()
        };

        let stalled = d.detect(&quiet, None, None, true, TurnState::AwaitingUser, None);
        assert_eq!(stalled.attention, AttentionLevel::NeedsInput);
        assert_eq!(
            stalled.attention_detail.as_deref(),
            Some(d.i18n.t("attention.detail.stalled"))
        );

        let silent = d.detect(&quiet, None, None, true, TurnState::Unknown, None);
        assert_eq!(
            silent.attention_detail.as_deref(),
            Some(d.i18n.t("attention.detail.silent"))
        );
    }

    #[test]
    fn a_session_we_stopped_nudging_still_calls_for_help() {
        // 整条链上最要紧的一句保证：我们不再替他敲字的那一刻，必须改成叫他。
        // 上一版把额度写进判定层，判定从「确认中断」降级成「疑似」，
        // `grade_attention` 对「疑似」的处理是不打扰——于是应用悄悄放弃，
        // 托盘上一片安静，用户还以为有人在守着。
        let d = detector();
        let exhausted = AgentSession {
            last_activity: ago(400),
            resume_streak: AppConfig::default().max_resume_count + 2,
            resume_count: 42,
            resume_failures: 3,
            ..session()
        };

        let r = d.detect(&exhausted, None, None, true, TurnState::AwaitingUser, None);
        assert_eq!(r.verdict, Verdict::ConfirmInterrupt, "它确实还停在那儿");
        assert_eq!(r.attention, AttentionLevel::NeedsInput);
        assert!(r.attention.is_pending(), "托盘角标上得有它这一个");
    }
}
