use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// 统计事件类别（对应核心工作流）
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StatsEvent {
    /// PDF 导入
    Import,
    /// MinerU 解析
    Parse,
    /// 全文翻译
    Translate,
    /// 拆解
    Digest,
    /// 导出（md / html / pdf）
    Export,
    /// 命令报错
    Error,
}

impl StatsEvent {
    pub fn key(&self) -> &'static str {
        match self {
            StatsEvent::Import => "import",
            StatsEvent::Parse => "parse",
            StatsEvent::Translate => "translate",
            StatsEvent::Digest => "digest",
            StatsEvent::Export => "export",
            StatsEvent::Error => "error",
        }
    }
}

/// 单类别统计：次数 + 总耗时（毫秒）
#[derive(Default, Clone, Copy)]
struct EventStats {
    count: u64,
    total_ms: u64,
}

/// 全局开关：默认关闭；关闭后 record 为 no-op（完全无副作用，可视为彻底关闭）
static ENABLED: AtomicBool = AtomicBool::new(false);

static STATS: OnceLock<Mutex<HashMap<StatsEvent, EventStats>>> = OnceLock::new();

fn map() -> &'static Mutex<HashMap<StatsEvent, EventStats>> {
    STATS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 启用 / 禁用统计（不落盘，由调用方负责持久化到 settings 表）
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// 统计是否开启
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// 记录一次事件耗时（毫秒）。统计关闭时直接忽略。
pub fn record(event: StatsEvent, ms: u64) {
    if !is_enabled() {
        return;
    }
    if let Ok(mut m) = map().lock() {
        let e = m.entry(event).or_default();
        e.count += 1;
        e.total_ms += ms;
    }
}

/// 统计快照（开关 + 各事件计数/耗时）
pub fn snapshot() -> serde_json::Value {
    let mut events = Vec::new();
    if let Ok(m) = map().lock() {
        for (ev, s) in m.iter() {
            events.push(json!({
                "event": ev.key(),
                "count": s.count,
                "total_ms": s.total_ms,
            }));
        }
    }
    json!({
        "enabled": is_enabled(),
        "events": events,
    })
}

/// 清零统计（开关状态不变）
pub fn reset() {
    if let Ok(mut m) = map().lock() {
        m.clear();
    }
}
