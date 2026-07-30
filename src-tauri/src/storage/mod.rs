use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyStats {
    pub date: String,
    pub total_scans: u32,
    pub total_detections: u32,
    pub total_resumes: u32,
    pub successful_resumes: u32,
    pub failed_resumes: u32,
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
            CREATE INDEX IF NOT EXISTS idx_detection_created ON detection_records(created_at);",
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
    pub fn get_stats(&self, days: u32) -> Vec<DailyStats> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT date, total_scans, total_detections, total_resumes, successful_resumes, failed_resumes
                 FROM daily_stats ORDER BY date DESC LIMIT ?1",
            )
            .unwrap();

        stmt.query_map(params![days], |row| {
            Ok(DailyStats {
                date: row.get(0)?,
                total_scans: row.get(1)?,
                total_detections: row.get(2)?,
                total_resumes: row.get(3)?,
                successful_resumes: row.get(4)?,
                failed_resumes: row.get(5)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    /// 获取最近的续跑记录
    pub fn get_recent_resumes(&self, limit: u32) -> Vec<ResumeRecord> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, session_id, agent_name, working_dir, prompt_type, success, message, created_at
                 FROM resume_records ORDER BY id DESC LIMIT ?1",
            )
            .unwrap();

        stmt.query_map(params![limit], |row| {
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
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
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
}

impl Default for Storage {
    fn default() -> Self {
        Self::new()
    }
}
