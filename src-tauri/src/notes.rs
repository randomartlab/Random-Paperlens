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

/// 校验并规范化笔记文件名
///
/// 过滤两平台非法字符的并集：Windows 为 `\ / : * ? " < > |`，macOS 为 `: /`；
/// 同时剔除控制字符与结尾的点/空格，并规避 Windows 保留设备名 ——
/// 否则用户在笔记名里输入 `*` `?` 等字符时，Windows 上写文件会直接失败（macOS 无此限制）。
pub(crate) fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| {
            !matches!(
                c,
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0'
            )
        })
        .filter(|c| !c.is_control())
        .take(120)
        .collect();
    // Windows 会静默丢弃结尾的点与空格，先自行去掉，保证落盘名可预期
    let cleaned = cleaned.trim().to_string();
    let cleaned = cleaned
        .trim_end_matches(|c: char| c == '.' || c == ' ')
        .trim()
        .to_string();
    if cleaned.is_empty() {
        return "未命名笔记".to_string();
    }
    // Windows 保留设备名不可作为文件名，追加下划线规避
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
        "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.contains(&cleaned.to_ascii_uppercase().as_str()) {
        return format!("{cleaned}_");
    }
    cleaned
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

/// 批量删除笔记（跳过不存在的文件），返回实际删除数量
#[tauri::command]
pub fn delete_notes_batch(
    names: Vec<String>,
    app: tauri::AppHandle,
) -> Result<usize, String> {
    let dir = notes_dir(&app)?;
    let mut removed = 0usize;
    for name in names {
        let path = dir.join(format!("{}.md", sanitize_name(&name)));
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| format!("删除笔记「{name}」失败: {e}"))?;
            removed += 1;
        }
    }
    Ok(removed)
}

/// 批量导出笔记到指定目录（md / html），返回导出成功的文件路径列表
#[tauri::command]
pub fn export_notes_batch(
    names: Vec<String>,
    format: String, // "md" | "html"
    out_dir: String,
    app: tauri::AppHandle,
) -> Result<Vec<String>, String> {
    let dir = notes_dir(&app)?;
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("创建导出目录失败: {e}"))?;
    let mut written = Vec::new();
    for name in names {
        let n = sanitize_name(&name);
        let content = match std::fs::read_to_string(dir.join(format!("{n}.md"))) {
            Ok(c) => c,
            Err(_) => continue, // 跳过已被外部删除的笔记
        };
        let data = if format == "html" {
            let body = crate::export::md_to_html(&content, &dir);
            wrap_html(&n, &body)
        } else {
            content
        };
        let out_path = std::path::Path::new(&out_dir).join(format!("{n}.{format}"));
        std::fs::write(&out_path, data).map_err(|e| format!("写入「{n}」失败: {e}"))?;
        written.push(out_path.to_string_lossy().to_string());
    }
    Ok(written)
}

/// 重命名笔记（.md 文件改名；目标已存在时报错，避免覆盖）
#[tauri::command]
pub fn rename_note(
    old_name: String,
    new_name: String,
    app: tauri::AppHandle,
) -> Result<String, String> {
    let dir = notes_dir(&app)?;
    let src = dir.join(format!("{}.md", sanitize_name(&old_name)));
    let dst_name = sanitize_name(&new_name);
    let dst = dir.join(format!("{dst_name}.md"));
    if !src.exists() {
        return Err(format!("笔记「{old_name}」不存在"));
    }
    if src == dst {
        return Ok(dst_name);
    }
    if dst.exists() {
        return Err(format!("已存在同名笔记「{dst_name}」，请换一个名称"));
    }
    std::fs::rename(&src, &dst).map_err(|e| format!("重命名失败: {e}"))?;
    Ok(dst_name)
}

/// 批量查找替换：对选中笔记执行纯文本替换，返回每篇替换次数
#[tauri::command]
pub fn replace_in_notes(
    names: Vec<String>,
    search: String,
    replace: String,
    app: tauri::AppHandle,
) -> Result<Vec<serde_json::Value>, String> {
    if search.is_empty() {
        return Err("查找内容不能为空".to_string());
    }
    let dir = notes_dir(&app)?;
    let mut out = Vec::new();
    for name in names {
        let n = sanitize_name(&name);
        let path = dir.join(format!("{n}.md"));
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let count = content.matches(&search).count();
        if count > 0 {
            let updated = content.replace(&search, &replace);
            std::fs::write(&path, updated).map_err(|e| format!("写入「{n}」失败: {e}"))?;
        }
        out.push(serde_json::json!({ "name": n, "count": count }));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::sanitize_name;

    /// Windows 非法字符必须被剔除，否则在 Windows 上写文件会直接失败（macOS 无此限制）
    #[test]
    fn sanitize_strips_cross_platform_illegal_chars() {
        assert_eq!(sanitize_name("研究*进展?"), "研究进展");
        assert_eq!(sanitize_name("a<b>c|d\"e"), "abcde");
        assert_eq!(sanitize_name("路径/穿越\\攻击"), "路径穿越攻击");
        assert_eq!(sanitize_name("冒号:分隔"), "冒号分隔");
    }

    /// Windows 会静默丢弃结尾的点与空格，且保留设备名不可作文件名
    #[test]
    fn sanitize_handles_trailing_dots_and_reserved_names() {
        assert_eq!(sanitize_name("结尾点..."), "结尾点");
        assert_eq!(sanitize_name("末尾空格   "), "末尾空格");
        assert_eq!(sanitize_name("CON"), "CON_");
        assert_eq!(sanitize_name("com1"), "com1_");
    }

    #[test]
    fn sanitize_falls_back_for_empty() {
        assert_eq!(sanitize_name("   "), "未命名笔记");
        assert_eq!(sanitize_name("///"), "未命名笔记");
    }
}
