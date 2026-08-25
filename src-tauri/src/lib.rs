mod db;
mod digest;
mod error_code;
mod export;
mod fields;
mod logging;
pub mod mineru;
#[macro_use]
mod notes;
mod paradigm;
mod stats;
mod translate;

use mineru::MinerUClient;
use crate::notes::{
    delete_note, delete_notes_batch, export_note, export_notes_batch, list_notes, print_note,
    read_note, rename_note, replace_in_notes, save_note,
};
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::Emitter;
use tauri::Manager;
use translate::{split_markdown, translate_batch, Translator, VisionConfig};

/// M5.2 崩溃恢复标记：正常退出时删除；异常退出（崩溃/强杀）时残留，下次启动据此提示
static SESSION_MARKER: OnceLock<std::path::PathBuf> = OnceLock::new();
static LAST_CRASH: AtomicBool = AtomicBool::new(false);

/// 全局应用状态：数据库连接（互斥保护，供多命令共享）+ 翻译任务控制标志
struct AppState {
    conn: Mutex<Connection>,
    mineru: Option<MinerUClient>,
    translation_controls: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

/// 任务中心行（不包含任何密钥信息）
#[derive(Serialize)]
struct TaskRow {
    id: String,
    doc_id: String,
    title: String,
    #[serde(rename = "type")]
    task_type: String,
    status: String,
    progress: f64,
    stage: String,
    detail: String,
    error: String,
    created_at: String,
    updated_at: String,
}

/// 任务中心：列出全部后台任务（解析 / 翻译 / 拆解），按更新时间倒序
#[tauri::command]
fn list_tasks(state: tauri::State<'_, AppState>) -> Result<Vec<TaskRow>, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT t.id, t.doc_id, COALESCE(d.title, '未知文献'), t.type, t.status, t.progress,
                    COALESCE(t.stage, ''), COALESCE(t.detail, ''), COALESCE(t.error, ''),
                    t.created_at, COALESCE(t.updated_at, t.created_at)
             FROM tasks t
             LEFT JOIN documents d ON d.id = t.doc_id
             ORDER BY COALESCE(t.updated_at, t.created_at) DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(TaskRow {
                id: r.get(0)?,
                doc_id: r.get(1)?,
                title: r.get(2)?,
                task_type: r.get(3)?,
                status: r.get(4)?,
                progress: r.get(5)?,
                stage: r.get(6)?,
                detail: r.get(7)?,
                error: r.get(8)?,
                created_at: r.get(9)?,
                updated_at: r.get(10)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// 清空已完成 / 失败的历史任务记录
#[tauri::command]
fn clear_finished_tasks(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM tasks WHERE status IN ('done', 'failed')",
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 删除单条已完成 / 失败的任务记录（运行中的任务不允许删除）
#[tauri::command]
fn delete_task(task_id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM tasks WHERE id = ?1 AND status IN ('done', 'failed')",
        [&task_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 新建任务记录（running）
fn insert_task(
    conn: &Connection,
    task_id: &str,
    doc_id: &str,
    kind: &str,
    detail: Option<&str>,
) -> Result<(), String> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO tasks (id, doc_id, type, status, progress, stage, detail, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'running', 0, '准备', ?4, ?5, ?5)",
        rusqlite::params![task_id, doc_id, kind, detail.unwrap_or_default(), now],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 更新任务记录（进度 / 阶段 / 详情 / 状态 / 错误）
fn update_task_status(
    conn: &Connection,
    task_id: &str,
    status: &str,
    progress: f64,
    stage: &str,
    detail: &str,
    error: Option<&str>,
) {
    let _ = conn.execute(
        "UPDATE tasks SET status = ?1, progress = ?2, stage = ?3, detail = ?4,
                error = ?5, updated_at = ?6
         WHERE id = ?7",
        rusqlite::params![
            status,
            progress,
            stage,
            detail,
            error.unwrap_or_default(),
            now_iso(),
            task_id
        ],
    );
}

/// 加载 MINERU_API_KEY：优先用户在设置页填写的 Key（settings 表），
/// 回退到环境变量 / .env（KEY=VALUE 行）
fn load_mineru_key(conn: &Connection, app: &tauri::AppHandle) -> Option<String> {
    // 1. 用户自填 Key（settings 表 mineru_api_key）优先
    let user_key = read_setting(conn, "mineru_api_key");
    if !user_key.is_empty() {
        return Some(user_key);
    }
    // 2. 环境变量
    if let Ok(k) = std::env::var("MINERU_API_KEY") {
        if !k.is_empty() {
            return Some(k);
        }
    }
    // 3. 候选 .env 路径：应用配置目录 / 当前目录（src-tauri）/ 项目根（父目录）
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

/// 查询 MinerU Token 是否已由用户配置（不回传 Key 本身）
#[tauri::command]
fn get_mineru_key(state: tauri::State<'_, AppState>) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "configured": !read_setting(&conn, "mineru_api_key").is_empty() }))
}

/// 保存用户自填的 MinerU Token（空串 = 清除，回退 .env / 环境变量）
#[tauri::command]
fn set_mineru_key(key: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let key = key.trim().to_string();
    write_setting(&conn, "mineru_api_key", &key)?;
    logging::info("MinerU Token 已更新（用户设置页）");
    Ok(())
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
    read_status: String,
    created_at: String,
}

/// PDF 导入：校验 → 去重 → 落盘 → 入库
#[tauri::command]
fn import_document(
    path: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Document, String> {
    let t_import = std::time::Instant::now();
    // 1. 扩展名校验
    let ext = std::path::Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if ext != "pdf" {
        return Err(error_code::err(error_code::IMPORT_INVALID, "仅支持 PDF 文件"));
    }

    // 2. 损坏文件拦截（PDF magic number）
    let bytes = std::fs::read(&path).map_err(|e| {
        error_code::err(error_code::IMPORT_IO, format!("读取文件失败: {e}"))
    })?;
    if bytes.len() < 5 || &bytes[..5] != b"%PDF-" {
        return Err(error_code::err(
            error_code::IMPORT_INVALID,
            "文件损坏：不是有效的 PDF",
        ));
    }

    let conn = state.conn.lock().map_err(|e| e.to_string())?;

    // 3. 重复导入检测（按真实路径）
    let canon = std::fs::canonicalize(&path)
        .map_err(|e| error_code::err(error_code::IMPORT_IO, format!("无法访问文件: {e}")))?;
    let canon_str = canon.to_str().unwrap_or("").to_string();
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM documents WHERE file_path = ?1)",
            [&canon_str],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if exists {
        return Err(error_code::err(error_code::IMPORT_DUP, "该 PDF 已导入过，是否覆盖？"));
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

    stats::record(stats::StatsEvent::Import, t_import.elapsed().as_millis() as u64);
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
        read_status: "unread".into(),
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
        insert_task(&conn, &task_id, &doc_id, "parse", Some("开始解析"))?;
    }

    // 3. 后台线程执行解析，事件推送进度与结果
    let app2 = app.clone();
    std::thread::spawn(move || {
        let emit = |event: &str, payload: serde_json::Value| {
            let _ = app2.emit(event, payload);
        };
        let update_task = |status: &str, progress: f64, stage: &str, detail: &str, error: Option<&str>| {
            if let Some(st) = app2.try_state::<AppState>() {
                if let Ok(conn) = st.conn.lock() {
                    update_task_status(&conn, &task_id, status, progress, stage, detail, error);
                }
            }
        };
        update_task("running", 0.0, "准备", "开始解析", None);
        emit(
            "parse-progress",
            json!({ "doc_id": doc_id, "stage": "上传中", "progress": 0.05 }),
        );
        update_task("running", 0.05, "上传中", "提交 MinerU", None);

        let t_parse = std::time::Instant::now();

        let result = (|| -> Result<String, String> {
            let batch_id = mineru.submit_file(Path::new(&source_path))?;
            update_task("running", 0.4, "解析中", "MinerU 已接收", None);
            emit(
                "parse-progress",
                json!({ "doc_id": doc_id, "stage": "解析中", "progress": 0.4 }),
            );
            let r = mineru.poll_batch(&batch_id, 600, Some(&|p: &str| {
                update_task("running", 0.6, "解析中", p, None);
                emit(
                    "parse-progress",
                    json!({ "doc_id": doc_id, "stage": "解析中", "progress": 0.6, "detail": p }),
                );
            }))?;
            let dest = data_dir.join("documents").join(&doc_id).join("parsed");
            let md = mineru.download_extract(&r, &dest)?;
            update_task("running", 1.0, "完成", "解析完成", None);
            emit(
                "parse-progress",
                json!({ "doc_id": doc_id, "stage": "完成", "progress": 1.0 }),
            );
            Ok(md.to_string_lossy().to_string())
        })();

        let parse_ok = result.is_ok();

        // 4. 回写状态
        let state = app2.state::<AppState>();
        let conn = state.conn.lock();
        match (result, conn) {
            (Ok(md), Ok(conn)) => {
                let now = chrono::Utc::now().to_rfc3339();
                // 从解析 Markdown 提取标题/作者/语言并回填
                if let Ok(content) = std::fs::read_to_string(&md) {
                    let (t, a) = extract_title_and_authors(&content);
                    let lang = detect_paper_language(
                        &content.chars().take(2000).collect::<String>(),
                    );
                    let _ = conn.execute(
                        "UPDATE documents SET status = 'parsed', updated_at = ?1,
                             title = COALESCE(?2, title), authors = COALESCE(?3, authors),
                             language = ?4
                         WHERE id = ?5",
                        rusqlite::params![now, t, a, lang, doc_id],
                    );
                } else {
                    let _ = conn.execute(
                        "UPDATE documents SET status = 'parsed', updated_at = ?1 WHERE id = ?2",
                        rusqlite::params![now, doc_id],
                    );
                }
                update_task_status(&conn, &task_id, "done", 1.0, "解析完成", "已生成 Markdown", None);
                drop(conn);
                emit("parse-done", json!({ "doc_id": doc_id, "md_path": md }));
            }
            (Err(e), Ok(conn)) => {
                let err_msg = error_code::err(error_code::PARSE_FAILED, &e);
                logging::error(&format!("解析失败 doc={doc_id}: {e}"));
                update_task_status(
                    &conn,
                    &task_id,
                    "failed",
                    0.0,
                    "解析失败",
                    "",
                    Some(&err_msg),
                );
                drop(conn);
                emit("parse-failed", json!({ "doc_id": doc_id, "error": err_msg }));
            }
            (_, Err(e)) => {
                logging::error(&format!("解析回写数据库失败 doc={doc_id}: {e}"));
                emit(
                    "parse-failed",
                    json!({ "doc_id": doc_id, "error": format!("数据库访问失败: {e}") }),
                );
            }
        };

        // 统计：解析成功记 Parse，失败记 Error（仅统计开启时生效）
        stats::record(
            if parse_ok {
                stats::StatsEvent::Parse
            } else {
                stats::StatsEvent::Error
            },
            t_parse.elapsed().as_millis() as u64,
        );
    });

    Ok(())
}

/// 已解析文献内容（含图片资源基础目录，供前端渲染本地图片）
#[derive(Serialize)]
struct ParsedDoc {
    content: String,
    base_dir: String,
}

/// 读取已解析的 Markdown 内容（用于预览视图）
#[tauri::command]
fn read_parsed(
    doc_id: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<ParsedDoc, String> {
    {
        let conn = state.conn.lock().map_err(|e| e.to_string())?;
        let status: String = conn
            .query_row(
                "SELECT status FROM documents WHERE id = ?1",
                [&doc_id],
                |r| r.get(0),
            )
            .map_err(|e| format!("文献不存在: {e}"))?;
        if !matches!(status.as_str(), "parsed" | "translated" | "digested") {
            return Err("文献尚未解析完成".into());
        }
    }

    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let parsed_dir = data_dir.join("documents").join(&doc_id).join("parsed");
    let md_path = std::fs::read_dir(&parsed_dir)
        .map_err(|e| format!("解析目录不可读: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
        .ok_or("未找到解析结果 Markdown")?;

    let content = std::fs::read_to_string(&md_path).map_err(|e| e.to_string())?;
    Ok(ParsedDoc {
        content,
        base_dir: parsed_dir.to_string_lossy().to_string(),
    })
}

/// 从解析出的 Markdown 提取标题与作者（MinerU 产物：首行为 # 标题，随后为作者行）
fn extract_title_and_authors(md: &str) -> (Option<String>, Option<String>) {
    // 标题：第一个以 # 开头的行
    let title = md
        .lines()
        .map(|l| l.trim())
        .find(|l| l.starts_with('#'))
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .filter(|t| !t.is_empty() && t.len() <= 200);

    // 作者：标题之后、摘要之前的短行，取逗号前部分（作者姓名）
    let mut authors: Vec<String> = Vec::new();
    let mut past_title = false;
    for line in md.lines() {
        let l = line.trim();
        if l.is_empty() {
            if !authors.is_empty() {
                break;
            }
            continue;
        }
        if l.starts_with('#') {
            past_title = true;
            continue;
        }
        if !past_title {
            continue;
        }
        // 摘要/正文段落通常较长
        if l.len() > 120 {
            break;
        }
        let name = l.split(',').next().unwrap_or(l).trim();
        if !name.is_empty()
            && name.len() <= 60
            && !authors.iter().any(|a| a == name)
            && !name.contains("https://")
        {
            authors.push(name.to_string());
            if authors.len() >= 5 {
                break;
            }
        }
    }

    (
        title,
        if authors.is_empty() {
            None
        } else {
            Some(authors.join(", "))
        },
    )
}

/// 已翻译的 Markdown 内容（翻译视图使用）
#[tauri::command]
fn read_translated(doc_id: String, app: tauri::AppHandle) -> Result<String, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let trans_dir = data_dir.join("documents").join(&doc_id).join("translated");
    let md_path = trans_dir.join("full.md");
    if !md_path.exists() {
        return Err("尚未翻译".into());
    }
    std::fs::read_to_string(&md_path).map_err(|e| e.to_string())
}

/// 双语对照段（原文/译文按段对齐）
#[derive(Serialize)]
struct BilingualSegment {
    index: usize,
    kind: String,
    original: String,
    translated: String,
}

/// 双语对照读取：原文来自解析产物（重新切分），译文来自断点续传文件 segments.json；
/// 返回全部段（未翻译段的译文为空字符串，前端可显示"翻译中"占位）
#[tauri::command]
fn read_bilingual(doc_id: String, app: tauri::AppHandle) -> Result<Vec<BilingualSegment>, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let doc_dir = data_dir.join("documents").join(&doc_id);

    // 原文：解析目录中的 Markdown
    let parsed_dir = doc_dir.join("parsed");
    let md_path = std::fs::read_dir(&parsed_dir)
        .map_err(|e| format!("解析目录不可读: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
        .ok_or("未找到解析结果 Markdown")?;
    let md = std::fs::read_to_string(&md_path).map_err(|e| e.to_string())?;

    // 译文映射：index -> 译文
    let seg_path = doc_dir.join("translated").join("segments.json");
    let content = std::fs::read_to_string(&seg_path).map_err(|e| format!("尚未翻译: {e}"))?;
    let v: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    let obj = v.as_object().ok_or("译文数据格式错误")?;

    let mut pairs = Vec::new();
    for seg in split_markdown(&md) {
        let translated = obj
            .get(&seg.index.to_string())
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        pairs.push(BilingualSegment {
            index: seg.index,
            kind: seg.kind,
            original: seg.content,
            translated,
        });
    }
    Ok(pairs)
}

/// 启动全文翻译任务（异步）：分段切分 → 并发翻译 → 断点续传 → 拼接保存
/// direction: "en_to_zh"（英文文献→中文，默认）/ "zh_to_en"（中文文献→英文）
#[tauri::command]
fn start_translate(
    doc_id: String,
    direction: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    // 1. 读取默认 API 配置（含明文 Key）
    let (base_url, api_key, model) = {
        let conn = state.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT base_url, COALESCE(key_ref, ''), COALESCE(model, '') FROM api_configs WHERE is_default = 1 LIMIT 1",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
        )
        .map_err(|e| format!("未配置默认 API（请先在设置页配置）: {e}"))?
    };
    if api_key.is_empty() {
        return Err("默认 API 未配置 Key".into());
    }

    // 2. 术语表
    let glossary: Vec<(String, String)> = {
        let conn = state.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT term, COALESCE(translation, '') FROM term_glossary")
            .map_err(|e| e.to_string())?;
        let result = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        result
    };

    // 3. 读取解析产物并切分
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let doc_dir = data_dir.join("documents").join(&doc_id);
    let parsed_dir = doc_dir.join("parsed");
    let md_path = std::fs::read_dir(&parsed_dir)
        .map_err(|e| format!("解析目录不可读: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
        .ok_or("未找到解析结果 Markdown")?;
    let md = std::fs::read_to_string(&md_path).map_err(|e| e.to_string())?;
    let segments = split_markdown(&md);
    let total = segments.len();
    if total == 0 {
        return Err("文献内容为空".into());
    }

    // 4. 断点续传：加载已完成段
    let trans_dir = doc_dir.join("translated");
    std::fs::create_dir_all(&trans_dir).map_err(|e| e.to_string())?;
    let seg_path = trans_dir.join("segments.json");
    let mut done_map: std::collections::HashMap<usize, String> = std::collections::HashMap::new();
    if let Ok(content) = std::fs::read_to_string(&seg_path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(obj) = v.as_object() {
                for (k, val) in obj {
                    if let (Ok(i), Some(s)) = (k.parse::<usize>(), val.as_str()) {
                        done_map.insert(i, s.to_string());
                    }
                }
            }
        }
    }
    let pending: Vec<translate::Segment> = segments
        .iter()
        .filter(|s| !done_map.contains_key(&s.index))
        .cloned()
        .collect();

    // 4.1 记录翻译任务（running）
    let task_id = uuid::Uuid::new_v4().to_string();
    {
        let conn = state.conn.lock().map_err(|e| e.to_string())?;
        insert_task(&conn, &task_id, &doc_id, "translate", Some(&direction))?;
    }

    // 5. 注册翻译控制标志（暂停/继续）并启动后台线程
    let ctrl = Arc::new(AtomicBool::new(false));
    {
        let mut controls = state
            .translation_controls
            .lock()
            .map_err(|e| e.to_string())?;
        controls.insert(doc_id.clone(), ctrl.clone());
    }
    let app2 = app.clone();
    std::thread::spawn(move || {
        let emit = |event: &str, payload: serde_json::Value| {
            let _ = app2.emit(event, payload);
        };
        let update_task = |status: &str, progress: f64, stage: &str, detail: &str, error: Option<&str>| {
            if let Some(st) = app2.try_state::<AppState>() {
                if let Ok(conn) = st.conn.lock() {
                    update_task_status(&conn, &task_id, status, progress, stage, detail, error);
                }
            }
        };
        update_task("running", 0.0, "准备", &direction, None);
        emit(
            "translate-progress",
            json!({ "doc_id": doc_id, "stage": "准备", "progress": 0.0 }),
        );

        let t_translate = std::time::Instant::now();

        // 段索引 → 原文，供前端实时渲染译文/双语视图
        let orig_by_idx: std::collections::HashMap<usize, String> = segments
            .iter()
            .map(|s| (s.index, s.content.clone()))
            .collect();

        // 视觉模型（可选）：预分析文内图片，辅助图注翻译
        let vision_notes = {
            let vision = match app2.state::<AppState>().conn.lock() {
                Ok(c) => read_vision_config(&c),
                Err(_) => VisionConfig::default(),
            };
            if vision.enabled() {
                analyze_images(&vision, &parsed_dir, &md)
            } else {
                HashMap::new()
            }
        };

        let translator = Translator {
            base_url,
            api_key,
            model,
            glossary,
            vision_notes,
            direction: translate::TranslateDirection::parse(&direction),
        };

        // 视觉分析结果落盘，供阅读视图展示图片识别（类型 + 要点）
        if !translator.vision_notes.is_empty() {
            let analysis_path = parsed_dir
                .parent()
                .unwrap_or(&parsed_dir)
                .join("images_analysis.json");
            let _ = std::fs::write(
                &analysis_path,
                serde_json::to_string(&translator.vision_notes).unwrap_or_default(),
            );
        }
        let mut pending_mut = pending.clone();
        let done_count = std::sync::atomic::AtomicUsize::new(0);

        let result = {
            let seg_path_ref = &seg_path;
            let done_map_ref = &mut done_map;
            let done_count_ref = &done_count;
            let mut cb = |idx: usize, kind: &str, text: &str| {
                // 逐段持久化已完成结果，中断后可断点续传
                done_map_ref.insert(idx, text.to_string());
                let _ = std::fs::write(
                    seg_path_ref,
                    serde_json::to_string(done_map_ref).unwrap_or_default(),
                );
                let n = done_count_ref.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                let progress = n as f64 / total as f64;
                let detail = format!("{n}/{total} · {kind}");
                update_task("running", progress, "翻译中", &detail, None);
                emit(
                    "translate-progress",
                    json!({
                        "doc_id": doc_id, "stage": "翻译中",
                        "progress": progress,
                        "detail": detail
                    }),
                );
                // 实时推送该段原文与译文，前端阅读视图据此逐段渲染译文/双语
                let original = orig_by_idx.get(&idx).cloned().unwrap_or_default();
                emit(
                    "translate-segment",
                    json!({
                        "doc_id": doc_id, "index": idx, "kind": kind,
                        "original": original, "translated": text
                    }),
                );
            };
            translate_batch(&translator, &mut pending_mut, 3, Some(ctrl.as_ref()), &mut cb)
        };

        let translate_ok = result.is_ok();

        match result {
            Ok(()) => {
                // 将翻译结果写回 done_map 并持久化
                for seg in &pending_mut {
                    done_map.insert(seg.index, seg.content.clone());
                }
                let _ = std::fs::write(
                    &seg_path,
                    serde_json::to_string(&done_map).unwrap_or_default(),
                );
                // 按原顺序拼接
                let full: String = segments
                    .iter()
                    .map(|s| done_map.get(&s.index).cloned().unwrap_or_else(|| s.content.clone()))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                let out_path = trans_dir.join("full.md");
                let write_result = std::fs::write(&out_path, &full);

                // 更新状态
                let state = app2.state::<AppState>();
                if let Ok(conn) = state.conn.lock() {
                    let now = chrono::Utc::now().to_rfc3339();
                    let _ = conn.execute(
                        "UPDATE documents SET status = 'translated', updated_at = ?1 WHERE id = ?2",
                        rusqlite::params![now, doc_id],
                    );
                }

                match write_result {
                    Ok(_) => {
                        update_task("done", 1.0, "翻译完成", "已保存译文", None);
                        emit(
                            "translate-done",
                            json!({ "doc_id": doc_id, "path": out_path.to_string_lossy() }),
                        );
                    }
                    Err(e) => {
                        let msg = format!("保存翻译结果失败: {e}");
                        update_task("failed", 1.0, "保存失败", &msg, Some(&msg));
                        emit(
                            "translate-failed",
                            json!({ "doc_id": doc_id, "error": msg }),
                        );
                    }
                }
            }
            Err(e) => {
                // 已完成的段已通过断点续传落盘（在回调中未落盘，故在此持久化已翻译的段）
                let _ = std::fs::write(
                    &seg_path,
                    serde_json::to_string(&done_map).unwrap_or_default(),
                );
                let code = error_code::from_translate_category(&e.category);
                logging::error(&format!("翻译失败 doc={doc_id} [{code}]: {}", e.message));
                let progress = done_count.load(std::sync::atomic::Ordering::SeqCst) as f64 / total as f64;
                let msg = format!("[{code}] {}", e.message);
                update_task("failed", progress, "翻译失败", &msg, Some(&msg));
                let mut err_val = serde_json::to_value(&e).unwrap_or_default();
                if let Some(obj) = err_val.as_object_mut() {
                    obj.insert("code".to_string(), json!(code));
                }
                emit("translate-failed", json!({ "doc_id": doc_id, "error": err_val }));
            }
        }

        // 任务结束，移除控制标志（释放暂停/继续入口）
        if let Ok(mut controls) = app2.state::<AppState>().translation_controls.lock() {
            controls.remove(&doc_id);
        }

        // 统计：翻译成功记 Translate，失败记 Error（仅统计开启时生效）
        stats::record(
            if translate_ok {
                stats::StatsEvent::Translate
            } else {
                stats::StatsEvent::Error
            },
            t_translate.elapsed().as_millis() as u64,
        );
    });

    Ok(())
}

/// 从 settings 表读取字符串配置项
fn read_setting(conn: &Connection, key: &str) -> String {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
        .unwrap_or_default()
}

/// 写入（或覆盖）settings 表字符串配置项
fn write_setting(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 从 settings 表读取视觉模型配置（可选外挂，用于图片识别）
fn read_vision_config(conn: &Connection) -> VisionConfig {
    VisionConfig {
        base_url: read_setting(conn, "vision_base_url"),
        api_key: read_setting(conn, "vision_api_key"),
        model: read_setting(conn, "vision_model"),
    }
}

/// 暂停翻译（该文献）：置位暂停标志，在途请求完成后不再启动新段
#[tauri::command]
fn pause_translate(
    doc_id: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let controls = state.translation_controls.lock().map_err(|e| e.to_string())?;
    let ctrl = controls.get(&doc_id).ok_or("未找到进行中的翻译任务")?;
    ctrl.store(true, Ordering::SeqCst);
    let _ = app.emit("translate-paused", json!({ "doc_id": doc_id }));
    Ok(())
}

/// 继续翻译（该文献）：清除暂停标志
#[tauri::command]
fn resume_translate(
    doc_id: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let controls = state.translation_controls.lock().map_err(|e| e.to_string())?;
    let ctrl = controls.get(&doc_id).ok_or("未找到进行中的翻译任务")?;
    ctrl.store(false, Ordering::SeqCst);
    let _ = app.emit("translate-resumed", json!({ "doc_id": doc_id }));
    Ok(())
}

/// 读取视觉模型配置（可选外挂）
#[tauri::command]
fn get_vision_config(state: tauri::State<'_, AppState>) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    Ok(json!({
        "base_url": read_setting(&conn, "vision_base_url"),
        "api_key": read_setting(&conn, "vision_api_key"),
        "model": read_setting(&conn, "vision_model"),
    }))
}

/// 保存视觉模型配置（Key 留空则不清空）
#[tauri::command]
fn save_vision_config(
    base_url: String,
    api_key: String,
    model: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    write_setting(&conn, "vision_base_url", &base_url)?;
    write_setting(&conn, "vision_api_key", &api_key)?;
    write_setting(&conn, "vision_model", &model)?;
    Ok(())
}

/// 测试视觉模型连接（发送 1×1 图片验证 Base URL / Key / 模型）
#[tauri::command]
fn test_vision_connection(
    base_url: String,
    api_key: String,
    model: String,
) -> Result<String, String> {
    let vision = VisionConfig {
        base_url,
        api_key,
        model,
    };
    if !vision.enabled() {
        return Err("请完整填写 Base URL、API Key、模型名".into());
    }
    vision.test_connection()
}

/// 读取图片视觉分析结果（路径 → 描述），供阅读视图展示；无结果时返回空对象
#[tauri::command]
fn read_image_notes(doc_id: String, app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let path = data_dir
        .join("documents")
        .join(&doc_id)
        .join("images_analysis.json");
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).map_err(|e| e.to_string()),
        Err(_) => Ok(serde_json::Value::Object(Default::default())),
    }
}

/// 范式识别（M3.1）：提取解析产物的标题/摘要/章节/特殊对象信号，调用范式识别器
#[tauri::command]
fn recognize_paradigm(
    doc_id: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<paradigm::ParadigmRecognition, String> {
    let input = build_paradigm_input(&state, &app, &doc_id)?;
    Ok(paradigm::recognize(&input))
}

/// 字段方案组装（M3.2）：复用识别信号 → 范式识别 → 路由引擎组装字段集
#[tauri::command]
fn get_field_plan(
    doc_id: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<fields::FieldPlan, String> {
    let input = build_paradigm_input(&state, &app, &doc_id)?;
    let rec = paradigm::recognize(&input);
    Ok(fields::assemble_field_plan(&rec))
}

/// 拆解执行（M3.3/3.4）：识别 → 字段方案 → 逐字段 LLM 拆解（引用锚定）→ 持久化 + 版本管理
#[tauri::command]
fn start_digest(
    doc_id: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    // 1. 读取默认 API 配置 + 术语表
    let (base_url, api_key, model) = {
        let conn = state.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT base_url, COALESCE(key_ref, ''), COALESCE(model, '') FROM api_configs WHERE is_default = 1 LIMIT 1",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
        )
        .map_err(|e| format!("未配置默认 API（请先在设置页配置）: {e}"))?
    };
    if api_key.is_empty() {
        return Err("默认 API 未配置 Key".into());
    }
    let glossary: Vec<(String, String)> = {
        let conn = state.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT term, COALESCE(translation, '') FROM term_glossary")
            .map_err(|e| e.to_string())?;
        let x = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        x
    };

    // 2. 读取解析产物
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let doc_dir = data_dir.join("documents").join(&doc_id);
    let parsed_dir = doc_dir.join("parsed");
    let md_path = std::fs::read_dir(&parsed_dir)
        .map_err(|e| format!("解析目录不可读: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
        .ok_or("未找到解析结果 Markdown")?;
    let md = std::fs::read_to_string(&md_path).map_err(|e| e.to_string())?;

    // 2.1 记录拆解任务（running）
    let task_id = uuid::Uuid::new_v4().to_string();
    {
        let conn = state.conn.lock().map_err(|e| e.to_string())?;
        insert_task(&conn, &task_id, &doc_id, "digest", Some("开始拆解"))?;
    }

    // 3. 后台线程执行拆解
    let app2 = app.clone();
    std::thread::spawn(move || {
        let emit = |event: &str, payload: serde_json::Value| {
            let _ = app2.emit(event, payload);
        };
        let update_task = |status: &str, progress: f64, stage: &str, detail: &str, error: Option<&str>| {
            if let Some(st) = app2.try_state::<AppState>() {
                if let Ok(conn) = st.conn.lock() {
                    update_task_status(&conn, &task_id, status, progress, stage, detail, error);
                }
            }
        };
        update_task("running", 0.0, "准备", "开始拆解", None);
        emit(
            "digest-progress",
            json!({ "doc_id": doc_id, "stage": "准备", "progress": 0.0 }),
        );

        let t_digest = std::time::Instant::now();

        // 3.1 范式识别 + 字段方案
        let input = match {
            let st = app2.state::<AppState>();
            build_paradigm_input(&st, &app2, &doc_id)
        } {
            Ok(i) => i,
            Err(e) => {
                logging::error(&format!("拆解准备失败 doc={doc_id}: {e}"));
                update_task("failed", 0.0, "准备失败", &e, Some(&e));
                emit(
                    "digest-failed",
                    json!({ "doc_id": doc_id, "error": { "category": "internal", "code": error_code::DIGEST_FAILED, "message": e, "hint": "请确认文献已解析后再试" } }),
                );
                stats::record(stats::StatsEvent::Error, t_digest.elapsed().as_millis() as u64);
                return;
            }
        };
        let rec = paradigm::recognize(&input);
        let plan = fields::assemble_field_plan(&rec);
        let total = plan.fields.len();
        if total == 0 {
            logging::error(&format!("拆解字段方案为空 doc={doc_id}"));
            update_task("failed", 0.0, "字段方案为空", "未生成任何拆解字段", None);
            emit(
                "digest-failed",
                json!({ "doc_id": doc_id, "error": { "category": "internal", "code": error_code::DIGEST_FAILED, "message": "字段方案为空", "hint": "请先完成范式识别" } }),
            );
            stats::record(stats::StatsEvent::Error, t_digest.elapsed().as_millis() as u64);
            return;
        }

        let title = input.title;
        let context = digest::numbered_context(&md, 22000);
        let system = digest::build_system_prompt(&rec, &plan.combine_strategy, total);
        let translator = Translator {
            base_url,
            api_key,
            model,
            glossary,
            vision_notes: HashMap::new(),
            direction: translate::TranslateDirection::EnToZh,
        };

        // 3.2 版本号（MAX+1）
        let version: i64 = {
            let st = app2.state::<AppState>();
            let conn = st.conn.lock().ok();
            match conn.and_then(|c| {
                c.query_row(
                    "SELECT COALESCE(MAX(version),0) FROM digest_versions WHERE doc_id = ?1",
                    [&doc_id],
                    |r| r.get::<_, i64>(0),
                )
                .ok()
            }) {
                Some(v) => v + 1,
                None => 1,
            }
        };
        let digests_dir = doc_dir.join("digests");
        let _ = std::fs::create_dir_all(&digests_dir);
        let digest_md_path = digests_dir.join(format!("digest_v{version}.md"));
        let fields_path = digests_dir.join(format!("fields_v{version}.json"));

        let mut results: Vec<digest::DigestFieldResult> = Vec::new();
        let mut md_out = format!(
            "# {title}\n\n> 范式：{} ｜ {} ｜ {} ｜ 共 {total} 个字段\n\n",
            rec.paradigm_name, rec.cross_type, plan.combine_strategy
        );

        // 3.3 逐字段拆解（每字段一次请求，实时进度 + 增量持久化）
        for (i, fld) in plan.fields.iter().enumerate() {
            let progress = i as f64 / total as f64;
            let detail = format!("{}/{} · {}", i + 1, total, fld.label);
            update_task("running", progress, "拆解中", &detail, None);
            emit(
                "digest-progress",
                json!({
                    "doc_id": doc_id, "stage": "拆解中",
                    "progress": progress,
                    "detail": detail
                }),
            );
            let user = digest::build_field_user(fld, &context);
            match digest::digest_field(&translator, &system, &user, &fld.ftype) {
                Ok((zh, en, table)) => {
                    let table_block = table.clone().unwrap_or_default();
                    results.push(digest::DigestFieldResult {
                        name: fld.name.clone(),
                        label: fld.label.clone(),
                        ftype: fld.ftype.clone(),
                        source: fld.source.clone(),
                        zh: zh.clone(),
                        en,
                        table,
                        failed: false,
                    });
                    md_out.push_str(&format!("## {}\n\n", fld.label));
                    if !table_block.is_empty() {
                        md_out.push_str(&format!("{table_block}\n\n"));
                    }
                    md_out.push_str(&format!("{zh}\n\n"));
                    emit(
                        "digest-field",
                        json!({ "doc_id": doc_id, "name": fld.name, "label": fld.label, "ftype": fld.ftype, "zh": zh }),
                    );
                }
                Err(e) => {
                    logging::warn(&format!("拆解字段「{}」失败 doc={doc_id}: {}", fld.label, e.message));
                    results.push(digest::DigestFieldResult {
                        name: fld.name.clone(),
                        label: fld.label.clone(),
                        ftype: fld.ftype.clone(),
                        source: fld.source.clone(),
                        zh: "引用缺失".to_string(),
                        en: "Citation missing".to_string(),
                        table: None,
                        failed: true,
                    });
                    md_out.push_str(&format!(
                        "## {}\n\n引用缺失（拆解失败：{}）\n\n",
                        fld.label, e.message
                    ));
                    emit(
                        "digest-field-failed",
                        json!({ "doc_id": doc_id, "field": fld.label, "error": e }),
                    );
                }
            }
            // 增量持久化：中断后不丢失已拆解字段
            let _ = std::fs::write(&fields_path, serde_json::to_string(&results).unwrap_or_default());
            let _ = std::fs::write(&digest_md_path, &md_out);
        }

        // 3.4 落库 + 状态更新
        let now = now_iso();
        if let Ok(conn) = app2.state::<AppState>().conn.lock() {
            let id = format!("{doc_id}-v{version}");
            let _ = conn.execute(
                "INSERT INTO digest_versions (id, doc_id, version, field_schema, content, score, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    id,
                    doc_id,
                    version,
                    serde_json::to_string(&plan).unwrap_or_default(),
                    serde_json::to_string(&results).unwrap_or_default(),
                    rec.confidence,
                    now
                ],
            );
            let _ = conn.execute(
                "UPDATE documents SET status = 'digested' WHERE id = ?1",
                [&doc_id],
            );
        }
        emit(
            "digest-done",
            json!({ "doc_id": doc_id, "version": version, "count": total }),
        );
        update_task("done", 1.0, "拆解完成", &format!("v{version} · 共 {total} 个字段"), None);
        stats::record(stats::StatsEvent::Digest, t_digest.elapsed().as_millis() as u64);
    });
    Ok(())
}

/// 读取最新版拆解结果
#[tauri::command]
fn read_digest(doc_id: String, state: tauri::State<'_, AppState>) -> Result<Option<DigestRecord>, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let row = conn
        .query_row(
            "SELECT version, content FROM digest_versions WHERE doc_id = ?1 ORDER BY version DESC LIMIT 1",
            [&doc_id],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    match row {
        Some((version, content)) => {
            let fields = parse_digest_content(&content);
            Ok(Some(DigestRecord { version, fields }))
        }
        None => Ok(None),
    }
}

/// 读取指定版本的拆解结果
#[tauri::command]
fn read_digest_version(
    doc_id: String,
    version: i64,
    state: tauri::State<'_, AppState>,
) -> Result<Option<DigestRecord>, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let row = conn
        .query_row(
            "SELECT content FROM digest_versions WHERE doc_id = ?1 AND version = ?2",
            rusqlite::params![doc_id, version],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(row.map(|content| DigestRecord {
        version,
        fields: parse_digest_content(&content),
    }))
}

/// 拆解历史版本列表（最新在前）
#[derive(Serialize)]
struct DigestVersionInfo {
    version: i64,
    created_at: String,
}

#[tauri::command]
fn list_digest_versions(
    doc_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<DigestVersionInfo>, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT version, created_at FROM digest_versions WHERE doc_id = ?1 ORDER BY version DESC")
        .map_err(|e| e.to_string())?;
    let x = stmt
        .query_map([&doc_id], |r| {
            Ok(DigestVersionInfo {
                version: r.get(0)?,
                created_at: r.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(x)
}

/// 保存拆解在线编辑 → 生成新版本（保存即版本，append-only 可回滚）
#[tauri::command]
fn save_digest_edit(
    doc_id: String,
    fields: Vec<digest::DigestFieldResult>,
    app: tauri::AppHandle,
) -> Result<DigestRecord, String> {
    let title = {
        let st = app.state::<AppState>();
        let conn = st.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT COALESCE(title, '') FROM documents WHERE id = ?1",
            [&doc_id],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_default()
    };
    persist_digest_version(&app, &doc_id, &title, &fields)
}

/// 回滚到指定版本：将目标版本内容保存为新的最新版本（保留历史）
#[tauri::command]
fn rollback_digest(
    doc_id: String,
    version: i64,
    app: tauri::AppHandle,
) -> Result<DigestRecord, String> {
    let content = {
        let st = app.state::<AppState>();
        let conn = st.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT content FROM digest_versions WHERE doc_id = ?1 AND version = ?2",
            rusqlite::params![doc_id, version],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("目标版本不存在")?
    };
    let fields = parse_digest_content(&content);
    let title = {
        let st = app.state::<AppState>();
        let conn = st.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT COALESCE(title, '') FROM documents WHERE id = ?1",
            [&doc_id],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_default()
    };
    persist_digest_version(&app, &doc_id, &title, &fields)
}

/// 解析拆解内容（兼容旧版单语 content 结构）
fn parse_digest_content(content: &str) -> Vec<digest::DigestFieldResult> {
    match serde_json::from_str::<Vec<digest::DigestFieldResult>>(content) {
        Ok(f) => f,
        Err(_) => {
            let old: Vec<serde_json::Value> = serde_json::from_str(content).unwrap_or_default();
            old.iter()
                .filter_map(|v| {
                    let name = v["name"].as_str()?.to_string();
                    Some(digest::DigestFieldResult {
                        name,
                        label: v["label"].as_str().unwrap_or("").to_string(),
                        ftype: v["ftype"].as_str().unwrap_or("").to_string(),
                        source: v["source"].as_str().unwrap_or("").to_string(),
                        zh: v["content"].as_str().unwrap_or("").to_string(),
                        en: String::new(),
                        table: None,
                        failed: v["failed"].as_bool().unwrap_or(false),
                    })
                })
                .collect()
        }
    }
}

/// 落盘新版本：fields_vN.json + digest_vN.md + digest_versions 行（MAX+1）
fn persist_digest_version(
    app: &tauri::AppHandle,
    doc_id: &str,
    title: &str,
    fields: &[digest::DigestFieldResult],
) -> Result<DigestRecord, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let digests_dir = data_dir
        .join("documents")
        .join(doc_id)
        .join("digests");
    std::fs::create_dir_all(&digests_dir).map_err(|e| e.to_string())?;

    let st = app.state::<AppState>();
    let conn = st.conn.lock().map_err(|e| e.to_string())?;
    let version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version),0) FROM digest_versions WHERE doc_id = ?1",
            [&doc_id],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
        + 1;
    let fields_json = serde_json::to_string(fields).unwrap_or_default();
    let md = fields_to_md(title, fields);
    let _ = std::fs::write(digests_dir.join(format!("fields_v{version}.json")), &fields_json);
    let _ = std::fs::write(digests_dir.join(format!("digest_v{version}.md")), &md);
    let id = format!("{doc_id}-v{version}");
    conn.execute(
        "INSERT INTO digest_versions (id, doc_id, version, field_schema, content, score, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![id, doc_id, version, "", fields_json, 0.0, now_iso()],
    )
    .map_err(|e| e.to_string())?;
    Ok(DigestRecord {
        version,
        fields: fields.to_vec(),
    })
}

/// 将拆解字段集渲染为 Markdown（digest_vN.md）
fn fields_to_md(title: &str, fields: &[digest::DigestFieldResult]) -> String {
    let mut out = format!("# {title}\n\n");
    for f in fields {
        out.push_str(&format!("## {}\n\n", f.label));
        if let Some(t) = &f.table {
            if !t.is_empty() {
                out.push_str(t);
                out.push('\n');
            }
        }
        out.push_str(&f.zh);
        out.push('\n');
        if !f.en.is_empty() {
            out.push_str(&format!("\n_English:_ {}\n", f.en));
        }
        out.push('\n');
    }
    out
}

/// 导出文档（M4.1）：md / html（内嵌图片 + 主题），写到用户选择路径
/// 异步执行（spawn_blocking），避免主线程读图/编码导致 UI 冻结
#[tauri::command]
async fn export_document(
    doc_id: String,
    format: String, // "md" | "html"
    theme: String,  // "light" | "sepia"（仅 html 生效）
    out_path: String,
    app: tauri::AppHandle,
) -> Result<String, String> {
    let t_export = std::time::Instant::now();
    let log_doc = doc_id.clone();
    let log_fmt = format.clone();
    let r: Result<String, String> = tauri::async_runtime::spawn_blocking(move || {
        export_document_sync(&app, &doc_id, &format, &theme, &out_path)
    })
    .await
    .map_err(|e| format!("导出线程异常: {e}"))?;
    match r {
        Ok(path) => {
            stats::record(stats::StatsEvent::Export, t_export.elapsed().as_millis() as u64);
            logging::info(&format!("导出完成 doc={log_doc} fmt={log_fmt} -> {path}"));
            Ok(path)
        }
        Err(e) => {
            stats::record(stats::StatsEvent::Error, t_export.elapsed().as_millis() as u64);
            logging::error(&format!("导出失败 doc={log_doc} fmt={log_fmt}: {e}"));
            Err(e)
        }
    }
}

fn export_document_sync(
    app: &tauri::AppHandle,
    doc_id: &str,
    format: &str,
    theme: &str,
    out_path: &str,
) -> Result<String, String> {
    let (parts, parsed_dir) = build_export_parts(app, doc_id)?;
    let content = if format == "html" {
        export::compose_html(&parts, &parsed_dir, theme)
    } else {
        export::compose_markdown(&parts)
    };
    std::fs::write(out_path, content).map_err(|e| format!("写入导出文件失败: {e}"))?;
    Ok(out_path.to_string())
}

/// 组装导出内容（md / html / PDF 共用）：标题 + 原文 + 译文 + 拆解（最新版）
/// 返回 (parts, parsed_dir) —— parsed_dir 供 compose_html 内嵌图片使用
fn build_export_parts(
    app: &tauri::AppHandle,
    doc_id: &str,
) -> Result<(export::ExportParts, std::path::PathBuf), String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let doc_dir = data_dir.join("documents").join(doc_id);
    let parsed_dir = doc_dir.join("parsed");

    // 标题
    let title = {
        let st = app.state::<AppState>();
        let conn = st.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT COALESCE(title, '') FROM documents WHERE id = ?1",
            [&doc_id],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_default()
    };

    // 原文
    let original = {
        let md_path = std::fs::read_dir(&parsed_dir)
            .map_err(|e| format!("解析目录不可读: {e}"))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
            .ok_or("未找到解析结果 Markdown")?;
        std::fs::read_to_string(&md_path).map_err(|e| e.to_string())?
    };

    // 译文（full.md 存在时）
    let translated = std::fs::read_to_string(doc_dir.join("translated").join("full.md")).ok();

    // 拆解（最新版）
    let digest = {
        let st = app.state::<AppState>();
        let conn = st.conn.lock().map_err(|e| e.to_string())?;
        let content: Option<String> = conn
            .query_row(
                "SELECT content FROM digest_versions WHERE doc_id = ?1 ORDER BY version DESC LIMIT 1",
                [&doc_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        content.map(|c| fields_to_md(&title, &parse_digest_content(&c)))
    };

    Ok((
        export::ExportParts {
            title,
            original,
            translated,
            digest,
        },
        parsed_dir,
    ))
}

/// 导出 PDF（M4.2）：HTML 中转 → 系统打印窗口（macOS 打印面板支持「存储为 PDF」）
/// 临时 HTML 写入缓存目录，加载进打印窗口后自动触发 window.print()
#[tauri::command]
async fn print_document(
    doc_id: String,
    theme: String, // "light" | "sepia"
    app: tauri::AppHandle,
) -> Result<String, String> {
    let t_print = std::time::Instant::now();
    let r: Result<String, String> = async {
        // 组装 + 生成 HTML 属较重操作，放后台线程避免 UI 卡顿
        let app2 = app.clone();
        let doc_id2 = doc_id.clone();
        let (parts, parsed_dir) = tauri::async_runtime::spawn_blocking(move || {
            build_export_parts(&app2, &doc_id2)
        })
        .await
        .map_err(|e| format!("打印线程异常: {e}"))??;

        let theme2 = theme.clone();
        let html = tauri::async_runtime::spawn_blocking(move || {
            export::compose_html(&parts, &parsed_dir, &theme2)
        })
        .await
        .map_err(|e| format!("打印线程异常: {e}"))?;

        // 写临时 HTML + 打开打印窗口（与笔记打印共用）
        notes::spawn_print_html(&app, html, &format!("print-{doc_id}-{}", now_iso()))
    }
    .await;

    // 统计：打印成功记 Export，失败记 Error（仅统计开启时生效）
    match r {
        Ok(path) => {
            stats::record(stats::StatsEvent::Export, t_print.elapsed().as_millis() as u64);
            logging::info(&format!("打印导出完成 doc={doc_id} -> {path}"));
            Ok(path)
        }
        Err(e) => {
            stats::record(stats::StatsEvent::Error, t_print.elapsed().as_millis() as u64);
            logging::error(&format!("打印导出失败 doc={doc_id}: {e}"));
            Err(e)
        }
    }
}

/// 拆解记录（最新版）
#[derive(Serialize)]
struct DigestRecord {
    version: i64,
    fields: Vec<digest::DigestFieldResult>,
}

/// 当前 UTC 时间 ISO 字符串（与 db 写入保持一致的简单实现）
fn now_iso() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_default()
}

/// 提取范式识别所需的全部信号（标题/摘要/章节/表格/图片/公式）
fn build_paradigm_input(
    state: &tauri::State<'_, AppState>,
    app: &tauri::AppHandle,
    doc_id: &str,
) -> Result<paradigm::ParadigmInput, String> {
    // 1. 标题（数据库，兜底取首个 h1 标题）
    let db_title: String = {
        let conn = state.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT COALESCE(title, '') FROM documents WHERE id = ?1",
            [&doc_id],
            |r| r.get::<_, String>(0),
        )
        .map_err(|e| format!("文献不存在: {e}"))?
    };

    // 2. 解析产物
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let parsed_dir = data_dir.join("documents").join(doc_id).join("parsed");
    let md_path = std::fs::read_dir(&parsed_dir)
        .map_err(|e| format!("解析目录不可读: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
        .ok_or("未找到解析结果 Markdown")?;
    let md = std::fs::read_to_string(&md_path).map_err(|e| e.to_string())?;

    // 3. 信号提取：章节标题 / 表格 / 图片 / 公式 / 摘要+关键词+正文样本
    let mut headings: Vec<String> = Vec::new();
    let mut paragraphs: Vec<String> = Vec::new();
    let mut table_count = 0usize;
    let mut image_count = 0usize;
    for seg in split_markdown(&md) {
        match seg.kind.as_str() {
            "heading" => {
                let h = seg.content.trim_start_matches('#').trim().to_string();
                if !h.is_empty() {
                    headings.push(h);
                }
            }
            "table" => table_count += 1,
            "image_caption" => image_count += 1,
            "paragraph" => paragraphs.push(seg.content),
            _ => {}
        }
    }
    let img_refs = md.matches("![").count();
    if img_refs > image_count {
        image_count = img_refs;
    }
    // Keywords 行（MinerU 常见 "**Keywords:** xxx, yyy"），并入摘要信号
    let mut keywords_line = String::new();
    for line in md.lines() {
        let t = line.trim().trim_start_matches("**").trim();
        let low = t.to_lowercase();
        if low.starts_with("keywords") || low.starts_with("关键词") || low.starts_with("key words") {
            let body = t.split_once([':', '：']).map(|(_, b)| b).unwrap_or(t);
            let body = body.trim();
            if !body.is_empty() && body.len() < 400 {
                keywords_line = body.to_string();
                break;
            }
        }
    }
    let mut abstract_text: String = paragraphs.iter().take(3).cloned().collect::<Vec<_>>().join(" ");
    if !keywords_line.is_empty() {
        abstract_text.push_str(&format!(" 关键词：{keywords_line}"));
    }
    let body_sample: String = paragraphs.iter().take(14).cloned().collect::<Vec<_>>().join(" ");
    let title = if db_title.trim().is_empty() || db_title == "未命名" {
        headings.first().cloned().unwrap_or_default()
    } else {
        db_title
    };

    Ok(paradigm::ParadigmInput {
        title,
        abstract_text,
        headings,
        body_sample: body_sample.chars().take(6000).collect(),
        table_count,
        image_count,
        math_symbols: md.matches('$').count(),
    })
}

/// 视觉模型已配置时，预分析文内图片（最多 12 张），返回 图片路径 → 描述
fn analyze_images(vision: &VisionConfig, parsed_dir: &Path, md: &str) -> HashMap<String, String> {
    let mut notes = HashMap::new();
    for path in translate::collect_image_refs(md).into_iter().take(12) {
        let full = parsed_dir.join(path.trim_start_matches("./"));
        if let Some(desc) = vision.describe_image(&full) {
            notes.insert(path.clone(), desc);
        }
    }
    notes
}

/// 语言识别：判定论文主导语言（中文/英文/其他）
/// 实现说明：采用 whatlang（纯 Rust，零模型依赖）；若精度不足可切换 fastText lid.176（见 research/report.md 第 10 章）
fn detect_paper_language(text: &str) -> String {
    match whatlang::detect(text) {
        Some(info) if info.is_reliable() => match info.lang() {
            whatlang::Lang::Cmn => "中文".to_string(),
            whatlang::Lang::Eng => "英文".to_string(),
            _ => "其他".to_string(),
        },
        _ => "其他".to_string(),
    }
}

fn mask_key(k: &str) -> String {
    if k.chars().count() <= 8 {
        "***".to_string()
    } else {
        format!("{}***", &k[..6])
    }
}

/// 查询全部 API 配置（Key 脱敏）
#[tauri::command]
fn list_api_configs(state: tauri::State<'_, AppState>) -> Result<Vec<serde_json::Value>, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT id, name, base_url, model, params, key_ref, is_default FROM api_configs ORDER BY is_default DESC, created_at")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            let key: Option<String> = r.get(5)?;
            Ok(json!({
                "id": r.get::<_, String>(0)?,
                "name": r.get::<_, String>(1)?,
                "base_url": r.get::<_, String>(2)?,
                "model": r.get::<_, Option<String>>(3)?,
                "params": r.get::<_, Option<String>>(4)?,
                "key_masked": key.as_deref().map(mask_key).unwrap_or_default(),
                "has_key": key.is_some(),
                "is_default": r.get::<_, i64>(6)? == 1,
            }))
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// 新增/更新 API 配置（id 空则新建；key 为空且已存在则保留旧 key）
#[tauri::command]
fn save_api_config(
    config: serde_json::Value,
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let id = config["id"].as_str().unwrap_or("").to_string();
    let name = config["name"].as_str().unwrap_or("未命名").to_string();
    let base_url = config["base_url"].as_str().unwrap_or("").to_string();
    let model = config["model"].as_str().map(|s| s.to_string());
    let params = config["params"].as_str().map(|s| s.to_string());
    let key_in = config["key"].as_str().map(|s| s.to_string()).unwrap_or_default();
    let is_default = config["is_default"].as_bool().unwrap_or(false);

    if base_url.is_empty() {
        return Err("Base URL 不能为空".into());
    }

    let final_id = if id.is_empty() {
        uuid::Uuid::new_v4().to_string()
    } else {
        id.clone()
    };

    // key 处理：传入空且已存在 → 保留旧值
    let key: String = if key_in.is_empty() && !id.is_empty() {
        conn.query_row(
            "SELECT COALESCE(key_ref, '') FROM api_configs WHERE id = ?1",
            [&id],
            |r| r.get(0),
        )
        .map_err(|e| format!("配置不存在: {e}"))?
    } else {
        key_in
    };

    if is_default {
        let _ = conn.execute("UPDATE api_configs SET is_default = 0", []);
    }

    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO api_configs (id, name, base_url, model, params, key_ref, is_default, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(id) DO UPDATE SET
            name=excluded.name, base_url=excluded.base_url, model=excluded.model,
            params=excluded.params, key_ref=excluded.key_ref, is_default=excluded.is_default",
        rusqlite::params![
            final_id,
            name,
            base_url,
            model,
            params,
            key,
            if is_default { 1 } else { 0 },
            now
        ],
    )
    .map_err(|e| e.to_string())?;

    Ok(json!({
        "id": final_id,
        "name": name,
        "base_url": base_url,
        "model": model,
        "has_key": !key.is_empty(),
        "is_default": is_default,
    }))
}

/// 删除 API 配置
#[tauri::command]
fn delete_api_config(id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM api_configs WHERE id = ?1", [&id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 测试 API 连通（OpenAI 兼容：GET {base_url}/models）
/// 若传入配置 id 且 key 为空，则使用库中已保存的 key
#[tauri::command]
fn test_api_connection(
    id: Option<String>,
    base_url: String,
    key: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let mut final_key = key;
    if final_key.is_empty() {
        if let Some(id) = &id {
            let conn = state.conn.lock().map_err(|e| e.to_string())?;
            let stored: Option<String> = conn
                .query_row(
                    "SELECT key_ref FROM api_configs WHERE id = ?1",
                    [id],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            final_key = stored.unwrap_or_default();
        }
    }
    if final_key.is_empty() {
        return Err("未提供 API Key".into());
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {final_key}"))
        .send()
        .map_err(|e| format!("连接失败: {e}"))?;
    if resp.status().is_success() {
        let body: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
        let count = body["data"].as_array().map(|a| a.len()).unwrap_or(0);
        Ok(format!("连接成功，可用模型 {} 个", count))
    } else {
        Err(format!("鉴权失败 HTTP {}", resp.status()))
    }
}

/// 术语表列表
#[tauri::command]
fn list_glossary(state: tauri::State<'_, AppState>) -> Result<Vec<serde_json::Value>, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT term, COALESCE(translation, ''), created_at FROM term_glossary ORDER BY created_at")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(json!({
                "term": r.get::<_, String>(0)?,
                "translation": r.get::<_, String>(1)?,
                "created_at": r.get::<_, String>(2)?,
            }))
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// 新增/更新术语（term 不存在则插入，存在则更新译文）
#[tauri::command]
fn save_glossary_entry(
    term: String,
    translation: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if term.trim().is_empty() {
        return Err("术语不能为空".into());
    }
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM term_glossary WHERE term = ?1)",
            [&term],
            |r| r.get::<_, i64>(0),
        )
        .map_err(|e| e.to_string())?
        == 1;
    if exists {
        conn.execute(
            "UPDATE term_glossary SET translation = ?2 WHERE term = ?1",
            rusqlite::params![term, translation],
        )
        .map_err(|e| e.to_string())?;
    } else {
        conn.execute(
            "INSERT INTO term_glossary (id, term, translation, created_at) VALUES (?1, ?2, ?3, datetime('now'))",
            rusqlite::params![uuid::Uuid::new_v4().to_string(), term, translation],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 删除术语
#[tauri::command]
fn delete_glossary_entry(term: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM term_glossary WHERE term = ?1", [&term])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 诊断信息（M5.2）：日志文件路径 + 上次是否异常退出
#[tauri::command]
fn get_diagnostics() -> serde_json::Value {
    let log_path = logging::log_dir()
        .map(|d| d.join("litdesk.log").to_string_lossy().to_string());
    json!({
        "log_path": log_path,
        "last_crash": LAST_CRASH.load(Ordering::Relaxed),
    })
}

/// 本地统计快照（M5.1）：返回当前开关状态与各事件计数/耗时
#[tauri::command]
fn get_stats() -> serde_json::Value {
    stats::snapshot()
}

/// 打开/关闭本地统计（M5.1），开关状态持久化到 settings 表
#[tauri::command]
fn set_stats_enabled(enabled: bool, state: tauri::State<'_, AppState>) -> Result<(), String> {
    stats::set_enabled(enabled);
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    write_setting(&conn, "stats_enabled", if enabled { "1" } else { "0" })
}

/// 清空本地统计（M5.1）
#[tauri::command]
fn reset_stats() {
    stats::reset();
}

/// 文献列表查询（M1 骨架，后续扩展筛选/分页）
#[tauri::command]
fn list_documents(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, title, authors, year, journal, tags, file_path, status, language, read_status, created_at
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
                "read_status": r.get::<_, String>(9)?,
                "created_at": r.get::<_, String>(10)?,
            }))
        })
        .map_err(|e| e.to_string())?;

    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// 设置文献阅读状态（F7）：unread 未读完 / read 已读完，由用户手动调整
#[tauri::command]
fn set_read_status(
    doc_id: String,
    read_status: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if read_status != "unread" && read_status != "read" {
        return Err(error_code::err(
            error_code::INTERNAL_ERR,
            format!("无效的阅读状态: {read_status}"),
        ));
    }
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let n = conn
        .execute(
            "UPDATE documents SET read_status = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![read_status, chrono::Utc::now().to_rfc3339(), doc_id],
        )
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(error_code::err(error_code::NOT_FOUND_ERR, "文献不存在"));
    }
    logging::info(&format!("阅读状态标记为 {read_status} doc={doc_id}"));
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
// 子模块命令经 #[macro_use] 展开时触发 rustc 的 never-type fallback lint（宏展开噪音，非代码缺陷）
#[allow(dependency_on_unit_never_type_fallback)]
pub fn run() {
    let app_result = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // M5.2 日志初始化（app_log_dir/litdesk.log）
            if let Ok(dir) = app.path().app_log_dir() {
                logging::init_logger(&dir);
            }
            // 初始化数据库：~/Library/Application Support/Rd学术阅读器/library.db
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            std::fs::create_dir_all(&data_dir)?;
            // M5.2 崩溃恢复：session.lock 残留说明上次未正常退出
            let marker = data_dir.join("session.lock");
            if marker.exists() {
                LAST_CRASH.store(true, Ordering::Relaxed);
                logging::warn("检测到上次异常退出（session.lock 残留），未完成任务需重新执行");
            } else {
                logging::info("应用正常启动");
            }
            let _ = std::fs::write(&marker, format!("pid={}\n", std::process::id()));
            let _ = SESSION_MARKER.set(marker);
            let db_path = data_dir.join("library.db");
            let conn = db::init_db(&db_path).expect("failed to init database");
            // 从持久化设置初始化本地统计开关（M5.1，默认关闭）
            stats::set_enabled(read_setting(&conn, "stats_enabled") == "1");
            let mineru = load_mineru_key(&conn, app.handle()).map(MinerUClient::new);
            app.manage(AppState {
                conn: Mutex::new(conn),
                mineru,
                translation_controls: Mutex::new(HashMap::new()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_documents,
            import_document,
            set_read_status,
            get_mineru_key,
            set_mineru_key,
            start_parse,
            read_parsed,
            list_tasks,
            clear_finished_tasks,
            delete_task,
            start_translate,
            pause_translate,
            resume_translate,
            read_translated,
            read_bilingual,
            list_api_configs,
            save_api_config,
            delete_api_config,
            test_api_connection,
            get_vision_config,
            save_vision_config,
            test_vision_connection,
            read_image_notes,
            recognize_paradigm,
            get_field_plan,
            start_digest,
            read_digest,
            read_digest_version,
            list_digest_versions,
            save_digest_edit,
            rollback_digest,
            export_document,
            print_document,
            list_glossary,
            save_glossary_entry,
            delete_glossary_entry,
            get_stats,
            set_stats_enabled,
            reset_stats,
            get_diagnostics,
            list_notes,
            read_note,
            save_note,
            delete_note,
            export_note,
            print_note,
            delete_notes_batch,
            export_notes_batch,
            rename_note,
            replace_in_notes
        ])
        .run(tauri::generate_context!());

    // 正常退出：清理崩溃标记（异常崩溃时该文件残留，供下次启动检测）
    if let Some(p) = SESSION_MARKER.get() {
        let _ = std::fs::remove_file(p);
    }
    if let Err(e) = app_result {
        eprintln!("应用运行异常: {e}");
    }
}
