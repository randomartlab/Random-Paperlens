//! M5.2 极简文件日志：纯 std 实现，避免引入第三方日志依赖（沙盒/离线环境友好）。
//! 日志写入 app_log_dir/litdesk.log，超 5MB 自动轮转为 litdesk.log.1。

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();
static FILE: OnceLock<Mutex<Option<std::fs::File>>> = OnceLock::new();

/// 初始化日志（应用启动时调用一次）：建目录、轮转、打开追加句柄
pub fn init_logger(dir: &Path) {
    let _ = LOG_DIR.set(dir.to_path_buf());
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let path = dir.join("litdesk.log");
    // 简单轮转：>5MB 时保留一份历史
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > 5 * 1024 * 1024 {
            let _ = std::fs::rename(&path, dir.join("litdesk.log.1"));
        }
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok();
    let _ = FILE.set(Mutex::new(file));
}

/// 日志文件所在目录（供前端"诊断"区展示）
pub fn log_dir() -> Option<&'static PathBuf> {
    LOG_DIR.get()
}

/// 读取日志末尾若干行（最多 8000 字符），供诊断面板一键回传，
/// 免去让用户自行定位日志文件
pub fn tail(lines_wanted: usize) -> String {
    let Some(dir) = LOG_DIR.get() else {
        return String::new();
    };
    let Ok(content) = std::fs::read_to_string(dir.join("litdesk.log")) else {
        return String::new();
    };
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(lines_wanted);
    let tail = lines[start..].join("\n");
    const MAX_CHARS: usize = 8000;
    let count = tail.chars().count();
    if count <= MAX_CHARS {
        tail
    } else {
        tail.chars().skip(count - MAX_CHARS).collect()
    }
}

fn write(level: &str, msg: &str) {
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let line = format!("[{ts}][{level}] {msg}\n");
    if let Some(m) = FILE.get() {
        if let Ok(mut g) = m.lock() {
            if let Some(f) = g.as_mut() {
                let _ = f.write_all(line.as_bytes());
                let _ = f.flush();
            }
        }
    }
    // 同时输出 stderr，便于开发期观察
    eprint!("{line}");
}

pub fn info(msg: &str) {
    write("INFO", msg);
}

pub fn warn(msg: &str) {
    write("WARN", msg);
}

pub fn error(msg: &str) {
    write("ERROR", msg);
}
