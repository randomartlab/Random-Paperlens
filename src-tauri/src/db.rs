use rusqlite::Connection;
use std::path::Path;

/// 数据库迁移：以 PRAGMA user_version 管理 schema 版本。
fn migrate(conn: &Connection) -> Result<(), rusqlite::Error> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;

    if version < 1 {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS documents (
                id          TEXT PRIMARY KEY,
                title       TEXT NOT NULL,
                authors     TEXT,
                year        INTEGER,
                journal     TEXT,
                tags        TEXT,
                file_path   TEXT NOT NULL,
                status      TEXT NOT NULL DEFAULT 'pending',
                language    TEXT,
                created_at  TEXT NOT NULL,
                updated_at  TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS tasks (
                id          TEXT PRIMARY KEY,
                doc_id      TEXT NOT NULL,
                type        TEXT NOT NULL,
                status      TEXT NOT NULL DEFAULT 'pending',
                progress    REAL NOT NULL DEFAULT 0,
                stage       TEXT,
                error       TEXT,
                created_at  TEXT NOT NULL,
                FOREIGN KEY (doc_id) REFERENCES documents(id)
            );

            CREATE TABLE IF NOT EXISTS api_configs (
                id          TEXT PRIMARY KEY,
                name        TEXT NOT NULL,
                base_url    TEXT NOT NULL,
                model       TEXT,
                params      TEXT,
                key_ref     TEXT,
                is_default  INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS digest_versions (
                id           TEXT PRIMARY KEY,
                doc_id       TEXT NOT NULL,
                version      INTEGER NOT NULL,
                field_schema TEXT,
                content      TEXT,
                score        REAL,
                created_at   TEXT NOT NULL,
                FOREIGN KEY (doc_id) REFERENCES documents(id)
            );

            CREATE TABLE IF NOT EXISTS term_glossary (
                id          TEXT PRIMARY KEY,
                term        TEXT NOT NULL,
                translation TEXT,
                language    TEXT,
                created_at  TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_tasks_doc ON tasks(doc_id);
            CREATE INDEX IF NOT EXISTS idx_versions_doc ON digest_versions(doc_id);

            PRAGMA user_version = 1;
            "#,
        )?;
    }

    if version < 2 {
        // api_configs 增加 created_at（排序/展示用；旧库升级与新建库均适用）
        conn.execute_batch(
            "ALTER TABLE api_configs ADD COLUMN created_at TEXT;
             PRAGMA user_version = 2;",
        )?;
    }

    if version < 3 {
        // settings 键值表：视觉模型（外挂图片分析）等全局配置
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            PRAGMA user_version = 3;",
        )?;
    }

    if version < 4 {
        // 阅读状态标记（F7）：unread 未读完 / read 已读完，由用户手动维护
        conn.execute_batch(
            "ALTER TABLE documents ADD COLUMN read_status TEXT NOT NULL DEFAULT 'unread';
             PRAGMA user_version = 4;",
        )?;
    }

    if version < 5 {
        // 任务中心：补充更新时间、详情字段，并为状态/时间查询建立索引
        conn.execute_batch(
            "ALTER TABLE tasks ADD COLUMN updated_at TEXT;
             ALTER TABLE tasks ADD COLUMN detail TEXT;
             CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
             CREATE INDEX IF NOT EXISTS idx_tasks_created ON tasks(created_at DESC);
             PRAGMA user_version = 5;",
        )?;
    }

    Ok(())
}

/// 打开（或创建）数据库并执行迁移。返回连接供全局状态持有。
pub fn init_db(db_path: &Path) -> Result<Connection, rusqlite::Error> {
    let conn = Connection::open(db_path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;",
    )?;
    migrate(&conn)?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    #[test]
    fn init_creates_schema() {
        let path = temp_dir().join(format!("litdesk_test_{}.db", uuid::Uuid::new_v4()));
        let conn = init_db(&path).expect("init db");
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 5);

        // 验证表存在
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        for t in [
            "documents",
            "tasks",
            "api_configs",
            "digest_versions",
            "term_glossary",
        ] {
            assert!(tables.iter().any(|x| x == t), "missing table {t}");
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn migrate_is_idempotent() {
        let path = temp_dir().join(format!("litdesk_test_{}.db", uuid::Uuid::new_v4()));
        init_db(&path).expect("first init");
        init_db(&path).expect("second init (idempotent)");
        let _ = std::fs::remove_file(path);
    }
}
