//! 拆解执行器（M3.3/3.4）
//!
//! 依据《research/design-schemas.md》§5 实现：
//! - 逐字段调用 LLM（每字段独立请求，便于进度显示与断点续存）
//! - 引用锚定：文献内容带段落编号，强制要求引用原文段落，禁编造
//! - 范式专属拆解指令（design-schemas §4.2）
//! - 每个字段结果按类型渲染（text/list/table/image/enum）

use serde::{Deserialize, Serialize};

use crate::fields::FieldDef;
use crate::paradigm::ParadigmRecognition;
use crate::translate::{TranslateError, Translator};

/// 单个字段的拆解结果（持久化结构）
#[derive(Serialize, Deserialize, Clone)]
pub struct DigestFieldResult {
    pub name: String,
    pub label: String,
    pub ftype: String,
    pub source: String,
    pub zh: String,          // 中文拆解（内联 [段落 N] 引用）
    pub en: String,          // 英文拆解（双语对照）
    pub table: Option<String>, // 表格类字段的 Markdown 表格（中英共用）
    pub failed: bool,        // 该字段拆解失败（占位而非编造）
    /// 失败原因分类：config（凭据/Base URL/模型名）/ network / llm / internal。
    /// 旧数据没有这个字段，反序列化时取默认值，保持兼容。
    #[serde(default)]
    pub error_category: String,
    /// 失败原因原文（面向用户：说明是哪个环节、怎么处理）
    #[serde(default)]
    pub error_message: String,
}

/// 带编号的上下文条目（供按字段检索）
pub struct NumberedSeg {
    /// 正文段落编号；章节标题为 None
    pub no: Option<usize>,
    /// 章节标题文本（heading 时有效）
    pub heading: Option<String>,
    pub content: String,
}

/// 把解析 Markdown 拆成带编号的条目：正文段落递增编号，章节标题单独保留
pub fn number_segments(md: &str) -> Vec<NumberedSeg> {
    let mut out = Vec::new();
    let mut para_no = 0usize;
    for s in crate::translate::split_markdown(md) {
        match s.kind.as_str() {
            "paragraph" => {
                para_no += 1;
                out.push(NumberedSeg {
                    no: Some(para_no),
                    heading: None,
                    content: s.content,
                });
            }
            "heading" => out.push(NumberedSeg {
                no: None,
                heading: Some(s.content.trim_start_matches('#').trim().to_string()),
                content: String::new(),
            }),
            _ => {}
        }
    }
    out
}

fn render_seg(seg: &NumberedSeg) -> String {
    match seg.no {
        Some(n) => format!("[段落 {n}]\n{}\n\n", seg.content),
        None => format!("【章节】{}\n\n", seg.heading.clone().unwrap_or_default()),
    }
}

/// 从字段定义提取检索词：英文按非字母数字切词（取长度 ≥4），中文取 label 的 2 字滑窗
fn field_terms(field: &FieldDef) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    for src in [field.name.as_str(), field.description.as_str()] {
        for w in src.split(|c: char| !c.is_ascii_alphanumeric()) {
            let w = w.trim().to_lowercase();
            if w.chars().count() >= 4 {
                terms.push(w);
            }
        }
    }
    let zh: Vec<char> = field
        .label
        .chars()
        .filter(|c| !c.is_ascii() && !c.is_whitespace())
        .collect();
    if zh.len() >= 2 {
        for w in zh.windows(2) {
            terms.push(w.iter().collect());
        }
    }
    terms.sort();
    terms.dedup();
    terms
}

/// 为单个字段挑选上下文
///
/// 关键词命中的段落优先入选（保证该字段能拿到对应原文），再按阅读顺序补足结构段落。
/// 替代"一律取开头 N 字符"——那会让长论文的方法/结果段永远落在截断之外，
/// 模型只能输出"引用缺失"。
pub fn context_for_field(segs: &[NumberedSeg], field: &FieldDef, cap_chars: usize) -> String {
    if segs.is_empty() {
        return String::new();
    }
    let terms = field_terms(field);
    let scores: Vec<usize> = segs
        .iter()
        .map(|s| {
            let text = s.content.to_lowercase();
            terms.iter().filter(|t| text.contains(t.as_str())).count()
        })
        .collect();

    // 命中段落按得分降序，占上下文预算的 70%
    let mut hit_idx: Vec<usize> = (0..segs.len()).filter(|&i| scores[i] > 0).collect();
    hit_idx.sort_by_key(|&i| std::cmp::Reverse(scores[i]));

    let mut chosen = vec![false; segs.len()];
    let mut used = 0usize;
    let hit_budget = cap_chars * 7 / 10;
    for i in hit_idx {
        let len = render_seg(&segs[i]).chars().count();
        if used + len > hit_budget {
            continue;
        }
        chosen[i] = true;
        used += len;
    }
    // 补足：按阅读顺序填入章节标题与其余段落，保持结构完整
    for i in 0..segs.len() {
        if chosen[i] {
            continue;
        }
        let len = render_seg(&segs[i]).chars().count();
        if used + len > cap_chars {
            continue;
        }
        chosen[i] = true;
        used += len;
    }

    let mut out = String::new();
    for (i, seg) in segs.iter().enumerate() {
        if chosen[i] {
            out.push_str(&render_seg(seg));
        }
    }
    out
}

/// 范式专属拆解核对指令（design-schemas §4.2）
fn paradigm_instruction(id: &str) -> &'static str {
    match id {
        "paradigm-experimental-baseline" => {
            "重点核对：基线是否公平（同设置/同资源）；指标是否齐备；消融是否解释了每个组件的贡献；SOTA 对比是否诚实"
        }
        "paradigm-rct" => {
            "重点核对：随机化与盲法是否完备；样本量估算是否合理；主要结局是否注册一致；harm 报告是否完整"
        }
        "paradigm-empirical-stat" => {
            "重点核对：识别策略是否可信；内生性处理是否充分；稳健性检验覆盖哪些维度；结果是否支持因果解释"
        }
        "paradigm-theoretical" => {
            "重点核对：概念界定是否清晰；论证前提与推理是否自洽；反驳是否回应到位；思想谱系定位是否准确"
        }
        "paradigm-case-study" => {
            "重点核对：案例选择是否有依据；证据是否多源三角验证；分析技术是否与问题匹配；命题是否可推广"
        }
        "paradigm-systematic-review" => {
            "重点核对：检索策略是否可复现；纳排标准是否明确；质量评估工具是否恰当；异质性与偏倚是否报告"
        }
        "paradigm-computational-sim" => {
            "重点核对：Verification 与 Validation 是否严格区分；收敛性是否验证；不确定性是否量化；模型假设是否声明"
        }
        "paradigm-design-science" => {
            "重点核对：问题-目标-设计是否对齐；评估方法是否匹配构件类型；设计理论贡献是否明确"
        }
        _ => "",
    }
}

/// 拆解系统提示词：角色 + 引用锚定规则 + 范式专属核对指令
pub fn build_system_prompt(rec: &ParadigmRecognition, strategy: &str, field_count: usize) -> String {
    let instruction = paradigm_instruction(&rec.paradigm_id);
    format!(
        "你是一名{rec_name}领域的论文拆解专家。请对以下文献按给定字段逐一拆解。\n\
         文献范式：{rec_name}（{cross_type}，字段合并策略：{strategy}，共 {field_count} 个字段）。\n\
         【规则】\n\
         1. 每个字段的输出必须引用原文段落（引用格式：[段落 N]，或给出原文关键摘录）；\n\
         2. 无法从原文定位支撑的字段，必须输出“引用缺失”四个字，严禁编造内容；\n\
         3. 表格类字段输出 Markdown 表格；图片类字段输出图片描述并注明其在原文中的位置；\n\
         4. 保留原文术语，不做二次翻译；\n\
         5. 只输出该字段的拆解内容本身，不要输出字段名、解释性开场白或任何与内容无关的文字。\n\
         {instruction}\n\
         {review_note}",
        rec_name = rec.paradigm_name,
        cross_type = rec.cross_type,
        strategy = strategy,
        field_count = field_count,
        instruction = if instruction.is_empty() {
            String::new()
        } else {
            format!("\n         【范式核对要求】{instruction}")
        },
        review_note = "若文献明显包含多学科交叉，除主范式视角外，可补充说明交叉学科视角。"
    )
}

/// 单个字段的用户提示词：文献上下文 + 字段要求 + 类型输出约束 + 双语 JSON 输出格式
pub fn build_field_user(field: &FieldDef, context: &str) -> String {
    let type_hint = match field.ftype.as_str() {
        "table" => "Markdown 表格（表头 + 数据行，放在 table 字段）",
        "list[string]" => "项目符号列表（每行一个条目）",
        "enum" => "从可选值中选择一个",
        "image" => "描述该图（类型、内容要点）并注明其在原文中的位置（[段落 N]）",
        "integer" => "一个整数",
        "string" => "一句简洁的结论文字",
        "code" => "代码块",
        _ => "简洁的段落文字",
    };
    let enum_hint = if !field.enum_values.is_empty() {
        format!("（可选值：{}）", field.enum_values.join(" / "))
    } else {
        String::new()
    };
    // 列表类字段：要求输出数组，一个条目一个元素（此前只要求字符串，模型常把条目挤成一行）
    let (zh_tpl, en_tpl, list_rule) = if field.ftype == "list[string]" {
        (
            "[\"条目一，内联标注 [段落 N]\", \"条目二，内联标注 [段落 N]\"]",
            "[\"item one with [Paragraph N]\", \"item two with [Paragraph N]\"]",
            "\n列表类字段：zh 与 en 都必须输出数组，一个条目一个元素，不要把多个条目塞进同一个字符串；\
             每个条目里仍要内联标注 [段落 N]。",
        )
    } else {
        (
            "\"中文拆解，必须内联标注所依据的原文段落，格式 [段落 N]（至少一处；无法定位时只输出 引用缺失）\"",
            "\"英文拆解，与 zh 内容一一对应\"",
            "",
        )
    };
    format!(
        "【文献内容】（段落编号用于引用锚定）\n{context}\n\n【待拆解字段】\n\
         字段：{label}\n说明：{desc}\n类型：{ftype} → {type_hint}{enum_hint}{list_rule}\n\n\
         【输出格式】严格输出一个 JSON 对象，不要输出任何其他文字：\n\
         {{\"zh\": {zh_tpl}, \"en\": {en_tpl}, \
         \"table\": \"仅表格类字段输出 Markdown 表格，其余字段省略此项\"}}",
        label = field.label,
        desc = field.description,
        ftype = field.ftype,
    )
}

/// 剥掉行首的序号 / 项目符号："一、" "1." "(2)" "①" "-" "•"
fn strip_item_marker(line: &str) -> String {
    const CN_ORDINALS: [char; 10] = ['一', '二', '三', '四', '五', '六', '七', '八', '九', '十'];
    const CIRCLED: [char; 10] = ['①', '②', '③', '④', '⑤', '⑥', '⑦', '⑧', '⑨', '⑩'];

    let t = line.trim_start();
    for b in ['-', '*', '•', '·', '–'] {
        if let Some(rest) = t.strip_prefix(b) {
            return rest.trim_start().to_string();
        }
    }
    let chars: Vec<char> = t.chars().collect();
    if let Some(c) = chars.first() {
        if CIRCLED.contains(c) {
            return chars[1..].iter().collect::<String>().trim_start().to_string();
        }
    }
    // (1) / （一） / [1]
    if let Some(first) = chars.first() {
        if matches!(first, '(' | '（' | '[' | '【') {
            let close = match first {
                '(' => ')',
                '（' => '）',
                '[' => ']',
                _ => '】',
            };
            if let Some(pos) = chars.iter().position(|c| *c == close) {
                let inner: String = chars[1..pos].iter().collect();
                let ok = !inner.is_empty()
                    && inner
                        .chars()
                        .all(|c| c.is_ascii_digit() || CN_ORDINALS.contains(&c));
                if ok && pos + 1 < chars.len() {
                    return chars[pos + 1..]
                        .iter()
                        .collect::<String>()
                        .trim_start()
                        .to_string();
                }
            }
        }
    }
    // 一、 / 1. / 1、 / 1)
    let mut i = 0;
    while matches!(chars.get(i), Some(c) if CN_ORDINALS.contains(c) || c.is_ascii_digit()) {
        i += 1;
    }
    if i > 0 && matches!(chars.get(i), Some('、') | Some('.') | Some('．') | Some(')') | Some('）'))
    {
        // 章节号（如 2.1）不当作条目序号：分隔符后面若还是数字则原样保留
        let next_is_digit = matches!(chars.get(i + 1), Some(c) if c.is_ascii_digit());
        if !next_is_digit {
            return chars[i + 1..]
                .iter()
                .collect::<String>()
                .trim_start()
                .to_string();
        }
    }
    t.to_string()
}

/// 统计以条目序号开头的片段数（按分号切分判断）
fn count_ordinal_markers(text: &str) -> usize {
    text.split(['；', ';'])
        .filter(|seg| {
            let s = seg.trim();
            !s.is_empty() && strip_item_marker(s) != s
        })
        .count()
}

/// 列表类字段的兜底整理：模型有时把多个条目塞进一行
/// （如"一、关键词A [段落 3]；二、关键词B [段落 5]"），这里拆成一行一条，
/// 并统一剥掉行首序号/项目符号，保证前端按条目逐条渲染。
fn normalize_list_text(text: &str) -> String {
    let t = text.trim();
    if t.is_empty() {
        return String::new();
    }
    let citation_marks = t.matches("[段落").count() + t.matches("[Paragraph").count();
    let ordinals = count_ordinal_markers(t);

    // 只有在"确实像多条挤在一起"时才按分号拆；否则保持原样
    let pieces: Vec<&str> = if !t.contains('\n') && (citation_marks >= 2 || ordinals >= 2) {
        t.split(['；', ';']).collect()
    } else {
        t.lines().collect()
    };

    pieces
        .iter()
        .map(|l| strip_item_marker(l))
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 将 JSON 值转为展示文本：字符串直接取；数组逐项拼接；对象取常用文本字段
fn json_value_text(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Array(items) => {
            let lines: Vec<String> = items
                .iter()
                .filter_map(|it| match it {
                    serde_json::Value::String(s) => {
                        let t = s.trim();
                        if t.is_empty() { None } else { Some(t.to_string()) }
                    }
                    serde_json::Value::Object(o) => {
                        let picked = ["term", "text", "item", "label", "definition", "zh"]
                            .iter()
                            .find_map(|k| o.get(*k).and_then(|x| x.as_str()))
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty());
                        Some(picked.unwrap_or_else(|| it.to_string()))
                    }
                    other => {
                        let s = other.to_string();
                        if s.is_empty() { None } else { Some(s) }
                    }
                })
                .collect();
            if lines.is_empty() { None } else { Some(lines.join("\n")) }
        }
        serde_json::Value::Object(o) => ["zh", "text", "content", "term", "definition"]
            .iter()
            .find_map(|k| o.get(*k).and_then(|x| x.as_str()))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        _ => None,
    }
}

/// 解析模型输出为 {zh, en, table}；zh/en/table 兼容字符串与数组；JSON 解析失败时兜底整段作为 zh
fn parse_digest_response(raw: &str, ftype: &str) -> (String, String, Option<String>) {
    let cleaned = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(cleaned) {
        let mut zh = json_value_text(&v["zh"]).unwrap_or_default();
        let mut en = json_value_text(&v["en"]).unwrap_or_default();
        // 列表类字段兜底：模型偶尔把多个条目挤成一行，统一整理成一条一行
        if ftype == "list[string]" {
            zh = normalize_list_text(&zh);
            en = normalize_list_text(&en);
        }
        let table = if ftype == "table" {
            json_value_text(&v["table"])
        } else {
            None
        };
        if !zh.is_empty() {
            return (zh, en, table);
        }
    }
    (raw.trim().to_string(), String::new(), None)
}

/// 上游凭据/配置类错误：一旦出现，后续字段必然同样失败，应立即中止并明确告知用户，
/// 而不是把每个字段都跑一遍、留下一片"引用缺失"，让人误以为是软件功能坏了。
pub fn is_credential_error(e: &TranslateError) -> bool {
    if e.category == "config" {
        return true;
    }
    let m = &e.message;
    m.contains("401")
        || m.contains("403")
        || m.contains("API Key")
        || m.contains("api key")
        || m.contains("API key")
        || m.contains("鉴权")
        || m.contains("无效")
        || m.contains("未授权")
        || m.contains("Unauthorized")
        || m.contains("invalid_api_key")
}

/// 中文拆解是否含段落引用（或已声明引用缺失）
fn has_citation(zh: &str) -> bool {
    zh.contains("引用缺失")
        || zh.contains("[段落")
        || zh.contains("段落 ")
        || zh.contains("（段落")
}

/// 逐字段拆解（双语 JSON 输出）：可恢复错误重试 1 次；缺段落引用时带提醒重试 1 次
pub fn digest_field(
    translator: &Translator,
    system: &str,
    user: &str,
    ftype: &str,
) -> Result<(String, String, Option<String>), TranslateError> {
    let call = || translator.chat(system, user, 0.3);
    let mut raw = match call() {
        Err(e) if e.retryable => call()?,
        r => r?,
    };
    let mut parsed = parse_digest_response(&raw, ftype);
    // 引用校验：未标注段落且非"引用缺失" → 带提醒重试一次
    if !has_citation(&parsed.0) {
        let retry_user = format!(
            "{user}\n\n你的上一条输出缺少段落引用。请重新输出，并必须在中文拆解中内联标注依据段落，\
             格式 [段落 N]；若确实无法定位，只输出 {{\"zh\":\"引用缺失\",\"en\":\"Citation missing\"}}。"
        );
        if let Ok(r2) = translator.chat(system, &retry_user, 0.3) {
            raw = r2;
            parsed = parse_digest_response(&raw, ftype);
        }
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str, label: &str, desc: &str) -> FieldDef {
        FieldDef {
            name: name.into(),
            label: label.into(),
            ftype: "text".into(),
            description: desc.into(),
            required: false,
            enum_values: Vec::new(),
            source: "通用".into(),
        }
    }

    /// 回归用例：字段相关段落在论文后段时，必须被检索进该字段的上下文。
    ///
    /// 旧实现固定取开头 22000 字符，长论文的方法/结果段落在截断之外，
    /// 模型只能输出"引用缺失"——这正是实机测到的问题。
    #[test]
    fn field_context_reaches_tail_of_long_paper() {
        let mut md = String::new();
        for i in 1..=150 {
            md.push_str(&format!("## Section {i}\n\n"));
            if i == 140 {
                md.push_str(
                    "We report the main regression results with fixed effects and robust standard errors.\n\n",
                );
            } else {
                md.push_str(&format!(
                    "Filler paragraph {i} contains ordinary narrative content for padding.\n\n"
                ));
            }
        }
        let segs = number_segments(&md);
        assert!(segs.len() > 100, "测试语料应产生足够多的段落");

        let f = field("main_regression", "核心回归结果", "核心回归结果表");
        let ctx = context_for_field(&segs, &f, 6000);

        assert!(
            ctx.contains("main regression results"),
            "位于论文后段的字段相关段落应被检索进上下文，否则该字段只能报引用缺失"
        );
    }

    /// 上下文预算须被遵守，避免把整篇论文塞进单次请求
    #[test]
    fn field_context_respects_budget() {
        let mut md = String::new();
        for i in 1..=300 {
            md.push_str(&format!("Paragraph {i} text body.\n\n"));
        }
        let segs = number_segments(&md);
        let f = field("dataset_name", "数据集名称", "使用的数据集名称");
        let ctx = context_for_field(&segs, &f, 2000);
        assert!(
            ctx.chars().count() <= 2200,
            "上下文不应超出预算，实际 {} 字符",
            ctx.chars().count()
        );
    }

    /// 段落编号需连续，引用锚定 [段落 N] 才有意义
    #[test]
    fn segment_numbering_is_sequential() {
        let md = "## 1. Introduction\n\nFirst paragraph.\n\nSecond paragraph.\n\n## 2. Methods\n\nThird paragraph.\n\n";
        let segs = number_segments(md);
        let nos: Vec<usize> = segs.iter().filter_map(|s| s.no).collect();
        assert_eq!(nos, vec![1, 2, 3]);
        assert!(segs.iter().any(|s| s.heading.as_deref() == Some("2. Methods")));
    }

    #[test]
    fn list_field_splits_packed_items() {
        // 模型把多个条目挤成一行时的兜底拆分
        let packed = "一、关键词A [段落 3]；二、关键词B [段落 5]";
        assert_eq!(
            normalize_list_text(packed),
            "关键词A [段落 3]\n关键词B [段落 5]"
        );
    }

    #[test]
    fn list_field_keeps_lines_and_strips_bullets() {
        let multi = "- 人工智能（Artificial intelligence）[段落14]\n- 虚拟现实（Virtual reality）[段落14]";
        assert_eq!(
            normalize_list_text(multi),
            "人工智能（Artificial intelligence）[段落14]\n虚拟现实（Virtual reality）[段落14]"
        );
    }

    #[test]
    fn list_field_keeps_single_item_untouched() {
        assert_eq!(normalize_list_text("只有一个关键词"), "只有一个关键词");
    }

    #[test]
    fn list_field_does_not_strip_section_numbers() {
        // 章节号（2.1）不能被当成条目序号剥掉
        assert_eq!(strip_item_marker("2.1 研究方法"), "2.1 研究方法");
        assert_eq!(strip_item_marker("1. 研究问题"), "研究问题");
        assert_eq!(strip_item_marker("（2）研究方法"), "研究方法");
    }
}
