use serde_json::json;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// 翻译单元：按 Markdown 结构切分的最小翻译块
#[derive(Clone, Debug)]
pub struct Segment {
    pub index: usize,
    pub kind: String, // heading / paragraph / table / code / image_caption
    pub content: String,
}

/// 将 Markdown 按结构切分为翻译单元（标题/表格/代码块单独成段，其余按段落合并）
pub fn split_markdown(md: &str) -> Vec<Segment> {
    let mut segments: Vec<Segment> = Vec::new();
    let mut buf = String::new();
    let mut kind = String::from("paragraph");
    let mut in_code = false;
    let mut in_refs = false; // 进入参考文献章节后，段落归类为 reference（仅译标题）
    let mut idx = 0usize;

    let flush = |segments: &mut Vec<Segment>,
                 buf: &mut String,
                 kind: &mut String,
                 idx: &mut usize| {
        let c = buf.trim_end();
        if !c.is_empty() {
            segments.push(Segment {
                index: *idx,
                kind: kind.clone(),
                content: c.to_string(),
            });
            *idx += 1;
        }
        buf.clear();
    };

    for line in md.lines() {
        let t = line.trim();
        if t.starts_with("```") {
            flush(&mut segments, &mut buf, &mut kind, &mut idx);
            kind = "code".to_string();
            in_code = !in_code;
            buf.push_str(line);
            buf.push('\n');
            if !in_code {
                flush(&mut segments, &mut buf, &mut kind, &mut idx);
                kind = "paragraph".to_string();
            }
            continue;
        }
        if in_code {
            buf.push_str(line);
            buf.push('\n');
            continue;
        }
        if t.starts_with('#') {
            flush(&mut segments, &mut buf, &mut kind, &mut idx);
            kind = "heading".to_string();
            buf.push_str(line);
            buf.push('\n');
            flush(&mut segments, &mut buf, &mut kind, &mut idx);
            kind = "paragraph".to_string();
            let heading = t
                .trim_start_matches('#')
                .trim()
                .trim_end_matches('#')
                .trim();
            if !in_refs
                && heading.len() < 30
                && matches!(
                    heading,
                    "References"
                        | "REFERENCES"
                        | "Reference"
                        | "Bibliography"
                        | "BIBLIOGRAPHY"
                        | "参考文献"
                        | "引用文献"
                )
            {
                in_refs = true;
            }
            continue;
        }
        if t.starts_with('|') || t.starts_with("<table") {
            if kind != "table" {
                flush(&mut segments, &mut buf, &mut kind, &mut idx);
                kind = "table".to_string();
            }
            buf.push_str(line);
            buf.push('\n');
            continue;
        }
        if t.is_empty() {
            flush(&mut segments, &mut buf, &mut kind, &mut idx);
            continue;
        }
        // 图注（图片行）独立成段，避免与段落混译
        if t.starts_with("![") || t.starts_with("**Figure") || t.starts_with("**图") {
            flush(&mut segments, &mut buf, &mut kind, &mut idx);
            kind = "image_caption".to_string();
            buf.push_str(line);
            buf.push('\n');
            flush(&mut segments, &mut buf, &mut kind, &mut idx);
            kind = "paragraph".to_string();
            continue;
        }
        if kind != "paragraph" && kind != "reference" {
            flush(&mut segments, &mut buf, &mut kind, &mut idx);
            kind = if in_refs {
                "reference".to_string()
            } else {
                "paragraph".to_string()
            };
        } else if in_refs && kind != "reference" {
            kind = "reference".to_string();
        }
        buf.push_str(line);
        buf.push('\n');
    }
    flush(&mut segments, &mut buf, &mut kind, &mut idx);

    // 目录/清单型段落：单独标记为 toc，并把条目行连通为硬换行（"  \n"），
    // 这样渲染端与翻译后处理都不会把条目折成一行（见 normalize_translated）
    for seg in segments.iter_mut() {
        if seg.kind != "paragraph" {
            continue;
        }
        let lines: Vec<&str> = seg
            .content
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect();
        if looks_like_toc(&lines) {
            seg.kind = "toc".to_string();
            seg.content = lines.join("  \n");
        }
    }
    segments
}

/// 判断一段连续行是否为「目录 / 清单」型内容：
/// 至少 3 行，且 60% 以上的行是"标题 + 页码"或带点线引导符（如 `1. Introduction .... 3`）。
/// 命中后按条目逐行处理，不再当作普通段落折叠。
fn looks_like_toc(lines: &[&str]) -> bool {
    if lines.len() < 3 {
        return false;
    }
    let mut hits = 0usize;
    for l in lines {
        let t = l.trim();
        if t.is_empty() {
            continue;
        }
        let has_leader = t.contains("....") || t.contains('…');
        // 以页码结尾：末位是数字，且数字前面是空白 / 点 / 省略号
        let ends_with_page = t.ends_with(|c: char| c.is_ascii_digit())
            && t.chars().count() >= 5
            && t.chars()
                .rev()
                .skip_while(|c| c.is_ascii_digit())
                .next()
                .map(|c| c.is_whitespace() || c == '.' || c == '…')
                .unwrap_or(false);
        if has_leader || ends_with_page {
            hits += 1;
        }
    }
    hits * 10 >= lines.len().saturating_mul(6)
}

/// 翻译错误的结构化分类，供前端区分：网络不可达 / LLM 业务错误码 / 配置问题
#[derive(Debug, Clone, serde::Serialize)]
pub struct TranslateError {
    pub category: String, // "network" | "llm" | "config" | "internal"
    pub message: String,
    pub hint: String, // 可能的问题解释，展示在错误对话框中
    #[serde(skip)]
    pub retryable: bool,
}

impl TranslateError {
    fn new(category: &str, message: String, hint: &str) -> Self {
        Self {
            category: category.to_string(),
            message,
            hint: hint.to_string(),
            retryable: false,
        }
    }

    /// 用户取消（不是失败）：任务线程据此收尾，界面显示「已取消」而不是错误
    pub fn cancelled() -> Self {
        Self::new(
            "cancelled",
            "已取消".to_string(),
            "已完成的段落会保留，可再次点击继续",
        )
    }

    pub fn is_cancelled(&self) -> bool {
        self.category == "cancelled"
    }
}

/// 网络层错误分类：超时 / 连接失败 / 其他
fn classify_network_error(e: &reqwest::Error) -> TranslateError {
    if e.is_timeout() {
        TranslateError::new(
            "network",
            format!("请求超时: {e}"),
            "网络连接不稳定或 API 响应过慢。请检查网络后重试",
        )
    } else if e.is_connect() {
        TranslateError::new(
            "network",
            format!("无法连接到 API 服务器: {e}"),
            "可能的原因：① 本机未联网或网络受限；② API 地址（Base URL）填写错误。请检查网络与设置页配置",
        )
    } else {
        TranslateError::new(
            "network",
            format!("网络请求失败: {e}"),
            "可能的原因：① 本机未联网或网络受限；② 域名无法解析（DNS）。请检查网络后重试",
        )
    }
}

/// 从 OpenAI 兼容的 JSON 错误响应体中提取错误详情
pub fn extract_llm_error_detail(body: &str) -> Option<String> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(s) = v["error"]["message"].as_str() {
            return Some(s.to_string());
        }
        if let Some(s) = v["error"]["code"].as_str() {
            return Some(s.to_string());
        }
        if let Some(s) = v["message"].as_str() {
            return Some(s.to_string());
        }
        if let Some(s) = v["error"].as_str() {
            return Some(s.to_string());
        }
    }
    let t = body.trim();
    if !t.is_empty() {
        Some(if t.chars().count() > 200 {
            t.chars().take(200).collect::<String>() + "…"
        } else {
            t.to_string()
        })
    } else {
        None
    }
}

/// HTTP 状态码 → LLM 业务错误分类（含可能的问题解释）
fn classify_http_error(status: u16, body: &str) -> TranslateError {
    let detail = extract_llm_error_detail(body);
    let suffix = |d: &Option<String>| d.as_ref().map(|d| format!(": {d}")).unwrap_or_default();
    let mut err = match status {
        400 => TranslateError::new(
            "llm",
            format!("API 错误 HTTP 400{}", suffix(&detail)),
            "可能的问题：请求参数有误，通常是内容超出模型最大长度。可尝试减少单次翻译的段落，或更换支持更长上下文的模型",
        ),
        401 => TranslateError::new(
            "llm",
            format!("API 错误 HTTP 401{}", suffix(&detail)),
            "可能的问题：API Key 无效或已过期。请到「设置 → API 配置」核对并更新 Key",
        ),
        403 => TranslateError::new(
            "llm",
            format!("API 错误 HTTP 403{}", suffix(&detail)),
            "可能的问题：该 Key 没有此模型的访问权限，或账户余额不足。请检查套餐权限与账户余额",
        ),
        404 => TranslateError::new(
            "llm",
            format!("API 错误 HTTP 404{}", suffix(&detail)),
            "可能的问题：API 地址（Base URL）或模型名称拼写错误，或该模型不存在。请核对设置页配置",
        ),
        429 => TranslateError::new(
            "llm",
            format!("API 错误 HTTP 429{}", suffix(&detail)),
            "可能的问题：请求频率超过接口限流。请稍等片刻后重试",
        ),
        _ if status >= 500 => TranslateError::new(
            "llm",
            format!("API 错误 HTTP {status}{}", suffix(&detail)),
            "可能的问题：API 服务端临时故障或网关错误。请稍后重试",
        ),
        _ => TranslateError::new(
            "llm",
            format!("API 错误 HTTP {status}{}", suffix(&detail)),
            "未知的 API 错误，请将错误详情反馈给管理员",
        ),
    };
    if status == 429 || status >= 500 {
        err.retryable = true;
    }
    err
}

/// 外挂视觉模型配置（可选）：用于识别图片类型并辅助图注翻译
#[derive(Clone, Default)]
pub struct VisionConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

impl VisionConfig {
    pub fn enabled(&self) -> bool {
        !self.base_url.trim().is_empty()
            && !self.api_key.trim().is_empty()
            && !self.model.trim().is_empty()
    }

    /// 调用 OpenAI 兼容的视觉接口分析单张图片，返回「类型 + 要点」描述
    pub fn describe_image(&self, path: &Path) -> Option<String> {
        let bytes = std::fs::read(path).ok()?;
        if bytes.is_empty() || bytes.len() > 12 * 1024 * 1024 {
            return None;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        let mime = match ext.as_str() {
            "png" => "image/png",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "bmp" => "image/bmp",
            _ => "image/jpeg",
        };
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .ok()?;
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let body = json!({
            "model": self.model,
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": "请分析这张图片。要求：1) 判断图片类型，只从以下范围选择：数据图表 / 示意图 / 案例照片 / 地图 / 表格截图 / 其他；2) 用一句中文概括主要内容（数据图说明坐标与关键趋势，案例图说明场景与对象）。仅输出两行结果，不要多余内容。" },
                    { "type": "image_url", "image_url": { "url": format!("data:{mime};base64,{}", base64_encode(&bytes)) } }
                ]
            }],
            "max_tokens": 300
        });
        let resp = client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .ok()?;
        let v: serde_json::Value = resp.json().ok()?;
        v["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// 测试视觉链路连通性：发送一张 1×1 图片，验证 Base URL / Key / 模型是否有效
    pub fn test_connection(&self) -> Result<String, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("构建 HTTP 客户端失败: {e}"))?;
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let tiny_png =
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
        let body = json!({
            "model": self.model,
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": "请回复两个字：正常" },
                    { "type": "image_url", "image_url": { "url": format!("data:image/png;base64,{tiny_png}") } }
                ]
            }],
            "max_tokens": 20
        });
        let resp = client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .map_err(|e| format!("网络请求失败: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().unwrap_or_default();
            let detail = extract_llm_error_detail(&body_text).unwrap_or_default();
            return Err(format!("HTTP {status} {detail}"));
        }
        let v: serde_json::Value = resp.json().map_err(|e| format!("响应解析失败: {e}"))?;
        let reply = v["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_string();
        Ok(format!(
            "连接正常，模型响应：{}",
            reply.chars().take(60).collect::<String>()
        ))
    }
}

/// 从 Markdown 中收集所有图片引用路径（去重，相对路径）
pub fn collect_image_refs(md: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut start = 0usize;
    while let Some(rel) = md[start..].find("![") {
        let img_start = start + rel;
        let after_open = img_start + 2;
        if let Some(rel2) = md[after_open..].find("](") {
            let path_start = after_open + rel2 + 2;
            if let Some(rel3) = md[path_start..].find(')') {
                let path = md[path_start..path_start + rel3].trim().to_string();
                if !path.is_empty() && !path.starts_with("http") && !out.contains(&path) {
                    out.push(path);
                }
                start = path_start + rel3 + 1;
                continue;
            }
        }
        start = after_open;
    }
    out
}

/// 从单个段（图注行）提取首个图片路径
fn extract_image_path(text: &str) -> Option<String> {
    let after = text.find("![")? + 2;
    let close = text[after..].find("](")? + after + 2;
    let end = text[close..].find(')')? + close;
    let p = text[close..end].trim().to_string();
    if p.is_empty() || p.starts_with("http") {
        None
    } else {
        Some(p)
    }
}

/// RFC 4648 Base64 编码（无外部依赖）
fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// 译文段落化：表格/代码块保持原样；目录按条目保留换行（硬换行）；
/// 其余（段落/标题/图注/参考文献）将换行折叠为空格，
/// 避免 LLM 一句一换行导致渲染成碎片段落
fn normalize_translated(kind: &str, text: &str) -> String {
    match kind {
        "table" | "code" => text.trim().to_string(),
        // 目录：一条一行，用 Markdown 硬换行连接，渲染时逐行显示
        "toc" => text
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join("  \n"),
        _ => text.split_whitespace().collect::<Vec<_>>().join(" "),
    }
}

/// 判断某段是否为图注/表注（用于注入图片分析上下文）
fn is_caption_line(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with("Figure")
        || t.starts_with("Fig.")
        || t.starts_with("Figure ")
        || t.starts_with("Table")
        || t.starts_with("Tab.")
        || t.starts_with("Schema")
        || t.starts_with("Diagram")
        || t.starts_with("Photo")
        || t.starts_with("图")
        || t.starts_with("表")
        || t.starts_with("插图")
}

/// 检测 LLM 返回的是否为「对话性/拒绝性回复」而非译文
/// （例如索要正文、说明规则、提示缺少内容等）。命中则判定为无效译文，重试或兜底原文。
fn looks_like_meta_reply(text: &str) -> bool {
    const MARKERS: [&str; 19] = [
        "请补充",
        "请提供",
        "请输入",
        "请告诉我",
        "我没有收到",
        "翻译要求",
        "正文内容",
        "完整文本",
        "无法翻译",
        "只提供了",
        "请您提供",
        "请发送",
        "请上传",
        "需要翻译的",
        "请重新发送",
        "我无法翻译",
        "仅包含图片引用",
        "没有需要翻译",
        "您提供的内容",
    ];
    MARKERS.iter().any(|m| text.contains(m))
}

/// 判断段内容是否仅为 Markdown 图片引用（![](...) 语法，去掉引用后无其他文字）
fn is_pure_image_ref(text: &str) -> bool {
    let mut rest = text;
    loop {
        let Some(i) = rest.find("![") else { break };
        let after_open = &rest[i + 2..];
        let Some(j) = after_open.find("](") else { break };
        let path_start = i + 2 + j + 2;
        let Some(k) = rest[path_start..].find(')') else { break };
        rest = &rest[path_start + k + 1..];
    }
    rest.trim().is_empty()
}

/// 翻译方向：支持中文文献 → 英文（ZhToEn）与英文文献 → 中文（EnToZh）
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TranslateDirection {
    #[default]
    EnToZh,
    ZhToEn,
}

impl TranslateDirection {
    /// 解析前端传入的方向标识（"en_to_zh" / "zh_to_en"），未知值回退为默认
    pub fn parse(s: &str) -> Self {
        match s {
            "zh_to_en" => TranslateDirection::ZhToEn,
            _ => TranslateDirection::EnToZh,
        }
    }
}

/// OpenAI 兼容翻译客户端
pub struct Translator {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub glossary: Vec<(String, String)>, // (术语, 译文)
    pub vision_notes: HashMap<String, String>, // 图片路径 → 视觉分析（可选）
    pub direction: TranslateDirection,
}

impl Translator {
    /// 按当前方向返回「目标语言」的中文名与英文名
    fn target_lang(&self) -> (&'static str, &'static str) {
        match self.direction {
            TranslateDirection::ZhToEn => ("英文", "English"),
            TranslateDirection::EnToZh => ("简体中文", "Chinese"),
        }
    }

    fn system_prompt(&self) -> String {
        let mut s = match self.direction {
            TranslateDirection::ZhToEn => {
                "You are a professional academic paper translation assistant. Translate the user's content into English. Rules:\
                 1) Preserve Markdown formatting, table structure, code blocks and math formulas (LaTeX $...$) unchanged;\
                 2) Keep proper nouns, institution names, personal names and citation numbers [n] unchanged;\
                 3) Translate accurately in academic style, strictly following the glossary;\
                 4) The translation must follow the register and grammar of English academic writing: avoid direct word-for-word translation from Chinese (Chinglish),\
                 reorder sentences according to English conventions, split or merge long sentences where appropriate, and use standard scholarly expressions\
                 (e.g. \"This paper proposes\" \"The results show that\"). It should read as if written by a native English-speaking scholar;\
                 5) Table-of-contents / list-like content (one entry per line) must be translated entry by entry and keep one entry per line;\
                 keep the numbering and page numbers unchanged, never merge entries into one paragraph."
                    .to_string()
            }
            TranslateDirection::EnToZh => {
                "你是一名专业的学术论文翻译助手。将用户提供的内容翻译为简体中文。规则：\
                     1) 完整保留 Markdown 格式、表格结构、代码块与数学公式（LaTeX $...$）不变；\
                     2) 保留英文专有名词、机构名、人名与文献引用编号 [n] 不变；\
                     3) 翻译应准确、学术化，术语严格按术语表执行；\
                     4) 译文须符合中文学术论文的语体与语法：避免欧化句式（少用“被”字被动句、\
                     避免英文式长定语从句前置），按中文语序重组句子，长句可适当拆分；\
                     使用中文学术惯用表达（如“本文提出”“研究表明”“值得注意的是”），\
                     读起来应像中文母语学者撰写，而不是逐词直译的翻译腔；\
                     5) 目录、清单、逐条排列的内容（每条一行）必须逐条翻译并保持一条一行，\
                     编号与页码原样保留，禁止把多条合并成一段。"
                    .to_string()
            }
        };
        if !self.glossary.is_empty() {
            s.push_str("\n\n术语表（必须严格遵守）：\n");
            for (t, tr) in &self.glossary {
                s.push_str(&format!("{t} = {tr}\n"));
            }
        }
        s
    }

    fn call_once(&self, text: &str) -> Result<String, TranslateError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| TranslateError::new("config", format!("构建 HTTP 客户端失败: {e}"), "请检查网络环境后重试"))?;
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let body = json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": self.system_prompt() },
                { "role": "user", "content": text }
            ],
            "temperature": 0.3
        });
        let resp = client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .map_err(|e| classify_network_error(&e))?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().unwrap_or_default();
            return Err(classify_http_error(status.as_u16(), &body_text));
        }
        let data: serde_json::Value = resp.json().map_err(|e| {
            TranslateError::new(
                "llm",
                format!("响应解析失败: {e}"),
                "可能的问题：API 返回了非预期格式（可能不是 OpenAI 兼容接口）。请确认 Base URL 指向正确的兼容服务",
            )
        })?;
        data["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                TranslateError::new(
                    "llm",
                    "响应缺少翻译内容".to_string(),
                    "可能的问题：模型未按预期返回翻译结果。可尝试更换模型，或检查内容是否超出模型支持范围",
                )
            })
    }

    /// 通用对话调用（拆解等任务复用）：自定义 system / user / temperature
    pub fn chat(&self, system: &str, user: &str, temperature: f64) -> Result<String, TranslateError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(180))
            .build()
            .map_err(|e| TranslateError::new("config", format!("构建 HTTP 客户端失败: {e}"), "请检查网络环境后重试"))?;
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let body = json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user }
            ],
            "temperature": temperature
        });
        let resp = client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .map_err(|e| classify_network_error(&e))?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().unwrap_or_default();
            return Err(classify_http_error(status.as_u16(), &body_text));
        }
        let data: serde_json::Value = resp.json().map_err(|e| {
            TranslateError::new(
                "llm",
                format!("响应解析失败: {e}"),
                "可能的问题：API 返回了非预期格式（可能不是 OpenAI 兼容接口）。请确认 Base URL 指向正确的兼容服务",
            )
        })?;
        data["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                TranslateError::new(
                    "llm",
                    "响应缺少内容".to_string(),
                    "可能的问题：模型未按预期返回结果。可尝试更换模型，或检查内容是否超出模型支持范围",
                )
            })
    }

    /// 单段翻译（指数退避重试 ≤3 次，仅对可恢复错误重试）
    pub fn translate(
        &self,
        text: &str,
        kind: &str,
        note: Option<&str>,
    ) -> Result<String, TranslateError> {
        // 图片引用行（内容仅为 ![](...) 语法）：无需调用模型，直接保留，避免模型输出对话性回复
        if kind == "image_caption" && is_pure_image_ref(text) {
            return Ok(text.to_string());
        }
        let mut content = text.to_string();
        // 章节标题：明确只输出译名，避免模型索要正文
        if kind == "heading" {
            content = match self.direction {
                TranslateDirection::ZhToEn => format!(
                    "这是论文的章节标题。请直接输出其英文译名（如 \"8.4. 讨论\" → \"8.4. Discussion\"），\
                     只输出译名本身，不要输出任何其他内容。\n\n{text}"
                ),
                TranslateDirection::EnToZh => format!(
                    "这是论文的章节标题。请直接输出其中文译名（如 \"8.4. Discussion\" → \"8.4. 讨论\"），\
                     只输出译名本身，不要输出任何其他内容。\n\n{text}"
                ),
            };
        }
        // 参考文献：只译标题，保留作者/期刊/年份/页码
        if kind == "reference" {
            let (lang, _) = self.target_lang();
            content = format!(
                "这是一条参考文献条目。请仅将其中的论文或书籍标题翻译为{lang}；\
                 作者名、期刊名、年份、卷期、页码、DOI、出版社及编号一律保持原样。\
                 若条目本身不含标题（如纯作者+期刊），则整条保持原文。\
                 只输出处理后的条目本身，不要添加任何解释或标注。\n\n{text}"
            );
        }
        // 图注/表注：注入视觉分析上下文（如有），并要求保留图片语法
        if kind == "image_caption" || is_caption_line(text) {
            if let Some(n) = note {
                content = format!(
                    "【图片自动分析】{n}\n\n该段为图注或表注。请结合图片分析准确理解内容，\
                     但只忠实翻译图注文字本身，不得增删或添加分析内容；\
                     若包含 Markdown 图片语法（![...](...)）请原样保留。\n\n{content}"
                );
            } else if text.contains("![") {
                content = format!(
                    "该段包含图片引用，请保持 Markdown 图片语法不变，仅翻译其余文字。\n\n{content}"
                );
            }
        }
        // 全局强化：无论内容多短（如仅标题），都必须输出译文本身，禁止任何对话性回复
        content = format!(
            "请只输出上面内容的译文本身，不要输出任何解释、提示、要求补充内容、规则说明或其他与译文无关的文字。\n\n{content}"
        );
        let mut last_err = TranslateError::new("llm", "未知错误".to_string(), "请重试");
        for attempt in 0..3 {
            match self.call_once(&content) {
                Ok(r) => {
                    if looks_like_meta_reply(&r) {
                        last_err = TranslateError::new(
                            "llm",
                            format!("模型返回了非译文内容（第 {} 次尝试）", attempt + 1),
                            "模型给出了对话性回复而非译文，已自动重试",
                        );
                        if attempt < 2 {
                            // 元回复重试：追加更强指令，不等待退避
                            content = format!(
                                "【再次提醒】你上一次的输出不是译文。必须只输出上面内容的译文本身，\
                                 不要解释、不要提示、不要索要内容、不要说明规则。\n\n{content}"
                            );
                            continue;
                        }
                        // 重试耗尽 → 兜底为原文，避免把对话性垃圾写入译文
                        return Ok(text.to_string());
                    }
                    return Ok(r);
                }
                Err(e) => {
                    if !e.retryable {
                        return Err(e);
                    }
                    last_err = e;
                    if attempt < 2 {
                        std::thread::sleep(std::time::Duration::from_secs(1 << attempt));
                    }
                }
            }
        }
        let category = last_err.category.clone();
        let hint = last_err.hint.clone();
        Err(TranslateError {
            category,
            message: format!("翻译失败（已重试 3 次）: {}", last_err.message),
            hint,
            retryable: false,
        })
    }
}

/// 并发翻译一批段（并行度 ≤concurrency），每段完成立即回调进度；
/// paused 为 Some 时暂停（在途请求完成后不再启动新段，直到恢复）
pub fn translate_batch(
    translator: &Translator,
    segments: &mut [Segment],
    concurrency: usize,
    paused: Option<&AtomicBool>,
    cancelled: Option<&AtomicBool>,
    on_done: &mut (dyn FnMut(usize, &str, &str) + Send), // (index, kind, translated_text)
) -> Result<(), TranslateError> {
    let concurrency = concurrency.max(1);
    let results: Vec<Mutex<Option<Result<String, TranslateError>>>> =
        (0..segments.len()).map(|_| Mutex::new(None)).collect();
    let failed: Mutex<Option<TranslateError>> = Mutex::new(None);
    let active: Mutex<usize> = Mutex::new(0);
    let cond = std::sync::Condvar::new();
    // 进度回调需在多个工作线程间共享，用互斥锁串行化（每段完成即回调，避免前端长时间停留 0%）
    let cb: Mutex<&mut (dyn FnMut(usize, &str, &str) + Send)> = Mutex::new(on_done);

    // 每段对应的图片分析注记：最近一个图注行引用的图片
    let mut seg_notes: Vec<Option<&str>> = Vec::with_capacity(segments.len());
    let mut last_note: Option<&str> = None;
    for seg in segments.iter() {
        if seg.kind == "image_caption" {
            if let Some(p) = extract_image_path(&seg.content) {
                last_note = translator.vision_notes.get(&p).map(|s| s.as_str());
            }
        }
        seg_notes.push(last_note);
    }

    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for (i, seg) in segments.iter().enumerate() {
            // 取消：不再启动新段，立即收尾（在途段跑完即止）；
            // 已完成段落已由回调逐段落盘，重新开始时续跑
            if let Some(c) = cancelled {
                if c.load(Ordering::SeqCst) {
                    let mut f = failed.lock().unwrap();
                    if f.is_none() {
                        *f = Some(TranslateError::cancelled());
                    }
                    break;
                }
            }
            // 暂停：暂停标志置位时不再启动新段，直到恢复
            if let Some(p) = paused {
                while p.load(Ordering::SeqCst) {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            }
            // 信号量限流：在途线程达到并发上限时等待
            {
                let mut n = active.lock().unwrap();
                while *n >= concurrency {
                    n = cond.wait(n).unwrap();
                }
                *n += 1;
            }
            let text = seg.content.clone();
            let kind = seg.kind.clone();
            let idx = seg.index;
            let note = seg_notes[i];
            let slot = &results[i];
            let failed_ref = &failed;
            let active_ref = &active;
            let cond_ref = &cond;
            let cb_ref = &cb;
            handles.push(scope.spawn(move || {
                // 译文段落化：按原文段落结构归一化换行/分段，避免一句一换行
                let r = translator
                    .translate(&text, &kind, note)
                    .map(|t| normalize_translated(&kind, &t));
                if let Err(e) = &r {
                    let mut f = failed_ref.lock().unwrap();
                    if f.is_none() {
                        *f = Some(e.clone());
                    }
                } else if let Ok(t) = &r {
                    // 段翻译成功 → 立即回调（前端推进进度 + 逐段落盘断点续传）
                    cb_ref.lock().unwrap()(idx, &kind, t);
                }
                *slot.lock().unwrap() = Some(r);
                let mut n = active_ref.lock().unwrap();
                *n -= 1;
                cond_ref.notify_one();
            }));
        }
        for h in handles {
            let _ = h.join();
        }
    });

    if let Some(e) = failed.lock().unwrap().clone() {
        return Err(e);
    }
    // 将翻译结果回填到 segments（已通过回调持久化，此处仅同步内存供拼接使用）
    for (i, seg) in segments.iter_mut().enumerate() {
        if let Some(Ok(t)) = results[i].lock().unwrap().clone() {
            seg.content = t;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toc_block_is_detected_and_kept_per_line() {
        let md = "# 论文标题\n\n1. Introduction .... 3\n2. Methods .... 5\n3. Results .... 9\n\n正文段落内容。";
        let segs = split_markdown(md);
        let toc = segs
            .iter()
            .find(|s| s.kind == "toc")
            .expect("应识别出 toc 段");
        assert!(toc.content.contains("Introduction .... 3"));
        assert_eq!(
            toc.content.matches("  \n").count(),
            2,
            "三条目录之间应保留两条硬换行：{}",
            toc.content
        );
        assert!(segs
            .iter()
            .any(|s| s.kind == "paragraph" && s.content.contains("正文段落")));
    }

    #[test]
    fn normal_paragraph_is_not_treated_as_toc() {
        let md = "第一段内容，讲了一件很长的事情，并且描述了背景。\n第二段内容，继续讲方法与结果。\n第三段收尾。";
        let segs = split_markdown(md);
        assert!(
            segs.iter().all(|s| s.kind != "toc"),
            "普通段落不应该被当成目录"
        );
    }

    #[test]
    fn toc_translation_keeps_hard_breaks() {
        let out = normalize_translated("toc", "1、引言 .... 3\n2、方法 .... 5\n");
        assert_eq!(out, "1、引言 .... 3  \n2、方法 .... 5");
    }

    #[test]
    fn plain_paragraph_translation_still_collapses() {
        let out = normalize_translated("paragraph", "一句一换行\n的译文\n应当折叠");
        assert_eq!(out, "一句一换行 的译文 应当折叠");
    }
}
