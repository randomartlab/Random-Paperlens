//! 学科研究范式识别器（M3.1）
//!
//! 依据《research/design-schemas.md》§3 实现，评分公式：
//! `total = 0.45 × 章节命中 + 0.40 × 关键词命中 + 0.15 × 对象命中 + 学科先验`
//!
//! 增强点（相对初版）：
//! - 关键词词典扩充至 12+ 词/范式，且去除过度泛化词（data/model/evaluation 等）
//! - 章节信号支持中英双语；命中 2 个专属章节即视为满分（放宽命比率）
//! - 引入学科大类→范式先验（report.md §4），打破范式间平局
//! - 对象信号仅作为加分项，不能单独成立范式

use serde::Serialize;

/// 范式识别的输入信号（由解析产物提取）
#[derive(Default, Clone)]
pub struct ParadigmInput {
    pub title: String,
    pub abstract_text: String, // 摘要（或开头若干段落，含 Keywords 行）
    pub headings: Vec<String>, // 章节标题（已去 # 标记）
    pub body_sample: String,   // 正文样本（前若干字符，用于关键词命中）
    pub keywords: String,      // 关键词行（作者自述领域，用于学科识别）
    pub table_count: usize,
    pub image_count: usize,
    pub math_symbols: usize, // md 中 $ 出现次数（判断是否含公式）
}

/// 次要范式（交叉拆解时并列输出）
#[derive(Serialize, Clone)]
pub struct SecondaryParadigm {
    pub paradigm_id: String,
    pub paradigm_name: String,
    pub weight: f64,
    pub basis: Vec<String>,
}

/// 次要学科（交叉学科候选）
#[derive(Serialize, Clone)]
pub struct SecondaryItem {
    pub name: String,
    pub weight: f64,
}

/// 国内学科识别结果（《研究生教育学科专业目录（2022 年）》口径）
#[derive(Serialize, Clone)]
pub struct DomesticDiscipline {
    pub category: String,   // 学科门类，如 "13 艺术学"
    pub code: String,       // 一级学科代码，如 "1301"
    pub discipline: String, // 一级学科名，如 "艺术学"
    pub confidence: f64,
    pub basis: Vec<String>,
    pub secondary: Vec<SecondaryItem>, // 次要一级学科候选
}

/// 国际学科识别结果（ASJC / WoS 口径）
#[derive(Serialize, Clone)]
pub struct InternationalDiscipline {
    pub supergroup: String, // 大类，如 "Arts and Humanities"
    pub code: String,       // ASJC 代码，如 "1213"
    pub discipline: String, // 类目名，如 "Visual Arts and Performing Arts"
    pub confidence: f64,
    pub basis: Vec<String>,
    pub secondary: Vec<SecondaryItem>, // 次要类目候选
}

/// 研究方向（主题层）：学科之下的具体方向，如 数字遗产 / 虚拟现实
#[derive(Serialize, Clone)]
pub struct ResearchDirection {
    pub name: String,
    pub weight: f64,
}

/// 范式识别输出契约（design-schemas.md §3.4）
#[derive(Serialize, Clone)]
pub struct ParadigmRecognition {
    pub paradigm_id: String,
    pub paradigm_name: String,
    pub confidence: f64,
    pub identification_basis: Vec<String>,
    pub cross_type: String, // 单学科 / 双学科交叉 / 多学科交叉
    pub secondary_paradigms: Vec<SecondaryParadigm>,
    pub human_review_required: bool,
    pub fallback_to_common: bool, // 置信度过低，回退通用字段集
    // 学科识别（M3.1 扩展）：国内（2022 目录）与国际（ASJC）两套口径 + 研究方向
    pub domestic: DomesticDiscipline,
    pub international: InternationalDiscipline,
    pub directions: Vec<ResearchDirection>,
}

struct ParadigmSpec {
    id: &'static str,
    name: &'static str,
    sections: &'static [&'static str],
    keywords: &'static [&'static str],
    object_label: &'static str,
}

/// 章节信号：英文 + 中文（中文论文同样可识别）
const PARADIGMS: &[ParadigmSpec] = &[
    ParadigmSpec {
        id: "paradigm-experimental-baseline",
        name: "实验对比型范式",
        // 补齐 IMRaD 通用章节信号：此前只认 Experiments/Benchmark/ablation study，
        // 导致章节为 Methods/Results/Participants 的实验论文一个专属词都命中不了，
        // 进而被反向误判为理论论述型
        sections: &[
            "experiment",
            "experimental",
            "benchmark",
            "ablation study",
            "evaluation",
            "results",
            "finding",
            "participant",
            "subject",
            "condition",
            "treatment",
            "实验",
            "基准",
            "结果",
            "被试",
            "参与者",
            "实验组",
            "对照组",
        ],
        keywords: &[
            "baseline",
            "state-of-the-art",
            "sota",
            "ablation",
            "f1-score",
            "top-1",
            "backbone",
            "hyperparameter",
            "benchmark",
            "experiment",
            "participant",
            "condition",
            "treatment",
            "control group",
            "anova",
            "questionnaire",
            "likert",
            "survey",
            "基线",
            "消融",
            "准确率",
            "数据集",
            "被试",
            "问卷",
            "量表",
            "实验组",
            "对照组",
        ],
        object_label: "指标/数据对比表格",
    },
    ParadigmSpec {
        id: "paradigm-rct",
        name: "对照实验型范式",
        sections: &[
            "randomized", "clinical trial", "consort", "randomization", "试验",
            "随机",
        ],
        keywords: &[
            "randomized controlled trial", "rct", "placebo", "double-blind",
            "trial registration", "consort", "intention-to-treat", "blinding",
            "随机对照", "安慰剂", "双盲", "盲法", "试验注册",
        ],
        object_label: "",
    },
    ParadigmSpec {
        id: "paradigm-empirical-stat",
        name: "实证统计型范式",
        sections: &[
            "empirical strategy", "identification", "robustness", "econometric",
            "数据", "实证", "回归",
        ],
        keywords: &[
            "regression", "instrumental variable", "difference-in-differences",
            "regression discontinuity", "endogeneity", "panel data",
            "fixed effects", "heterogeneity", "robustness",
            "回归", "内生性", "工具变量", "双重差分", "断点回归", "面板数据",
            "固定效应", "异质性", "稳健性",
        ],
        object_label: "回归/描述统计表格",
    },
    ParadigmSpec {
        id: "paradigm-theoretical",
        name: "理论论述型范式",
        sections: &[], // 主题式结构，无标准方法章节（反向信号）
        keywords: &[
            "argues", "argue", "concept of", "genealogy", "critique",
            "objection", "discourse", "contends", "philosophical", "hermeneutic",
            "概念", "谱系", "批评", "话语", "反驳", "论证", "思辨", "诠释",
        ],
        object_label: "",
    },
    ParadigmSpec {
        id: "paradigm-case-study",
        name: "案例研究型范式",
        sections: &[
            "case study", "case selection", "cross-case", "fieldwork",
            "ethnograph", "案例", "个案",
        ],
        keywords: &[
            "case study", "interviews", "participant observation", "ethnography",
            "fieldwork", "unit of analysis", "single case", "multiple cases",
            "triangulation", "grounded theory",
            "访谈", "民族志", "田野", "三角验证", "扎根理论",
        ],
        object_label: "跨案例对比矩阵",
    },
    ParadigmSpec {
        id: "paradigm-systematic-review",
        name: "综述与元分析型范式",
        sections: &[
            "systematic review", "meta-analysis", "search strategy", "prisma",
            "综述", "元分析", "荟萃分析",
        ],
        keywords: &[
            "systematic review", "meta-analysis", "prisma", "inclusion criteria",
            "exclusion criteria", "forest plot", "funnel plot", "publication bias",
            "effect size", "cochrane",
            "纳入标准", "排除标准", "发表偏倚", "森林图", "效应量",
        ],
        object_label: "数据提取表",
    },
    ParadigmSpec {
        id: "paradigm-computational-sim",
        name: "计算模拟型范式",
        sections: &[
            "numerical method", "simulation", "verification", "validation",
            "finite element", "computational model", "模型", "模拟", "数值",
        ],
        keywords: &[
            "finite element", "fem", "fvm", "mesh", "convergence",
            "boundary condition", "numerical simulation", "computational fluid",
            "cfd", "discretization", "governing equation",
            "有限元", "网格", "收敛", "边界条件", "数值模拟", "计算流体",
            "控制方程", "离散化",
        ],
        object_label: "控制方程/公式",
    },
    ParadigmSpec {
        id: "paradigm-design-science",
        name: "设计与构建型范式",
        sections: &[
            "design science", "implementation", "artifact", "prototype",
            "user study", "设计", "实现",
        ],
        keywords: &[
            "design science", "prototype", "user study", "artifact", "dsrm",
            "usability", "design principles", "heuristic evaluation", "wireframe",
            "设计科学", "原型", "用户研究", "构件", "可用性", "设计原则",
        ],
        object_label: "系统架构图",
    },
];

/// 实证研究通用结构信号（独立于各范式专属章节词典）
///
/// 用于判定论文是否具备实证研究骨架。此前该判断复用"其他范式的专属章节词"，
/// 专属词典覆盖不足时（如章节名为 Methods/Results 的社科实验论文）
/// 会得出"没有实证章节"的结论，反而把理论论述型推上满分。
const EMPIRICAL_STRUCTURE_SECTIONS: &[&str] = &[
    "method",
    "methodology",
    "research design",
    "participant",
    "subject",
    "data collection",
    "procedure",
    "experiment",
    "results",
    "finding",
    "data analysis",
    "measurement",
    "questionnaire",
    "survey",
    "方法",
    "研究设计",
    "被试",
    "参与者",
    "数据收集",
    "实验",
    "结果",
    "数据分析",
    "问卷",
    "量表",
];

/// 实证研究的统计 / 方法学关键词（正文层，子串匹配以覆盖词形变化）
const EMPIRICAL_STATS_KEYWORDS: &[&str] = &[
    "anova",
    "regression",
    "correlation",
    "significan",
    "cronbach",
    "likert",
    "standard deviation",
    "hypothesis",
    "sample size",
    "spss",
    "t-test",
    "chi-square",
    "confidence interval",
    "effect size",
    "within-subjects",
    "between-subjects",
    "方差分析",
    "回归",
    "显著性",
    "信度",
    "效度",
    "量表",
    "假设检验",
    "样本量",
];

/// 判定论文是否具备实证研究骨架：≥2 个结构章节，或 ≥1 个结构章节且 ≥2 个统计方法词
fn is_empirical_research(headings: &[String], haystack: &str) -> bool {
    let lower: Vec<String> = headings.iter().map(|h| h.to_lowercase()).collect();
    let section_hits = EMPIRICAL_STRUCTURE_SECTIONS
        .iter()
        .filter(|s| lower.iter().any(|h| h.contains(**s)))
        .count();
    if section_hits >= 2 {
        return true;
    }
    if section_hits == 0 {
        return false;
    }
    EMPIRICAL_STATS_KEYWORDS
        .iter()
        .filter(|k| haystack.contains(**k))
        .count()
        >= 2
}

/// 学科识别的弱信号词：跨学科论文中高频出现、区分度低，
/// 仅当出现在标题 / 关键词中才计分（正文与摘要中出现不计），
/// 避免 "communication / media / design" 等泛用词把论文误判到新闻传播学、设计学
const WEAK_DISCIPLINE_TERMS: &[&str] = &[
    "communication",
    "media",
    "design",
    "behavior",
    "behaviour",
    "experience",
    "system",
    "technology",
    "information",
    "learning",
    "management",
    "art",
];

/// 学科大类→范式先验（report.md §4）：命中学科关键词后为主/次范式加分
struct DisciplinePrior {
    keywords: &'static [&'static str],
    primary: usize,   // PARADIGMS 索引
    secondary: usize, // PARADIGMS 索引
}

const DISCIPLINE_PRIORS: &[DisciplinePrior] = &[
    // 数理科学 → 计算模拟主 / 实验对比次
    DisciplinePrior {
        keywords: &["equation", "physics", "mathematical", "numerical", "simulation", "fluid"],
        primary: 6,
        secondary: 0,
    },
    // 生命与医学 → 对照实验主 / 综述元分析次
    DisciplinePrior {
        keywords: &["clinical", "patients", "randomized", "therapy", "surgery", "treatment"],
        primary: 1,
        secondary: 5,
    },
    // 工程与信息 → 实验对比主 / 设计构建次
    DisciplinePrior {
        keywords: &["algorithm", "system", "framework", "implementation", "performance", "benchmark"],
        primary: 0,
        secondary: 7,
    },
    // 经济与管理 → 实证统计主 / 案例次
    DisciplinePrior {
        keywords: &["regression", "panel", "empirical", "firms", "market", "survey data", "instrumental"],
        primary: 2,
        secondary: 4,
    },
    // 社会科学 → 实证统计主 / 案例次
    DisciplinePrior {
        keywords: &["survey", "questionnaire", "interviews", "qualitative", "focus group"],
        primary: 2,
        secondary: 4,
    },
    // 人文与艺术 → 理论论述主 / 案例次
    DisciplinePrior {
        keywords: &["concept", "text", "critique", "discourse", "aesthetic", "narrative", "hermeneutic"],
        primary: 3,
        secondary: 4,
    },
];

/// 国内学科词典（《研究生教育学科专业目录（2022 年）》口径）
/// category=学科门类，code=一级学科代码，name=一级学科名
struct DomesticSpec {
    category: &'static str,
    code: &'static str,
    name: &'static str,
    keywords: &'static [&'static str],
}

const DOMESTIC_DISCIPLINES: &[DomesticSpec] = &[
    DomesticSpec { category: "01 哲学", code: "0101", name: "哲学", keywords: &["philosophy", "philosophical", "metaphysics", "epistemology", "ethics", "哲学", "思辨", "认识论", "存在论", "形而上学", "伦理学"] },
    DomesticSpec { category: "02 经济学", code: "0201", name: "理论经济学", keywords: &["political economy", "economic history", "macroeconomic", "microeconomic", "发展经济学", "政治经济学", "经济思想史", "宏观经济学", "微观经济学"] },
    DomesticSpec { category: "02 经济学", code: "0202", name: "应用经济学", keywords: &["finance", "financial", "fiscal", "international trade", "industrial economy", "货币", "金融", "财税", "国际贸易", "产业经济", "区域经济"] },
    DomesticSpec { category: "03 法学", code: "0301", name: "法学", keywords: &["legal", "law", "court", "legislation", "constitution", "judicial", "criminal law", "法学", "法律", "司法", "立法", "宪法", "刑法", "民法"] },
    DomesticSpec { category: "03 法学", code: "0303", name: "社会学", keywords: &["sociology", "social inequality", "community", "social network", "class structure", "社会学", "社会", "阶层", "不平等", "社区", "社会网络"] },
    DomesticSpec { category: "03 法学", code: "0304", name: "民族学", keywords: &["ethnology", "ethnography", "folklore", "ethnic", "minority", "民族学", "民俗", "民族文化", "人类学", "民族", "田野"] },
    DomesticSpec { category: "04 教育学", code: "0401", name: "教育学", keywords: &["education", "teaching", "curriculum", "pedagogy", "教育学", "教学", "课程", "教育", "教学法"] },
    DomesticSpec { category: "04 教育学", code: "0402", name: "心理学", keywords: &["psychology", "cognitive", "behavior", "memory", "emotion", "perception", "心理学", "认知", "行为", "记忆", "情绪", "知觉"] },
    DomesticSpec { category: "05 文学", code: "0501", name: "中国语言文学", keywords: &["chinese literature", "ancient chinese", "现代文学", "中国文学", "古代文学", "汉语", "中文"] },
    DomesticSpec { category: "05 文学", code: "0502", name: "外国语言文学", keywords: &["foreign language", "linguistics", "grammar", "phonology", "translation studies", "语言学", "语法", "语音", "翻译研究", "外语"] },
    DomesticSpec { category: "05 文学", code: "0503", name: "新闻传播学", keywords: &["communication", "media", "journalism", "audience", "news", "propaganda", "新闻", "传播", "媒介", "媒体", "受众", "舆情", "宣传", "出版"] },
    DomesticSpec { category: "06 历史学", code: "0601", name: "考古学", keywords: &["archaeolog", "excavation", "site", "考古", "出土", "遗址", "文物考古"] },
    DomesticSpec { category: "06 历史学", code: "0602", name: "中国史", keywords: &["chinese history", "ancient history", "中国历史", "古代史", "近代史", "清史"] },
    DomesticSpec { category: "06 历史学", code: "0603", name: "世界史", keywords: &["world history", "global history", "世界历史", "全球史", "世界史"] },
    DomesticSpec { category: "07 理学", code: "0701", name: "数学", keywords: &["mathematics", "mathematical", "theorem", "proof", "algebra", "topology", "calculus", "probability", "数学", "定理", "证明", "代数", "拓扑", "微积分", "概率"] },
    DomesticSpec { category: "07 理学", code: "0702", name: "物理学", keywords: &["physics", "quantum", "particle", "electromagnetic", "relativity", "optics", "物理", "量子", "粒子", "电磁", "相对论", "光学"] },
    DomesticSpec { category: "07 理学", code: "0703", name: "化学", keywords: &["chemistry", "molecule", "chemical", "synthesis", "reaction", "catalyst", "化学", "分子", "合成", "反应", "催化"] },
    DomesticSpec { category: "07 理学", code: "0710", name: "生物学", keywords: &["biology", "cell", "gene", "protein", "genome", "organism", "dna", "生物", "细胞", "基因", "蛋白质", "基因组", "有机体"] },
    DomesticSpec { category: "07 理学", code: "0714", name: "统计学", keywords: &["statistics", "statistical", "hypothesis testing", "回归分析", "统计学", "统计", "假设检验"] },
    DomesticSpec { category: "08 工学", code: "0810", name: "信息与通信工程", keywords: &["telecommunication", "signal processing", "通信", "信号", "信息网络", "信息与通信"] },
    DomesticSpec { category: "08 工学", code: "0812", name: "计算机科学与技术", keywords: &["computer science", "software", "algorithm", "programming", "dataset", "machine learning", "deep learning", "neural network", "benchmark", "计算机", "算法", "软件", "数据集", "机器学习", "深度学习", "程序", "神经网络"] },
    DomesticSpec { category: "08 工学", code: "0813", name: "建筑学", keywords: &["architecture", "architectural", "建筑", "建筑学", "空间设计"] },
    DomesticSpec { category: "10 医学", code: "1002", name: "临床医学", keywords: &["clinical", "patient", "disease", "surgery", "diagnosis", "therapy", "医学", "临床", "患者", "疾病", "手术", "诊断", "治疗"] },
    DomesticSpec { category: "12 管理学", code: "1201", name: "管理科学与工程", keywords: &["management science", "operations research", "decision science", "管理科学", "运筹", "决策"] },
    DomesticSpec { category: "12 管理学", code: "1202", name: "工商管理", keywords: &["business management", "organization", "strategy", "leadership", "marketing", "企业", "组织", "战略", "领导力", "营销"] },
    DomesticSpec { category: "13 艺术学", code: "1301", name: "艺术学", keywords: &["art", "aesthetic", "painting", "sculpture", "music", "dance", "theater", "film", "television", "calligraphy", "艺术", "艺术学", "美学", "绘画", "雕塑", "音乐", "舞蹈", "戏剧", "电影", "电视", "书法"] },
    DomesticSpec { category: "14 交叉学科", code: "1403", name: "设计学", keywords: &["design", "industrial design", "interaction design", "visual communication", "设计", "设计学", "工业设计", "交互设计", "视觉传达"] },
    DomesticSpec { category: "14 交叉学科", code: "1405", name: "智能科学与技术", keywords: &["artificial intelligence", "intelligent", "智能", "人工智能", "智能体", "智能科技"] },
    DomesticSpec { category: "14 交叉学科（自设）", code: "99F1", name: "非物质文化遗产学", keywords: &["intangible cultural heritage", "cultural heritage", "heritage", "heritage protection", "非遗", "非物质文化遗产", "文化遗产", "遗产保护", "传承"] },
];

/// 国际学科词典（ASJC / WoS 口径）：supergroup=大类，code=ASJC 代码，name=类目名
struct InternationalSpec {
    supergroup: &'static str,
    code: &'static str,
    name: &'static str,
    keywords: &'static [&'static str],
}

const INTERNATIONAL_DISCIPLINES: &[InternationalSpec] = &[
    InternationalSpec { supergroup: "Computer Science", code: "1700", name: "Computer Science", keywords: &["computer science", "software", "algorithm", "programming", "dataset", "machine learning", "deep learning", "neural network", "benchmark", "计算机", "算法", "软件", "数据集", "机器学习", "深度学习", "程序", "神经网络"] },
    InternationalSpec { supergroup: "Engineering", code: "2200", name: "Engineering", keywords: &["engineering", "mechanical", "electrical", "civil engineering", "structural", "工程", "机械", "电气", "土木", "结构"] },
    InternationalSpec { supergroup: "Mathematics", code: "2600", name: "Mathematics", keywords: &["mathematics", "mathematical", "theorem", "proof", "algebra", "topology", "calculus", "probability", "数学", "定理", "证明", "代数", "拓扑", "微积分", "概率"] },
    InternationalSpec { supergroup: "Physics and Astronomy", code: "3100", name: "Physics and Astronomy", keywords: &["physics", "quantum", "particle", "electromagnetic", "relativity", "optics", "物理", "量子", "粒子", "电磁", "相对论", "光学"] },
    InternationalSpec { supergroup: "Chemistry", code: "1600", name: "Chemistry", keywords: &["chemistry", "molecule", "chemical", "synthesis", "reaction", "catalyst", "化学", "分子", "合成", "反应", "催化"] },
    InternationalSpec { supergroup: "Biochemistry, Genetics and Molecular Biology", code: "1300", name: "Biochemistry, Genetics and Molecular Biology", keywords: &["biology", "genome", "gene", "protein", "cell biology", "organism", "dna", "生物", "基因组", "基因", "蛋白质", "细胞", "有机体"] },
    InternationalSpec { supergroup: "Medicine", code: "2700", name: "Medicine", keywords: &["clinical", "patient", "disease", "surgery", "diagnosis", "therapy", "medical", "trial", "医学", "临床", "患者", "疾病", "手术", "诊断", "治疗"] },
    InternationalSpec { supergroup: "Environmental Science", code: "2300", name: "Environmental Science", keywords: &["environment", "climate change", "ecosystem", "pollution", "sustainability", "环境", "气候变化", "生态系统", "污染", "可持续"] },
    InternationalSpec { supergroup: "Arts and Humanities", code: "1213", name: "Visual Arts and Performing Arts", keywords: &["art", "aesthetic", "painting", "sculpture", "music", "dance", "theater", "film", "television", "cinema", "艺术", "美学", "绘画", "雕塑", "音乐", "舞蹈", "戏剧", "电影", "电视", "影视"] },
    InternationalSpec { supergroup: "Arts and Humanities", code: "1206", name: "Conservation", keywords: &["conservation", "heritage", "museum", "preservation", "restoration", "artifact", "遗产", "博物馆", "保护", "修复", "文物"] },
    InternationalSpec { supergroup: "Arts and Humanities", code: "1202", name: "History", keywords: &["history", "historical", "archive", "ancient", "historiography", "历史", "档案", "古代", "史学", "史料"] },
    InternationalSpec { supergroup: "Arts and Humanities", code: "1208", name: "Literature and Literary Theory", keywords: &["literature", "novel", "poetry", "fiction", "literary", "文学", "小说", "诗歌", "文学批评", "文本"] },
    InternationalSpec { supergroup: "Arts and Humanities", code: "1203", name: "Language and Linguistics", keywords: &["linguistics", "grammar", "phonology", "semantics", "syntax", "language", "语言学", "语言", "语法", "语音", "语义", "句法"] },
    InternationalSpec { supergroup: "Social Sciences", code: "3315", name: "Communication", keywords: &["communication", "media", "journalism", "audience", "news", "propaganda", "传播", "媒介", "媒体", "受众", "新闻", "舆情", "宣传"] },
    InternationalSpec { supergroup: "Social Sciences", code: "3316", name: "Cultural Studies", keywords: &["cultural studies", "cultural identity", "culture", "文化研究", "文化", "文化认同"] },
    InternationalSpec { supergroup: "Social Sciences", code: "3312", name: "Sociology and Political Science", keywords: &["sociology", "society", "political", "government", "democracy", "policy", "社会", "政治", "政府", "民主", "政策"] },
    InternationalSpec { supergroup: "Social Sciences", code: "3304", name: "Education", keywords: &["education", "teaching", "curriculum", "pedagogy", "learning", "教育", "教学", "课程", "学习"] },
    InternationalSpec { supergroup: "Psychology", code: "3200", name: "Psychology", keywords: &["psychology", "cognitive", "behavior", "mental health", "perception", "memory", "emotion", "心理学", "认知", "行为", "心理", "知觉", "记忆", "情绪"] },
    InternationalSpec { supergroup: "Social Sciences", code: "3309", name: "Library and Information Science", keywords: &["information science", "library science", "documentation", "情报学", "图书馆学", "信息科学"] },
    InternationalSpec { supergroup: "Business, Management and Accounting", code: "1400", name: "Business, Management and Accounting", keywords: &["management", "organization", "business", "strategy", "leadership", "accounting", "管理", "组织", "企业", "战略", "领导力", "会计"] },
    InternationalSpec { supergroup: "Economics, Econometrics and Finance", code: "2000", name: "Economics, Econometrics and Finance", keywords: &["economics", "economic", "market", "firm", "trade", "inflation", "finance", "financial", "asset", "portfolio", "经济", "经济学", "市场", "企业", "贸易", "金融", "资产", "股票"] },
    InternationalSpec { supergroup: "Social Sciences", code: "3308", name: "Law", keywords: &["legal", "law", "court", "legislation", "constitution", "judicial", "法学", "法律", "司法", "立法", "宪法"] },
];

/// 研究方向（主题层）词典：学科之下的具体方向
struct DirectionSpec {
    name: &'static str,
    keywords: &'static [&'static str],
}

const RESEARCH_DIRECTIONS: &[DirectionSpec] = &[
    DirectionSpec { name: "数字遗产与非遗数字化", keywords: &["heritage", "intangible cultural heritage", "digital heritage", "preservation", "数字化保护", "数字遗产", "非遗", "文化遗产", "遗产保护", "传承"] },
    DirectionSpec { name: "虚拟现实与沉浸式技术", keywords: &["virtual reality", "vr", "augmented reality", "immersive", "虚拟现实", "增强现实", "沉浸", "xr", "ar"] },
    DirectionSpec { name: "人工智能应用", keywords: &["artificial intelligence", "machine learning", "llm", "large language model", "deep learning", "人工智能", "机器学习", "大语言模型", "深度学习"] },
    DirectionSpec { name: "交互设计与互动叙事", keywords: &["interactive", "interaction design", "narrative", "game", "storytelling", "交互", "互动", "叙事", "游戏"] },
    DirectionSpec { name: "影视与视听传播", keywords: &["film", "cinema", "television", "video", "audiovisual", "短视频", "影视", "视听", "电影", "电视"] },
    DirectionSpec { name: "媒介与平台研究", keywords: &["social media", "platform", "digital media", "社交媒体", "平台", "数字媒体", "新媒体"] },
    DirectionSpec { name: "文化传播与国际传播", keywords: &["cultural communication", "cross-cultural", "international communication", "文化传播", "跨文化", "国际传播"] },
    DirectionSpec { name: "教育与学习", keywords: &["education", "learning", "teaching", "training", "教育", "教学", "学习"] },
    DirectionSpec { name: "智能传播与舆情", keywords: &["public opinion", "sentiment", "disinformation", "舆情", "舆论", "虚假信息"] },
    DirectionSpec { name: "设计与创意产业", keywords: &["creative industry", "design thinking", "innovation", "创意产业", "设计思维", "创新"] },
];

/// 特殊对象命中：0 或 1（按解析产物的结构统计粗判）
fn object_hit(spec: &ParadigmSpec, input: &ParadigmInput) -> bool {
    match spec.id {
        "paradigm-experimental-baseline"
        | "paradigm-empirical-stat"
        | "paradigm-case-study"
        | "paradigm-systematic-review" => input.table_count > 0,
        "paradigm-computational-sim" => input.math_symbols >= 4,
        "paradigm-design-science" => input.image_count > 0,
        _ => false,
    }
}

/// 主流程：输入信号 → 评分 → 主/次范式 → 交叉类型 → 输出契约
pub fn recognize(input: &ParadigmInput) -> ParadigmRecognition {
    let haystack = format!(
        "{} {} {} {}",
        input.title,
        input.abstract_text,
        input.body_sample,
        input.headings.join(" ")
    )
    .to_lowercase();

    // 实证骨架检测（独立于范式专属词典）：替代此前"是否有其他范式的专属章节命中"
    // 这一覆盖不足的反向信号 —— 它会让 Methods/Results 结构的论文被反判为理论论述型
    let empirical_study = is_empirical_research(&input.headings, &haystack);

    // 学科先验加分
    let mut prior: Vec<f64> = vec![0.0; PARADIGMS.len()];
    for d in DISCIPLINE_PRIORS {
        if d.keywords.iter().any(|k| haystack.contains(*k)) {
            prior[d.primary] += 0.06;
            prior[d.secondary] += 0.03;
        }
    }

    // —— 国内学科识别（2022 目录口径）：按关键词命中数打分，命中 4 个视为高置信 ——
    // 弱信号词（communication/media/design 等）只在标题与关键词中计分：
    // 这些词在任何跨学科论文的正文里都会出现，用全文匹配会主导学科判定
    let title_kw_hay = format!("{} {}", input.title, input.keywords).to_lowercase();
    let mut dom_scores: Vec<(usize, &DomesticSpec, Vec<&'static str>)> = DOMESTIC_DISCIPLINES
        .iter()
        .map(|d| {
            let matched: Vec<&'static str> = d
                .keywords
                .iter()
                .filter(|k| {
                    if WEAK_DISCIPLINE_TERMS.contains(k) {
                        word_hits(&title_kw_hay, k)
                    } else {
                        word_hits(&haystack, k)
                    }
                })
                .cloned()
                .collect();
            (matched.len(), d, matched)
        })
        .collect();
    dom_scores.sort_by(|a, b| b.0.cmp(&a.0));
    let (dom_best, dom_best_spec, dom_best_matched) = &dom_scores[0];
    let domestic = DomesticDiscipline {
        category: dom_best_spec.category.to_string(),
        code: dom_best_spec.code.to_string(),
        discipline: if *dom_best > 0 {
            dom_best_spec.name.to_string()
        } else {
            "未识别".to_string()
        },
        confidence: (*dom_best as f64 / 4.0).min(1.0),
        basis: if *dom_best > 0 {
            dom_best_matched
                .iter()
                .take(5)
                .map(|k| k.to_string())
                .collect()
        } else {
            vec!["未匹配到足够学科信号".to_string()]
        },
        secondary: dom_scores
            .iter()
            .skip(1)
            .filter(|(c, _, _)| *c >= 2)
            .map(|(c, spec, _)| SecondaryItem {
                name: spec.name.to_string(),
                weight: (*c as f64 / 4.0).min(1.0),
            })
            .take(3)
            .collect(),
    };

    // —— 国际学科识别（ASJC / WoS 口径）——
    let mut intl_scores: Vec<(usize, &InternationalSpec, Vec<&'static str>)> =
        INTERNATIONAL_DISCIPLINES
            .iter()
            .map(|d| {
                let matched: Vec<&'static str> = d
                    .keywords
                    .iter()
                    .filter(|k| {
                        if WEAK_DISCIPLINE_TERMS.contains(k) {
                            word_hits(&title_kw_hay, k)
                        } else {
                            word_hits(&haystack, k)
                        }
                    })
                    .cloned()
                    .collect();
                (matched.len(), d, matched)
            })
            .collect();
    intl_scores.sort_by(|a, b| b.0.cmp(&a.0));
    let (intl_best, intl_best_spec, intl_best_matched) = &intl_scores[0];
    let international = InternationalDiscipline {
        supergroup: intl_best_spec.supergroup.to_string(),
        code: intl_best_spec.code.to_string(),
        discipline: if *intl_best > 0 {
            intl_best_spec.name.to_string()
        } else {
            "Unidentified".to_string()
        },
        confidence: (*intl_best as f64 / 4.0).min(1.0),
        basis: if *intl_best > 0 {
            intl_best_matched
                .iter()
                .take(5)
                .map(|k| k.to_string())
                .collect()
        } else {
            vec!["no sufficient signals".to_string()]
        },
        secondary: intl_scores
            .iter()
            .skip(1)
            .filter(|(c, _, _)| *c >= 2)
            .map(|(c, spec, _)| SecondaryItem {
                name: spec.name.to_string(),
                weight: (*c as f64 / 4.0).min(1.0),
            })
            .take(3)
            .collect(),
    };

    // —— 研究方向（主题层）：命中 ≥2 个信号词才列入，取前 3 ——
    let mut dir_scores: Vec<(usize, &DirectionSpec)> = RESEARCH_DIRECTIONS
        .iter()
        .map(|d| {
            let c = d.keywords.iter().filter(|k| word_hits(&haystack, k)).count();
            (c, d)
        })
        .collect();
    dir_scores.sort_by(|a, b| b.0.cmp(&a.0));
    let directions: Vec<ResearchDirection> = dir_scores
        .iter()
        .filter(|(c, _)| *c >= 2)
        .map(|(c, spec)| ResearchDirection {
            name: spec.name.to_string(),
            weight: (*c as f64 / 3.0).min(1.0),
        })
        .take(3)
        .collect();

    struct Hit {
        idx: usize,
        score: f64,
        basis: Vec<String>,
    }
    let mut hits: Vec<Hit> = Vec::with_capacity(PARADIGMS.len());

    for (i, p) in PARADIGMS.iter().enumerate() {
        let lower_headings: Vec<String> =
            input.headings.iter().map(|h| h.to_lowercase()).collect();
        let matched_sections: Vec<&'static str> = p
            .sections
            .iter()
            .filter(|s| lower_headings.iter().any(|h| h.contains(**s)))
            .cloned()
            .collect();
        // 章节命中：命中 2 个专属章节即视为满分（多数论文只有 2-3 个关键章节）
        let section_hit = if p.sections.is_empty() {
            // 理论论述型：仅当论文确实不具备实证骨架时才成立
            if empirical_study {
                0.0
            } else {
                1.0
            }
        } else {
            (matched_sections.len() as f64 / 2.0).min(1.0)
        };

        let matched_keywords: Vec<&'static str> = p
            .keywords
            .iter()
            .filter(|k| haystack.contains(**k))
            .cloned()
            .collect();
        // 关键词命中：命中 4 个即视为满分
        let kw_hit = (matched_keywords.len() as f64 / 4.0).min(1.0);

        let obj_hit = object_hit(p, input);

        // 对象信号只能作为加分项：无任何章节/关键词文本信号时，范式不成立
        let has_text_signal = section_hit > 0.0 || kw_hit > 0.0;
        let score = if has_text_signal {
            0.45 * section_hit + 0.40 * kw_hit + 0.15 * if obj_hit { 1.0 } else { 0.0 }
                + prior[i]
        } else {
            0.0
        };

        let mut basis: Vec<String> = Vec::new();
        if section_hit > 0.0 && !matched_sections.is_empty() {
            basis.push(format!("章节信号命中：{}", matched_sections.join(" / ")));
        }
        if !matched_keywords.is_empty() {
            basis.push(format!(
                "关键词命中：{}",
                matched_keywords
                    .iter()
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" / ")
            ));
        }
        if obj_hit && !p.object_label.is_empty() && has_text_signal {
            basis.push(format!("检测到{}", p.object_label));
        }
        if prior[i] > 0.0 && has_text_signal {
            basis.push("命中学科大类先验".to_string());
        }
        if p.sections.is_empty() && section_hit > 0.0 {
            basis.push("无实证方法章节（主题式结构）".to_string());
        }

        hits.push(Hit {
            idx: i,
            score,
            basis,
        });
    }

    hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    let best = &hits[0];
    let primary = &PARADIGMS[best.idx];
    let primary_basis = if best.basis.is_empty() {
        vec!["信号匹配不足，建议人工确认范式".to_string()]
    } else {
        best.basis.clone()
    };

    // 次要范式：得分 ≥ 0.15 的其他范式
    let secondary: Vec<SecondaryParadigm> = hits
        .iter()
        .skip(1)
        .filter(|h| h.score >= 0.15)
        .map(|h| SecondaryParadigm {
            paradigm_id: PARADIGMS[h.idx].id.to_string(),
            paradigm_name: PARADIGMS[h.idx].name.to_string(),
            weight: h.score,
            basis: h.basis.clone(),
        })
        .collect();

    let second_score = secondary.first().map(|s| s.weight).unwrap_or(0.0);
    let multi = hits.iter().filter(|h| h.score >= 0.15).count() >= 3;

    // 交叉类型判定（阈值校准：0.75 / 0.50 / 0.25）
    let (cross_type, fallback) = if best.score < 0.50 {
        ("单学科".to_string(), true)
    } else if multi {
        ("多学科交叉".to_string(), false)
    } else if best.score >= 0.75 && second_score < 0.25 {
        ("单学科".to_string(), false)
    } else if second_score >= 0.25 {
        ("双学科交叉".to_string(), false)
    } else {
        ("单学科".to_string(), false)
    };

    ParadigmRecognition {
        paradigm_id: primary.id.to_string(),
        paradigm_name: primary.name.to_string(),
        // 各分项与学科先验相加后可能略超 1，收敛到 [0,1] 便于前端直接当百分比展示
        confidence: best.score.min(1.0),
        identification_basis: primary_basis,
        cross_type,
        secondary_paradigms: secondary,
        human_review_required: best.score < 0.50,
        fallback_to_common: fallback,
        domestic,
        international,
        directions,
    }
}

/// 关键词命中：短英文词需按词边界匹配（避免 "art" 误命中 "artificial/particle"），
/// 多词短语与含中文的关键词走普通子串匹配
fn word_hits(hay: &str, kw: &str) -> bool {
    let is_single_ascii_word = kw.chars().all(|c| c.is_ascii_alphabetic())
        && kw.chars().all(|c| !c.is_ascii_uppercase())
        && !kw.contains(' ');
    if !is_single_ascii_word {
        return hay.contains(kw);
    }
    let bytes = hay.as_bytes();
    let mut start = 0;
    while let Some(pos) = hay[start..].find(kw) {
        let abs = start + pos;
        let before_ok = abs == 0 || !bytes[abs - 1].is_ascii_alphanumeric();
        let after = abs + kw.len();
        // 允许英文复数/第三人称单数后缀：纯词边界匹配会漏掉最常见的词形变化
        // （participant → participants、experiment → experiments）
        let after_ok = after >= bytes.len()
            || !bytes[after].is_ascii_alphanumeric()
            || (bytes[after] == b's'
                && (after + 1 >= bytes.len() || !bytes[after + 1].is_ascii_alphanumeric()))
            || (bytes[after] == b'e'
                && after + 1 < bytes.len()
                && bytes[after + 1] == b's'
                && (after + 2 >= bytes.len() || !bytes[after + 2].is_ascii_alphanumeric()));
        if before_ok && after_ok {
            return true;
        }
        start = abs + kw.len();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_experimental_paper() {
        let input = ParadigmInput {
            title: "Improving Image Classification with New Baseline".into(),
            abstract_text: "We benchmark our method against SOTA baselines on three datasets, report accuracy and F1 metrics, and conduct ablation studies.".into(),
            headings: vec![
                "1. Introduction".into(),
                "2. Method".into(),
                "3. Experiments".into(),
                "4. Results".into(),
                "5. Conclusion".into(),
            ],
            body_sample: "baseline ablation dataset accuracy".into(),
            table_count: 3,
            image_count: 2,
            math_symbols: 2,
            ..Default::default()
        };
        let r = recognize(&input);
        assert_eq!(r.paradigm_id, "paradigm-experimental-baseline");
        assert!(r.confidence >= 0.7);
        assert_eq!(r.cross_type, "单学科");
    }

    #[test]
    fn theoretical_paper_no_method_sections() {
        let input = ParadigmInput {
            title: "The Concept of Genealogy in Modern Critique".into(),
            abstract_text: "This paper argues that the concept of discourse requires a new genealogy.".into(),
            headings: vec![
                "1. 何谓批评".into(),
                "2. 概念的谱系".into(),
                "3. 反驳与回应".into(),
            ],
            body_sample: "argue concept critique objection discourse".into(),
            ..Default::default()
        };
        let r = recognize(&input);
        assert_eq!(r.paradigm_id, "paradigm-theoretical");
    }

    #[test]
    fn empirical_paper_with_regression() {
        let input = ParadigmInput {
            title: "Firm Performance and Market Competition".into(),
            abstract_text: "We estimate a panel regression with fixed effects and instrumental variables to address endogeneity.".into(),
            headings: vec![
                "1. Introduction".into(),
                "2. Data".into(),
                "3. Empirical Strategy".into(),
                "4. Results".into(),
                "5. Robustness".into(),
                "6. Conclusion".into(),
            ],
            body_sample: "regression endogeneity panel data fixed effects robustness".into(),
            table_count: 4,
            ..Default::default()
        };
        let r = recognize(&input);
        assert_eq!(r.paradigm_id, "paradigm-empirical-stat");
    }

    /// 回归用例：VR 聊天机器人实验论文（Methods / Participants / Results 结构）
    ///
    /// 实测中曾被误判为"理论论述型"并归入新闻传播学。原因是专属章节词典缺少
    /// IMRaD 通用章节名，导致 Theory 型靠"无实证章节"的反向信号拿满分；
    /// 学科则被正文里的 communication / media 等泛用词带偏。
    #[test]
    fn social_science_experiment_not_misjudged_as_theoretical() {
        let input = ParadigmInput {
            title: "Confiding to AI: Impacts of ICE Framework-Based Body Movements of VR Chatbots on User Self-Disclosure and Experience".into(),
            abstract_text: "This study proposes the ICE Movements Framework and conducted a single-factor within-subjects experiment involving 56 university students. Quantitative and qualitative results revealed that body movements significantly enhanced users' self-disclosure willingness, satisfaction, trust, and intention to use.".into(),
            keywords: "Artificial intelligence; virtual reality; chatbots; physical movements; self-disclosure".into(),
            headings: vec![
                "ABSTRACT".into(),
                "1. Introduction".into(),
                "2. Related studies".into(),
                "3. Methods".into(),
                "3.5. Participants".into(),
                "3.6. Quantitative data - scale measurement".into(),
                "3.9. Data statistics and analyses".into(),
                "4. Results".into(),
                "5. Discussion".into(),
            ],
            body_sample: "the experiment involved 56 participants; ANOVA showed significant differences across conditions; reliability and validity were evaluated".into(),
            table_count: 6,
            image_count: 12,
            math_symbols: 0,
            ..Default::default()
        };
        let r = recognize(&input);
        assert_ne!(
            r.paradigm_id, "paradigm-theoretical",
            "具备 Methods/Results 结构的实证论文不应被判为理论论述型"
        );
        assert_eq!(r.paradigm_id, "paradigm-experimental-baseline");
        assert!(
            r.confidence > 0.7,
            "识别置信度应反映明确的结构信号，实际为 {}",
            r.confidence
        );
        assert_ne!(
            r.domestic.discipline, "新闻传播学",
            "学科不应因正文中的 communication / media 等泛用词被误判"
        );
    }

    #[test]
    fn chinese_systematic_review() {
        let input = ParadigmInput {
            title: "教育干预的元分析".into(),
            abstract_text: "系统综述检索了 12 个数据库，纳入 45 项随机对照试验，报告纳入与排除标准及发表偏倚评估。".into(),
            headings: vec![
                "1. 引言".into(),
                "2. 检索策略".into(),
                "3. 纳入标准".into(),
                "4. 结果".into(),
            ],
            body_sample: "效应量 森林图 异质性 荟萃分析".into(),
            table_count: 2,
            image_count: 1,
            ..Default::default()
        };
        let r = recognize(&input);
        assert_eq!(r.paradigm_id, "paradigm-systematic-review");
    }

    #[test]
    fn vr_ich_paper_dual_classification() {
        // VR + 非物质文化遗产 → 国内：非物质文化遗产学；国际：Conservation
        let input = ParadigmInput {
            title: "Preserving Intangible Cultural Heritage through Virtual Reality".into(),
            abstract_text: "We present a virtual reality system for the digital preservation of intangible cultural heritage, including immersive interaction design for museum visitors.".into(),
            headings: vec![
                "1. Introduction".into(),
                "2. Related Work".into(),
                "3. System Design".into(),
                "4. Implementation".into(),
                "5. User Study".into(),
                "6. Conclusion".into(),
            ],
            body_sample: "heritage intangible cultural heritage virtual reality immersive digital preservation".into(),
            image_count: 4,
            ..Default::default()
        };
        let r = recognize(&input);
        assert_eq!(r.paradigm_id, "paradigm-design-science");
        // 国内口径应命中「非物质文化遗产学」（遗产信号强）
        assert_eq!(r.domestic.discipline, "非物质文化遗产学");
        // 国际口径应命中「Conservation」
        assert_eq!(r.international.discipline, "Conservation");
        // 方向层应同时识别出数字遗产与虚拟现实
        let names: Vec<&str> = r.directions.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"数字遗产与非遗数字化"));
        assert!(names.contains(&"虚拟现实与沉浸式技术"));
    }
}
