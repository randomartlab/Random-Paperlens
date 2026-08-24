//! M7 笔记与摘录（F8）：笔记以纯 `.md` 文件存储于 `app_data_dir/notes/`，
//! 用户可直接在文件系统中打开/编辑/备份。导出 md / html，PDF 走系统打印窗口。

use serde::Serialize;
use std::path::PathBuf;
use tauri::Manager;

/// 笔记目录：app_data_dir/notes/
pub fn notes_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取数据目录失败: {e}"))?
        .join("notes");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建笔记目录失败: {e}"))?;
    Ok(dir)
}

/// 校验并规范化笔记文件名（去路径分隔符 / 非法字符，补 .md 后缀）
fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':' | '\0'))
        .take(120)
        .collect();
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() {
        "未命名笔记".to_string()
    } else {
        cleaned
    }
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// 将笔记 Markdown 包装为完整 HTML（带基础排版，浅/深色自适应）
fn wrap_html(title: &str, body: &str) -> String {
    let t = escape_html(title);
    format!(
        r#"<!DOCTYPE html><html lang="zh"><head><meta charset="utf-8"><title>{t}</title><style>
body{{max-width:800px;margin:40px auto;padding:0 24px;font-family:-apple-system,"PingFang SC","Hiragino Sans GB","Microsoft YaHei",sans-serif;line-height:1.75;color:#2d3748}}
h1{{font-size:1.6em;border-bottom:1px solid #e2e8f0;padding-bottom:.4em}}
blockquote{{border-left:3px solid #cbd5e0;margin:1em 0;padding:.3em 1em;color:#4a5568;background:#f7fafc}}
pre{{background:#f7fafc;padding:1em;overflow-x:auto;border-radius:6px}}
code{{background:#f7fafc;padding:.15em .4em;border-radius:4px}}
img{{max-width:100%}}
table{{border-collapse:collapse}} td,th{{border:1px solid #e2e8f0;padding:.4em .7em}}
@media (prefers-color-scheme:dark){{body{{background:#1a202c;color:#e2e8f0}}blockquote,pre,code{{background:#2d3748}}h1{{border-color:#4a5568}}td,th{{border-color:#4a5568}}}}
</style></head><body><h1>{t}</h1>{body}</body></html>"#
    )
}

#[derive(Serialize)]
pub struct NoteMeta {
    name: String,
    updated_at: String,
    size: u64,
}

/// 笔记列表（按修改时间倒序）
#[tauri::command]
pub fn list_notes(app: tauri::AppHandle) -> Result<Vec<NoteMeta>, String> {
    let dir = notes_dir(&app)?;
    let mut out = Vec::new();
    for e in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("md") {
            continue;
        }
        let meta = e.metadata().map_err(|e| e.to_string())?;
        let updated = meta
            .modified()
            .ok()
            .map(chrono::DateTime::<chrono::Utc>::from)
            .map(|t| t.to_rfc3339())
            .unwrap_or_default();
        out.push(NoteMeta {
            name: p
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string(),
            updated_at: updated,
            size: meta.len(),
        });
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(out)
}

/// 读取笔记内容
#[tauri::command]
pub fn read_note(name: String, app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let path = notes_dir(&app)?.join(format!("{}.md", sanitize_name(&name)));
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("读取笔记失败: {e}（文件可能已被移动或删除）"))?;
    Ok(serde_json::json!({ "name": sanitize_name(&name), "content": content }))
}

/// 保存笔记（UTF-8 写回 .md 文件；同名覆盖）
#[tauri::command]
pub fn save_note(name: String, content: String, app: tauri::AppHandle) -> Result<String, String> {
    let n = sanitize_name(&name);
    let path = notes_dir(&app)?.join(format!("{n}.md"));
    std::fs::write(&path, content).map_err(|e| format!("保存笔记失败: {e}"))?;
    Ok(n)
}

/// 删除笔记
#[tauri::command]
pub fn delete_note(name: String, app: tauri::AppHandle) -> Result<(), String> {
    let path = notes_dir(&app)?.join(format!("{}.md", sanitize_name(&name)));
    std::fs::remove_file(&path).map_err(|e| format!("删除笔记失败: {e}"))?;
    Ok(())
}

/// 导出笔记：md / html 直接写文件；pdf 请使用 print_note（系统打印窗口）
#[tauri::command]
pub fn export_note(
    name: String,
    format: String, // "md" | "html"
    out_path: String,
    app: tauri::AppHandle,
) -> Result<String, String> {
    let n = sanitize_name(&name);
    let dir = notes_dir(&app)?;
    let content = std::fs::read_to_string(dir.join(format!("{n}.md")))
        .map_err(|e| format!("读取笔记失败: {e}"))?;
    let data = if format == "html" {
        let body = crate::export::md_to_html(&content, &dir);
        wrap_html(&n, &body)
    } else {
        content
    };
    std::fs::write(&out_path, data).map_err(|e| format!("写入导出文件失败: {e}"))?;
    Ok(out_path)
}

/// 打印笔记（PDF 导出）：HTML 中转 → 系统打印窗口（macOS 打印面板支持「存储为 PDF」）
#[tauri::command]
pub async fn print_note(
    name: String,
    app: tauri::AppHandle,
) -> Result<String, String> {
    let n = sanitize_name(&name);
    let app2 = app.clone();
    let n2 = n.clone();
    let html = tauri::async_runtime::spawn_blocking(move || {
        let dir = notes_dir(&app2)?;
        let content = std::fs::read_to_string(dir.join(format!("{n2}.md")))
            .map_err(|e| format!("读取笔记失败: {e}"))?;
        let body = crate::export::md_to_html(&content, &dir);
        Ok::<String, String>(wrap_html(&n2, &body))
    })
    .await
    .map_err(|e| format!("打印线程异常: {e}"))??;
    spawn_print_html(&app, html, &format!("print-note-{}", std::process::id()))
}

/// 写临时 HTML + 打开打印窗口（print_document / print_note 共用），返回临时文件路径
pub fn spawn_print_html(
    app: &tauri::AppHandle,
    html: String,
    tag: &str,
) -> Result<String, String> {
    let cache_dir = app.path().app_cache_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;
    let tmp_path = cache_dir.join(format!("{tag}.html"));
    std::fs::write(&tmp_path, html).map_err(|e| format!("写入临时文件失败: {e}"))?;

    // 重复导出时先关闭旧打印窗口
    if let Some(old) = app.get_webview_window("print-window") {
        let _ = old.close();
    }

    let url =
        tauri::Url::from_file_path(&tmp_path).map_err(|e| format!("临时文件 URL 无效: {e:?}"))?;
    let win = tauri::WebviewWindowBuilder::new(
        app,
        "print-window",
        tauri::WebviewUrl::External(url),
    )
    .title("打印预览")
    .inner_size(820.0, 1080.0)
    .focused(true)
    .on_page_load(|window, payload| {
        if payload.event() == tauri::webview::PageLoadEvent::Finished {
            let _ = window.eval("setTimeout(()=>window.print(),150)");
        }
    })
    .build()
    .map_err(|e| format!("创建打印窗口失败: {e}"))?;

    // 窗口销毁时清理临时文件
    let tmp_clean = tmp_path.clone();
    win.on_window_event(move |event| {
        if let tauri::WindowEvent::Destroyed = event {
            let _ = std::fs::remove_file(&tmp_clean);
        }
    });

    Ok(tmp_path.to_string_lossy().to_string())
}
