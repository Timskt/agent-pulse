use crate::cost::{DailyCost, ProjectCost, UsageEntry, UsageSnapshot};
use rusqlite::{Connection, params};
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
    pub resume_count: u32,
    pub total_tokens: u64,
    pub cost_usd: f64,
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
                resume_count INTEGER DEFAULT 0,
                total_tokens INTEGER DEFAULT 0,
                cost_usd REAL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_history_last_seen ON session_history(last_seen);",
        );
    }

    /// 记录一次续跑事件
    pub fn record_resume(
        &self,
        session_id: &str,
        agent_name: &str,
        working_dir: &str,
        prompt_type: &str,
        success: bool,
        message: &str,
    ) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO resume_records (session_id, agent_name, working_dir, prompt_type, success, message)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![session_id, agent_name, working_dir, prompt_type, success as i32, message],
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
            params![today, success as i32, (!success) as i32],
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
    ) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO detection_records (session_id, agent_name, verdict, signals, has_active_goal)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session_id, agent_name, verdict, signals, has_active_goal as i32],
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
        let Ok(mut stmt) = conn.prepare(
            "SELECT id, session_id, agent_name, working_dir, prompt_type, success, message, created_at
                 FROM resume_records ORDER BY id DESC LIMIT ?1",
        ) else {
            return Vec::new();
        };

        let rows = stmt.query_map(params![limit], |row| {
            Ok(ResumeRecord {
                id: row.get(0)?,
                session_id: row.get(1)?,
                agent_name: row.get(2)?,
                working_dir: row.get(3)?,
                prompt_type: row.get(4)?,
                success: row.get::<_, i32>(5)? != 0,
                message: row.get(6)?,
                created_at: row.get(7)?,
            })
        });

        match rows {
            Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
            Err(e) => {
                tracing::warn!("[Storage] 读取续跑记录失败: {e}");
                Vec::new()
            }
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
            .query_row("SELECT COUNT(*) FROM resume_records WHERE success = 1", [], |r| r.get(0))
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
                first_seen, last_seen, last_status, resume_count, total_tokens, cost_usd)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(session_key) DO UPDATE SET
                session_id = ?2, working_dir = ?4, session_file = ?5,
                tty = ?6, terminal_app = ?7, last_seen = ?8, last_status = ?9,
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

    /// 查询会话历史（关键字为空则返回最近的）
    pub fn session_history(&self, limit: u32, query: &str) -> Vec<SessionHistoryEntry> {
        let conn = self.conn.lock().unwrap();
        let like = format!("%{}%", query.trim());
        let sql = "SELECT session_key, session_id, agent_name, working_dir, session_file, tty,
                          terminal_app, first_seen, last_seen, last_status, resume_count,
                          total_tokens, cost_usd
                   FROM session_history
                   WHERE (?2 = '' OR working_dir LIKE ?1 OR agent_name LIKE ?1 OR session_file LIKE ?1)
                   ORDER BY last_seen DESC LIMIT ?3";
        let mut stmt = match conn.prepare(sql) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(params![like, query.trim(), limit], |row| {
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
                resume_count: row.get(10)?,
                total_tokens: row.get::<_, i64>(11)? as u64,
                cost_usd: row.get(12)?,
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
