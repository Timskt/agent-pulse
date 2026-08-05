//! 把库里的记录导成 CSV
//!
//! 边界：这个模块**只负责把行拼成文本、把文本落到盘上**。取哪些行是
//! `storage` 的事，用户点哪个按钮是前端的事。
//!
//! 为什么值得单独一个模块、还带这么多测试：CSV 是个看着简单、错起来悄无声息
//! 的格式。这里要导的恰好全是**会带逗号、引号、换行的字段**——项目路径、
//! 报错信息、模型名。转义漏一处不会报错，只会让某一行往右串一格，
//! 于是「花费」那列读到的是错误信息的后半截。表格软件不会提示，
//! 用户看到的是一份**看起来正常但是错的**账。

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// 一个单元格，分两类
///
/// 分类的理由是**公式注入的防护会破坏机器可读性**，所以不能一刀切地全加。
/// 防护手段是在字段前面加个单引号让表格软件按文本处理；可那个单引号对表格
/// 软件是标记，对 `pandas.read_csv` 就是数据——`-1` 会被读成字符串 `"'-1"`。
///
/// 于是只在真的可能含敌意内容的列上加：
/// - [`Cell::Text`]：自由文本。报错信息里有 agent 和外部工具的 stderr 原文，
///   项目目录名由用户的文件系统决定。这些列加防护。
/// - [`Cell::Value`]：形状已知的值——时间戳、数字、枚举键、会话 ID。
///   这些列**只做 RFC 4180 转义，不加前缀**，因为它们要能直接被求和、
///   排序、喂给脚本。
///
/// 两类都会做 RFC 4180 转义，那一步跟内容可信不可信无关，是格式要求。
pub enum Cell {
    Text(String),
    Value(String),
}

/// 自由文本单元格（会做公式注入防护）
fn text(s: &str) -> Cell {
    Cell::Text(s.to_string())
}

/// 形状已知的单元格（不加前缀，保持机器可读）
fn value(s: impl Into<String>) -> Cell {
    Cell::Value(s.into())
}

/// 一份待写出的 CSV：表头 + 数据行
///
/// 表头单独拿出来而不是当成第一行，是为了让「列数对不对」这件事可测：
/// 有了 `header` 才能断言每一行的字段数都跟它一致。
pub struct Csv {
    pub header: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
}

impl Csv {
    pub fn new(header: &[&str]) -> Self {
        Self {
            header: header.iter().map(|s| s.to_string()).collect(),
            rows: Vec::new(),
        }
    }

    pub fn push(&mut self, row: Vec<Cell>) {
        debug_assert_eq!(
            row.len(),
            self.header.len(),
            "CSV 行的字段数跟表头对不上——这一行会整体串列"
        );
        self.rows.push(row);
    }

    /// 渲染成完整的 CSV 文本
    ///
    /// 行分隔用 `\r\n`：RFC 4180 是这么写的，而且 Windows 版 Excel 直接双击
    /// 打开时只认这个。Numbers 和 LibreOffice 两种都认，所以取严的那个。
    pub fn render(&self) -> String {
        let mut out = String::new();
        let cells = self.header.len().max(1);
        // 预留一下容量：导出几千行时省掉十几次 realloc
        out.reserve((self.rows.len() + 1) * cells * 12);
        // 表头是我们自己词表里的字，按 `Value` 处理
        write_row(
            &mut out,
            &self
                .header
                .iter()
                .map(|h| value(h.clone()))
                .collect::<Vec<_>>(),
        );
        for row in &self.rows {
            write_row(&mut out, row);
        }
        out
    }
}

fn write_row(out: &mut String, row: &[Cell]) {
    for (i, cell) in row.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let rendered = match cell {
            Cell::Text(s) => escape_text(s),
            Cell::Value(s) => escape_value(s),
        };
        let _ = write!(out, "{rendered}");
    }
    out.push_str("\r\n");
}

/// RFC 4180 转义：含逗号、引号、换行的字段整体引起来，内部引号写成两个
///
/// 这一步跟内容可信不可信无关，是格式要求。少了它，一个叫
/// `feat: 支持 a, b` 的报错信息会把后面所有列往右顶一格——**而且不会报错**。
/// 用户打开看到的是一份看起来正常但是错的表：「花费」那列读到的是报错信息
/// 的后半截。
fn escape_value(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

/// 自由文本的转义：先中和公式，再按 RFC 4180 引
///
/// 以 `=` `+` `-` `@` 开头的字段，Excel / Numbers / Google Sheets 会当**公式**
/// 处理。这里导出的自由文本有两个不受我们控制的来源：报错信息里含外部工具的
/// stderr 原文，项目目录名由用户的文件系统决定。前面加一个单引号，表格软件
/// 就按纯文本显示。
///
/// 前导空白也要跳过再判断——某些版本的 Excel 会先 trim 再看首字符，
/// 于是 ` =1+1` 照样被当公式。
///
/// 只有这一类列加前缀。数字和时间戳走 [`escape_value`]，否则单引号会漏进
/// 脚本的读数里（`pandas` 会把 `'-1` 读成字符串）。
fn escape_text(field: &str) -> String {
    if starts_dangerous(field) {
        escape_value(&format!("'{field}"))
    } else {
        escape_value(field)
    }
}

/// 这个字段会被表格软件当公式吗
fn starts_dangerous(field: &str) -> bool {
    let trimmed = field.trim_start_matches(['\t', '\r', '\n', ' ']);
    matches!(
        trimmed.chars().next(),
        Some('=') | Some('+') | Some('-') | Some('@')
    )
}

/// 带 UTF-8 BOM 的字节流
///
/// BOM 不是装饰。Windows 版 Excel 双击打开无 BOM 的 UTF-8 CSV 时会按系统
/// 本地编码（简体中文机器上是 GBK）去解，中文项目名全变乱码。加三个字节
/// 就能让它认出 UTF-8，而 Numbers / LibreOffice / pandas 都会忽略 BOM。
pub fn with_bom(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() + 3);
    bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

/// 落盘目录：优先下载文件夹，取不到就退到主目录
///
/// 不弹系统保存对话框是刻意的——那需要引 `tauri-plugin-dialog` 并放开一条
/// 新权限。导出这件事的收益不值得扩权限面，写到下载夹再把文件亮出来
/// （`shell:allow-open`，本来就有）已经够用。
fn export_dir() -> PathBuf {
    dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 文件名里的时间戳部分，形如 `20260804-013755`
///
/// 用本地时间而不是 UTC：这个字符串是给人看的，用户对「我刚才那次导出」的
/// 记忆是本地钟点。
fn stamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

/// 把一份 CSV 写到下载夹，返回完整路径
///
/// `slug` 是文件名里那截人能认的部分（`resumes` / `sessions` / …）。
/// 带上时间戳是为了**不覆盖**：导出两次该得到两份文件，第二次悄悄盖掉第一次
/// 是数据丢失。
pub fn write_csv(slug: &str, csv: &Csv) -> Result<String, String> {
    let dir = export_dir();
    let path = dir.join(format!("agentpulse-{slug}-{}.csv", stamp()));
    write_to(&path, csv)?;
    Ok(path.to_string_lossy().to_string())
}

fn write_to(path: &Path, csv: &Csv) -> Result<(), String> {
    std::fs::write(path, with_bom(&csv.render()))
        .map_err(|e| format!("写文件失败 {}: {e}", path.display()))
}

/// 在文件管理器里把这个文件选中亮出来
///
/// 亮出所在位置而不是直接打开文件：双击级别的「打开」会拉起 Excel / Numbers，
/// 那是用户自己该决定的事。选中状态刚好回答「导到哪儿去了」。
///
/// 路径一律走 argv 数组，**不拼 shell 字符串**——这条是仓库里的既有约定
/// （用户可编辑的提示词从不进 shell 串）。这里的路径含用户名和项目名，
/// 同样不能假设里面没有空格、引号、`$`。
pub fn reveal(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if !p.exists() {
        return Err(format!("文件不在了: {path}"));
    }

    #[cfg(target_os = "macos")]
    let (program, args): (&str, Vec<&str>) = ("open", vec!["-R", path]);

    // `explorer` 的选中参数没有空格：`/select,<path>` 必须是同一个 argv 项
    #[cfg(target_os = "windows")]
    let select_arg = format!("/select,{path}");
    #[cfg(target_os = "windows")]
    let (program, args): (&str, Vec<&str>) = ("explorer", vec![select_arg.as_str()]);

    // Linux 上没有通用的「选中某个文件」，退一步打开所在目录
    #[cfg(all(unix, not(target_os = "macos")))]
    let dir = p
        .parent()
        .map(|d| d.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());
    #[cfg(all(unix, not(target_os = "macos")))]
    let (program, args): (&str, Vec<&str>) = ("xdg-open", vec![dir.as_str()]);

    std::process::Command::new(program)
        .args(&args)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{program} 起不来: {e}"))
}

/// 数字转字符串时统一保留小数位
///
/// 成本这一列如果直接 `to_string()`，`0.1 + 0.2` 那种浮点尾巴会漏到表格里
/// （`0.30000000000000004`）。固定四位小数：再少会把不到一分钱的调用抹成 0，
/// 而那恰恰是「便宜的模型调用了很多次」这种账要看的东西。
pub fn money(v: f64) -> String {
    format!("{v:.4}")
}

// ─────────────────────────────────────────────────────────────────────────
// 各数据集的建表
//
// 每个函数只做一件事：把一批已经查出来的记录摆成 CSV。查询条件由调用方决定，
// 这样「导出的就是你正在看的那些行」不需要在这里重新实现一遍筛选逻辑。
// ─────────────────────────────────────────────────────────────────────────

use crate::cost::{DailyCost, ProjectCost, UsageSnapshot};
use crate::i18n::I18n;
use crate::storage::{
    ModelCost, ResumeRecord, SessionHistoryEntry, SessionHistorySummary, StatsOverview,
};

/// 续跑记录
pub fn resumes_csv(records: &[ResumeRecord], i: &I18n) -> Csv {
    let mut csv = Csv::new(&[
        i.t("csv.time"),
        i.t("csv.agent"),
        i.t("csv.project"),
        i.t("csv.session_id"),
        i.t("csv.prompt_type"),
        i.t("csv.outcome"),
        i.t("csv.stuck_secs"),
        i.t("csv.message"),
    ]);
    for r in records {
        csv.push(vec![
            value(r.created_at.clone()),
            value(r.agent_name.clone()),
            // 目录名由用户的文件系统决定，可能以 `-` 或 `@` 开头
            text(&r.working_dir),
            value(r.session_id.clone()),
            value(r.prompt_type.clone()),
            // 旧行的 `outcome` 是空串。这里补成 `success` 推出来的说法而不是留空，
            // 是因为空单元格在表格里读作「没这回事」；那些行是有结果的，
            // 只是当时只记了一个布尔
            value(if r.outcome.is_empty() {
                if r.success {
                    "landed"
                } else {
                    "failed"
                }
            } else {
                &r.outcome
            }),
            // `-1` 是「不知道」的哨兵值，不能当成 -1 秒导出去
            value(if r.stuck_secs < 0 {
                i.t_owned("csv.unknown")
            } else {
                r.stuck_secs.to_string()
            }),
            // 这一列含外部工具的 stderr 原文，最需要防护的就是它
            text(&r.message),
        ]);
    }
    csv
}

/// 会话档案
pub fn sessions_csv(entries: &[SessionHistoryEntry], i: &I18n) -> Csv {
    let mut csv = Csv::new(&[
        i.t("csv.project"),
        i.t("csv.agent"),
        i.t("csv.terminal"),
        i.t("csv.session_id"),
        i.t("csv.first_seen"),
        i.t("csv.last_seen"),
        i.t("csv.last_status"),
        i.t("csv.live"),
        i.t("csv.ended_at"),
        i.t("csv.resume_count"),
        i.t("csv.tokens"),
        i.t("csv.cost"),
    ]);
    for e in entries {
        csv.push(vec![
            text(&e.working_dir),
            value(e.agent_name.clone()),
            value(e.terminal_app.clone()),
            value(e.session_id.clone()),
            value(e.first_seen.clone()),
            value(e.last_seen.clone()),
            value(e.last_status.clone()),
            // 「还在不在」单独一列，不靠 `ended_at` 是否为空让读表的人自己推
            value(if e.is_live() {
                i.t_owned("csv.yes")
            } else {
                i.t_owned("csv.no")
            }),
            value(e.ended_at.clone()),
            value(e.resume_count.to_string()),
            value(e.total_tokens.to_string()),
            value(money(e.cost_usd)),
        ]);
    }
    csv
}

/// 每日花费
pub fn daily_cost_csv(days: &[DailyCost], i: &I18n) -> Csv {
    let mut csv = Csv::new(&[
        i.t("csv.date"),
        i.t("csv.tokens"),
        i.t("csv.requests"),
        i.t("csv.cost"),
    ]);
    for d in days {
        csv.push(vec![
            value(d.date.clone()),
            value(d.total_tokens.to_string()),
            value(d.requests.to_string()),
            value(money(d.cost_usd)),
        ]);
    }
    csv
}

/// 项目花费
pub fn project_cost_csv(projects: &[ProjectCost], i: &I18n) -> Csv {
    let mut csv = Csv::new(&[
        i.t("csv.project"),
        i.t("csv.tokens"),
        i.t("csv.requests"),
        i.t("csv.cost"),
    ]);
    for p in projects {
        csv.push(vec![
            // 项目名是目录的 basename，`@scope` 这种开头很常见
            text(&p.project),
            value(p.total_tokens.to_string()),
            value(p.requests.to_string()),
            value(money(p.cost_usd)),
        ]);
    }
    csv
}

/// 模型花费
pub fn model_cost_csv(models: &[ModelCost], i: &I18n) -> Csv {
    let mut csv = Csv::new(&[
        i.t("csv.model"),
        i.t("csv.tokens"),
        i.t("csv.requests"),
        i.t("csv.cost"),
    ]);
    for m in models {
        csv.push(vec![
            value(m.model.clone()),
            value(m.total_tokens.to_string()),
            value(m.requests.to_string()),
            value(money(m.cost_usd)),
        ]);
    }
    csv
}

/// 统计摘要：竖着排，一行一个指标
///
/// 摘要只有一组数，横着排会得到一张一行十几列的表——那种表在任何表格软件里
/// 都得横向滚动才能看完。竖排还有个好处：以后加指标是往下追加，
/// 不会让已经导出的文件在列的位置上错位。
pub fn stats_summary_csv(
    overview: &StatsOverview,
    sessions: &SessionHistorySummary,
    usage: &UsageSnapshot,
    i: &I18n,
) -> Csv {
    let mut csv = Csv::new(&[i.t("csv.metric"), i.t("csv.value")]);
    let counts: [(&'static str, u64); 9] = [
        ("csv.scans", overview.total_scans as u64),
        ("csv.detections", overview.total_detections as u64),
        ("csv.resumes", overview.total_resumes as u64),
        ("csv.resumes_ok", overview.successful_resumes as u64),
        ("csv.resumes_failed", overview.failed_resumes as u64),
        ("csv.sessions_total", sessions.total as u64),
        ("csv.sessions_live", sessions.live as u64),
        ("csv.requests", usage.requests as u64),
        ("csv.tokens", usage.total_tokens),
    ];
    for (key, n) in counts {
        csv.push(vec![value(i.t_owned(key)), value(n.to_string())]);
    }
    // 花费不是整数，单独一行走 `money`
    csv.push(vec![
        value(i.t_owned("csv.cost")),
        value(money(usage.cost_usd)),
    ]);
    csv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_fields_are_untouched() {
        assert_eq!(escape_value("Claude Code"), "Claude Code");
        assert_eq!(escape_value(""), "");
        assert_eq!(escape_value("2026-08-04 01:37:55"), "2026-08-04 01:37:55");
        assert_eq!(escape_text("Claude Code"), "Claude Code");
    }

    #[test]
    fn commas_force_quoting() {
        // 用户点名担心的场景：项目名带逗号
        assert_eq!(escape_text("my-proj, old"), "\"my-proj, old\"");
        assert_eq!(escape_value("a,b"), "\"a,b\"");
    }

    #[test]
    fn inner_quotes_are_doubled() {
        assert_eq!(escape_text("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(escape_value("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn newlines_stay_inside_one_field() {
        // 报错信息经常是多行的。换行没被引起来的话，一条记录会被读成两行，
        // 后半行还会因为列数不够而整体错位
        assert_eq!(escape_text("line1\nline2"), "\"line1\nline2\"");
        assert_eq!(escape_value("line1\r\nline2"), "\"line1\r\nline2\"");
    }

    #[test]
    fn formula_injection_is_neutralized_in_text_columns() {
        // 这四个前导符号都会被表格软件当公式
        for payload in ["=1+1", "+1", "-1", "@SUM(A1)"] {
            let out = escape_text(payload);
            assert!(
                out.starts_with('\'') || out.starts_with("\"'"),
                "{payload} 没被中和: {out}"
            );
        }
    }

    #[test]
    fn value_columns_are_never_prefixed() {
        // 这条是上面那条的另一半，两条一起才说清了取舍：
        // 数字列不能加前缀，否则 `pandas` 读到的是字符串 `'-1`
        assert_eq!(escape_value("-1"), "-1");
        assert_eq!(escape_value("0.0000"), "0.0000");
        assert_eq!(escape_value("2026-08-04 01:37:55"), "2026-08-04 01:37:55");
    }

    #[test]
    fn scoped_package_names_stay_readable_as_values() {
        // `@scope/pkg` 是极常见的目录名。它走 `Text` 会被加前缀（安全优先），
        // 但走 `Value` 的列必须原样——这条盯的是别把前缀漏到标识列上
        assert_eq!(escape_value("@myorg/frontend"), "@myorg/frontend");
        assert_eq!(escape_text("@myorg/frontend"), "'@myorg/frontend");
    }

    #[test]
    fn formula_injection_survives_leading_whitespace() {
        // 某些 Excel 版本会先跳过前导空白再判断首字符
        assert_eq!(escape_text("\t=1+1"), "'\t=1+1");
        assert_eq!(escape_text(" =1+1"), "' =1+1");
    }

    #[test]
    fn dangerous_and_comma_together() {
        // 两种处理要能叠加：先中和公式，再按 RFC 4180 引起来
        let out = escape_text("=cmd|'/c calc'!A1, x");
        assert!(out.starts_with("\"'="), "应该既中和又引起来: {out}");
        assert!(out.ends_with('"'));
    }

    #[test]
    fn render_puts_header_first_and_uses_crlf() {
        let mut csv = Csv::new(&["a", "b"]);
        csv.push(vec![value("1"), value("2")]);
        assert_eq!(csv.render(), "a,b\r\n1,2\r\n");
    }

    #[test]
    fn render_with_no_rows_is_just_the_header() {
        // 空导出要给一份只有表头的文件，不是零字节文件：
        // 用户打开能看到「确实没有记录」，而不是怀疑导出坏了
        let csv = Csv::new(&["a", "b"]);
        assert_eq!(csv.render(), "a,b\r\n");
    }

    #[test]
    fn bom_is_three_bytes_then_content() {
        let bytes = with_bom("a");
        assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF]);
        assert_eq!(&bytes[3..], b"a");
    }

    #[test]
    fn money_keeps_four_decimals() {
        assert_eq!(money(0.1 + 0.2), "0.3000");
        assert_eq!(money(0.0), "0.0000");
        // 半分钱级别的调用不能被抹成 0
        assert_ne!(money(0.0004), "0.0000");
    }

    #[test]
    fn round_trip_through_a_naive_parser() {
        // 拿一个只会「按引号状态切分」的最小解析器验一遍：
        // 转义对不对，最终标准是能不能被读回来
        let mut csv = Csv::new(&["project", "message", "cost"]);
        csv.push(vec![
            text("/Users/sky/my, proj"),
            text("failed: expected \"a\"\nretrying"),
            value(money(1.5)),
        ]);
        let rendered = csv.render();
        let rows = parse(&rendered);
        assert_eq!(rows.len(), 2, "表头 + 一行数据");
        assert_eq!(rows[1].len(), 3, "字段数不能因为逗号和换行变多");
        assert_eq!(rows[1][0], "/Users/sky/my, proj");
        assert_eq!(rows[1][1], "failed: expected \"a\"\nretrying");
        assert_eq!(rows[1][2], "1.5000");
    }

    #[test]
    fn builders_keep_every_row_the_same_width_as_the_header() {
        // `push` 里的 `debug_assert` 只在 debug 构建生效，而且它盯的是「行内
        // 字段数」。这条从渲染结果反向验一遍：任何一行的列数跟表头不一致，
        // 整行都会串位
        let i = I18n::new(crate::i18n::Lang::Zh);
        let entry = SessionHistoryEntry {
            session_key: "k".into(),
            session_id: "s".into(),
            agent_name: "Claude Code".into(),
            working_dir: "/Users/sky/a, b".into(),
            session_file: String::new(),
            tty: String::new(),
            terminal_app: "iTerm2".into(),
            first_seen: "2026-08-01 09:00:00".into(),
            last_seen: "2026-08-01 10:00:00".into(),
            last_status: "active".into(),
            ended_at: String::new(),
            resume_count: 2,
            total_tokens: 1234,
            cost_usd: 0.5,
        };
        let csv = sessions_csv(&[entry], &i);
        let rows = parse(&csv.render());
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[1].len(),
            csv.header.len(),
            "带逗号的目录名把这一行顶宽了"
        );
        // 「还在运行」那列要读得懂，不是让人自己看 `ended_at` 空不空
        assert_eq!(rows[1][7], "是");
    }

    #[test]
    fn unknown_stuck_secs_is_not_exported_as_minus_one() {
        let i = I18n::new(crate::i18n::Lang::Zh);
        let rec = ResumeRecord {
            id: 1,
            session_id: "s".into(),
            agent_name: "Claude Code".into(),
            working_dir: "/tmp/p".into(),
            prompt_type: "goal".into(),
            success: true,
            outcome: String::new(),
            stuck_secs: -1,
            message: "ok".into(),
            created_at: "2026-08-01 09:00:00".into(),
        };
        let csv = resumes_csv(&[rec], &i);
        let rows = parse(&csv.render());
        assert_eq!(rows[1][6], "未知", "-1 是哨兵值，不是负一秒");
        assert_eq!(rows[1][5], "landed", "旧行的空 outcome 要按 success 补上");
    }

    #[test]
    fn writing_produces_a_readable_utf8_file() {
        // 走真实的落盘路径，但写到临时目录——测试不该往用户的下载夹里扔文件
        let mut csv = Csv::new(&["项目", "花费"]);
        csv.push(vec![text("/Users/sky/中文项目, v2"), value(money(1.25))]);

        let path = std::env::temp_dir().join(format!("agentpulse-test-{}.csv", stamp()));
        write_to(&path, &csv).expect("写入应该成功");

        let bytes = std::fs::read(&path).expect("读回来");
        assert_eq!(
            &bytes[..3],
            &[0xEF, 0xBB, 0xBF],
            "少了 BOM，Excel 会读成乱码"
        );

        let content = String::from_utf8(bytes[3..].to_vec()).expect("BOM 之后是合法 UTF-8");
        let rows = parse(&content);
        assert_eq!(rows[0], vec!["项目", "花费"]);
        assert_eq!(
            rows[1][0], "/Users/sky/中文项目, v2",
            "中文和逗号都要能读回来"
        );
        assert_eq!(rows[1][1], "1.2500");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn revealing_a_missing_file_says_so_instead_of_launching_anything() {
        // 路径来自前端传回来的字符串，不能假设那个文件还在
        let missing = std::env::temp_dir().join("agentpulse-does-not-exist-9f3a.csv");
        let err = reveal(&missing.to_string_lossy()).expect_err("文件不在，应该报错");
        assert!(err.contains("不在了"), "错误信息该说清是文件没了: {err}");
    }

    /// 极简 CSV 解析器，只在测试里用
    fn parse(text: &str) -> Vec<Vec<String>> {
        let mut rows = Vec::new();
        let mut row = Vec::new();
        let mut field = String::new();
        let mut in_quotes = false;
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '"' if in_quotes => {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        field.push('"');
                    } else {
                        in_quotes = false;
                    }
                }
                '"' => in_quotes = true,
                ',' if !in_quotes => row.push(std::mem::take(&mut field)),
                '\r' if !in_quotes => {}
                '\n' if !in_quotes => {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                }
                other => field.push(other),
            }
        }
        rows
    }
}
