use crate::cost::{DailyCost, ProjectCost, UsageEntry, UsageSnapshot};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// 续跑事件记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeRecord {
    pub id: i64,
    pub session_id: String,
    pub agent_name: String,
    pub working_dir: String,
    pub prompt_type: String,
    pub success: bool,
    /// 核验结果的稳定键：`landed` / `silent` / `failed` / `unverifiable`。
    /// v1.6 之前的行是空串——那时候只存了 `success` 这一个布尔。
    pub outcome: String,
    /// 出手时它已经卡了多久（秒）。**`-1` 表示不知道**，不是「零秒」
    /// （[`crate::adapters::AgentSession::stuck_secs`]）：v1.7 之前的行没这个数，
    /// 没有记录文件的会话也算不出来。
    pub stuck_secs: i64,
    pub message: String,
    pub created_at: String,
}

/// 检测事件记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionRecord {
    pub id: i64,
    pub session_id: String,
    pub agent_name: String,
    pub verdict: String,
    pub signals: String,
    pub has_active_goal: bool,
    /// 中断原因键（`InterruptReason::key`）；旧行是空串
    ///
    /// v1.6 就开始往库里写这一列了，但一直没有查询读它——
    /// 「为什么停」躺在库里一年，界面上却只显示「已中断」。
    #[serde(default)]
    pub reason: String,
    pub created_at: String,
}

/// 每日统计摘要
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DailyStats {
    pub date: String,
    pub total_scans: u32,
    pub total_detections: u32,
    pub total_resumes: u32,
    pub successful_resumes: u32,
    pub failed_resumes: u32,
}

/// 历史会话条目（v1.2 会话时间线）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHistoryEntry {
    pub session_key: String,
    pub session_id: String,
    pub agent_name: String,
    pub working_dir: String,
    pub session_file: String,
    pub tty: String,
    pub terminal_app: String,
    pub first_seen: String,
    pub last_seen: String,
    pub last_status: String,
    /// 从视野里消失的时间；空串表示**本轮还看得见它**
    ///
    /// 跟 `last_status` 分开存，因为那两句话问的不是一件事：
    /// `last_status` 是「最后一眼它在干什么」（历史，不该被追改），
    /// 这一列是「它还在不在」（现状）。合成一列的后果就是
    /// 用户关掉的会话在历史里永远挂着「运行中」——
    /// 那句话是现在时，可写它的那一刻已经过去了。
    #[serde(default)]
    pub ended_at: String,
    pub resume_count: u32,
    pub total_tokens: u64,
    pub cost_usd: f64,
}

impl SessionHistoryEntry {
    /// 本轮还看得见它吗
    pub fn is_live(&self) -> bool {
        self.ended_at.is_empty()
    }
}

/// 按模型聚合的成本
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCost {
    pub model: String,
    pub total_tokens: u64,
    pub cost_usd: f64,
    pub requests: u32,
}

/// 续跑记录的筛选条件，计数和取页共用一份
///
/// 占位符：`?1` 关键词原文（空串表示不筛）、`?2` 关键词的 LIKE 形式、
/// `?3` 核验态、`?4` 提示词类型。**两处必须完全一致**——分开写过一次，
/// 加筛选项时只改了取页那条，于是总数说有 20 条、列表只给 3 条，
/// 翻页按钮点下去是空的。
const RESUME_FILTER: &str =
    "(?1 = '' OR agent_name LIKE ?2 OR working_dir LIKE ?2 OR message LIKE ?2) \
     AND (?3 = 'all' OR outcome = ?3 \
          OR (outcome = '' AND ?3 = 'landed' AND success = 1) \
          OR (outcome = '' AND ?3 = 'failed' AND success = 0)) \
     AND (?4 = 'all' OR prompt_type = ?4)";

/// `resume_records` 的取列顺序，与 [`row_to_resume`] 一一对应
///
/// 抽成常量是因为它出现在两个查询里。v1.7 加 `stuck_secs` 那次两处都得改，
/// 而漏掉一处编译器一声不响——只会让其中一个入口读出来的 `message` 是时间戳、
/// `created_at` 是空的。列表和取列写在一起就没这个缝。
const RESUME_COLUMNS: &str = "id, session_id, agent_name, working_dir, prompt_type, \
     success, outcome, stuck_secs, message, created_at";

/// `session_history` 的取列顺序，与 [`row_to_history`] 一一对应
///
/// 同 [`RESUME_COLUMNS`]：这张表也有两个查询入口，加 `ended_at` 那次
/// 两处都得跟着改，漏一处不报错、只会让「还在不在」读到 `resume_count`。
const HISTORY_COLUMNS: &str = "session_key, session_id, agent_name, working_dir, session_file, \
     tty, terminal_app, first_seen, last_seen, last_status, ended_at, \
     resume_count, total_tokens, cost_usd";

/// `session_history` 的搜索条件，计数和取页共用一份
///
/// `?1` 关键词原文（空串表示不筛）、`?2` 关键词的 LIKE 形式。
/// 两个入口以前各写一遍，措辞已经开始漂——一个搜 `terminal_app`，
/// 另一个不搜，于是同一个关键词在列表里有结果、翻页时总数却对不上。
const HISTORY_FILTER: &str = "(?1 = '' OR working_dir LIKE ?2 OR agent_name LIKE ?2 \
     OR session_file LIKE ?2 OR terminal_app LIKE ?2)";

/// 一行 `session_history` → [`SessionHistoryEntry`]
fn row_to_history(row: &rusqlite::Row) -> rusqlite::Result<SessionHistoryEntry> {
    Ok(SessionHistoryEntry {
        session_key: row.get(0)?,
        session_id: row.get(1)?,
        agent_name: row.get(2)?,
        working_dir: row.get(3)?,
        session_file: row.get(4)?,
        tty: row.get(5)?,
        terminal_app: row.get(6)?,
        first_seen: row.get(7)?,
        last_seen: row.get(8)?,
        last_status: row.get(9)?,
        // 补列时给了 `DEFAULT ''`，`Option` 那层防的是手工改过库：
        // NULL 说的也是「没记过收尾时间」，让它跟空串走同一个出口。
        ended_at: row.get::<_, Option<String>>(10)?.unwrap_or_default(),
        resume_count: row.get(11)?,
        total_tokens: row.get::<_, i64>(12)? as u64,
        cost_usd: row.get(13)?,
    })
}

/// 要落库的一次续跑事件
///
/// 全是借用：调用方手里本来就有 `&AgentSession` 和刚算出来的结果字符串，
/// 为了写一行库把它们各克隆一份没有意义。
pub struct ResumeEvent<'a> {
    pub session_id: &'a str,
    pub agent_name: &'a str,
    pub working_dir: &'a str,
    /// `goal` | `generic`
    pub prompt_type: &'a str,
    /// 这一次算不算「催过了」（[`crate::resumer::ResumeOutcome::counts_as_nudge`]）
    pub success: bool,
    /// 四个核验态的稳定键（[`crate::resumer::ResumeOutcome::storage_key`]）
    pub outcome: &'a str,
    /// 出手前它已经卡了多久（[`crate::adapters::AgentSession::stuck_secs`]）；
    /// `None` 落库成 `-1`，意思是「这条算不出来」
    pub stuck_secs: Option<i64>,
    /// 给人看的那句话：通道、失败原因
    pub message: &'a str,
}

/// 一行 `resume_records` → [`ResumeRecord`]
///
/// 列的顺序由 [`RESUME_COLUMNS`] 一处定死，错位了不会报错、只会把
/// `message` 显示成时间。
fn row_to_resume(row: &rusqlite::Row) -> rusqlite::Result<ResumeRecord> {
    Ok(ResumeRecord {
        id: row.get(0)?,
        session_id: row.get(1)?,
        agent_name: row.get(2)?,
        working_dir: row.get(3)?,
        prompt_type: row.get(4)?,
        success: row.get::<_, i32>(5)? != 0,
        outcome: row.get(6)?,
        // 补列时给了 `DEFAULT -1`，所以老行读出来就是 -1；`Option` 那层是防
        // 手工改过库的情形——NULL 说的也是「不知道」，让它走同一个出口。
        stuck_secs: row.get::<_, Option<i64>>(7)?.unwrap_or(-1),
        message: row.get(8)?,
        created_at: row.get(9)?,
    })
}

/// 分页后的续跑记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeRecordPage {
    pub records: Vec<ResumeRecord>,
    pub total: u32,
}

/// 分页后的会话历史
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHistoryPage {
    pub entries: Vec<SessionHistoryEntry>,
    pub total: u32,
}

/// 会话历史的汇总数字（跟着搜索条件走，不跟着分页走）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionHistorySummary {
    pub total: u32,
    pub live: u32,
    pub resumes: u32,
    pub cost_usd: f64,
    pub total_tokens: u64,
}

/// 一个会话的完整档案：它自己 + 它身上发生过的事
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDetail {
    pub entry: SessionHistoryEntry,
    /// 这个会话的续跑记录，时间正序
    pub resumes: Vec<ResumeRecord>,
    /// 这个会话被判定中断的记录，时间正序
    pub detections: Vec<DetectionRecord>,
}

/// 统计总览
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatsOverview {
    pub total_scans: u32,
    pub total_detections: u32,
    pub total_resumes: u32,
    pub successful_resumes: u32,
    pub failed_resumes: u32,
    pub active_sessions: u32,
}

/// 一个指标的本期与上期
///
/// 两边都是 `Option`，因为「算不出来」必须能跟「算出来是 0」分开说。
/// 拿 0 兼职表示没数据，全新安装的第一天就会看到「成功率 0%，比上期跌了 100%」——
/// 那是一句假话，而且它出现的时机恰好是用户对这个工具最没建立信任的时候。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrendMetric {
    pub current: Option<f64>,
    pub previous: Option<f64>,
}

/// 两个等长时段的对比（v1.7）
///
/// 「上期」不是「上一段日历时间」，而是「上一段**我们真的在守护**的时间」：
/// 见 [`Storage::stats_trend`] 里对 `daily_stats` 的覆盖判断。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatsTrend {
    /// 窗口长度（天）。1 = 今日 vs 昨日
    pub window_days: u32,
    /// 确认中断的次数（`detection_records` 一次中断一行）
    pub interruptions: TrendMetric,
    /// 续跑次数
    pub resumes: TrendMetric,
    /// 敲进去的比例，0–100
    pub landed_rate: TrendMetric,
    /// 平均卡了多久才被催醒（秒）
    pub stuck_secs: TrendMetric,
}

impl StatsTrend {
    /// 上期压根不存在（那段时间应用没在跑）
    ///
    /// 判据用计数类指标：它们只要「那段时间在守护」就一定有值，哪怕是 0。
    /// 比率类的不能当判据——上期开着但一次没续跑，成功率也是 `None`，
    /// 那是「这个指标算不出来」，不是「上期不存在」。
    pub fn previous_is_missing(&self) -> bool {
        self.interruptions.previous.is_none() && self.resumes.previous.is_none()
    }
}

/// 一个时段的原始计数，[`StatsTrend`] 的中间产物
#[derive(Debug, Clone, Copy, Default)]
struct TrendBucket {
    /// 这段时间里应用到底有没有在跑
    covered: bool,
    interruptions: u32,
    resumes: u32,
    landed: u32,
    /// 只累加 `stuck_secs >= 0` 的那些行
    stuck_total: i64,
    stuck_rows: u32,
}

impl TrendBucket {
    /// 没续跑过就没有成功率——不是 0%，是没有
    fn landed_rate(&self) -> Option<f64> {
        (self.resumes > 0).then(|| self.landed as f64 * 100.0 / self.resumes as f64)
    }

    /// 一条能算的记录都没有时同理
    fn avg_stuck(&self) -> Option<f64> {
        (self.stuck_rows > 0).then(|| self.stuck_total as f64 / self.stuck_rows as f64)
    }

    /// 计数类指标：只要这段时间在守护，0 就是一个真实的答案
    fn count(&self, n: u32) -> Option<f64> {
        self.covered.then_some(n as f64)
    }
}

/// SQLite 持久化存储引擎
pub struct Storage {
    conn: Mutex<Connection>,
}

impl Storage {
    /// 打开或创建数据库
    pub fn new() -> Self {
        let db_path = Self::db_path();
        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let conn = Connection::open(&db_path)
            .unwrap_or_else(|_| Connection::open_in_memory().expect("无法创建内存数据库"));

        let storage = Self {
            conn: Mutex::new(conn),
        };
        storage.init_tables();
        storage
    }

    fn db_path() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("agent-pulse")
            .join("agentpulse.db")
    }

    /// 只在内存里建一个库，用于测试
    ///
    /// [`Self::new`] 打开的是用户真正那份 `agentpulse.db`，跑一次测试就往里
    /// 塞几条假记录——统计页上会多出凭空的续跑次数。所以测试走这个入口。
    #[cfg(test)]
    fn in_memory() -> Self {
        let storage = Self {
            conn: Mutex::new(Connection::open_in_memory().expect("无法创建内存数据库")),
        };
        storage.init_tables();
        storage
    }

    /// 初始化表结构
    fn init_tables(&self) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS resume_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                agent_name TEXT NOT NULL,
                working_dir TEXT DEFAULT '',
                prompt_type TEXT DEFAULT 'generic',
                success INTEGER DEFAULT 1,
                outcome TEXT DEFAULT '',
                stuck_secs INTEGER DEFAULT -1,
                message TEXT DEFAULT '',
                created_at TEXT DEFAULT (datetime('now', 'localtime'))
            );

            CREATE TABLE IF NOT EXISTS detection_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                agent_name TEXT NOT NULL,
                verdict TEXT NOT NULL,
                signals TEXT DEFAULT '',
                has_active_goal INTEGER DEFAULT 0,
                created_at TEXT DEFAULT (datetime('now', 'localtime'))
            );

            CREATE TABLE IF NOT EXISTS daily_stats (
                date TEXT PRIMARY KEY,
                total_scans INTEGER DEFAULT 0,
                total_detections INTEGER DEFAULT 0,
                total_resumes INTEGER DEFAULT 0,
                successful_resumes INTEGER DEFAULT 0,
                failed_resumes INTEGER DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_resume_created ON resume_records(created_at);
            CREATE INDEX IF NOT EXISTS idx_detection_created ON detection_records(created_at);

            -- v1.7：会话档案抽屉按 `session_id` 取这两张表，而上面那两个索引
            -- 只盖了 `created_at`——于是每开一次抽屉都是两次全表扫描。
            --
            -- 复合到 `(session_id, created_at)` 而不是只索引 `session_id`：
            -- 两个查询都紧跟着 `ORDER BY created_at`，把排序键放进索引，
            -- SQLite 直接顺着索引读，连排序那一步都省了。
            --
            -- 现在的量级下这不是能感知的卡顿（2 万行约 1 毫秒）。加它是因为
            -- 全表扫描的代价**随行数线性涨**，而这两张表只增不减：等用户攒够
            -- 数据、真的觉得抽屉变慢的时候，那台机器上已经没人愿意等一次
            -- 迁移了。索引在 2 万行时占 733 KB，比它替下来的扫描便宜。
            CREATE INDEX IF NOT EXISTS idx_resume_session ON resume_records(session_id, created_at);
            CREATE INDEX IF NOT EXISTS idx_detection_session ON detection_records(session_id, created_at);

            -- v1.2 洞察层：逐请求用量。主键即去重键，天然幂等，
            -- 因此重复解析同一段 jsonl 不会把账单算两遍。
            CREATE TABLE IF NOT EXISTS usage_records (
                dedup_key TEXT PRIMARY KEY,
                ts TEXT NOT NULL,
                date TEXT NOT NULL,
                model TEXT NOT NULL,
                project TEXT DEFAULT '',
                session_file TEXT DEFAULT '',
                input_tokens INTEGER DEFAULT 0,
                output_tokens INTEGER DEFAULT 0,
                cache_write_tokens INTEGER DEFAULT 0,
                cache_read_tokens INTEGER DEFAULT 0,
                cost_usd REAL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_usage_date ON usage_records(date);
            CREATE INDEX IF NOT EXISTS idx_usage_ts ON usage_records(ts);
            CREATE INDEX IF NOT EXISTS idx_usage_file ON usage_records(session_file);
            CREATE INDEX IF NOT EXISTS idx_usage_project ON usage_records(project);

            -- 增量读取游标：重启后不必重新解析历史日志
            CREATE TABLE IF NOT EXISTS usage_cursors (
                path TEXT PRIMARY KEY,
                byte_offset INTEGER NOT NULL
            );

            -- v1.2 洞察层：会话历史时间线
            CREATE TABLE IF NOT EXISTS session_history (
                session_key TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                agent_name TEXT NOT NULL,
                working_dir TEXT DEFAULT '',
                session_file TEXT DEFAULT '',
                tty TEXT DEFAULT '',
                terminal_app TEXT DEFAULT '',
                first_seen TEXT NOT NULL,
                last_seen TEXT NOT NULL,
                last_status TEXT DEFAULT '',
                ended_at TEXT DEFAULT '',
                resume_count INTEGER DEFAULT 0,
                total_tokens INTEGER DEFAULT 0,
                cost_usd REAL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_history_last_seen ON session_history(last_seen);",
        );
        drop(conn);
        self.migrate();
    }

    /// 给已经存在的库补列
    ///
    /// `CREATE TABLE IF NOT EXISTS` 只在**没有**这张表的时候管事。上面那一批
    /// 建表语句对一个装过旧版本的机器来说全是空操作，于是往里加一列
    /// 只对全新安装生效——老用户的库永远缺那一列，读它的查询要么报错
    /// 要么被 `unwrap_or` 吞成默认值，看起来像功能没做。
    ///
    /// 所以补列走这里，一次把机制建好，以后加字段只是往表里加一行声明：
    /// [`Self::ensure_column`] 自己判断列在不在，重复运行没有副作用，
    /// 也不需要维护版本号——库的真实形状就是唯一的事实来源。
    fn migrate(&self) {
        // (表, 列, 类型与默认值)
        const COLUMNS: &[(&str, &str, &str)] = &[
            // v1.6：把「为什么停」跟「停了」分开存。旧行留空字符串，
            // 统计层要按原因分组时，空串就是「那时候还没记原因」。
            ("detection_records", "reason", "TEXT DEFAULT ''"),
            // v1.6：`success` 只能说「成没成」，可「敲了没反应」和「压根没敲出去」
            // 对用户是两件不同的事，前者要查焦点/输入法，后者要查权限/定位。
            // 旧行留空字符串，筛选时按 `success` 兜底。
            ("resume_records", "outcome", "TEXT DEFAULT ''"),
            // v1.7：出手时它已经卡了多久。默认值是 **-1 而不是 0**——统计层要拿
            // 它算平均值，一批凭空的 0 会把「平均卡了 20 分钟」稀释成 3 分钟，
            // 而这个数正是用来判断「守护的反应够不够快」的。
            ("resume_records", "stuck_secs", "INTEGER DEFAULT -1"),
            // v1.7：会话什么时候从视野里消失。以前没有这一列，于是
            // 「它还在不在」只能去猜 `last_status`，而那一列是**最后一眼看到的
            // 状态**、写进去就再没人改过——用户关掉的会话因此永远显示「运行中」。
            // 旧行默认空串（=「还看得见」），装上新版本后第一轮扫描就会
            // 把真的已经不在的那些收尾掉，见 `close_missing_sessions`。
            ("session_history", "ended_at", "TEXT DEFAULT ''"),
        ];
        for (table, column, decl) in COLUMNS {
            self.ensure_column(table, column, decl);
        }
        self.merge_fragmented_history();
    }

    /// 把同一个会话在历史里裂开的那些行并成一行（一次性修补）
    ///
    /// v1.7 之前，没有记录文件的会话拿 `adapter-pid-首次发现时间` 当主键，
    /// 而「首次发现时间」只活在进程内——AgentPulse 一重启就变成「现在」，
    /// 同一个会话于是换个键重新落库。真实库里因此出现过一个会话摊成 16 行、
    /// 63 行里 61 行是重复的情况。键已经在
    /// [`crate::adapters::AgentSession::history_key`] 修好了，但**已经生出来的
    /// 那些行不会自己合起来**，历史页照样是一堆看不出意思的重复。
    ///
    /// 合并规则跟 [`Self::upsert_session_history`] 本来的写法对齐，所以聚合值
    /// 一个都不会丢：`first_seen` 取最早、`last_seen` 取最晚，另外三个取
    /// 最大值（那个 upsert 一直是 `MAX`，因为同一份用量会被反复写入，
    /// 求和会把它累计成好几倍）。
    ///
    /// 幂等：跑完就没有重复了，第二次跑是空操作。只碰
    /// `session_file = ''` 的行——用记录文件当键的行从来没有这个毛病。
    fn merge_fragmented_history(&self) {
        let conn = self.conn.lock().unwrap();
        // 同一个 `session_id` + 同一个工作目录 = 同一个会话。
        // 不用重算新键，因为老行里没存 `adapter_id`；而 `session_id`
        // 本身就带着 adapter 前缀和进程代际（旧形状如 `cc-68590`，新形状还含启动时刻）。
        let groups: Vec<(String, String, i64)> = {
            let mut stmt = match conn.prepare(
                "SELECT session_id, working_dir, COUNT(*) FROM session_history \
                 WHERE session_file = '' GROUP BY session_id, working_dir HAVING COUNT(*) > 1",
            ) {
                Ok(s) => s,
                Err(_) => return,
            };
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .map(|it| it.filter_map(|r| r.ok()).collect())
                .unwrap_or_default();
            rows
        };
        if groups.is_empty() {
            return;
        }
        let mut merged = 0usize;
        for (session_id, working_dir, count) in &groups {
            // 留下 `last_seen` 最新的那一行当代表，把这一组的聚合值写到它上面，
            // 再删掉同组其余的行。先写后删：万一中间失败，宁可多几行重复，
            // 也不要把已经算好的用量删掉。
            let updated = conn.execute(
                "UPDATE session_history SET
                    first_seen = (SELECT MIN(first_seen) FROM session_history
                                  WHERE session_id = ?1 AND working_dir = ?2 AND session_file = ''),
                    last_seen  = (SELECT MAX(last_seen)  FROM session_history
                                  WHERE session_id = ?1 AND working_dir = ?2 AND session_file = ''),
                    resume_count = (SELECT MAX(resume_count) FROM session_history
                                    WHERE session_id = ?1 AND working_dir = ?2 AND session_file = ''),
                    total_tokens = (SELECT MAX(total_tokens) FROM session_history
                                    WHERE session_id = ?1 AND working_dir = ?2 AND session_file = ''),
                    cost_usd = (SELECT MAX(cost_usd) FROM session_history
                                WHERE session_id = ?1 AND working_dir = ?2 AND session_file = '')
                 WHERE session_key = (SELECT session_key FROM session_history
                                      WHERE session_id = ?1 AND working_dir = ?2 AND session_file = ''
                                      ORDER BY last_seen DESC, session_key DESC LIMIT 1)",
                params![session_id, working_dir],
            );
            if updated.is_err() {
                continue;
            }
            let _ = conn.execute(
                "DELETE FROM session_history
                 WHERE session_id = ?1 AND working_dir = ?2 AND session_file = ''
                   AND session_key <> (SELECT session_key FROM session_history
                                       WHERE session_id = ?1 AND working_dir = ?2 AND session_file = ''
                                       ORDER BY last_seen DESC, session_key DESC LIMIT 1)",
                params![session_id, working_dir],
            );
            merged += (*count as usize).saturating_sub(1);
        }
        if merged > 0 {
            tracing::info!(
                "[AgentPulse] 会话历史去重：{} 组重复行并掉 {merged} 行（旧版本主键含重启时间所致）",
                groups.len()
            );
        }
    }

    /// 列不存在就加上；存在就什么都不做
    fn ensure_column(&self, table: &str, column: &str, decl: &str) {
        let conn = self.conn.lock().unwrap();
        // 表名和列名都是本文件里的字面量，不来自外部输入，所以可以拼进 SQL；
        // `PRAGMA` 和 `ALTER TABLE` 都不接受占位符，这里没有别的写法。
        let existing: Result<i64, _> = conn.query_row(
            &format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = ?1"),
            params![column],
            |r| r.get(0),
        );
        if existing.unwrap_or(1) == 0 {
            let _ = conn.execute(
                &format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"),
                [],
            );
        }
    }

    /// 记录一次续跑事件
    ///
    /// 收成一个结构体而不是排一串参数，是因为这里有四个相邻的 `&str` 和一个
    /// 夹在中间的 `bool`——`(prompt_type, success, outcome, message)` 顺序写反
    /// 编译器一声不响，只有翻记录时才发现「结果」那一列显示的是提示词类型。
    /// 字段名把位置这件事从调用方手里拿走了。
    pub fn record_resume(&self, event: ResumeEvent<'_>) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO resume_records (session_id, agent_name, working_dir, prompt_type, success, outcome, stuck_secs, message)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                event.session_id,
                event.agent_name,
                event.working_dir,
                event.prompt_type,
                event.success as i32,
                event.outcome,
                event.stuck_secs.unwrap_or(-1),
                event.message
            ],
        );
        // 更新每日统计
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let _ = conn.execute(
            "INSERT INTO daily_stats (date, total_resumes, successful_resumes, failed_resumes)
             VALUES (?1, 1, ?2, ?3)
             ON CONFLICT(date) DO UPDATE SET
                total_resumes = total_resumes + 1,
                successful_resumes = successful_resumes + ?2,
                failed_resumes = failed_resumes + ?3",
            params![today, event.success as i32, (!event.success) as i32],
        );
    }

    /// 记录一次检测事件
    pub fn record_detection(
        &self,
        session_id: &str,
        agent_name: &str,
        verdict: &str,
        signals: &str,
        has_active_goal: bool,
        reason: &str,
    ) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO detection_records (session_id, agent_name, verdict, signals, has_active_goal, reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![session_id, agent_name, verdict, signals, has_active_goal as i32, reason],
        );
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let _ = conn.execute(
            "INSERT INTO daily_stats (date, total_detections)
             VALUES (?1, 1)
             ON CONFLICT(date) DO UPDATE SET total_detections = total_detections + 1",
            params![today],
        );
    }

    /// 记录一次扫描
    pub fn record_scan(&self) {
        let conn = self.conn.lock().unwrap();
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let _ = conn.execute(
            "INSERT INTO daily_stats (date, total_scans)
             VALUES (?1, 1)
             ON CONFLICT(date) DO UPDATE SET total_scans = total_scans + 1",
            params![today],
        );
    }

    /// 获取最近 N 天的统计
    ///
    /// 返回按日期升序、且**补齐空缺日期**的连续序列：
    /// `daily_stats` 只在有活动的那天写行，直接喂给柱状图会得到一条日期不连续、
    /// 从右往左倒着长的图。补齐后前端可以直接按数组顺序画时间轴。
    pub fn get_stats(&self, days: u32) -> Vec<DailyStats> {
        let days = days.max(1);
        let today = chrono::Local::now().date_naive();
        let start = today - chrono::Duration::days(days as i64 - 1);
        let start_str = start.format("%Y-%m-%d").to_string();

        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT date, total_scans, total_detections, total_resumes, successful_resumes, failed_resumes
             FROM daily_stats WHERE date >= ?1 ORDER BY date ASC",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };

        let rows: HashMap<String, DailyStats> = stmt
            .query_map(params![start_str], |row| {
                Ok(DailyStats {
                    date: row.get(0)?,
                    total_scans: row.get(1)?,
                    total_detections: row.get(2)?,
                    total_resumes: row.get(3)?,
                    successful_resumes: row.get(4)?,
                    failed_resumes: row.get(5)?,
                })
            })
            .map(|iter| {
                iter.filter_map(|r| r.ok())
                    .map(|s| (s.date.clone(), s))
                    .collect()
            })
            .unwrap_or_default();

        (0..days)
            .map(|i| {
                let date = (start + chrono::Duration::days(i as i64))
                    .format("%Y-%m-%d")
                    .to_string();
                rows.get(&date).cloned().unwrap_or(DailyStats {
                    date,
                    ..Default::default()
                })
            })
            .collect()
    }

    /// 获取最近的续跑记录
    pub fn get_recent_resumes(&self, limit: u32) -> Vec<ResumeRecord> {
        let conn = self.conn.lock().unwrap();
        // 查询失败就返回空列表：历史记录读不出来是遗憾，不是让整个应用崩掉的理由
        let Ok(mut stmt) = conn.prepare(&format!(
            "SELECT {RESUME_COLUMNS} FROM resume_records ORDER BY id DESC LIMIT ?1"
        )) else {
            return Vec::new();
        };

        let rows = stmt.query_map(params![limit], row_to_resume);

        match rows {
            Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
            Err(e) => {
                tracing::warn!("[Storage] 读取续跑记录失败: {e}");
                Vec::new()
            }
        }
    }

    /// 分页、搜索、按结果与提示词类型筛选续跑记录
    ///
    /// `outcome` 认 `all` 和四个核验态（`landed` / `silent` / `failed` /
    /// `unverifiable`）；`prompt_type` 认 `all` / `goal` / `generic`。
    /// 认不出的值一律当 `all`——筛选条件拼错时应该多给几条，而不是给一个
    /// 空列表让人以为「真的没有记录」。
    pub fn get_resume_page(
        &self,
        limit: u32,
        offset: u32,
        query: &str,
        outcome: &str,
        prompt_type: &str,
    ) -> ResumeRecordPage {
        let conn = self.conn.lock().unwrap();
        let query = query.trim();
        let like = format!("%{query}%");
        let outcome_filter = match outcome {
            "landed" | "silent" | "failed" | "unverifiable" => outcome,
            _ => "all",
        };
        let type_filter = match prompt_type {
            "goal" | "generic" => prompt_type,
            _ => "all",
        };
        let total = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM resume_records WHERE {RESUME_FILTER}"),
                params![query, like, outcome_filter, type_filter],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0) as u32;
        let mut stmt = match conn.prepare(&format!(
            "SELECT {RESUME_COLUMNS} FROM resume_records \
             WHERE {RESUME_FILTER} ORDER BY id DESC LIMIT ?5 OFFSET ?6"
        )) {
            Ok(stmt) => stmt,
            Err(_) => {
                return ResumeRecordPage {
                    records: vec![],
                    total,
                }
            }
        };
        let records = stmt
            .query_map(
                params![query, like, outcome_filter, type_filter, limit, offset],
                row_to_resume,
            )
            .map(|rows| rows.filter_map(|row| row.ok()).collect())
            .unwrap_or_default();
        ResumeRecordPage { records, total }
    }

    /// 获取统计摘要
    pub fn stats_overview(&self) -> StatsOverview {
        let conn = self.conn.lock().unwrap();
        StatsOverview {
            total_scans: conn
                .query_row(
                    "SELECT COALESCE(SUM(total_scans), 0) FROM daily_stats",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0),
            total_detections: conn
                .query_row(
                    "SELECT COALESCE(SUM(total_detections), 0) FROM daily_stats",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0),
            total_resumes: conn
                .query_row(
                    "SELECT COALESCE(SUM(total_resumes), 0) FROM daily_stats",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0),
            successful_resumes: conn
                .query_row(
                    "SELECT COALESCE(SUM(successful_resumes), 0) FROM daily_stats",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0),
            failed_resumes: conn
                .query_row(
                    "SELECT COALESCE(SUM(failed_resumes), 0) FROM daily_stats",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0),
            active_sessions: 0,
        }
    }

    /// 本期 vs 上期（v1.7）
    ///
    /// `window_days = 1` 是今日 vs 昨日，`7` 是近 7 天 vs 前 7 天。窗口按**本地
    /// 日历日**切，不按「距今 24 小时」：用户看统计时问的是「今天怎么样」，
    /// 而不是「过去 1440 分钟怎么样」，后者会让上午看到的「今日」掺进昨天下半夜。
    ///
    /// 「上期」判定的是**有没有在守护**，不是「有没有记录」。这两者在一处关键的
    /// 地方会分岔：全新安装的第二天，前 7 天一行都没有——如果把它读成「上期
    /// 中断 0 次」，用户会看到「中断次数 +3，恶化了」，可上期压根不存在。
    /// 判据用 `daily_stats`：它每扫一轮就记一次，只要那几天应用开着就一定有行。
    pub fn stats_trend(&self, window_days: u32) -> StatsTrend {
        let window_days = window_days.clamp(1, 90);
        let conn = self.conn.lock().unwrap();
        let today = chrono::Local::now().date_naive();
        let span = chrono::Duration::days(window_days as i64);
        // 三个边界切出两段闭开区间：[prev_start, cur_start) 和 [cur_start, 明天)
        let cur_start = (today - span + chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let prev_start = (today - span - span + chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let cur_end = (today + chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();

        let mut cur = TrendBucket::default();
        let mut prev = TrendBucket::default();

        // 覆盖判断：那一段里有没有扫描过
        //
        // 注意比的是 `date >= ?1 AND date < ?2`，跟下面两句用 `created_at` 比
        // 日期前缀是同一套边界。`created_at` 是 `YYYY-MM-DD HH:MM:SS`，
        // 字典序跟时间序一致，所以直接跟日期串比大小是对的，而且还能用上
        // `idx_resume_created` / `idx_detection_created`——包成 `date(created_at)`
        // 就会退化成全表扫。
        let covered = |from: &str, to: &str| -> bool {
            conn.query_row(
                "SELECT COUNT(*) FROM daily_stats WHERE date >= ?1 AND date < ?2 AND total_scans > 0",
                params![from, to],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
                > 0
        };
        cur.covered = covered(&cur_start, &cur_end);
        prev.covered = covered(&prev_start, &cur_start);

        // 中断次数：`record_detection` 只在会话**刚**被确认中断时写一行，
        // 所以这里数的是事件个数，不是「有多少轮扫描看它还停着」
        let count_detections = |from: &str, to: &str| -> u32 {
            conn.query_row(
                "SELECT COUNT(*) FROM detection_records WHERE created_at >= ?1 AND created_at < ?2",
                params![from, to],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0) as u32
        };
        cur.interruptions = count_detections(&cur_start, &cur_end);
        prev.interruptions = count_detections(&prev_start, &cur_start);

        // 续跑侧一条 SQL 取齐：总数、落地数、以及可算的卡住时长
        //
        // `stuck_secs >= 0` 要写在 SUM/COUNT 里面而不是 WHERE 里：老行是 -1，
        // 放进 WHERE 会把它们整行剔掉，于是「续跑次数」也跟着少——一个为了
        // 算平均值加的过滤条件，顺手改掉了另一个指标。
        let resume_row = |from: &str, to: &str| -> (u32, u32, i64, u32) {
            conn.query_row(
                "SELECT COUNT(*), \
                        COALESCE(SUM(success), 0), \
                        COALESCE(SUM(CASE WHEN stuck_secs >= 0 THEN stuck_secs ELSE 0 END), 0), \
                        COALESCE(SUM(CASE WHEN stuck_secs >= 0 THEN 1 ELSE 0 END), 0) \
                 FROM resume_records WHERE created_at >= ?1 AND created_at < ?2",
                params![from, to],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)? as u32,
                        r.get::<_, i64>(1)? as u32,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)? as u32,
                    ))
                },
            )
            .unwrap_or((0, 0, 0, 0))
        };
        (cur.resumes, cur.landed, cur.stuck_total, cur.stuck_rows) =
            resume_row(&cur_start, &cur_end);
        (prev.resumes, prev.landed, prev.stuck_total, prev.stuck_rows) =
            resume_row(&prev_start, &cur_start);

        StatsTrend {
            window_days,
            interruptions: TrendMetric {
                current: cur.count(cur.interruptions),
                previous: prev.count(prev.interruptions),
            },
            resumes: TrendMetric {
                current: cur.count(cur.resumes),
                previous: prev.count(prev.resumes),
            },
            landed_rate: TrendMetric {
                current: cur.landed_rate(),
                previous: prev.landed_rate(),
            },
            stuck_secs: TrendMetric {
                current: cur.avg_stuck(),
                previous: prev.avg_stuck(),
            },
        }
    }

    /// 获取总体统计: (total_detections, total_resumes, successful_resumes)
    pub fn get_totals(&self) -> (u32, u32, u32) {
        let conn = self.conn.lock().unwrap();
        let total_detections: u32 = conn
            .query_row("SELECT COUNT(*) FROM detection_records", [], |r| r.get(0))
            .unwrap_or(0);
        let total_resumes: u32 = conn
            .query_row("SELECT COUNT(*) FROM resume_records", [], |r| r.get(0))
            .unwrap_or(0);
        let success_resumes: u32 = conn
            .query_row(
                "SELECT COUNT(*) FROM resume_records WHERE success = 1",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        (total_detections, total_resumes, success_resumes)
    }

    // ───────────────────────── v1.2 洞察层 ─────────────────────────

    /// 批量写入用量记录（幂等）
    ///
    /// `INSERT OR IGNORE` + `dedup_key` 主键：同一个 API 请求无论被解析多少次、
    /// 出现在多少个 jsonl 分支里，只会被计一次费。
    pub fn record_usage_batch(&self, entries: &[UsageEntry]) -> usize {
        if entries.is_empty() {
            return 0;
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = match conn.transaction() {
            Ok(t) => t,
            Err(_) => return 0,
        };
        let mut inserted = 0usize;
        {
            let mut stmt = match tx.prepare(
                "INSERT OR IGNORE INTO usage_records
                   (dedup_key, ts, date, model, project, session_file,
                    input_tokens, output_tokens, cache_write_tokens, cache_read_tokens, cost_usd)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            ) {
                Ok(s) => s,
                Err(_) => return 0,
            };
            for e in entries {
                let date = &e.timestamp[..e.timestamp.len().min(10)];
                if let Ok(n) = stmt.execute(params![
                    e.dedup_key,
                    e.timestamp,
                    date,
                    e.model,
                    e.project,
                    e.session_file,
                    e.input_tokens,
                    e.output_tokens,
                    e.cache_write_tokens,
                    e.cache_read_tokens,
                    e.cost_usd,
                ]) {
                    inserted += n;
                }
            }
        }
        let _ = tx.commit();
        inserted
    }

    /// 读取增量解析游标
    pub fn load_usage_cursors(&self) -> HashMap<String, u64> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare("SELECT path, byte_offset FROM usage_cursors") {
            Ok(s) => s,
            Err(_) => return HashMap::new(),
        };
        stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
        })
        .map(|iter| iter.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    }

    /// 保存增量解析游标
    pub fn save_usage_cursors(&self, cursors: &[(String, u64)]) {
        if cursors.is_empty() {
            return;
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = match conn.transaction() {
            Ok(t) => t,
            Err(_) => return,
        };
        {
            if let Ok(mut stmt) = tx.prepare(
                "INSERT INTO usage_cursors (path, byte_offset) VALUES (?1, ?2)
                 ON CONFLICT(path) DO UPDATE SET byte_offset = ?2",
            ) {
                for (path, offset) in cursors {
                    let _ = stmt.execute(params![path, *offset as i64]);
                }
            }
        }
        let _ = tx.commit();
    }

    /// 单个会话文件的累计用量
    pub fn usage_for_session_file(&self, session_file: &str) -> Option<UsageSnapshot> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                    COALESCE(SUM(cache_write_tokens),0), COALESCE(SUM(cache_read_tokens),0),
                    COALESCE(SUM(cost_usd),0), COUNT(*)
             FROM usage_records WHERE session_file = ?1",
            params![session_file],
            |row| {
                let input: i64 = row.get(0)?;
                let output: i64 = row.get(1)?;
                let cw: i64 = row.get(2)?;
                let cr: i64 = row.get(3)?;
                let requests: u32 = row.get(5)?;
                Ok(UsageSnapshot {
                    input_tokens: input as u64,
                    output_tokens: output as u64,
                    cache_write_tokens: cw as u64,
                    cache_read_tokens: cr as u64,
                    total_tokens: (input + output + cw + cr) as u64,
                    cost_usd: row.get(4)?,
                    requests,
                })
            },
        )
        .ok()
        .filter(|s| s.requests > 0)
    }

    /// 指定日期（本地）的总花费
    pub fn cost_for_date(&self, date: &str) -> f64 {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COALESCE(SUM(cost_usd),0) FROM usage_records WHERE date = ?1",
            params![date],
            |row| row.get(0),
        )
        .unwrap_or(0.0)
    }

    /// 最近 N 天的每日成本（升序、补齐空缺日期）
    pub fn daily_costs(&self, days: u32) -> Vec<DailyCost> {
        let days = days.max(1);
        let today = chrono::Local::now().date_naive();
        let start = today - chrono::Duration::days(days as i64 - 1);

        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT date,
                    COALESCE(SUM(input_tokens + output_tokens + cache_write_tokens + cache_read_tokens),0),
                    COALESCE(SUM(cost_usd),0), COUNT(*)
             FROM usage_records WHERE date >= ?1 GROUP BY date ORDER BY date ASC",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };

        let rows: HashMap<String, DailyCost> = stmt
            .query_map(params![start.format("%Y-%m-%d").to_string()], |row| {
                Ok(DailyCost {
                    date: row.get(0)?,
                    total_tokens: row.get::<_, i64>(1)? as u64,
                    cost_usd: row.get(2)?,
                    requests: row.get(3)?,
                })
            })
            .map(|iter| {
                iter.filter_map(|r| r.ok())
                    .map(|c| (c.date.clone(), c))
                    .collect()
            })
            .unwrap_or_default();

        (0..days)
            .map(|i| {
                let date = (start + chrono::Duration::days(i as i64))
                    .format("%Y-%m-%d")
                    .to_string();
                rows.get(&date).cloned().unwrap_or(DailyCost {
                    date,
                    total_tokens: 0,
                    cost_usd: 0.0,
                    requests: 0,
                })
            })
            .collect()
    }

    /// 最近 N 天花钱最多的项目
    pub fn project_costs(&self, days: u32, limit: u32) -> Vec<ProjectCost> {
        let today = chrono::Local::now().date_naive();
        let start = today - chrono::Duration::days(days.max(1) as i64 - 1);

        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT project,
                    COALESCE(SUM(input_tokens + output_tokens + cache_write_tokens + cache_read_tokens),0),
                    COALESCE(SUM(cost_usd),0), COUNT(*)
             FROM usage_records WHERE date >= ?1 AND project <> ''
             GROUP BY project ORDER BY 3 DESC LIMIT ?2",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };

        stmt.query_map(
            params![start.format("%Y-%m-%d").to_string(), limit],
            |row| {
                Ok(ProjectCost {
                    project: row.get(0)?,
                    total_tokens: row.get::<_, i64>(1)? as u64,
                    cost_usd: row.get(2)?,
                    requests: row.get(3)?,
                })
            },
        )
        .map(|iter| iter.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    }

    /// 最近 N 天按模型聚合的成本
    pub fn model_costs(&self, days: u32, limit: u32) -> Vec<ModelCost> {
        let today = chrono::Local::now().date_naive();
        let start = today - chrono::Duration::days(days.max(1) as i64 - 1);
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT model, COALESCE(SUM(input_tokens + output_tokens + cache_write_tokens + cache_read_tokens),0), COALESCE(SUM(cost_usd),0), COUNT(*) FROM usage_records WHERE date >= ?1 GROUP BY model ORDER BY 3 DESC LIMIT ?2",
        ) { Ok(stmt) => stmt, Err(_) => return vec![] };
        stmt.query_map(
            params![start.format("%Y-%m-%d").to_string(), limit],
            |row| {
                Ok(ModelCost {
                    model: row.get(0)?,
                    total_tokens: row.get::<_, i64>(1)? as u64,
                    cost_usd: row.get(2)?,
                    requests: row.get(3)?,
                })
            },
        )
        .map(|rows| rows.filter_map(|row| row.ok()).collect())
        .unwrap_or_default()
    }

    /// 指定区间的 token / 缓存 / 请求汇总
    pub fn usage_summary(&self, days: u32) -> UsageSnapshot {
        let today = chrono::Local::now().date_naive();
        let start = today - chrono::Duration::days(days.max(1) as i64 - 1);
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0), COALESCE(SUM(cache_write_tokens),0), COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cost_usd),0), COUNT(*) FROM usage_records WHERE date >= ?1",
            params![start.format("%Y-%m-%d").to_string()],
            |row| { let i: i64 = row.get(0)?; let o: i64 = row.get(1)?; let cw: i64 = row.get(2)?; let cr: i64 = row.get(3)?; Ok(UsageSnapshot { input_tokens: i as u64, output_tokens: o as u64, cache_write_tokens: cw as u64, cache_read_tokens: cr as u64, total_tokens: (i + o + cw + cr) as u64, cost_usd: row.get(4)?, requests: row.get(5)? }) },
        ).unwrap_or_default()
    }

    /// 最近 N 小时内消耗的 token 总量（用于限流窗口预测）
    pub fn tokens_in_last_hours(&self, hours: u32) -> u64 {
        let since = chrono::Local::now() - chrono::Duration::hours(hours.max(1) as i64);
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COALESCE(SUM(input_tokens + output_tokens + cache_write_tokens + cache_read_tokens),0)
             FROM usage_records WHERE ts >= ?1",
            params![since.format("%Y-%m-%d %H:%M:%S").to_string()],
            |row| row.get::<_, i64>(0),
        )
        .map(|v| v as u64)
        .unwrap_or(0)
    }

    /// 记录/更新一条会话历史
    ///
    /// `session_key` 用会话文件路径（没有则用 adapter+pid+首次发现时间），
    /// 这样进程重启换了 PID 但仍是同一份会话时能合并成一条时间线。
    #[allow(clippy::too_many_arguments)]
    pub fn upsert_session_history(
        &self,
        session_key: &str,
        session_id: &str,
        agent_name: &str,
        working_dir: &str,
        session_file: &str,
        tty: &str,
        terminal_app: &str,
        status: &str,
        resume_count: u32,
        total_tokens: u64,
        cost_usd: f64,
    ) {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO session_history
               (session_key, session_id, agent_name, working_dir, session_file, tty, terminal_app,
                first_seen, last_seen, last_status, ended_at, resume_count, total_tokens, cost_usd)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9, '', ?10, ?11, ?12)
             ON CONFLICT(session_key) DO UPDATE SET
                session_id = ?2, working_dir = ?4, session_file = ?5,
                tty = ?6, terminal_app = ?7, last_seen = ?8, last_status = ?9,
                -- 又看见它了就把收尾时间清掉：`claude --resume` 会接着写同一个
                -- 记录文件，也就是同一个 `session_key`。不清的话，一个复活的
                -- 会话会一直挂着「已结束」，而它明明正在跑。
                ended_at = '',
                resume_count = MAX(resume_count, ?10),
                total_tokens = MAX(total_tokens, ?11),
                cost_usd = MAX(cost_usd, ?12)",
            params![
                session_key,
                session_id,
                agent_name,
                working_dir,
                session_file,
                tty,
                terminal_app,
                now,
                status,
                resume_count,
                total_tokens as i64,
                cost_usd
            ],
        );
    }

    /// 给本轮已经看不见的会话盖上收尾时间戳；返回这轮收了几个
    ///
    /// **这是「关掉的会话为什么还显示运行中」的答案。** 以前只有
    /// [`Self::upsert_session_history`] 一条写路径，而它只碰**本轮发现的**会话——
    /// 用户一关窗口，那个会话就再也不会出现在参数里，于是它那一行连同
    /// `last_status = 'active'` 被永久冻在库里。缺的不是某个字段，
    /// 是「消失」这件事压根没有人写。
    ///
    /// `live_keys` 必须来自 [`crate::adapters::AgentSession::history_key`]，
    /// 跟上面那个写入口同一个定义。
    ///
    /// 调用方要保证**这轮真的扫过**（至少启用了一个适配器）。空列表在这里
    /// 是合法输入，意思是「一个都不剩了」，会把所有还挂着的行收尾掉；
    /// 可要是因为适配器全关了而拿到空列表，那句话就变成了谎话。
    pub fn close_missing_sessions(&self, live_keys: &[String]) -> usize {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let conn = self.conn.lock().unwrap();
        // 只动还挂着的行：`ended_at = ''`。已经收过尾的不再重写，
        // 否则每轮扫描都把时间戳往后推，「什么时候结束的」就永远是「刚刚」。
        let sql = if live_keys.is_empty() {
            "UPDATE session_history SET ended_at = ?1 WHERE ended_at = ''".to_string()
        } else {
            // 键是本进程刚生成的路径字符串，仍然走占位符——工作目录里
            // 一个引号就能让拼接出来的 SQL 变成别的语句。
            let holes = (0..live_keys.len())
                .map(|i| format!("?{}", i + 2))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "UPDATE session_history SET ended_at = ?1 \
                 WHERE ended_at = '' AND session_key NOT IN ({holes})"
            )
        };
        let mut args: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(live_keys.len() + 1);
        args.push(&now);
        for key in live_keys {
            args.push(key);
        }
        conn.execute(&sql, args.as_slice()).unwrap_or(0)
    }

    /// 分页、搜索、按状态筛选会话历史
    ///
    /// `status` 取 `all` / `live` / `ended`；不认识的值一律当 `all`，
    /// 宁可多给几行也不要因为拼错一个字就返回空列表。
    pub fn get_session_history_page(
        &self,
        limit: u32,
        offset: u32,
        query: &str,
        status: &str,
    ) -> SessionHistoryPage {
        let conn = self.conn.lock().unwrap();
        let query = query.trim();
        let like = format!("%{query}%");
        let live_clause = match status {
            "live" => " AND ended_at = ''",
            "ended" => " AND ended_at <> ''",
            _ => "",
        };
        let total = conn
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM session_history WHERE {HISTORY_FILTER}{live_clause}"
                ),
                params![query, like],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0) as u32;
        // 还活着的排最前面：会话历史是拿来找「刚才那个跑着的活儿」的，
        // 而结束时间一到，一条刚关掉的会话就会被昨天的记录挤下去。
        let sql = format!(
            "SELECT {HISTORY_COLUMNS} FROM session_history WHERE {HISTORY_FILTER}{live_clause} \
             ORDER BY (ended_at = '') DESC, last_seen DESC LIMIT ?3 OFFSET ?4"
        );
        let mut stmt = match conn.prepare(&sql) {
            Ok(stmt) => stmt,
            Err(_) => {
                return SessionHistoryPage {
                    entries: vec![],
                    total,
                }
            }
        };
        let entries = stmt
            .query_map(params![query, like, limit, offset], row_to_history)
            .map(|rows| rows.filter_map(|row| row.ok()).collect())
            .unwrap_or_default();
        SessionHistoryPage { entries, total }
    }

    /// 会话历史的汇总数字：总数、还活着的、累计花费
    ///
    /// 单独走一条 SQL 而不是在前端数当前那一页——那样翻到第二页，
    /// 「共 40 个会话」会变成「共 20 个」。
    pub fn session_history_summary(&self, query: &str) -> SessionHistorySummary {
        let conn = self.conn.lock().unwrap();
        let query = query.trim();
        let like = format!("%{query}%");
        conn.query_row(
            &format!(
                "SELECT COUNT(*), \
                        COALESCE(SUM(ended_at = ''), 0), \
                        COALESCE(SUM(resume_count), 0), \
                        COALESCE(SUM(cost_usd), 0), \
                        COALESCE(SUM(total_tokens), 0) \
                 FROM session_history WHERE {HISTORY_FILTER}"
            ),
            params![query, like],
            |row| {
                Ok(SessionHistorySummary {
                    total: row.get::<_, i64>(0)? as u32,
                    live: row.get::<_, i64>(1)? as u32,
                    resumes: row.get::<_, i64>(2)? as u32,
                    cost_usd: row.get(3)?,
                    total_tokens: row.get::<_, i64>(4)? as u64,
                })
            },
        )
        .unwrap_or_default()
    }

    /// 查询会话历史（关键字为空则返回最近的）
    pub fn session_history(&self, limit: u32, query: &str) -> Vec<SessionHistoryEntry> {
        let conn = self.conn.lock().unwrap();
        let query = query.trim();
        let like = format!("%{query}%");
        let sql = format!(
            "SELECT {HISTORY_COLUMNS} FROM session_history WHERE {HISTORY_FILTER} \
             ORDER BY (ended_at = '') DESC, last_seen DESC LIMIT ?3"
        );
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(params![query, like, limit], row_to_history)
            .map(|iter| iter.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    /// 一个会话的完整档案：它自己那一行，加上它身上发生过的续跑
    ///
    /// 抽屉里要一次讲完「这个会话经历了什么」，所以在这一层就把两张表拼好。
    /// 让前端拿 `session_id` 再发一次请求也能做，但那会多一次往返，
    /// 而且续跑记录的筛选语义会跟记录中心那边分叉。
    pub fn session_detail(&self, session_key: &str) -> Option<SessionDetail> {
        let entry = {
            let conn = self.conn.lock().unwrap();
            let sql =
                format!("SELECT {HISTORY_COLUMNS} FROM session_history WHERE session_key = ?1");
            conn.query_row(&sql, params![session_key], row_to_history)
                .ok()?
        };
        // 用 `session_id` 关联：续跑记录落库时只存了它。
        // 同一份记录文件跨进程重启后 `session_id` 会变，所以这里可能少列
        // 上一个进程里的续跑——`resume_count` 是累计值，两个数对不上时
        // 以它为准，抽屉里也这么标。
        let resumes = self.resumes_for_session(&entry.session_id);
        let detections = self.detections_for_session(&entry.session_id);
        Some(SessionDetail {
            entry,
            resumes,
            detections,
        })
    }

    /// 某个会话的续跑记录，按时间正序（讲故事要从头讲）
    fn resumes_for_session(&self, session_id: &str) -> Vec<ResumeRecord> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT {RESUME_COLUMNS} FROM resume_records WHERE session_id = ?1 \
             ORDER BY created_at ASC, id ASC"
        );
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(params![session_id], row_to_resume)
            .map(|iter| iter.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    /// 某个会话被判定中断的记录，按时间正序
    fn detections_for_session(&self, session_id: &str) -> Vec<DetectionRecord> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT id, session_id, agent_name, verdict, signals, has_active_goal, reason, created_at \
             FROM detection_records WHERE session_id = ?1 ORDER BY created_at ASC, id ASC",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(params![session_id], |row| {
            Ok(DetectionRecord {
                id: row.get(0)?,
                session_id: row.get(1)?,
                agent_name: row.get(2)?,
                verdict: row.get(3)?,
                signals: row.get(4)?,
                has_active_goal: row.get::<_, i32>(5)? != 0,
                reason: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                created_at: row.get(7)?,
            })
        })
        .map(|iter| iter.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    }
}

impl Default for Storage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 攒一条记录，只有真正关心的字段要写
    fn write(storage: &Storage, agent: &str, prompt_type: &str, success: bool, outcome: &str) {
        storage.record_resume(ResumeEvent {
            session_id: "s1",
            agent_name: agent,
            working_dir: "/tmp/proj",
            prompt_type,
            success,
            outcome,
            stuck_secs: None,
            message: "",
        });
    }

    /// 直接写一行**没有** outcome 的记录，模拟 v1.6 之前留下的库
    ///
    /// 不能走 `record_resume`——它现在总会写上 outcome。要测的正是
    /// 「那一列是空串」这种只可能由旧版本产生的行。
    fn write_legacy(storage: &Storage, agent: &str, success: bool) {
        let conn = storage.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO resume_records (session_id, agent_name, working_dir, prompt_type, success, message)
             VALUES ('s0', ?1, '/tmp/old', 'generic', ?2, '')",
            params![agent, success as i32],
        )
        .expect("插入旧记录失败");
    }

    fn page(storage: &Storage, outcome: &str, prompt_type: &str) -> ResumeRecordPage {
        storage.get_resume_page(50, 0, "", outcome, prompt_type)
    }

    /// 问 SQLite 打算怎么执行这条 SQL
    ///
    /// 索引这种东西没法用「结果对不对」来测：加不加索引，返回的行**完全一样**，
    /// 只是慢。所以只能直接问查询计划——`SCAN` 是全表扫，`SEARCH ... USING INDEX`
    /// 才是走了索引。
    fn plan(storage: &Storage, sql: &str) -> String {
        let conn = storage.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .expect("解释查询计划失败");
        let rows: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(3))
            .expect("读查询计划失败")
            .filter_map(|r| r.ok())
            .collect();
        rows.join(" | ")
    }

    #[test]
    fn outcome_survives_a_round_trip() {
        let storage = Storage::in_memory();
        write(&storage, "claude", "goal", false, "silent");

        let records = storage.get_recent_resumes(10);
        assert_eq!(records.len(), 1);
        // 读回来的必须还是那四个键之一，而不是被 success 挤成一个布尔
        assert_eq!(records[0].outcome, "silent");
        assert!(!records[0].success);
    }

    #[test]
    fn each_of_the_four_outcomes_is_filterable() {
        let storage = Storage::in_memory();
        write(&storage, "a", "generic", true, "landed");
        write(&storage, "b", "generic", false, "silent");
        write(&storage, "c", "generic", false, "failed");
        write(&storage, "d", "generic", true, "unverifiable");

        for key in ["landed", "silent", "failed", "unverifiable"] {
            let got = page(&storage, key, "all");
            assert_eq!(got.total, 1, "{key} 应该只筛出一条");
            assert_eq!(got.records.len(), 1, "{key} 的列表和总数对不上");
            assert_eq!(got.records[0].outcome, key);
        }
        assert_eq!(page(&storage, "all", "all").total, 4);
    }

    /// 总数和列表必须走同一份筛选条件
    ///
    /// 这两条 SQL 曾经各写一遍。分开写的时候加筛选项只会改到取页那条，
    /// 于是页脚说「共 20 条」、列表只给 3 条，翻页按钮点下去全是空的。
    #[test]
    fn total_and_list_agree_under_every_filter() {
        let storage = Storage::in_memory();
        for _ in 0..3 {
            write(&storage, "claude", "goal", false, "silent");
        }
        write(&storage, "codex", "generic", true, "landed");

        for outcome in ["all", "landed", "silent", "failed"] {
            for prompt_type in ["all", "goal", "generic"] {
                let got = page(&storage, outcome, prompt_type);
                assert_eq!(
                    got.total as usize,
                    got.records.len(),
                    "outcome={outcome} prompt_type={prompt_type}：总数和列表对不上"
                );
            }
        }
    }

    #[test]
    fn prompt_type_and_outcome_stack_instead_of_replacing_each_other() {
        let storage = Storage::in_memory();
        write(&storage, "a", "goal", false, "silent");
        write(&storage, "b", "generic", false, "silent");

        assert_eq!(page(&storage, "silent", "all").total, 2);
        // 两个条件同时给的时候要是「与」，不是后一个盖掉前一个
        let goal_only = page(&storage, "silent", "goal");
        assert_eq!(goal_only.total, 1);
        assert_eq!(goal_only.records[0].prompt_type, "goal");
    }

    /// 旧记录按 `success` 兜底，但只兜 landed / failed 这两档
    ///
    /// 空串的那些行没经过核验，硬把它们算进 `silent` 或 `unverifiable` 等于
    /// 替历史数据编一个当时不存在的结论。
    #[test]
    fn legacy_rows_fall_back_to_the_success_flag() {
        let storage = Storage::in_memory();
        write_legacy(&storage, "old-ok", true);
        write_legacy(&storage, "old-bad", false);

        assert_eq!(page(&storage, "landed", "all").total, 1);
        assert_eq!(page(&storage, "failed", "all").total, 1);
        // 这两档是核验才能得出的结论，旧行不该混进来
        assert_eq!(page(&storage, "silent", "all").total, 0);
        assert_eq!(page(&storage, "unverifiable", "all").total, 0);
        assert_eq!(page(&storage, "all", "all").total, 2);
    }

    /// 认不出的筛选值当「全部」，而不是筛出空列表
    ///
    /// 前端传错一个值时，多给几条记录是可恢复的；给一个空列表会让人以为
    /// 「真的没有记录」，然后去查一个不存在的问题。
    #[test]
    fn unknown_filters_widen_instead_of_emptying() {
        let storage = Storage::in_memory();
        write(&storage, "a", "goal", true, "landed");

        assert_eq!(page(&storage, "landed_maybe", "all").total, 1);
        assert_eq!(page(&storage, "LANDED", "all").total, 1);
        assert_eq!(page(&storage, "all", "Goal").total, 1);
        assert_eq!(page(&storage, "", "").total, 1);
    }

    #[test]
    fn search_covers_agent_dir_and_message() {
        let storage = Storage::in_memory();
        storage.record_resume(ResumeEvent {
            session_id: "s1",
            agent_name: "claude",
            working_dir: "/home/me/api-server",
            prompt_type: "goal",
            success: false,
            outcome: "failed",
            stuck_secs: None,
            message: "辅助功能没授权",
        });

        for needle in ["claude", "api-server", "辅助功能"] {
            let got = storage.get_resume_page(50, 0, needle, "all", "all");
            assert_eq!(got.total, 1, "搜「{needle}」应该命中");
        }
        assert_eq!(
            storage.get_resume_page(50, 0, "nope", "all", "all").total,
            0
        );
        // 空关键词是「不筛」，不是「搜一个空串」
        assert_eq!(storage.get_resume_page(50, 0, "   ", "all", "all").total, 1);
    }

    /// 翻页时总数是**满足条件的全部**，不是这一页的条数
    #[test]
    fn paging_keeps_the_unpaged_total() {
        let storage = Storage::in_memory();
        for _ in 0..5 {
            write(&storage, "claude", "goal", true, "landed");
        }

        let first = storage.get_resume_page(2, 0, "", "all", "all");
        assert_eq!(first.records.len(), 2);
        assert_eq!(first.total, 5, "总数应该是 5，否则页数会算错");

        let last = storage.get_resume_page(2, 4, "", "all", "all");
        assert_eq!(last.records.len(), 1);
        assert_eq!(last.total, 5);

        // 越界的 offset 给空列表，但总数照旧——页脚不该跟着变成 0
        let past_end = storage.get_resume_page(2, 99, "", "all", "all");
        assert!(past_end.records.is_empty());
        assert_eq!(past_end.total, 5);
    }

    /// 补列是幂等的：已经装过 v1.6 的库再启动一次不该出错
    #[test]
    fn migrate_runs_twice_without_complaining() {
        let storage = Storage::in_memory();
        storage.migrate();
        storage.migrate();
        write(&storage, "a", "goal", true, "landed");
        assert_eq!(storage.get_recent_resumes(10)[0].outcome, "landed");
    }

    // ─────────────────────── v1.7 趋势对比 ───────────────────────

    /// `n` 天前的日期串
    fn day_ago(n: i64) -> String {
        (chrono::Local::now().date_naive() - chrono::Duration::days(n))
            .format("%Y-%m-%d")
            .to_string()
    }

    /// 往某一天插一条续跑
    ///
    /// 时间钉在 `12:00:00`：用 `00:00:00` 的话，一旦哪天把窗口边界从「>=」
    /// 改成「>」，这批测试会全绿着漏过去——边界值恰好是它们的时间戳。
    fn write_at(storage: &Storage, days_ago: i64, success: bool, stuck: Option<i64>) {
        let conn = storage.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO resume_records \
             (session_id, agent_name, working_dir, prompt_type, success, outcome, stuck_secs, message, created_at) \
             VALUES ('s', 'claude', '/tmp', 'generic', ?1, ?2, ?3, '', ?4)",
            params![
                success as i32,
                if success { "landed" } else { "silent" },
                stuck.unwrap_or(-1),
                format!("{} 12:00:00", day_ago(days_ago)),
            ],
        )
        .expect("插入续跑记录失败");
    }

    fn write_detection_at(storage: &Storage, days_ago: i64) {
        let conn = storage.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO detection_records \
             (session_id, agent_name, verdict, signals, has_active_goal, created_at) \
             VALUES ('s', 'claude', 'ConfirmInterrupt', '', 0, ?1)",
            params![format!("{} 12:00:00", day_ago(days_ago))],
        )
        .expect("插入检测记录失败");
    }

    /// 声明「那天应用开着」——趋势的覆盖判断只认这张表
    fn mark_running(storage: &Storage, days_ago: i64) {
        let conn = storage.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO daily_stats (date, total_scans) VALUES (?1, 10) \
             ON CONFLICT(date) DO UPDATE SET total_scans = total_scans + 10",
            params![day_ago(days_ago)],
        )
        .expect("写每日统计失败");
    }

    /// 全新安装：上期不该被读成「一切都是 0」
    ///
    /// 这是整组测试里最要紧的一条。用 0 兼职表示「没数据」的话，第一天打开
    /// 统计页就会看到「成功率比上期跌了 100%」——一句假话，而且正好出现在
    /// 用户还没建立信任的时候。
    #[test]
    fn a_fresh_install_has_no_previous_period_at_all() {
        let storage = Storage::in_memory();
        mark_running(&storage, 0);
        write_at(&storage, 0, true, Some(60));

        let trend = storage.stats_trend(1);
        assert_eq!(trend.resumes.current, Some(1.0));
        assert!(trend.previous_is_missing(), "昨天压根没跑过应用");
        assert_eq!(trend.resumes.previous, None);
        assert_eq!(trend.interruptions.previous, None);
        assert_eq!(trend.landed_rate.previous, None);
    }

    /// 上期开着应用但什么都没发生 → 那 0 是真的 0
    ///
    /// 跟上一条正好构成对照：同样是「上期没有记录」，一个该说「没数据」，
    /// 一个该说「0 次」。区分它们的是那几天应用到底在不在跑。
    #[test]
    fn a_quiet_previous_period_really_is_zero() {
        let storage = Storage::in_memory();
        mark_running(&storage, 0);
        mark_running(&storage, 1);
        write_at(&storage, 0, true, Some(60));

        let trend = storage.stats_trend(1);
        assert_eq!(trend.resumes.previous, Some(0.0), "昨天开着，就是真的 0 次");
        assert_eq!(trend.interruptions.previous, Some(0.0));
        // 但成功率仍然是「没有」——0 次续跑算不出成功率，那是 0/0
        assert_eq!(trend.landed_rate.previous, None);
        assert_eq!(trend.stuck_secs.previous, None);
    }

    /// 今日/昨日的分界就在午夜，不是「往前数 24 小时」
    #[test]
    fn the_window_cuts_on_calendar_days() {
        let storage = Storage::in_memory();
        for d in 0..=1 {
            mark_running(&storage, d);
        }
        write_at(&storage, 0, true, None);
        write_at(&storage, 0, true, None);
        write_at(&storage, 1, false, None);

        let trend = storage.stats_trend(1);
        assert_eq!(trend.resumes.current, Some(2.0));
        assert_eq!(trend.resumes.previous, Some(1.0));
    }

    /// 7 天窗口：前 7 天是第 8–14 天，不能跟本期重叠、也不能漏掉第 14 天
    #[test]
    fn the_seven_day_window_tiles_without_overlap_or_gap() {
        let storage = Storage::in_memory();
        for d in 0..20 {
            mark_running(&storage, d);
            write_at(&storage, d, true, None);
        }

        let trend = storage.stats_trend(7);
        // 第 0–6 天共 7 条
        assert_eq!(trend.resumes.current, Some(7.0));
        // 第 7–13 天共 7 条；数成 6 或 8 就说明边界差了一天
        assert_eq!(trend.resumes.previous, Some(7.0));
    }

    /// 成功率算的是本期自己的比例，不是拿累计数除
    #[test]
    fn landed_rate_is_per_period() {
        let storage = Storage::in_memory();
        for d in 0..=1 {
            mark_running(&storage, d);
        }
        // 今天 3 条里 2 条落地；昨天 2 条里 0 条落地
        write_at(&storage, 0, true, None);
        write_at(&storage, 0, true, None);
        write_at(&storage, 0, false, None);
        write_at(&storage, 1, false, None);
        write_at(&storage, 1, false, None);

        let trend = storage.stats_trend(1);
        let rate = trend.landed_rate.current.expect("今天有 3 条，算得出来");
        assert!((rate - 66.666).abs() < 0.01, "应该是 2/3，实际 {rate}");
        assert_eq!(trend.landed_rate.previous, Some(0.0), "昨天 0/2 是真的 0%");
    }

    /// 不知道卡了多久的那些行，不能被当成「卡了 0 秒」拉平均
    ///
    /// 老记录和没有记录文件的 agent 都会留下 -1。把它们算进去，「平均卡了
    /// 20 分钟」会被稀释成几分钟——而这个数正是用来判断守护够不够快的。
    #[test]
    fn unknown_stuck_durations_dont_dilute_the_average() {
        let storage = Storage::in_memory();
        mark_running(&storage, 0);
        write_at(&storage, 0, true, Some(600));
        write_at(&storage, 0, true, Some(1200));
        write_at(&storage, 0, true, None); // 算不出来的那种
        write_at(&storage, 0, true, None);

        let trend = storage.stats_trend(1);
        assert_eq!(
            trend.stuck_secs.current,
            Some(900.0),
            "该是 (600+1200)/2，不是 /4"
        );
        // 但它们仍然算续跑次数——为了算平均值加的过滤，不该顺手改掉别的指标
        assert_eq!(trend.resumes.current, Some(4.0));
    }

    /// 一条 stuck 都算不出来时，说「没有」而不是 0 秒
    #[test]
    fn no_measurable_stuck_row_means_no_answer() {
        let storage = Storage::in_memory();
        mark_running(&storage, 0);
        write_at(&storage, 0, true, None);

        assert_eq!(storage.stats_trend(1).stuck_secs.current, None);
    }

    /// 中断次数数的是 `detection_records` 的行数
    #[test]
    fn interruptions_count_detection_events() {
        let storage = Storage::in_memory();
        for d in 0..=1 {
            mark_running(&storage, d);
        }
        write_detection_at(&storage, 0);
        write_detection_at(&storage, 0);
        write_detection_at(&storage, 1);

        let trend = storage.stats_trend(1);
        assert_eq!(trend.interruptions.current, Some(2.0));
        assert_eq!(trend.interruptions.previous, Some(1.0));
    }

    /// 窗口天数被夹住，不会因为前端传个 0 或者 9999 就查穿
    #[test]
    fn the_window_length_is_clamped() {
        let storage = Storage::in_memory();
        assert_eq!(storage.stats_trend(0).window_days, 1);
        assert_eq!(storage.stats_trend(9999).window_days, 90);
        assert_eq!(storage.stats_trend(7).window_days, 7);
    }

    /// 老库（没有 stuck_secs 列）补列后读出来是 -1，不是 0
    #[test]
    fn legacy_rows_report_an_unknown_stuck_duration() {
        let storage = Storage::in_memory();
        write_legacy(&storage, "claude", true);

        let records = storage.get_recent_resumes(10);
        assert_eq!(records[0].stuck_secs, -1, "旧行是「不知道」，不是「0 秒」");
    }

    // ── 会话生命周期：「关掉的会话还显示运行中」那个 bug ──

    /// 写一条会话历史，只有关心的字段要填
    fn see(storage: &Storage, key: &str, status: &str) {
        storage.upsert_session_history(
            key,
            "sess-1",
            "claude",
            "/tmp/proj",
            key,
            "ttys001",
            "iTerm2",
            status,
            0,
            0,
            0.0,
        );
    }

    fn entry_of(storage: &Storage, key: &str) -> SessionHistoryEntry {
        storage
            .session_history(50, "")
            .into_iter()
            .find(|e| e.session_key == key)
            .unwrap_or_else(|| panic!("{key} 那一行没了"))
    }

    #[test]
    fn a_session_that_vanished_stops_being_live() {
        let storage = Storage::in_memory();
        see(&storage, "/tmp/a.jsonl", "active");
        assert!(entry_of(&storage, "/tmp/a.jsonl").is_live());

        // 用户把它关了：下一轮扫描的活键里没有它
        storage.close_missing_sessions(&[]);

        let entry = entry_of(&storage, "/tmp/a.jsonl");
        assert!(!entry.is_live(), "关掉的会话必须不再算「还在」");
        assert!(!entry.ended_at.is_empty(), "得留下什么时候没的");
        // 最后一眼看到的状态是历史，不该被追改成 exited
        assert_eq!(entry.last_status, "active", "最后一眼的状态是历史，不能改");
    }

    #[test]
    fn the_ones_still_here_are_left_alone() {
        let storage = Storage::in_memory();
        see(&storage, "/tmp/a.jsonl", "active");
        see(&storage, "/tmp/b.jsonl", "active");

        storage.close_missing_sessions(&["/tmp/a.jsonl".to_string()]);

        assert!(entry_of(&storage, "/tmp/a.jsonl").is_live(), "a 还开着");
        assert!(!entry_of(&storage, "/tmp/b.jsonl").is_live(), "b 已经关了");
    }

    /// `claude --resume` 会接着写同一个记录文件，也就是同一个 `session_key`
    #[test]
    fn a_revived_session_is_live_again() {
        let storage = Storage::in_memory();
        see(&storage, "/tmp/a.jsonl", "active");
        storage.close_missing_sessions(&[]);
        assert!(!entry_of(&storage, "/tmp/a.jsonl").is_live());

        see(&storage, "/tmp/a.jsonl", "active");

        let entry = entry_of(&storage, "/tmp/a.jsonl");
        assert!(entry.is_live(), "又看见它了，「已结束」这句话就得收回");
        assert!(entry.ended_at.is_empty());
    }

    /// 收尾时间只写一次
    ///
    /// 每轮扫描都往后推的话，「什么时候结束的」永远是「刚刚」——
    /// 一个上周关掉的会话会显示成一分钟前才结束。
    #[test]
    fn the_end_timestamp_is_not_rewritten_every_scan() {
        let storage = Storage::in_memory();
        see(&storage, "/tmp/a.jsonl", "active");
        storage.close_missing_sessions(&[]);
        let first = entry_of(&storage, "/tmp/a.jsonl").ended_at;

        // 手工把它改早，模拟「上周就结束了」
        {
            let conn = storage.conn.lock().unwrap();
            conn.execute(
                "UPDATE session_history SET ended_at = '2020-01-01 00:00:00'",
                [],
            )
            .expect("改不动库");
        }
        storage.close_missing_sessions(&[]);

        let again = entry_of(&storage, "/tmp/a.jsonl").ended_at;
        assert_eq!(again, "2020-01-01 00:00:00", "已经收过尾的不该被重写");
        assert!(!first.is_empty());
    }

    #[test]
    fn live_and_ended_are_filterable() {
        let storage = Storage::in_memory();
        see(&storage, "/tmp/a.jsonl", "active");
        see(&storage, "/tmp/b.jsonl", "active");
        storage.close_missing_sessions(&["/tmp/a.jsonl".to_string()]);

        let live = storage.get_session_history_page(50, 0, "", "live");
        assert_eq!(live.total, 1);
        assert_eq!(live.entries[0].session_key, "/tmp/a.jsonl");

        let ended = storage.get_session_history_page(50, 0, "", "ended");
        assert_eq!(ended.total, 1);
        assert_eq!(ended.entries[0].session_key, "/tmp/b.jsonl");

        assert_eq!(storage.get_session_history_page(50, 0, "", "all").total, 2);
        // 拼错的筛选值放宽成 all，不能返回空
        assert_eq!(storage.get_session_history_page(50, 0, "", "wat").total, 2);
    }

    /// 还活着的排在前面，别被昨天的记录挤下去
    #[test]
    fn live_sessions_sort_first() {
        let storage = Storage::in_memory();
        see(&storage, "/tmp/old.jsonl", "active");
        see(&storage, "/tmp/new.jsonl", "active");
        // old 关掉，但把它的 last_seen 推到未来，让它在纯时间排序里稳赢
        storage.close_missing_sessions(&["/tmp/new.jsonl".to_string()]);
        {
            let conn = storage.conn.lock().unwrap();
            conn.execute(
                "UPDATE session_history SET last_seen = '2999-01-01 00:00:00' \
                 WHERE session_key = '/tmp/old.jsonl'",
                [],
            )
            .expect("改不动库");
        }

        let page = storage.get_session_history_page(50, 0, "", "all");
        assert_eq!(
            page.entries[0].session_key, "/tmp/new.jsonl",
            "还开着的会话得排最前面"
        );
    }

    #[test]
    fn the_summary_counts_everything_not_just_this_page() {
        let storage = Storage::in_memory();
        for i in 0..5 {
            storage.upsert_session_history(
                &format!("/tmp/{i}.jsonl"),
                "sess-1",
                "claude",
                "/tmp/proj",
                "",
                "",
                "",
                "active",
                2,
                100,
                0.5,
            );
        }
        storage.close_missing_sessions(&["/tmp/0.jsonl".to_string()]);

        // 只取一页两条，汇总仍要说 5
        let page = storage.get_session_history_page(2, 0, "", "all");
        assert_eq!(page.entries.len(), 2);

        let summary = storage.session_history_summary("");
        assert_eq!(summary.total, 5);
        assert_eq!(summary.live, 1, "只剩一个还开着");
        assert_eq!(summary.resumes, 10, "5 个会话各续跑 2 次");
        assert!((summary.cost_usd - 2.5).abs() < 1e-9);
        assert_eq!(summary.total_tokens, 500);
    }

    /// 汇总要跟着搜索条件走
    #[test]
    fn the_summary_follows_the_search_box() {
        let storage = Storage::in_memory();
        storage.upsert_session_history(
            "/tmp/a.jsonl",
            "s1",
            "claude",
            "/tmp/alpha",
            "",
            "",
            "",
            "active",
            0,
            0,
            0.0,
        );
        storage.upsert_session_history(
            "/tmp/b.jsonl",
            "s2",
            "codex",
            "/tmp/beta",
            "",
            "",
            "",
            "active",
            0,
            0,
            0.0,
        );

        assert_eq!(storage.session_history_summary("").total, 2);
        assert_eq!(storage.session_history_summary("alpha").total, 1);
        assert_eq!(storage.session_history_summary("codex").total, 1);
    }

    #[test]
    fn a_session_detail_gathers_its_resumes_and_detections() {
        let storage = Storage::in_memory();
        see(&storage, "/tmp/a.jsonl", "interrupted");
        // 三张表靠 `session_id` 对上，`see` 写的是 sess-1
        storage.record_detection(
            "sess-1",
            "claude",
            "ConfirmInterrupt",
            "沉默 5 分钟",
            true,
            "rate_limit",
        );
        storage.record_resume(ResumeEvent {
            session_id: "sess-1",
            agent_name: "claude",
            working_dir: "/tmp/proj",
            prompt_type: "goal",
            success: false,
            outcome: "silent",
            stuck_secs: Some(300),
            message: "敲进去了但没反应",
        });

        let detail = storage.session_detail("/tmp/a.jsonl").expect("档案没了");
        assert_eq!(detail.entry.session_key, "/tmp/a.jsonl");
        assert_eq!(detail.resumes.len(), 1);
        assert_eq!(detail.resumes[0].outcome, "silent");
        assert_eq!(detail.detections.len(), 1);
        // v1.6 就在写这一列了，可一直没有查询读它
        assert_eq!(
            detail.detections[0].reason, "rate_limit",
            "「为什么停」得读出来"
        );
    }

    #[test]
    fn an_unknown_session_has_no_detail() {
        let storage = Storage::in_memory();
        assert!(storage.session_detail("/tmp/nope.jsonl").is_none());
    }

    // ── 老库修补：同一个会话在历史里裂成好多行 ──

    /// 造一行旧格式的历史（主键里带重启时间，这是 v1.7 之前的形状）
    #[allow(clippy::too_many_arguments)]
    fn write_legacy_history(
        storage: &Storage,
        key: &str,
        session_id: &str,
        working_dir: &str,
        first_seen: &str,
        last_seen: &str,
        resume_count: u32,
        tokens: i64,
        cost: f64,
    ) {
        let conn = storage.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO session_history
               (session_key, session_id, agent_name, working_dir, session_file, tty, terminal_app,
                first_seen, last_seen, last_status, ended_at, resume_count, total_tokens, cost_usd)
             VALUES (?1, ?2, 'claude', ?3, '', '', '', ?4, ?5, 'active', '', ?6, ?7, ?8)",
            params![
                key,
                session_id,
                working_dir,
                first_seen,
                last_seen,
                resume_count,
                tokens,
                cost
            ],
        )
        .expect("插入旧历史失败");
    }

    #[test]
    fn a_session_split_across_restarts_is_merged_back() {
        let storage = Storage::in_memory();
        // 同一个会话，三次重启生出三行（真实库里最多见到 16 行）
        write_legacy_history(
            &storage,
            "claude-code-68590-2026-07-31 00:08:47",
            "cc-68590",
            "/tmp/proj",
            "2026-07-31 00:08:47",
            "2026-07-31 00:12:27",
            1,
            100,
            0.1,
        );
        write_legacy_history(
            &storage,
            "claude-code-68590-2026-07-31 00:12:46",
            "cc-68590",
            "/tmp/proj",
            "2026-07-31 00:12:46",
            "2026-07-31 00:20:00",
            3,
            500,
            0.9,
        );
        write_legacy_history(
            &storage,
            "claude-code-68590-2026-07-31 00:21:03",
            "cc-68590",
            "/tmp/proj",
            "2026-07-31 00:21:03",
            "2026-07-31 00:25:00",
            2,
            300,
            0.4,
        );

        storage.merge_fragmented_history();

        let rows = storage.session_history(50, "");
        assert_eq!(rows.len(), 1, "三行该并成一行");
        let row = &rows[0];
        // 聚合值一个都不能丢
        assert_eq!(
            row.first_seen, "2026-07-31 00:08:47",
            "最早那次才是首次见到"
        );
        assert_eq!(row.last_seen, "2026-07-31 00:25:00", "最后一次是最晚那个");
        assert_eq!(row.resume_count, 3, "续跑次数取最大值");
        assert_eq!(row.total_tokens, 500);
        assert!((row.cost_usd - 0.9).abs() < 1e-9);
    }

    #[test]
    fn merging_is_idempotent() {
        let storage = Storage::in_memory();
        for (i, seen) in ["00:08:47", "00:12:46"].iter().enumerate() {
            write_legacy_history(
                &storage,
                &format!("claude-code-68590-2026-07-31 {seen}"),
                "cc-68590",
                "/tmp/proj",
                &format!("2026-07-31 {seen}"),
                &format!("2026-07-31 {seen}"),
                i as u32,
                0,
                0.0,
            );
        }
        storage.merge_fragmented_history();
        let after_once = storage.session_history(50, "");
        storage.merge_fragmented_history();
        let after_twice = storage.session_history(50, "");

        assert_eq!(after_once.len(), 1);
        assert_eq!(after_twice.len(), 1, "再跑一次得是空操作");
        assert_eq!(after_once[0].session_key, after_twice[0].session_key);
    }

    /// 不同会话不能被并到一起
    #[test]
    fn merging_never_collapses_two_real_sessions() {
        let storage = Storage::in_memory();
        // 同一个 pid 前缀但工作目录不同 = 两个会话
        write_legacy_history(
            &storage,
            "k1",
            "cc-1",
            "/tmp/alpha",
            "2026-07-31 00:00:00",
            "2026-07-31 00:00:00",
            0,
            0,
            0.0,
        );
        write_legacy_history(
            &storage,
            "k2",
            "cc-1",
            "/tmp/beta",
            "2026-07-31 00:00:00",
            "2026-07-31 00:00:00",
            0,
            0,
            0.0,
        );
        // 同一个目录但不同会话
        write_legacy_history(
            &storage,
            "k3",
            "cc-2",
            "/tmp/alpha",
            "2026-07-31 00:00:00",
            "2026-07-31 00:00:00",
            0,
            0,
            0.0,
        );

        storage.merge_fragmented_history();

        assert_eq!(storage.session_history(50, "").len(), 3, "这是三个会话");
    }

    /// 用记录文件当主键的行从来没有这个毛病，不该被碰
    #[test]
    fn rows_keyed_by_transcript_are_left_alone() {
        let storage = Storage::in_memory();
        see(&storage, "/tmp/a.jsonl", "active");
        see(&storage, "/tmp/b.jsonl", "active");
        // 这两行 session_id 相同、工作目录相同，只有记录文件不同——
        // 那是两份真实记录，合并逻辑必须绕开它们
        storage.merge_fragmented_history();
        assert_eq!(storage.session_history(50, "").len(), 2);
    }

    /// 抽屉那两个查询必须走索引，而且必须是**这条**索引
    ///
    /// 这两个断言防的是同一件事的两个方向：索引被人删掉（退回全表扫描），
    /// 或者查询的 `WHERE` / `ORDER BY` 被改成索引盖不住的形状。两种情况
    /// 都不会让任何别的测试变红——行数、内容、顺序全都一样，只是慢，
    /// 而且要等用户攒够数据才慢。
    #[test]
    fn the_session_drawer_queries_use_their_index() {
        let storage = Storage::in_memory();

        let resume_plan = plan(
            &storage,
            &format!(
                "SELECT {RESUME_COLUMNS} FROM resume_records WHERE session_id = 'x' \
                 ORDER BY created_at ASC, id ASC"
            ),
        );
        assert!(
            resume_plan.contains("USING INDEX idx_resume_session"),
            "续跑记录没走 session 索引，实际计划：{resume_plan}"
        );

        let detection_plan = plan(
            &storage,
            "SELECT id, session_id, agent_name, verdict, signals, has_active_goal, reason, created_at \
             FROM detection_records WHERE session_id = 'x' ORDER BY created_at ASC, id ASC",
        );
        assert!(
            detection_plan.contains("USING INDEX idx_detection_session"),
            "检测记录没走 session 索引，实际计划：{detection_plan}"
        );
    }

    /// 复合索引的第二列换来的是「不用再排一次」
    ///
    /// 只索引 `session_id` 也能让上面那条测试通过，但查询计划里会多一个
    /// `USE TEMP B-TREE FOR ORDER BY`——SQLite 得把取出来的行再排一遍。
    /// 把 `created_at` 放进索引第二列之后这一步消失了，所以单独钉住它，
    /// 否则以后有人把索引简化成单列，白掉的那部分没人会发现。
    #[test]
    fn the_drawer_queries_do_not_sort_afterwards() {
        let storage = Storage::in_memory();
        let resume_plan = plan(
            &storage,
            &format!(
                "SELECT {RESUME_COLUMNS} FROM resume_records WHERE session_id = 'x' \
                 ORDER BY created_at ASC, id ASC"
            ),
        );
        assert!(
            !resume_plan.contains("TEMP B-TREE"),
            "还在事后排序，说明排序键没进索引：{resume_plan}"
        );
    }
}
