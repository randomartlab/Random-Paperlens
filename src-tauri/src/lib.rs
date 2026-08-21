mod db;
mod mineru;

use mineru::MinerUClient;
use rusqlite::Connection;
use serde::Serialize;
use serde_json::json;
use std::path::Path;
use std::sync::Mutex;
use tauri::Emitter;
use tauri::Manager;

/// 全局应用状态：数据库连接（互斥保护，供多命令共享）
struct AppState {
    conn: Mutex<Connection>,
    mineru: Option<MinerUClient>,
}

/// 从候选路径或环境变量加载 MINERU_API_KEY（.env 支持：KEY=VALUE 行）
fn load_mineru_key(app: &tauri::AppHandle) -> Option<String> {
    // 1. 环境变量优先
    if let Ok(k) = std::env::var("MINERU_API_KEY") {
        if !k.is_empty() {
            return Some(k);
        }
    }
    // 2. 候选 .env 路径：应用配置目录 / 当前目录（src-tauri）/ 项目根（父目录）
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(dir) = app.path().app_config_dir() {
        candidates.push(dir.join(".env"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(".env"));
        candidates.push(cwd.join("../.env"));
    }
    for path in candidates {
        if let Ok(content) = std::fs::read_to_string(&path) {
            for line in content.lines() {
                let line = line.trim();
                if let Some(v) = line.strip_prefix("MINERU_API_KEY=") {
                    let v = v.trim().trim_matches('"').trim_matches('\'');
                    if !v.is_empty() {
                        return Some(v.to_string());
                    }
                }
            }
        }
    }
    None
}

/// 文献条目（与前端共享的结构）
#[derive(Serialize)]
struct Document {
    id: String,
    title: String,
    authors: Option<String>,
    year: Option<i64>,
    journal: Option<String>,
    tags: Option<String>,
    file_path: String,
    status: String,
    language: Option<String>,
    created_at: String,
}

/// PDF 导入：校验 → 去重 → 落盘 → 入库
#[tauri::command]
fn import_document(
    path: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Document, String> {
    // 1. 扩展名校验
    let ext = std::path::Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if ext != "pdf" {
        return Err("仅支持 PDF 文件".into());
    }

    // 2. 损坏文件拦截（PDF magic number）
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    if bytes.len() < 5 || &bytes[..5] != b"%PDF-" {
        return Err("文件损坏：不是有效的 PDF".into());
    }

    let conn = state.conn.lock().map_err(|e| e.to_string())?;

    // 3. 重复导入检测（按真实路径）
    let canon = std::fs::canonicalize(&path).map_err(|e| e.to_string())?;
    let canon_str = canon.to_str().unwrap_or("").to_string();
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM documents WHERE file_path = ?1)",
            [&canon_str],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if exists {
        return Err("该 PDF 已导入过，是否覆盖？".into());
    }

    // 4. 复制到应用数据目录 documents/{id}/original.pdf
    let id = uuid::Uuid::new_v4().to_string();
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let doc_dir = data_dir.join("documents").join(&id);
    std::fs::create_dir_all(&doc_dir).map_err(|e| e.to_string())?;
    let target = doc_dir.join("original.pdf");
    std::fs::copy(&path, &target).map_err(|e| e.to_string())?;
    let target_str = target.to_str().unwrap_or("").to_string();

    // 5. 入库
    let now = chrono::Utc::now().to_rfc3339();
    let title = std::path::Path::new(&path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("未命名")
        .to_string();
    conn.execute(
        "INSERT INTO documents (id, title, file_path, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'pending', ?4, ?4)",
        rusqlite::params![id, title, target_str, now],
    )
    .map_err(|e| e.to_string())?;

    Ok(Document {
        id,
        title,
        authors: None,
        year: None,
        journal: None,
        tags: None,
        file_path: target_str,
        status: "pending".into(),
        language: None,
        created_at: now,
    })
}

/// 启动文献解析任务（异步）：后台线程执行 MinerU 上传/轮询/下载，进度与结果通过事件推送
#[tauri::command]
fn start_parse(
    doc_id: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let mineru = state
        .mineru
        .as_ref()
        .ok_or("MinerU API Key 未配置（请检查项目 .env 或环境变量）")?
        .clone();
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;

    // 1. 取文献源文件路径（快速，短暂持锁）
    let source_path: String = {
        let conn = state.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT file_path FROM documents WHERE id = ?1",
            [&doc_id],
            |r| r.get(0),
        )
        .map_err(|e| format!("文献不存在: {e}"))?
    };

    // 2. 记录解析任务（running）
    let task_id = uuid::Uuid::new_v4().to_string();
    {
        let conn = state.conn.lock().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO tasks (id, doc_id, type, status, created_at) VALUES (?1, ?2, 'parse', 'running', ?3)",
            rusqlite::params![task_id, doc_id, now],
        )
        .map_err(|e| e.to_string())?;
    }

    // 3. 后台线程执行解析，事件推送进度与结果
    let app2 = app.clone();
    std::thread::spawn(move || {
        let emit = |event: &str, payload: serde_json::Value| {
            let _ = app2.emit(event, payload);
        };
        emit(
            "parse-progress",
            json!({ "doc_id": doc_id, "stage": "上传中", "progress": 0.05 }),
        );

        let result = (|| -> Result<String, String> {
            let batch_id = mineru.submit_file(Path::new(&source_path))?;
            emit(
                "parse-progress",
                json!({ "doc_id": doc_id, "stage": "解析中", "progress": 0.4 }),
            );
            let r = mineru.poll_batch(&batch_id, 600, Some(&|p: &str| {
                emit(
                    "parse-progress",
                    json!({ "doc_id": doc_id, "stage": "解析中", "progress": 0.6, "detail": p }),
                );
            }))?;
            let dest = data_dir.join("documents").join(&doc_id).join("parsed");
            let md = mineru.download_extract(&r, &dest)?;
            emit(
                "parse-progress",
                json!({ "doc_id": doc_id, "stage": "完成", "progress": 1.0 }),
            );
            Ok(md.to_string_lossy().to_string())
        })();

        // 4. 回写状态
        let state = app2.state::<AppState>();
        let conn = state.conn.lock();
        match (result, conn) {
            (Ok(md), Ok(conn)) => {
                let now = chrono::Utc::now().to_rfc3339();
                let _ = conn.execute(
                    "UPDATE documents SET status = 'parsed', updated_at = ?1 WHERE id = ?2",
                    rusqlite::params![now, doc_id],
                );
                let _ = conn.execute(
                    "UPDATE tasks SET status = 'done', stage = 'parsed' WHERE id = ?1",
                    rusqlite::params![task_id],
                );
                drop(conn);
                emit("parse-done", json!({ "doc_id": doc_id, "md_path": md }));
            }
            (Err(e), Ok(conn)) => {
                let now = chrono::Utc::now().to_rfc3339();
                let _ = conn.execute(
                    "UPDATE tasks SET status = 'failed', error = ?1, updated_at = ?2 WHERE id = ?3",
                    rusqlite::params![e, now, task_id],
                );
                drop(conn);
                emit("parse-failed", json!({ "doc_id": doc_id, "error": e }));
            }
            (_, Err(e)) => {
                emit(
                    "parse-failed",
                    json!({ "doc_id": doc_id, "error": format!("数据库访问失败: {e}") }),
                );
            }
        };
    });

    Ok(())
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
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // 初始化数据库：~/Library/Application Support/文献阅读台/library.db
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("library.db");
            let conn = db::init_db(&db_path).expect("failed to init database");
            let mineru = load_mineru_key(app.handle()).map(MinerUClient::new);
            app.manage(AppState {
                conn: Mutex::new(conn),
                mineru,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_documents,
            import_document,
            start_parse
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
