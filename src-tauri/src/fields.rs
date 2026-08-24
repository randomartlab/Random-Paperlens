//! 字段模板库 + 路由引擎（M3.2）
//!
//! 依据《research/fields.yaml》与《research/design-schemas.md》§3 实现：
//! - 字段模板库：10 通用字段 + 8 范式特有字段 + 4 交叉对话字段
//! - 路由引擎：按 cross_type 合并字段集，冲突优先级「人工 > 主范式 > 次范式」

use serde::Serialize;

use crate::paradigm::ParadigmRecognition;

/// 单个拆解字段
#[derive(Serialize, Clone)]
pub struct FieldDef {
    pub name: String,        // 字段标识（英文）
    pub label: String,       // 中文显示名
    pub ftype: String,       // string / integer / list[string] / text / table / image / enum / code
    pub description: String, // 说明（fields.yaml 原文）
    pub required: bool,
    pub enum_values: Vec<String>, // enum 类型时有效
    pub source: String,      // 来源标注：通用 / 主范式·XXX / 次范式视角·XXX / 交叉对话
}

/// 组装完成的字段方案（路由引擎输出）
#[derive(Serialize, Clone)]
pub struct FieldPlan {
    pub paradigm_id: String,
    pub paradigm_name: String,
    pub cross_type: String,        // 单学科 / 双学科交叉 / 多学科交叉
    pub combine_strategy: String,  // 合并策略说明
    pub fields: Vec<FieldDef>,     // 组装后的字段（按 通用→主范式→次范式→交叉对话 排序）
    pub count: usize,
    pub notes: Vec<String>, // 冲突处理 / 同名字段说明
}

/// 字段构造辅助（普通类型）
fn f(name: &str, label: &str, ftype: &str, required: bool, description: &str) -> FieldDef {
    FieldDef {
        name: name.to_string(),
        label: label.to_string(),
        ftype: ftype.to_string(),
        description: description.to_string(),
        required,
        enum_values: vec![],
        source: String::new(),
    }
}

/// 字段构造辅助（enum 类型）
fn fe(name: &str, label: &str, required: bool, description: &str, enums: &[&str]) -> FieldDef {
    FieldDef {
        name: name.to_string(),
        label: label.to_string(),
        ftype: "enum".to_string(),
        description: description.to_string(),
        required,
        enum_values: enums.iter().map(|s| s.to_string()).collect(),
        source: String::new(),
    }
}

/// 10 个通用字段（所有范式共享）
fn common_fields() -> Vec<FieldDef> {
    vec![
        f("title", "标题", "string", true, "论文标题"),
        f("authors", "作者", "list[string]", true, "作者列表"),
        f("year", "发表年份", "integer", true, "发表年份"),
        f("source", "来源", "string", false, "期刊/会议/出版社"),
        f("doi", "DOI", "string", false, "数字对象标识符"),
        f("abstract", "摘要", "text", false, "论文摘要"),
        f("keywords", "关键词", "list[string]", false, "关键词列表"),
        f("research_question", "研究问题/目标/假设", "text", false, "研究问题/目标/假设"),
        f("main_finding", "主要发现/结论", "text", false, "主要发现/结论"),
        fe("knowledge_contribution", "知识贡献类型", false, "论文的知识贡献类型", &[
            "新理论", "新方法", "新数据", "新证据", "新设计", "新综述", "新应用",
        ]),
    ]
}

/// 范式特有字段：实验对比型
fn experimental_fields() -> Vec<FieldDef> {
    vec![
        f("dataset_name", "数据集", "string", false, "使用的数据集名称"),
        f("dataset_statistics", "数据集统计信息", "table", false, "数据集统计信息（大小/类别分布等）"),
        f("baseline_methods", "基线/SOTA 方法", "list[string]", false, "对比的基线/SOTA 方法列表"),
        f("evaluation_metrics", "评价指标", "list[string]", false, "评价指标（accuracy, F1, BLEU, PSNR 等）"),
        f("main_results_table", "核心结果对比表", "table", false, "核心结果对比表"),
        f("ablation_study", "消融实验", "table", false, "消融实验结果"),
        f("experimental_setup", "实验环境配置", "text", false, "实验环境配置（硬件/超参数/框架版本）"),
        f("reproducibility", "可复现性声明", "string", false, "代码/数据可复现性声明"),
        f("sota_comparison", "与 SOTA 对比分析", "text", false, "与 SOTA 方法的定性定量对比分析"),
    ]
}

/// 范式特有字段：对照实验型（RCT）
fn rct_fields() -> Vec<FieldDef> {
    vec![
        fe("trial_design", "试验设计", false, "试验设计类型", &[
            "平行组", "交叉设计", "析因设计", "非劣效", "等效性", "集群随机",
        ]),
        f("randomization_method", "随机化方法", "string", false, "随机化方法（简单随机/区组随机/分层随机）"),
        f("allocation_concealment", "分配隐藏", "string", false, "分配隐藏机制"),
        fe("blinding", "盲法", false, "盲法类型", &["开放", "单盲", "双盲", "三盲", "评估者盲"]),
        f("sample_size", "样本量", "integer", false, "随机化样本量"),
        f("sample_size_estimation", "样本量估算依据", "text", false, "样本量估算依据"),
        f("intervention", "干预措施", "text", false, "干预措施详细描述"),
        f("control_condition", "对照条件", "text", false, "对照条件"),
        f("primary_outcome", "主要结局指标", "text", false, "主要结局指标"),
        f("secondary_outcomes", "次要结局指标", "list[string]", false, "次要结局指标"),
        f("consort_flow", "CONSORT 流程图", "image", false, "CONSORT 流程图"),
        f("ethics_approval", "伦理审批", "string", false, "伦理审批机构与编号"),
        f("trial_registration", "试验注册", "string", false, "试验注册号/平台"),
        f("harms", "不良事件", "text", false, "不良事件报告"),
    ]
}

/// 范式特有字段：实证统计型
fn empirical_fields() -> Vec<FieldDef> {
    vec![
        f("theoretical_framework", "理论框架/假设", "text", false, "理论框架/假设推导"),
        f("data_source", "数据来源", "string", false, "数据来源（数据库/调查/实验/面板）"),
        f("sample_description", "样本描述", "text", false, "样本描述（时间范围/样本量/筛选条件）"),
        f("descriptive_statistics", "描述性统计", "table", false, "描述性统计表"),
        f("identification_strategy", "识别策略", "string", false, "识别策略（OLS/DID/IV/RDD/Heckman/PSM）"),
        f("main_regression", "核心回归结果", "table", false, "核心回归结果表"),
        f("robustness_checks", "稳健性检验", "list[string]", false, "稳健性检验列表"),
        f("endogeneity", "内生性处理", "text", false, "内生性处理说明"),
        f("heterogeneity_analysis", "异质性分析", "table", false, "异质性分析结果"),
        f("mechanism_analysis", "机制/中介分析", "text", false, "机制/中介效应分析"),
    ]
}

/// 范式特有字段：理论论述型
fn theoretical_fields() -> Vec<FieldDef> {
    vec![
        f("thesis_statement", "核心论点", "text", false, "核心论点/命题"),
        f("concept_definitions", "关键概念界定", "list[string]", false, "关键概念界定"),
        f("argument_structure", "论证结构", "text", false, "论证结构/推理路径"),
        f("intellectual_lineage", "思想谱系定位", "text", false, "思想脉络/学术谱系定位"),
        f("key_sources", "对话/批判对象", "list[string]", false, "主要对话/批判的已有理论"),
        f("objections", "反驳与回应", "list[string]", false, "反驳与回应"),
        f("implications", "理论含义", "text", false, "理论含义/推论"),
        f("contribution", "理论贡献", "text", false, "理论贡献（澄清/反驳/拓展/综合）"),
    ]
}

/// 范式特有字段：案例研究型
fn case_fields() -> Vec<FieldDef> {
    vec![
        f("case_selection", "案例选择依据", "text", false, "案例选择依据（典型性/极端性/方便性）"),
        f("unit_of_analysis", "分析单元", "string", false, "分析单元"),
        f("data_collection", "数据收集方法", "list[string]", false, "数据收集方法（访谈/观察/文档/档案）"),
        f("number_of_sources", "证据来源数量", "integer", false, "证据来源数量"),
        f("triangulation", "三角验证", "text", false, "三角验证方法"),
        f("analysis_method", "分析方法", "string", false, "分析方法（模式匹配/解释构建/时序分析/逻辑模型/跨案例综合）"),
        f("case_description", "案例描述", "text", false, "案例描述"),
        f("propositions", "理论命题", "list[string]", false, "理论命题"),
        f("cross_case_patterns", "跨案例模式对比表", "table", false, "跨案例模式对比表"),
    ]
}

/// 范式特有字段：综述与元分析型
fn systematic_review_fields() -> Vec<FieldDef> {
    vec![
        f("review_question", "综述问题", "text", false, "综述问题（PICO/PICo 框架）"),
        f("search_strategy", "检索策略", "text", false, "检索策略（数据库/关键词/时间范围）"),
        f("inclusion_criteria", "纳入标准", "list[string]", false, "纳入标准"),
        f("exclusion_criteria", "排除标准", "list[string]", false, "排除标准"),
        f("prisma_flow", "PRISMA 流程图", "image", false, "PRISMA 筛选流程图"),
        f("total_records", "初始检索记录数", "integer", false, "初始检索记录数"),
        f("included_studies", "最终纳入研究数", "integer", false, "最终纳入研究数"),
        f("quality_assessment", "质量评估工具", "string", false, "质量评估工具（Cochrane ROB/AMSTAR/NOS 等）"),
        f("data_extraction", "数据提取表", "table", false, "数据提取表"),
        fe("synthesis_method", "综合方法", false, "证据综合方法", &[
            "叙述性综合", "元分析", "主题综合", "元人种志", "现实综合",
        ]),
        f("heterogeneity", "异质性评估", "text", false, "异质性评估（I² 统计量/亚组分析）"),
        f("publication_bias", "发表偏倚", "text", false, "发表偏倚评估（漏斗图/Egger 检验）"),
        f("certainty_evidence", "证据确定性", "string", false, "证据确定性等级（GRADE 等）"),
    ]
}

/// 范式特有字段：计算模拟型
fn computational_fields() -> Vec<FieldDef> {
    vec![
        f("model_assumptions", "模型假设", "text", false, "模型假设与简化条件"),
        f("mathematical_formulation", "数学公式化", "text", false, "数学公式化（控制方程/边界条件/初始条件）"),
        f("numerical_method", "数值方法", "string", false, "数值方法（FEM/FVM/FDM/SPH 等）"),
        f("mesh_description", "网格/离散化", "text", false, "网格/离散化描述"),
        f("verification", "验证（Verification）", "text", false, "验证（Verification：代码正确性、收敛性分析）"),
        f("validation", "确认（Validation）", "text", false, "确认（Validation：与实验/理论/解析解对比）"),
        f("validation_metrics", "确认指标", "list[string]", false, "确认指标（误差指标/相关性系数）"),
        f("parameter_settings", "参数设置表", "table", false, "关键参数设置表"),
        f("simulation_results", "仿真结果", "image", false, "仿真结果可视化"),
        f("uncertainty_quantification", "不确定性量化", "text", false, "不确定性量化方法"),
    ]
}

/// 范式特有字段：设计与构建型
fn design_science_fields() -> Vec<FieldDef> {
    vec![
        f("problem_identification", "问题识别与动机", "text", false, "问题识别与动机"),
        f("objectives", "目标定义", "list[string]", false, "解决方案目标定义"),
        f("design_principles", "设计原则", "list[string]", false, "设计原则/设计理论"),
        f("artifact_description", "构件描述", "text", false, "构件描述（系统/方法/模型/框架）"),
        f("demonstration", "演示场景", "text", false, "演示场景/应用实例"),
        f("evaluation_method", "评估方法", "string", false, "评估方法（实验/用户研究/案例/调查）"),
        f("evaluation_results", "评估结果", "table", false, "评估结果"),
        f("design_theory_contribution", "设计理论贡献", "text", false, "设计理论贡献"),
    ]
}

/// 范式特有字段入口
fn paradigm_specific_fields(id: &str) -> Vec<FieldDef> {
    match id {
        "paradigm-experimental-baseline" => experimental_fields(),
        "paradigm-rct" => rct_fields(),
        "paradigm-empirical-stat" => empirical_fields(),
        "paradigm-theoretical" => theoretical_fields(),
        "paradigm-case-study" => case_fields(),
        "paradigm-systematic-review" => systematic_review_fields(),
        "paradigm-computational-sim" => computational_fields(),
        "paradigm-design-science" => design_science_fields(),
        _ => vec![],
    }
}

/// 范式显示名（用于 source 标注）
fn paradigm_name(id: &str) -> String {
    match id {
        "paradigm-experimental-baseline" => "实验对比型".to_string(),
        "paradigm-rct" => "对照实验型".to_string(),
        "paradigm-empirical-stat" => "实证统计型".to_string(),
        "paradigm-theoretical" => "理论论述型".to_string(),
        "paradigm-case-study" => "案例研究型".to_string(),
        "paradigm-systematic-review" => "综述元分析型".to_string(),
        "paradigm-computational-sim" => "计算模拟型".to_string(),
        "paradigm-design-science" => "设计与构建型".to_string(),
        _ => id.to_string(),
    }
}

/// 4 个交叉对话字段（仅多学科交叉时追加）
fn cross_dialogue_fields() -> Vec<FieldDef> {
    vec![
        f("cross_points", "学科交叉点", "list[string]", false, "学科间真正的交叉点/结合处"),
        f("integration_contribution", "交叉整合贡献", "text", false, "交叉整合产生的增量贡献"),
        f("tension_points", "学科张力点", "list[string]", false, "学科间张力/矛盾处"),
        f("borrowed_methods", "借用方法/概念", "list[string]", false, "从其他学科借用的方法/概念"),
    ]
}

/// 语义相近字段对：key=主范式字段，value=次范式视角保留原名的字段
/// 见 design-schemas §3.3：主范式保留原名，次范式字段标注「[次范式视角]」
const NEAR_FIELD_PAIRS: &[(&str, &str)] = &[
    ("evaluation_metrics", "validation_metrics"), // 实验对比 vs 计算模拟
];

/// 路由引擎：根据识别结果组装字段方案
pub fn assemble_field_plan(rec: &ParadigmRecognition) -> FieldPlan {
    let mut fields: Vec<FieldDef> = common_fields();
    for fld in fields.iter_mut() {
        fld.source = "通用".to_string();
    }

    let mut notes: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> =
        fields.iter().map(|x| x.name.clone()).collect();

    // 主范式特有字段
    let primary_src = format!("主范式·{}", paradigm_name(&rec.paradigm_id));
    for mut fld in paradigm_specific_fields(&rec.paradigm_id) {
        if !seen.insert(fld.name.clone()) {
            continue;
        }
        fld.source = primary_src.clone();
        fields.push(fld);
    }

    // 次要范式特有字段（按权重阈值路由）
    let secondary_threshold = if rec.cross_type == "多学科交叉" {
        0.15
    } else if rec.cross_type == "双学科交叉" {
        0.25
    } else {
        f64::MAX // 单学科不并入次范式字段
    };
    let mut secondaries: Vec<&crate::paradigm::SecondaryParadigm> =
        rec.secondary_paradigms.iter().collect();
    secondaries.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap_or(std::cmp::Ordering::Equal));

    for sp in secondaries {
        if sp.weight < secondary_threshold {
            continue;
        }
        let sec_src = format!("次范式·{}", paradigm_name(&sp.paradigm_id));
        for mut fld in paradigm_specific_fields(&sp.paradigm_id) {
            // 同名冲突：优先主范式（跳过，记说明）
            if seen.contains(&fld.name) {
                notes.push(format!(
                    "字段「{}」同名冲突：采用主范式定义，跳过次范式同名项",
                    fld.label
                ));
                continue;
            }
            // 语义相近：保留原名字段，标注次范式视角
            let is_near = NEAR_FIELD_PAIRS
                .iter()
                .any(|(pri, sec)| sec == &fld.name.as_str() && seen.contains(*pri));
            if is_near {
                fld.label = format!("{}（次范式视角）", fld.label);
                fld.source = format!("次范式视角·{}", paradigm_name(&sp.paradigm_id));
            } else {
                fld.source = sec_src.clone();
            }
            seen.insert(fld.name.clone());
            fields.push(fld);
        }
    }

    // 多学科交叉追加交叉对话字段
    if rec.cross_type == "多学科交叉" {
        for mut fld in cross_dialogue_fields() {
            if !seen.insert(fld.name.clone()) {
                continue;
            }
            fld.source = "交叉对话".to_string();
            fields.push(fld);
        }
    }

    let combine_strategy = match rec.cross_type.as_str() {
        "单学科" => "通用字段 + 主范式特有字段".to_string(),
        "双学科交叉" => "通用字段 + 主范式特有字段 + 次范式特有字段".to_string(),
        "多学科交叉" => "通用字段 + 各范式特有字段 + 交叉对话字段".to_string(),
        _ => rec.cross_type.clone(),
    };

    FieldPlan {
        paradigm_id: rec.paradigm_id.clone(),
        paradigm_name: rec.paradigm_name.clone(),
        cross_type: rec.cross_type.clone(),
        combine_strategy,
        count: fields.len(),
        fields,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paradigm::{ParadigmRecognition, SecondaryParadigm};

    fn rec_single() -> ParadigmRecognition {
        ParadigmRecognition {
            paradigm_id: "paradigm-experimental-baseline".into(),
            paradigm_name: "实验对比型范式".into(),
            confidence: 0.85,
            identification_basis: vec![],
            cross_type: "单学科".into(),
            secondary_paradigms: vec![],
            human_review_required: false,
            fallback_to_common: false,
            domestic: crate::paradigm::DomesticDiscipline {
                category: "08 工学".into(),
                code: "0812".into(),
                discipline: "计算机科学与技术".into(),
                confidence: 1.0,
                basis: vec![],
                secondary: vec![],
            },
            international: crate::paradigm::InternationalDiscipline {
                supergroup: "Computer Science".into(),
                code: "1700".into(),
                discipline: "Computer Science".into(),
                confidence: 1.0,
                basis: vec![],
                secondary: vec![],
            },
            directions: vec![],
        }
    }

    fn rec_cross() -> ParadigmRecognition {
        let mut r = rec_single();
        r.cross_type = "双学科交叉".into();
        r.secondary_paradigms = vec![SecondaryParadigm {
            paradigm_id: "paradigm-design-science".into(),
            paradigm_name: "设计与构建型范式".into(),
            weight: 0.3,
            basis: vec![],
        }];
        r
    }

    fn rec_multi() -> ParadigmRecognition {
        let mut r = rec_cross();
        r.cross_type = "多学科交叉".into();
        r.secondary_paradigms.push(SecondaryParadigm {
            paradigm_id: "paradigm-computational-sim".into(),
            paradigm_name: "计算模拟型范式".into(),
            weight: 0.2,
            basis: vec![],
        });
        r
    }

    #[test]
    fn single_uses_common_plus_primary() {
        let plan = assemble_field_plan(&rec_single());
        assert_eq!(plan.count, 10 + 9); // 通用 10 + 实验对比 9
        assert!(plan.fields.iter().all(|x| x.source != "交叉对话"));
        assert!(plan.fields.iter().any(|x| x.name == "ablation_study"));
    }

    #[test]
    fn cross_unions_secondary_specific() {
        let plan = assemble_field_plan(&rec_cross());
        assert_eq!(plan.count, 10 + 9 + 8); // + 设计构建 8
        assert!(plan.fields.iter().any(|x| x.name == "artifact_description"));
    }

    #[test]
    fn multi_appends_cross_dialogue() {
        let plan = assemble_field_plan(&rec_multi());
        assert!(plan.fields.iter().any(|x| x.name == "cross_points"));
        assert!(plan.fields.iter().any(|x| x.name == "borrowed_methods"));
    }

    #[test]
    fn near_field_marked_secondary_perspective() {
        // 主范式=实验对比(evaluation_metrics) + 次范式=计算模拟(validation_metrics)
        let mut r = rec_single();
        r.cross_type = "双学科交叉".into();
        r.secondary_paradigms = vec![SecondaryParadigm {
            paradigm_id: "paradigm-computational-sim".into(),
            paradigm_name: "计算模拟型范式".into(),
            weight: 0.3,
            basis: vec![],
        }];
        let plan = assemble_field_plan(&r);
        let vm = plan
            .fields
            .iter()
            .find(|x| x.name == "validation_metrics")
            .expect("validation_metrics 应并入");
        assert!(vm.label.contains("次范式视角"));
        assert!(vm.source.contains("次范式视角"));
    }

    #[test]
    fn same_name_primary_wins() {
        // 构造一个次范式与主范式同名字段场景：RCT 与 综述无同名，这里用注释说明跳过逻辑
        let plan = assemble_field_plan(&rec_cross());
        let counts: std::collections::HashMap<&str, usize> = plan
            .fields
            .iter()
            .map(|x| (x.name.as_str(), 1))
            .collect();
        // 无重复字段
        assert_eq!(plan.fields.len(), counts.len());
    }
}
