//! M5.2 统一错误码体系：错误信息统一携带 `[E-xxxx]` 前缀，
//! 便于日志定位与前端展示（前端无需改动即可显示错误码文本）。
//! 编码分组：1000 导入 / 2000 解析 / 3000 翻译 / 4000 拆解 / 9000 系统。

// 导入
pub const IMPORT_INVALID: &str = "E-1001"; // 文件校验失败（非 PDF / 损坏）
pub const IMPORT_DUP: &str = "E-1002"; // 重复导入
pub const IMPORT_IO: &str = "E-1003"; // 文件读取/复制失败
// 解析
pub const PARSE_FAILED: &str = "E-2001"; // MinerU 解析失败
// 翻译
pub const TRANSLATE_FAILED: &str = "E-3001"; // 翻译失败（通用）
pub const TRANSLATE_NETWORK: &str = "E-3014"; // 网络不可达/超时
pub const TRANSLATE_CONFIG: &str = "E-3015"; // 配置错误（Base URL/模型名）
pub const TRANSLATE_LLM: &str = "E-3016"; // 模型返回异常（鉴权/限流/格式）
// 拆解
pub const DIGEST_FAILED: &str = "E-4001"; // 拆解失败
// 系统
pub const NOT_FOUND_ERR: &str = "E-9003"; // 资源不存在
pub const INTERNAL_ERR: &str = "E-9999"; // 内部错误（参数非法等）

/// 生成带错误码的错误信息：`[E-1001] 消息`
pub fn err(code: &str, msg: impl AsRef<str>) -> String {
    format!("[{code}] {}", msg.as_ref())
}

/// 从翻译错误分类（translate::TranslateError.category）映射到错误码
pub fn from_translate_category(category: &str) -> &'static str {
    match category {
        "network" => TRANSLATE_NETWORK,
        "llm" => TRANSLATE_LLM,
        "config" => TRANSLATE_CONFIG,
        _ => TRANSLATE_FAILED,
    }
}
