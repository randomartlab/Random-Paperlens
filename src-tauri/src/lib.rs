mod db;

use rusqlite::Connection;
use serde_json::json;
use std::sync::Mutex;
use tauri::Manager;

/// 全局应用状态：数据库连接（互斥保护，供多命令共享）
struct AppState {
    conn: Mutex<Connection>,
}

/// 文献列表查询（M1 骨架，后续扩展筛选/分页）
#[tauri::command]
fn list_documents(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, title, authors, year, journal, tags, file_path, status, language, created_at
             FROM documents ORDER BY created_at DESC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([], |r| {
            Ok(json!({
                "id": r.get::<_, String>(0)?,
                "title": r.get::<_, String>(1)?,
                "authors": r.get::<_, Option<String>>(2)?,
                "year": r.get::<_, Option<i64>>(3)?,
                "journal": r.get::<_, Option<String>>(4)?,
                "tags": r.get::<_, Option<String>>(5)?,
                "file_path": r.get::<_, String>(6)?,
                "status": r.get::<_, String>(7)?,
                "language": r.get::<_, Option<String>>(8)?,
                "created_at": r.get::<_, String>(9)?,
            }))
        })
        .map_err(|e| e.to_string())?;

    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // 初始化数据库：~/Library/Application Support/文献阅读台/library.db
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("library.db");
            let conn = db::init_db(&db_path).expect("failed to init database");
            app.manage(AppState {
                conn: Mutex::new(conn),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![list_documents])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
