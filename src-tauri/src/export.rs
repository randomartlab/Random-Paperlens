//! 导出模块（M4.1）：Markdown / 自包含 HTML（内嵌 base64 图片 + 主题）
//!
//! - Markdown：原文 + 译文 + 拆解 三段式，图片保留相对路径
//! - HTML：单文件自包含（图片内嵌 base64，含浅色/米色两套主题），可离线打开

use std::path::Path;

/// 导出内容各组成部分
pub struct ExportParts {
    pub title: String,
    pub original: String,
    pub translated: Option<String>,
    pub digest: Option<String>,
}

/// 导出内容范围：决定哪些部分写入导出文件
///
/// - `all`：原文 + 译文 + 拆解
/// - `no-original`：译文 + 拆解（除原文外的全部解析内容）
/// - `bilingual`：原文 + 译文
/// - `translated`：仅译文（中文翻译）
/// - `digest`：仅拆解结果
#[derive(Clone, Copy)]
pub struct ExportScope {
    pub original: bool,
    pub translated: bool,
    pub digest: bool,
}

impl ExportScope {
    pub fn parse(scope: &str) -> Self {
        match scope {
            "no-original" => Self {
                original: false,
                translated: true,
                digest: true,
            },
            "bilingual" => Self {
                original: true,
                translated: true,
                digest: false,
            },
            "translated" => Self {
                original: false,
                translated: true,
                digest: false,
            },
            "digest" => Self {
                original: false,
                translated: false,
                digest: true,
            },
            _ => Self {
                original: true,
                translated: true,
                digest: true,
            },
        }
    }
}

/// 拼装 Markdown（按导出范围择取章节）
pub fn compose_markdown(parts: &ExportParts, scope: &str) -> String {
    let scope = ExportScope::parse(scope);
    let mut out = format!("# {}\n\n", parts.title);
    out.push_str("> 由 Rd学术阅读器导出\n\n");
    if scope.original {
        out.push_str("---\n\n## 原文\n\n");
        out.push_str(&parts.original);
        out.push_str("\n\n");
    }
    if scope.translated {
        if let Some(t) = &parts.translated {
            out.push_str("---\n\n## 译文\n\n");
            out.push_str(t);
            out.push_str("\n\n");
        }
    }
    if scope.digest {
        if let Some(d) = &parts.digest {
            out.push_str("---\n\n## AI 拆解\n\n");
            out.push_str(d);
            out.push('\n');
        }
    }
    out
}

/// 拼装自包含 HTML（内嵌图片 + 主题，按导出范围择取章节）
pub fn compose_html(parts: &ExportParts, base_dir: &Path, theme: &str, scope: &str) -> String {
    let scope = ExportScope::parse(scope);
    let mut sections: Vec<String> = Vec::new();
    if scope.original {
        sections.push(section_html("原文", &parts.original, base_dir));
    }
    if scope.translated {
        if let Some(t) = &parts.translated {
            sections.push(section_html("译文", t, base_dir));
        }
    }
    if scope.digest {
        if let Some(d) = &parts.digest {
            sections.push(section_html("AI 拆解", d, base_dir));
        }
    }
    let body = format!(
        "<h1>{}</h1>\n<p class=\"meta\">由 Rd学术阅读器导出</p>\n{}",
        escape_html(&parts.title),
        sections.join("\n<hr/>\n")
    );
    format!(
        "<!DOCTYPE html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\"/>\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"/>\
         <title>{}</title><style>{}</style></head><body><main>{}</main></body></html>",
        escape_html(&parts.title),
        theme_css(theme),
        body
    )
}

fn section_html(title: &str, md: &str, base_dir: &Path) -> String {
    format!("<section><h2>{title}</h2>{}</section>", md_to_html(md, base_dir))
}

/// 主题 CSS（浅色 / 米色）
fn theme_css(theme: &str) -> String {
    let (bg, fg, muted, border, head, code_bg) = if theme == "sepia" {
        ("#f5ecd9", "#3a3226", "#8a7f6a", "#ddd0b4", "#2b2418", "#efe4c9")
    } else {
        ("#ffffff", "#1f2937", "#6b7280", "#e5e7eb", "#111827", "#f3f4f6")
    };
    format!(
        "body{{margin:0;background:{bg};color:{fg};font-family:-apple-system,'PingFang SC','Microsoft YaHei',sans-serif;line-height:1.75}}\
         main{{max-width:820px;margin:0 auto;padding:40px 24px}}\
         h1{{font-size:1.6em;border-bottom:2px solid {border};padding-bottom:.3em}}\
         h2{{font-size:1.3em;border-bottom:1px solid {border};padding-bottom:.2em;margin-top:1.6em}}\
         h3{{font-size:1.1em;color:{head}}}h4{{color:{head}}}\
         .meta{{color:{muted};font-size:.85em}}\
         table{{border-collapse:collapse;width:100%;margin:1em 0;font-size:.92em}}\
         th,td{{border:1px solid {border};padding:6px 10px;text-align:left}}\
         th{{background:{code_bg}}}code{{background:{code_bg};border-radius:4px;padding:1px 5px;font-size:.9em}}\
         pre{{background:{code_bg};border-radius:8px;padding:12px;overflow-x:auto}}\
         pre code{{background:transparent;padding:0}}\
         img{{max-width:100%;border-radius:8px;margin:.5em 0}}\
         blockquote{{border-left:3px solid {border};margin:.8em 0;padding:.2em 1em;color:{muted}}}\
         hr{{border:none;border-top:1px solid {border};margin:2em 0}}"
    )
}

/// 简易 Markdown → HTML（覆盖 标题/表格/代码/图片/引用/列表/段落/行内样式）
pub fn md_to_html(md: &str, base_dir: &Path) -> String {
    let mut stats = EmbedStats::default();
    let mut out = String::new();
    let mut in_code = false;
    let mut in_list = false;
    let mut code_buf: Vec<String> = Vec::new();
    let mut para_buf: Vec<String> = Vec::new();
    let mut para_started = false;

    let flush_para = |out: &mut String, para: &mut Vec<String>, started: &mut bool| {
        if *started {
            out.push_str("<p>");
            out.push_str(&inline_html(&para.join(" ")));
            out.push_str("</p>\n");
            para.clear();
            *started = false;
        }
    };

    let mut rows: Vec<String> = Vec::new();
    for raw in md.split('\n') {
        let line = raw.trim_end();
        let t = line.trim();
        if in_code {
            if t.starts_with("```") {
                in_code = false;
                out.push_str("<pre><code>");
                out.push_str(&escape_html(&code_buf.join("\n")));
                out.push_str("</code></pre>\n");
                code_buf.clear();
                continue;
            }
            code_buf.push(line.to_string());
            continue;
        }
        if t.starts_with("```") {
            flush_para(&mut out, &mut para_buf, &mut para_started);
            close_list(&mut out, &mut in_list);
            rows.clear();
            in_code = true;
            continue;
        }
        if t.starts_with('|') {
            if !rows.is_empty() && para_started {
                flush_para(&mut out, &mut para_buf, &mut para_started);
            }
            close_list(&mut out, &mut in_list);
            // 表格分隔行（|---|）
            if is_table_sep(t) {
                continue;
            }
            rows.push(line.to_string());
            continue;
        }
        if !rows.is_empty() {
            out.push_str(&table_html(&rows));
            rows.clear();
        }
        if let Some(h) = heading_level(t) {
            flush_para(&mut out, &mut para_buf, &mut para_started);
            close_list(&mut out, &mut in_list);
            out.push_str(&format!(
                "<h{0}>{1}</h{0}>\n",
                h,
                inline_html(&t[h..].trim())
            ));
            continue;
        }
        if t.starts_with(">") {
            flush_para(&mut out, &mut para_buf, &mut para_started);
            close_list(&mut out, &mut in_list);
            out.push_str(&format!(
                "<blockquote>{}</blockquote>\n",
                inline_html(t.trim_start_matches('>').trim())
            ));
            continue;
        }
        if t.starts_with("![") {
            flush_para(&mut out, &mut para_buf, &mut para_started);
            close_list(&mut out, &mut in_list);
            out.push_str(&image_html(t, base_dir, &mut stats));
            continue;
        }
        if t == "---" || t == "***" || t == "___" {
            flush_para(&mut out, &mut para_buf, &mut para_started);
            close_list(&mut out, &mut in_list);
            out.push_str("<hr/>\n");
            continue;
        }
        // 列表
        let (is_li, li_text) = list_item(t);
        if is_li {
            flush_para(&mut out, &mut para_buf, &mut para_started);
            if !in_list {
                out.push_str("<ul>\n");
                in_list = true;
            }
            out.push_str(&format!("<li>{}</li>\n", inline_html(li_text)));
            continue;
        }
        close_list(&mut out, &mut in_list);
        if t.is_empty() {
            flush_para(&mut out, &mut para_buf, &mut para_started);
            continue;
        }
        if !para_started {
            para_started = true;
        }
        para_buf.push(line.to_string());
    }
    if in_code {
        out.push_str("<pre><code>");
        out.push_str(&escape_html(&code_buf.join("\n")));
        out.push_str("</code></pre>\n");
    }
    flush_para(&mut out, &mut para_buf, &mut para_started);
    close_list(&mut out, &mut in_list);
    if !rows.is_empty() {
        out.push_str(&table_html(&rows));
    }
    out
}

fn close_list(out: &mut String, in_list: &mut bool) {
    if *in_list {
        out.push_str("</ul>\n");
        *in_list = false;
    }
}

/// 判断是否为 Markdown 表格分隔行（仅含 | - : 与空格）
fn is_table_sep(t: &str) -> bool {
    t.trim_start_matches('|')
        .trim_end_matches('|')
        .chars()
        .all(|c| matches!(c, '|' | '-' | ':' | ' '))
        && t.contains('-')
}

fn heading_level(t: &str) -> Option<usize> {
    let m = t.find(|c: char| c != '#').unwrap_or(t.len());
    if m > 0 && m <= 4 {
        let rest = &t[m..];
        if rest.starts_with(' ') || rest.is_empty() {
            return Some(m);
        }
    }
    None
}

fn list_item(t: &str) -> (bool, &str) {
    if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
        return (true, rest);
    }
    if t.starts_with(|c: char| c.is_ascii_digit())
        && t.contains(". ")
    {
        if let Some(idx) = t.find(". ") {
            if t[..idx].chars().all(|c| c.is_ascii_digit()) {
                return (true, &t[idx + 2..]);
            }
        }
    }
    (false, t)
}

fn table_html(rows: &[String]) -> String {
    let mut out = String::from("<table>\n");
    for (i, row) in rows.iter().enumerate() {
        let cells: Vec<&str> = row
            .trim()
            .trim_start_matches('|')
            .trim_end_matches('|')
            .split('|')
            .map(|c| c.trim())
            .collect();
        let tag = if i == 0 { "th" } else { "td" };
        out.push_str("<tr>");
        for c in cells {
            out.push_str(&format!("<{tag}>{}</{tag}>", inline_html(c)));
        }
        out.push_str("</tr>\n");
    }
    out.push_str("</table>\n");
    out
}

/// 图片内嵌统计（防止超大 HTML）
#[derive(Default)]
struct EmbedStats {
    count: usize,
    bytes: usize,
}

const MAX_EMBED_IMAGES: usize = 25;
const MAX_EMBED_BYTES: usize = 15_000_000;

/// 图片 → <img>；HTML 导出时内嵌 base64（有数量/大小上限）
fn image_html(t: &str, base_dir: &Path, stats: &mut EmbedStats) -> String {
    let alt = t
        .trim_start_matches("![")
        .split_once(']')
        .map(|(a, _)| a)
        .unwrap_or("");
    let src = t
        .split_once('(')
        .and_then(|(_, rest)| rest.split_once(')'))
        .map(|(s, _)| s.trim())
        .unwrap_or("");
    let src_clean = src.split_whitespace().next().unwrap_or("");
    let src_clean = src_clean.trim_matches(|c| c == '"' || c == '\'');
    if src_clean.is_empty() {
        return String::new();
    }
    if src_clean.starts_with("data:") || src_clean.starts_with("http") {
        return format!("<img alt=\"{}\" src=\"{}\"/>", escape_html(alt), escape_html(src_clean));
    }
    // 本地图片：读文件内嵌 base64（达到上限则保留相对路径，避免导出文件过大）
    let full = base_dir.join(src_clean.trim_start_matches("./"));
    let mime = match full.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    };
    match std::fs::read(&full) {
        Ok(bytes) => {
            let can_embed = stats.count < MAX_EMBED_IMAGES
                && stats.bytes + bytes.len() <= MAX_EMBED_BYTES;
            if can_embed {
                let data = b64_encode(&bytes);
                stats.count += 1;
                stats.bytes += bytes.len();
                return format!(
                    "<img alt=\"{}\" src=\"data:{mime};base64,{data}\"/>",
                    escape_html(alt)
                );
            }
            format!("<img alt=\"{}\" src=\"{}\"/>", escape_html(alt), escape_html(src_clean))
        }
        Err(_) => format!("<img alt=\"{}\" src=\"{}\"/>", escape_html(alt), escape_html(src_clean)),
    }
}

/// 行内 Markdown（粗体/斜体/行内代码）
fn inline_html(s: &str) -> String {
    let esc = escape_html(s);
    // 行内代码 `...`
    let mut out = String::new();
    let mut in_code = false;
    for part in esc.split('`') {
        if in_code {
            out.push_str("<code>");
            out.push_str(part);
            out.push_str("</code>");
        } else {
            out.push_str(part);
        }
        in_code = !in_code;
    }
    // 粗体 **x**
    let mut bold = String::new();
    let mut in_bold = false;
    for part in out.split("**") {
        if in_bold {
            bold.push_str("<strong>");
            bold.push_str(part);
            bold.push_str("</strong>");
        } else {
            bold.push_str(part);
        }
        in_bold = !in_bold;
    }
    // 斜体 *x*
    let mut ital = String::new();
    let mut in_ital = false;
    for part in bold.split('*') {
        if in_ital && !part.is_empty() {
            ital.push_str("<em>");
            ital.push_str(part);
            ital.push_str("</em>");
        } else {
            ital.push_str(part);
        }
        in_ital = !in_ital;
    }
    ital
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// base64 编码（无外部依赖）
fn b64_encode(bytes: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn b64_encodes_correctly() {
        assert_eq!(b64_encode(b"hello"), "aGVsbG8=");
        assert_eq!(b64_encode(b"Man"), "TWFu");
        assert_eq!(b64_encode(b"Ma"), "TWE=");
    }

    #[test]
    fn md_to_html_handles_blocks() {
        let md = "# 标题\n\n正文 **加粗** 与 `代码`。\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n- 项一\n- 项二\n\n```rs\nfn main() {}\n```\n";
        let html = md_to_html(md, Path::new("/tmp"));
        assert!(html.contains("<h1>标题</h1>"));
        assert!(html.contains("<strong>加粗</strong>"));
        assert!(html.contains("<code>代码</code>"));
        assert!(html.contains("<table>"));
        assert!(html.contains("<th>A</th>"));
        assert!(html.contains("<ul>"));
        assert!(html.contains("<pre><code>fn main() {}"));
    }

    #[test]
    fn compose_markdown_joins_sections() {
        let parts = ExportParts {
            title: "T".into(),
            original: "orig".into(),
            translated: Some("trans".into()),
            digest: Some("dig".into()),
        };
        let md = compose_markdown(&parts, "all");
        assert!(md.contains("## 原文"));
        assert!(md.contains("## 译文"));
        assert!(md.contains("## AI 拆解"));
    }

    /// 导出范围须精确控制章节取舍：仅译文不得夹带原文，仅拆解不得夹带译文
    #[test]
    fn compose_markdown_respects_scope() {
        let parts = ExportParts {
            title: "T".into(),
            original: "ORIGINAL-BODY".into(),
            translated: Some("TRANSLATED-BODY".into()),
            digest: Some("DIGEST-BODY".into()),
        };

        let only_translated = compose_markdown(&parts, "translated");
        assert!(only_translated.contains("TRANSLATED-BODY"));
        assert!(!only_translated.contains("ORIGINAL-BODY"));
        assert!(!only_translated.contains("DIGEST-BODY"));

        let no_original = compose_markdown(&parts, "no-original");
        assert!(!no_original.contains("ORIGINAL-BODY"));
        assert!(no_original.contains("TRANSLATED-BODY"));
        assert!(no_original.contains("DIGEST-BODY"));

        let only_digest = compose_markdown(&parts, "digest");
        assert!(only_digest.contains("DIGEST-BODY"));
        assert!(!only_digest.contains("ORIGINAL-BODY"));
        assert!(!only_digest.contains("TRANSLATED-BODY"));

        let bilingual = compose_markdown(&parts, "bilingual");
        assert!(bilingual.contains("ORIGINAL-BODY"));
        assert!(bilingual.contains("TRANSLATED-BODY"));
        assert!(!bilingual.contains("DIGEST-BODY"));
    }

    #[test]
    fn compose_html_embeds_theme() {
        let parts = ExportParts {
            title: "T".into(),
            original: "# A\n\nhello".into(),
            translated: None,
            digest: None,
        };
        let html = compose_html(&parts, Path::new("/tmp"), "sepia", "all");
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("#f5ecd9"));
        assert!(html.contains("<h1>T</h1>"));
    }
}
