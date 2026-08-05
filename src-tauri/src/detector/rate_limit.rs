//! 限流识别：认出「这是限流」，以及「该等多久」
//!
//! 这个模块存在的理由是一个很具体的事故形状：**认不出限流的代价不对称。**
//!
//! `InterruptReason::RateLimited` 配的手段是 `Wait`，这条今天对所有供应商都
//! 生效，最保守的那条路已经在了。真正的暴露面在**识别**：
//! `default_rate_limit_keywords()` 只有八条，中转站把限流写成「上游负载已饱和」、
//! 或者只回一个 `upstream_busy` 时，一条都不命中——判定落到别处，手段变成
//! `Nudge`，应用就按冷却一遍遍往那个终端里敲字。有的供应商对此的反应是封号。
//!
//! 所以这里不做「供应商 → 策略」表。表要先知道这是限流才谈得上查，而出事的
//! 前提恰恰是没认出来。这里做的是两件更靠前的事：
//!
//! 1. [`upstream_rejection`]：关键词没对上时，退一步看这行**长得像不像**
//!    上游拒绝（HTTP 4xx/5xx 形状、`x-ratelimit` 头的残影、中转站的中文说法）。
//!    认出来就倒向 `Wait`，而不是继续敲。
//! 2. [`parse_wait_hint`]：把限流消息自带的等待时间抠出来（`retrying in 34s`）。
//!    这比让用户猜「这家要等多久」准得多，也不用维护一张表——agent 自己把答案
//!    打在终端上了。
//!
//! 两件事都是纯函数：给一段文本，返回一个结论。没有网络、没有时钟、没有配置。

/// 上游拒绝的形状证据
///
/// 只带一个「哪句话让我这么认为」的字符串，因为它唯一的用途是**说给人听**：
/// 判定要在界面和日志里解释自己，而「限流」这种结论如果不带出处，用户没法
/// 判断它是不是认错了。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectionShape {
    /// 命中的那一段（原文片段，不是关键词名）
    pub marker: String,
}

/// HTTP 状态码里「等一下会好」的那些
///
/// 挑选标准是**这个码出现时，再敲一句「继续」有没有可能让事情更糟**：
///
/// - `429` 明确就是限流。
/// - `529` 是 Anthropic 的过载码，不在标准表里，但 Claude Code 用户见得最多。
/// - `503` / `502` / `504` 是中转站过载和网关超时的常见形状。它们**也可能**
///   是上游真的挂了——但那种情况下敲字同样没用，代价只是多等一个冷却，
///   而认错方向的代价是号没了。取保守侧。
/// - `500` **故意不在这里**：它已经在 `default_error_keywords()` 里，
///   而且太常见了（一行普通的堆栈里就可能有），拿它当限流形状会把大量
///   真故障误判成「等等就好」，反而让真正该叫人的时候没人来。
const REJECTION_CODES: &[&str] = &["429", "529", "503", "502", "504"];

/// 不带状态码、但同样是「上游不让我过」的说法
///
/// 中英文都要有：中转站的错误信息经常是中文，而 `contains_keyword` 对中文
/// 跳过词边界判断，所以这些短语可以直接用。挑的都是**只在这个语境下出现**
/// 的说法——「繁忙」这种太泛的词不收，否则 agent 自己写一句
/// 「服务器繁忙时应该重试」就能把会话判成限流。
const REJECTION_PHRASES: &[&str] = &[
    "x-ratelimit",
    "ratelimit-remaining",
    "retry-after",
    "upstream_busy",
    "upstream busy",
    "server_busy",
    "capacity",
    // 写全词而不是 `throttl` 这样的词干：`contains_keyword` 对 ASCII 关键词
    // 要求词边界，而词干后面永远紧跟着字母（`throttled` 的 `e`），于是词干
    // **一个都匹配不上**。第一版写的就是词干，是变异检查逼出的那条
    // 「每条短语单独认」的测试把它照出来的。
    "throttled",
    "throttling",
    "上游负载",
    "上游繁忙",
    "并发超限",
    "请求过于频繁",
    "触发限流",
    "负载已饱和",
];

/// 这行故障日志长得像上游拒绝吗
///
/// 只在 `rate_limit_keywords` 一条都没对上之后才问这个问题——它是**兜底**，
/// 不是替代。用户配的关键词永远先走，这样「我加了一条我们家中转站的说法」
/// 仍然是最直接、最可预期的解法。
///
/// 输入必须是**已经小写化**的故障文本（跟 `first_match` 一样的约定），
/// 而且必须是 `error_output`——记录里自己标成故障的那些行。拿散文喂进来会
/// 重犯已经踩过的那个坑：agent 写一句「不再撞上 429」就把会话判成限流。
pub fn upstream_rejection(lower_errors: &str) -> Option<RejectionShape> {
    for code in REJECTION_CODES {
        if super::contains_keyword(lower_errors, code) {
            return Some(RejectionShape {
                marker: (*code).to_string(),
            });
        }
    }
    for phrase in REJECTION_PHRASES {
        if super::contains_keyword(lower_errors, phrase) {
            return Some(RejectionShape {
                marker: (*phrase).to_string(),
            });
        }
    }
    None
}

/// 从限流消息里抠出的等待时间上限（秒）
///
/// 封顶是为了拦住两类输入：一个把 `resets at 3pm` 之类算成明天的解析错误，
/// 以及供应商真的说「等 24 小时」——后者是真的，但一个桌面守护进程把某个
/// 会话静默搁置一整天，跟坏了没有区别。超过这个数就交给人：
/// 那正是 `HandOff` 存在的意义。
pub const MAX_WAIT_HINT_SECS: u64 = 3600;

/// 把「等多久」从限流消息里读出来
///
/// 这是我们相对 cc-switch 那类代理的**优势面**：代理能读 `Retry-After` 头
/// （它读了，但没喂给任何等待逻辑），我们读不到头——可 agent 会把
/// 「retrying in 34s」直接打在终端上，而那句话我们看得见。
///
/// 认这几种形状（大小写不敏感，输入应已小写）：
///
/// ```text
/// retrying in 34s          → 34
/// retrying in 2m           → 120
/// retry after 60 seconds   → 60
/// try again in 5 minutes   → 300
/// wait 90 sec              → 90
/// 请在 30 秒后重试          → 30
/// ```
///
/// 刻意**不认** `resets at 3pm` / `resets at 2026-08-05T10:00:00Z`：那要挑时区
/// （见 `docs/architecture.md` §12.5 那条「不钉时区的日期测试只证明了写测试的人
/// 在哪个时区」），而挑错了就会算出一个负数或者一个明天的时刻。相对时长没有
/// 这个问题。认不出来时返回 `None`，调用方退回配置里的冷却下限——那是个
/// 保守的已知值，比一个猜错的时刻好。
pub fn parse_wait_hint(lower_text: &str) -> Option<u64> {
    let candidates = [
        "retrying in",
        "retry after",
        "retry in",
        "try again in",
        "wait",
    ];
    for lead in candidates {
        let mut from = 0usize;
        while let Some(pos) = lower_text[from..].find(lead) {
            let start = from + pos + lead.len();
            if let Some(secs) = parse_duration_at(&lower_text[start..]) {
                return Some(secs.min(MAX_WAIT_HINT_SECS));
            }
            from = from + pos + lead.len();
        }
    }
    parse_chinese_wait(lower_text)
}

/// 从「引导词之后」这段文本的开头读一个时长
///
/// 允许中间隔着标点和空格（`retrying in… 34s`），但**不允许跨过其它数字**：
/// 只看第一个数字。否则 `retry after the 3rd attempt in 500ms` 这种句子会
/// 被读成一个离引导词很远的数。
fn parse_duration_at(rest: &str) -> Option<u64> {
    let bytes = rest.as_bytes();
    let mut i = 0usize;
    // 跳过引导词和数字之间的空白与标点，但不跳过字母——隔着一个单词的
    // 数字跟这个引导词就没关系了
    while i < bytes.len() && !bytes[i].is_ascii_digit() {
        if bytes[i].is_ascii_alphabetic() {
            return None;
        }
        i += 1;
    }
    let num_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == num_start {
        return None;
    }
    let value: u64 = rest[num_start..i].parse().ok()?;

    // 单位紧跟或隔一个空格。没有单位时按秒——`retrying in 34` 这种写法
    // 现实里存在，而按秒理解是保守的（比按分钟少等，但不会把一次限流
    // 当成没发生）。
    let unit_rest = rest[i..].trim_start();
    let unit = unit_rest
        .as_bytes()
        .iter()
        .take_while(|b| b.is_ascii_alphabetic())
        .count();
    let unit = &unit_rest[..unit];
    let secs = match unit {
        "" | "s" | "sec" | "secs" | "second" | "seconds" => value,
        "m" | "min" | "mins" | "minute" | "minutes" => value.saturating_mul(60),
        "h" | "hr" | "hrs" | "hour" | "hours" => value.saturating_mul(3600),
        // 毫秒：`retrying in 500ms` 是真的会出现的，但它表达的是
        // 「马上重试」，当成 0 秒等待即可，不必也不该按 500 秒等
        "ms" => 0,
        _ => return None,
    };
    Some(secs)
}

/// 中文的「N 秒后重试」/「N 分钟后再试」
///
/// 单独一条路是因为中文里数字在引导词**前面**，上面那套「找引导词再往后读」
/// 的逻辑对它无效。
fn parse_chinese_wait(text: &str) -> Option<u64> {
    const UNITS: &[(&str, u64)] = &[("秒", 1), ("分钟", 60), ("分", 60), ("小时", 3600)];
    for (unit, mult) in UNITS {
        let mut from = 0usize;
        while let Some(pos) = text[from..].find(unit) {
            let at = from + pos;
            // 单位后面得跟「后」或「再」这类字，否则「用了 30 秒」也会命中
            let after = text[at + unit.len()..].trim_start();
            let is_wait = after.starts_with("后") || after.starts_with("再");
            if is_wait {
                // 往前读连续的 ASCII 数字。先 `trim_end`：中文里
                // 「30 秒后」这样数字和单位之间带空格是常态，不容忍这个空格
                // 就等于只认「30秒后」一种写法。
                let head = text[..at].trim_end();
                let digits: String = head
                    .chars()
                    .rev()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                if !digits.is_empty() {
                    let digits: String = digits.chars().rev().collect();
                    if let Ok(v) = digits.parse::<u64>() {
                        return Some(v.saturating_mul(*mult).min(MAX_WAIT_HINT_SECS));
                    }
                }
            }
            from = at + unit.len();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 形状识别 ──

    #[test]
    fn a_429_is_a_rejection() {
        let got = upstream_rejection("http 429 returned by upstream");
        assert_eq!(got.map(|s| s.marker), Some("429".to_string()));
    }

    /// Anthropic 的 529 不在任何标准状态码表里，但 Claude Code 用户见得最多
    #[test]
    fn anthropics_529_counts_too() {
        assert!(upstream_rejection("api error: 529 overloaded_error").is_some());
    }

    /// 这条守的是「认不出来的中转站说法也要拦住」——需求里那个封号场景
    #[test]
    fn a_relay_speaking_chinese_is_still_a_rejection() {
        let got = upstream_rejection("请求失败：上游负载已饱和，请稍后重试");
        assert!(
            got.is_some(),
            "中转站的中文限流说法一条关键词都没对上时，兜底得认出来，否则会一直敲字"
        );
    }

    /// 表里每一条都得**单独**被认出来
    ///
    /// 上面那条只证明了「这句话能认出来」，而那句话同时含 `上游负载` 和
    /// `负载已饱和` 两条——删掉任意一条它照样绿。变异检查把这个指出来了：
    /// 把 `上游负载` 换成一个永不命中的字符串，全部测试仍然通过。
    ///
    /// 所以这里给每条配一句**只有它能命中**的话。这不是在钉一个调参旋钮
    /// （那种该像 `EVENT_RING_CAP` 一样只做符号断言），而是在钉产品要求本身：
    /// 表里少一条，就是少认一种真实存在的限流说法，而认不出来的代价是号被封。
    #[test]
    fn every_listed_phrase_is_recognized_on_its_own() {
        // 每一项都只含目标短语，不含表里任何其它短语
        let cases = [
            ("x-ratelimit", "x-ratelimit-limit: 40000"),
            ("ratelimit-remaining", "ratelimit-remaining: 0"),
            ("retry-after", "retry-after: 30"),
            ("upstream_busy", r#"{"code":"upstream_busy"}"#),
            ("upstream busy", "the upstream busy signal was returned"),
            ("server_busy", r#"{"error":"server_busy"}"#),
            ("capacity", "at capacity right now"),
            ("throttled", "request was throttled by the gateway"),
            ("throttling", "throttling is active for this key"),
            ("上游负载", "上游负载过高，请稍后再试"),
            ("上游繁忙", "上游繁忙，切换线路"),
            ("并发超限", "并发超限，请降低请求速率"),
            ("请求过于频繁", "请求过于频繁"),
            ("触发限流", "已触发限流策略"),
            ("负载已饱和", "当前线路负载已饱和"),
        ];

        // 先确认这张清单没有漏掉表里的条目——漏了的话那一条就没人守，
        // 而这条测试会假绿
        assert_eq!(
            cases.len(),
            REJECTION_PHRASES.len(),
            "表里有 {} 条短语，测试只覆盖了 {} 条",
            REJECTION_PHRASES.len(),
            cases.len()
        );
        for phrase in REJECTION_PHRASES {
            assert!(
                cases.iter().any(|(p, _)| p == phrase),
                "{phrase} 没有对应的用例"
            );
        }

        for (phrase, message) in cases {
            let got = upstream_rejection(message);
            assert_eq!(
                got.as_ref().map(|s| s.marker.as_str()),
                Some(phrase),
                "「{message}」该由 {phrase} 认出来"
            );
        }
    }

    #[test]
    fn a_bare_upstream_busy_is_a_rejection() {
        assert!(upstream_rejection(r#"{"error":"upstream_busy"}"#).is_some());
    }

    /// 500 故意不算：它太常见，拿它当限流会把真故障说成「等等就好」
    #[test]
    fn a_500_is_not_a_rejection_shape() {
        assert_eq!(
            upstream_rejection("internal server error 500"),
            None,
            "500 已经在 error_keywords 里，再算成限流形状会让真故障没人叫"
        );
    }

    /// 词边界要生效，否则一行用量统计就能把会话判成限流
    #[test]
    fn a_number_inside_another_number_is_not_a_code() {
        assert_eq!(
            upstream_rejection("used 14290 tokens in 5029 ms"),
            None,
            "14290 里的 429 和 5029 里的 502 都不是状态码"
        );
    }

    #[test]
    fn ordinary_failure_text_is_not_a_rejection() {
        assert_eq!(upstream_rejection("error: file not found"), None);
    }

    // ── 等待时长 ──

    #[test]
    fn retrying_in_seconds_is_read_as_seconds() {
        assert_eq!(parse_wait_hint("retrying in 34s"), Some(34));
    }

    #[test]
    fn minutes_become_seconds() {
        assert_eq!(parse_wait_hint("retrying in 2m"), Some(120));
        assert_eq!(parse_wait_hint("try again in 5 minutes"), Some(300));
    }

    #[test]
    fn retry_after_with_a_spelled_unit_works() {
        assert_eq!(parse_wait_hint("retry after 60 seconds"), Some(60));
    }

    /// 没有单位时按秒理解：这种写法现实里存在，按秒是保守的那一侧
    #[test]
    fn a_bare_number_is_seconds() {
        assert_eq!(parse_wait_hint("retrying in 45"), Some(45));
    }

    /// 500ms 说的是「马上重试」，不是等 500 秒
    #[test]
    fn milliseconds_mean_no_real_wait() {
        assert_eq!(parse_wait_hint("retrying in 500ms"), Some(0));
    }

    #[test]
    fn chinese_wait_hints_are_read() {
        assert_eq!(parse_wait_hint("请在 30 秒后重试"), Some(30));
        assert_eq!(parse_wait_hint("请 5 分钟后再试"), Some(300));
    }

    /// 「用了 30 秒」不是「等 30 秒」——单位后面得跟「后」或「再」
    #[test]
    fn a_duration_that_is_not_a_wait_is_ignored() {
        assert_eq!(
            parse_wait_hint("本次请求用了 30 秒"),
            None,
            "描述耗时的句子不是等待提示，按它去等就是凭空搁置一个会话"
        );
    }

    /// 隔着一个单词的数字跟这个引导词没关系
    ///
    /// 句子里**必须真的有一个数字**，否则这条测不出任何东西：没有数字时
    /// 无论跨不跨字母都返回 `None`，把字母那道闸门整个删掉它照样绿。
    /// 第一版就是这么写的（`"retry after the third attempt"`），变异检查
    /// 直接指出来了。
    #[test]
    fn a_number_behind_another_word_is_not_the_wait() {
        assert_eq!(
            parse_wait_hint("retry after the quota window, 900 seconds total"),
            None,
            "「900 秒」描述的是配额窗口，不是要等的时间；\
             引导词后面紧跟的是单词就该收手，不能翻过它去抓远处的数"
        );
    }

    /// 封顶守的是「静默搁置一整天跟坏了没区别」
    #[test]
    fn an_absurdly_long_wait_is_capped() {
        assert_eq!(
            parse_wait_hint("retry after 48 hours"),
            Some(MAX_WAIT_HINT_SECS),
            "供应商说等 48 小时是真的，但守护进程该把这件事交给人，不是自己等一天"
        );
    }

    #[test]
    fn no_hint_means_no_answer() {
        assert_eq!(
            parse_wait_hint("rate limit reached"),
            None,
            "认不出来要说不知道，让调用方退回配置里那个保守的已知值"
        );
    }

    /// 溢出不能 panic：`u64::MAX` 分钟乘 60 会绕
    #[test]
    fn an_overflowing_number_does_not_panic() {
        let got = parse_wait_hint(&format!("retrying in {} minutes", u64::MAX));
        assert_eq!(got, Some(MAX_WAIT_HINT_SECS));
    }
}
