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
