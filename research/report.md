# 学科研究范式与论文拆解方案预设库

生成日期：2026-08-21 ｜ 调研方式：tavily 深度搜索（10 路并行，覆盖 8 大学术范式 + 学科交叉参照）

---

## 1. 概述

本报告为"文献阅读台"的 AI 拆解功能（F4/模块 D）提供**学科研究范式预设库**，包含：

- 8 种常见研究范式的结构要求、报告标准、拆解字段集
- 范式识别信号（AI 用于自动判断论文所属范式）
- 学科大类→范式映射表（用于交叉学科检测）
- 交叉拆解的路由规则与字段合并策略

搜索来源：tavily（检索关键词涵盖各范式权威方法论文献、国际报告标准、学科期刊惯例）。

---

## 2. 八大学术范式详述

### 2.1 实验对比型范式（CS/AI/ML）

**适用学科**：计算机科学、人工智能、数据科学、信息检索、NLP、计算机视觉

**论文结构**：IMRaD 变体（Introduction → Method → Experiments → Discussion）

**报告规范**：NeurIPS/ACL 模板惯例

**识别信号**：
- 章节含"Experiment"、"Benchmark"、"Evaluation"、"Results"
- 出现"baseline"、"SOTA"、"state-of-the-art"、"ablation"等术语
- 包含数据集表格（dataset statistics）和指标对比表
- 代码仓库链接（GitHub）或可复现性声明

**特有拆解字段**：
| 字段 | 类型 | 说明 |
|------|------|------|
| dataset_name | string | 使用的数据集名称 |
| dataset_statistics | table | 数据集统计信息（大小/类别分布等） |
| baseline_methods | list[string] | 对比的基线/SOTA 方法列表 |
| evaluation_metrics | list[string] | 评价指标（accuracy, F1, BLEU, PSNR 等） |
| main_results_table | table | 核心结果对比表 |
| ablation_study | table | 消融实验结果 |
| experimental_setup | text | 实验环境配置（硬件/超参数/框架版本） |
| reproducibility | string | 代码/数据可复现性声明 |
| sota_comparison | text | 与 SOTA 方法的定性定量对比分析 |

**渲染重点**：指标对比表需高亮最优值（加粗/着色），消融实验表需展示逐项移除效果。

---

### 2.2 对照实验型范式（医学/心理/教育）

**适用学科**：医学、公共卫生、临床心理学、药学、教育学、农学

**论文结构**：CONSORT 结构化报告（Title/Abstract → Methods → Results → Discussion + Open Science 区块）

**报告规范**：CONSORT 2025（30 项清单 + 流程图，新增 Open Science 区块：试验注册/协议访问/数据共享/资金冲突）

**识别信号**：
- 出现"RCT"、"randomized"、"clinical trial"、"controlled trial"等术语
- 章节含"Methods"细分子节（随机化/盲法/样本量）
- 包含 CONSORT 流程图
- 有"trial registration"字段和伦理审批声明
- 使用 PICO/PICo 框架

**特有拆解字段**：
| 字段 | 类型 | 说明 |
|------|------|------|
| trial_design | enum | 平行组/交叉设计/析因设计/非劣效/等效性/集群随机 |
| randomization_method | string | 随机化方法（简单随机/区组随机/分层随机） |
| allocation_concealment | string | 分配隐藏机制 |
| blinding | enum | 开放/单盲/双盲/三盲/评估者盲 |
| sample_size | integer | 随机化样本量 |
| sample_size_estimation | text | 样本量估算依据 |
| intervention | text | 干预措施详细描述 |
| control_condition | text | 对照条件 |
| primary_outcome | text | 主要结局指标 |
| secondary_outcomes | list[string] | 次要结局指标 |
| consort_flow | image | CONSORT 流程图 |
| ethics_approval | string | 伦理审批机构与编号 |
| trial_registration | string | 试验注册号/平台 |
| harms | text | 不良事件报告 |

**渲染重点**：流程图需完整展示入组-分配-随访-分析全阶段，主要结局指标需给出效应量（OR/RR/HR）和置信区间。

---

### 2.3 实证统计型范式（经济/管理/社会/传播）

**适用学科**：经济学、管理学、社会学、政治学、传播学、金融学、公共政策

**论文结构**：Introduction → Literature → Theory/Model → Data → Econometric Model → Results → Robustness → Conclusion（MIT Empirical Paper Guidelines / NBER 惯例）

**报告规范**：MIT Empirical Paper Guidelines；识别策略包括 OLS/DID/IV/RDD/Heckman/PSM

**识别信号**：
- 章节含"Data"、"Empirical Strategy"、"Identification"、"Robustness"等
- 出现"regression"、"endogeneity"、"instrumental variable"、"DID"、"RDD"等术语
- 包含描述性统计表（descriptive statistics）和回归结果表
- 有 robustness checks 部分
- 数据来源说明（数据库/调查/面板）

**特有拆解字段**：
| 字段 | 类型 | 说明 |
|------|------|------|
| theoretical_framework | text | 理论框架/假设推导 |
| data_source | string | 数据来源（数据库/调查/实验/面板） |
| sample_description | text | 样本描述（时间范围/样本量/筛选条件） |
| descriptive_statistics | table | 描述性统计表 |
| identification_strategy | string | 识别策略（OLS/DID/IV/RDD/Heckman/PSM） |
| main_regression | table | 核心回归结果表 |
| robustness_checks | list[string] | 稳健性检验列表 |
| endogeneity | text | 内生性处理说明 |
| heterogeneity_analysis | table | 异质性分析结果 |
| mechanism_analysis | text | 机制/中介效应分析 |

**渲染重点**：回归表需展示系数、标准误、显著性标记、样本量、R²；异质性分析按子样本分组展示。

---

### 2.4 理论论述型范式（哲学/人文/艺术）

**适用学科**：哲学、文学理论、艺术学理论、法学理论、历史学、语言学

**论文结构**：主题式结构（Thesis → Argument → Objection → Response），非 IMRaD

**报告规范**：学科期刊惯例（无统一规范，以论证逻辑为纲）

**识别信号**：
- 无"Method"、"Experiment"、"Data"等实证章节
- 章节标题为概念/命题/主题式（如"何谓XX"、"XX的谱系"、"XX的困境"）
- 出现"argue"、"contend"、"concept"、"genealogy"、"critique"等术语
- 大量引用经典理论文本
- 包含"objection"（反驳）和"reply"（回应）结构

**特有拆解字段**：
| 字段 | 类型 | 说明 |
|------|------|------|
| thesis_statement | text | 核心论点/命题 |
| concept_definitions | list[string] | 关键概念界定 |
| argument_structure | text | 论证结构/推理路径 |
| intellectual_lineage | text | 思想脉络/学术谱系定位 |
| key_sources | list[string] | 主要对话/批判的已有理论 |
| objections | list[string] | 反驳与回应 |
| implications | text | 理论含义/推论 |
| contribution | text | 理论贡献（澄清/反驳/拓展/综合） |

**渲染重点**：论证结构可视化（思维导图/流程图），概念界定采用高亮引用块，反驳-回应成对展示。

---

### 2.5 案例研究型范式（人类学/社会学/管理/法学）

**适用学科**：人类学、社会学、管理案例研究、法学案例、民族志、政治学案例

**论文结构**：Introduction → Literature → Methods → Case Description → Cross-Case Analysis → Discussion

**报告规范**：Yin (2018) 案例研究规范——四数据收集原则（多源证据/案例数据库/证据链/社交媒体谨慎）+ 五分析技术（模式匹配/解释构建/时序分析/逻辑模型/跨案例综合）

**识别信号**：
- 章节含"Case Study"、"Case Description"、"Case Selection"等
- 出现"single case"、"multiple case"、"cross-case"、"pattern matching"等术语
- 明确说明"how/why"研究问题
- 涉及多源证据（访谈/观察/文档/档案）
- 分析单元（unit of analysis）定义清晰

**特有拆解字段**：
| 字段 | 类型 | 说明 |
|------|------|------|
| case_selection | text | 案例选择依据（典型性/极端性/方便性） |
| unit_of_analysis | string | 分析单元 |
| data_collection | list[string] | 数据收集方法（访谈/观察/文档/档案） |
| number_of_sources | integer | 证据来源数量 |
| triangulation | text | 三角验证方法 |
| analysis_method | string | 分析方法（模式匹配/解释构建/时序分析/逻辑模型/跨案例综合） |
| case_description | text | 案例描述 |
| propositions | list[string] | 理论命题 |
| cross_case_patterns | table | 跨案例模式对比表 |

**渲染重点**：跨案例对比表以矩阵形式展示（案例 × 维度），分析过程展示证据链（evidence chain）。

---

### 2.6 综述与元分析型范式

**适用学科**：循证医学、社会科学综述、教育学综述、环境学综述

**论文结构**：PRISMA 2020 结构化报告（27 项清单 + 流程图，覆盖 title→abstract→intro→methods→results→discussion→funding）

**报告规范**：PRISMA 2020 / PROSPERO 注册；质量评估工具包括 Cochrane ROB、AMSTAR、NOS；异质性评估用 I² 统计量；发表偏倚用漏斗图/Egger 检验；证据等级用 GRADE

**识别信号**：
- 章节含"Systematic Review"、"Meta-Analysis"、"Literature Review"等
- 出现"PRISMA"、"search strategy"、"inclusion criteria"、"exclusion criteria"等术语
- 包含 PRISMA 流程图（筛选四阶段）
- 有质量评估（ROB/AMSTAR/NOS）部分
- 出现森林图（forest plot）和漏斗图（funnel plot）

**特有拆解字段**：
| 字段 | 类型 | 说明 |
|------|------|------|
| review_question | text | 综述问题（PICO/PICo 框架） |
| search_strategy | text | 检索策略（数据库/关键词/时间范围） |
| inclusion_criteria | list[string] | 纳入标准 |
| exclusion_criteria | list[string] | 排除标准 |
| prisma_flow | image | PRISMA 筛选流程图 |
| total_records | integer | 初始检索记录数 |
| included_studies | integer | 最终纳入研究数 |
| quality_assessment | string | 质量评估工具（Cochrane ROB/AMSTAR/NOS 等） |
| data_extraction | table | 数据提取表 |
| synthesis_method | enum | 叙述性综合/元分析/主题综合/元人种志/现实综合 |
| heterogeneity | text | 异质性评估（I² 统计量/亚组分析） |
| publication_bias | text | 发表偏倚评估（漏斗图/Egger 检验） |
| certainty_evidence | string | 证据确定性等级（GRADE 等） |

**渲染重点**：PRISMA 流程图必须完整展示，森林图展示合并效应量，异质性评估用 I² 数值+色带警示。

---

### 2.7 计算模拟型范式（物理/化学/工程/材料/气候）

**适用学科**：物理学、化学、机械工程、材料科学、气候科学、土木工程、航空航天

**论文结构**：Introduction → Model (Assumptions + Math Formulation) → Numerical Methods → V&V → Results → Discussion

**报告规范**：ASME V&V 10/20/40、IEEE 1597.1；严格区分 Verification（代码正确性/收敛性分析）与 Validation（与现实对比）

**识别信号**：
- 章节含"Model"、"Numerical Method"、"Simulation"、"Verification"、"Validation"等
- 出现"FEM"、"FVM"、"mesh"、"convergence"、"boundary condition"等术语
- 包含数学公式和控制方程
- 有网格/离散化描述
- 不确定性量化（UQ）部分

**特有拆解字段**：
| 字段 | 类型 | 说明 |
|------|------|------|
| model_assumptions | text | 模型假设与简化条件 |
| mathematical_formulation | text | 数学公式化（控制方程/边界条件/初始条件） |
| numerical_method | string | 数值方法（FEM/FVM/FDM/SPH 等） |
| mesh_description | text | 网格/离散化描述 |
| verification | text | 验证（Verification：代码正确性、收敛性分析） |
| validation | text | 确认（Validation：与实验/理论/解析解对比） |
| validation_metrics | list[string] | 确认指标（误差指标/相关性系数） |
| parameter_settings | table | 关键参数设置表 |
| simulation_results | image | 仿真结果可视化 |
| uncertainty_quantification | text | 不确定性量化方法 |

**渲染重点**：公式需 KaTeX 渲染，Verification vs Validation 分区展示，参数表以结构化格式呈现。

---

### 2.8 设计与构建型范式（HCI/软件工程/设计学）

**适用学科**：人机交互、软件工程、设计学、信息系统、交互设计

**论文结构**：DSRM 六步（问题识别 → 目标定义 → 设计开发 → 演示 → 评估 → 沟通）

**报告规范**：DSRM (Peffers et al., 2007) / HCI 期刊惯例；HCI 三模式（interior 内观设计/exterior 外观设计/gestalt 整体设计）

**识别信号**：
- 章节含"Design"、"Implementation"、"Evaluation"、"Artifact"等
- 出现"design science"、"prototype"、"user study"、"artifact"等术语
- 包含系统架构/设计图
- 有用户研究/评估部分
- 设计理论贡献陈述

**特有拆解字段**：
| 字段 | 类型 | 说明 |
|------|------|------|
| problem_identification | text | 问题识别与动机 |
| objectives | list[string] | 解决方案目标定义 |
| design_principles | list[string] | 设计原则/设计理论 |
| artifact_description | text | 构件描述（系统/方法/模型/框架） |
| demonstration | text | 演示场景/应用实例 |
| evaluation_method | string | 评估方法（实验/用户研究/案例/调查） |
| evaluation_results | table | 评估结果 |
| design_theory_contribution | text | 设计理论贡献 |

**渲染重点**：系统架构图优先展示，设计原则以列表+说明形式呈现，评估结果以表格展示。

---

## 3. 补充范式：混合方法研究

**适用学科**：跨学科研究、教育学、护理学、社会科学交叉

**识别信号**：
- 同时出现定量和定性方法
- 章节含"Mixed Methods"、"Integration"、"Convergent"等
- 数据来源既含数字又含文本

**三种设计类型**：
| 类型 | 时序 | 优先级 | 整合时机 |
|------|------|--------|---------|
| convergent parallel | 并行 | 平等 | 分析后合并 |
| explanatory sequential | 定量→定性 | 定量优先 | 定性解释定量结果 |
| exploratory sequential | 定性→定量 | 定性优先 | 定性发现指导定量测量 |

**拆解策略**：混合方法论文分别提取定量部分字段和定性部分字段，在"整合"（integration）部分展示交叉验证结果。

---

## 4. 学科大类→范式映射表

| 学科大类 | 主导范式 | 次生范式 | 主导权重 | 关键词信号 |
|---------|---------|---------|---------|-----------|
| 数理科学 | 计算模拟型 | 实验对比型 | 70% | 数学/物理/方程/数值/仿真 |
| 生命与医学 | 对照实验型 | 综述元分析型 | 65% | 临床/试验/RCT/随机/患者 |
| 工程与信息 | 实验对比型 | 设计与构建型 | 60% | 算法/系统/指标/性能/实现 |
| 经济与管理 | 实证统计型 | 案例研究型 | 55% | 回归/数据/面板/实证/识别 |
| 社会科学 | 实证统计型 | 案例研究型 | 50% | 调查/问卷/访谈/质性/定量 |
| 人文与艺术 | 理论论述型 | 案例研究型 | 75% | 概念/文本/批评/谱系/阐释 |

**用途**：AI 读取论文标题/摘要/关键词后，通过关键词匹配与权重计算，识别论文所属学科大类，进而确定主导范式。

---

## 5. 交叉拆解路由规则

### 5.1 交叉类型判断

| 交叉类型 | 定义 | 判定条件 |
|---------|------|---------|
| 单学科 | 论文涉及单一学科大类 | 主导范式权重 ≥ 阈值（默认 70%） |
| 双学科交叉 | 论文融合两个学科 | 主导范式权重 40-70%，次生范式权重 ≥ 25% |
| 多学科交叉 | 论文涉及三个及以上学科 | 三个及以上范式权重 ≥ 15% |

### 5.2 字段合并策略

| 交叉类型 | 合并策略 | 说明 |
|---------|---------|------|
| 单学科 | 主范式优先 | 仅使用该范式对应的特有字段集 |
| 双学科交叉 | 并集合并 | 通用字段 + 范式 A 特有字段 + 范式 B 特有字段 |
| 多学科交叉 | 序列化拆解 | 按主导范式→次生范式→第三范式顺序拆解，交叉对话部分单独分析 |

### 5.3 字段冲突优先级

当两个范式的字段定义重叠时，按以下优先级处理：

1. 人工修正 > 主范式定义 > 次生范式定义
2. 同名字段：优先采用主范式的类型定义和渲染规则
3. 去重规则：同名同类型字段只保留一份，同名不同类型字段以主范式为准

---

## 6. AI 识别范式信号清单

AI 通过以下信号判断一篇论文的范式归属：

### 6.1 章节结构信号

| 范式 | 信号章节 |
|------|---------|
| 实验对比型 | "Experiments"、"Benchmark"、"Evaluation"、"Results" |
| 对照实验型 | "Methods"（含随机化/盲法子节）、"Trial"、"CONSORT" |
| 实证统计型 | "Data"、"Empirical Strategy"、"Identification"、"Robustness" |
| 理论论述型 | 无标准方法章节，标题为概念/主题式 |
| 案例研究型 | "Case Study"、"Case Selection"、"Cross-Case" |
| 综述元分析型 | "Systematic Review"、"Search Strategy"、"PRISMA" |
| 计算模拟型 | "Model"、"Numerical Methods"、"V&V"、"Simulation" |
| 设计与构建型 | "Design"、"Implementation"、"Artifact"、"Evaluation" |

### 6.2 关键词信号

AI 维护一个范式关键词词典，按命中率计算范式归属概率。具体实现时，从标题/摘要/关键词/章节标题中提取匹配。

### 6.3 置信度判定

- 高置信度（≥ 80%）：章节结构 + 关键词双重匹配，且无冲突信号
- 中置信度（50-79%）：仅关键词匹配，或存在部分冲突信号
- 低置信度（< 50%）：匹配不足，默认走通用拆解字段集

---

## 7. 通用拆解字段（所有范式共享）

所有范式共享以下通用字段，无论论文属于哪种范式：

| 字段 | 类型 | 说明 |
|------|------|------|
| title | string | 标题 |
| authors | list[string] | 作者列表 |
| year | integer | 发表年份 |
| source | string | 期刊/会议/出版社 |
| doi | string | DOI |
| abstract | text | 摘要 |
| keywords | list[string] | 关键词 |
| research_question | text | 研究问题/目标/假设 |
| main_finding | text | 主要发现/结论 |
| knowledge_contribution | enum | 新理论/新方法/新数据/新证据/新设计/新综述/新应用 |

---

## 8. 执行建议

### 8.1 范式识别器实现

1. **输入**：论文标题 + 摘要 + 关键词 + 章节标题列表（从解析结果中提取）
2. **处理**：AI 调用（结构化输出模式）→ 输出 paradigm_id + confidence + cross_type
3. **输出示例**：
   ```json
   {
     "paradigm_id": "paradigm-experimental-baseline",
     "confidence": 0.85,
     "cross_type": "单学科",
     "secondary_paradigms": [],
     "identification_basis": ["章节含Experiments", "关键词含baseline/SOTA"]
   }
   ```

### 8.2 字段集组装器

1. **输入**：范式识别结果 + 交叉类型
2. **处理**：从字段模板库加载通用字段 + 范式特有字段（按合并策略组合）
3. **输出**：完整的拆解字段列表（含字段名/类型/说明/渲染规则）

### 8.3 渲染规则

每个字段在渲染时需指定：
- 渲染类型（text/table/image/list/enum/code）
- 是否支持引用锚定
- 特殊展示要求（如高亮、矩阵、流程图）

---

## 9. 来源说明

本报告各范式的方法论依据来自以下权威来源（通过 tavily 深度搜索获取）：

- 实验对比型：NeurIPS/ACL 会议模板惯例、CS 领域 benchmark 论文惯例
- 对照实验型：CONSORT 2025 声明（BMJ 2025 更新版，30 项清单 + Open Science 新增区块）
- 实证统计型：MIT Economics Empirical Paper Guidelines、NBER 工作论文惯例、Leamer (1983) 敏感性分析
- 理论论述型：哲学/人文期刊论证惯例、主题式论文结构分析
- 案例研究型：Yin, R. K. (2018). *Case Study Research and Applications*（6th ed., SAGE）
- 综述元分析型：Page et al. (2021). PRISMA 2020 statement（BMJ 2021, n71）
- 计算模拟型：ASME V&V 10/20/40 标准、IEEE 1597.1 仿真验证规范
- 设计与构建型：Peffers et al. (2007). DSRM（JAIS 2007）、HCI 设计研究惯例
- 混合方法：Creswell & Plano Clark (2017). *Designing and Conducting Mixed Methods Research*（3rd ed., SAGE）

---

## 10. 中英文论文形式差异与语言识别（补充调研）

生成日期：2026-08-21 ｜ 调研方式：tavily 深度搜索（5 路并行，覆盖 GB/T 7713 国标、中英文话语体系对比、语言识别技术）

> 本章回应项目需求："系统应具备中文/英文论文识别功能，该识别结果决定是否给 LLM 启动翻译流程"，并为拆解字段补充语言维度。

### 10.1 中文期刊论文结构规范（GB/T 7713.2-2022，2023-07-01 实施）

适用于一切反映自然、社会和人文科学体系的学术论文，是中文期刊论文拆解字段的**基准规范**。

**组成部分（前置部分 → 正文 → 附录）**：
| 部分 | 要求 |
|------|------|
| 题名 | 简明，一般不宜超过 25 字；可加副标题 |
| 作者信息 | 置于题名之下；可标注通信作者（也可在文末） |
| 摘要 | 报道性 400 字左右 / 报道指示性 300 字左右 / 指示性 150 字左右；内容含目的、方法、结果、结论；应具有独立性和自明性；**宜有外文（多用英文）摘要**，外文摘要可含更多信息 |
| 关键词 | 3~8 个；宜从《汉语主题词表》或专业词表选取；**宜标注与中文对应的外文关键词** |
| 其他项目 | 基金资助项目应标注基金名称及编号；宜标注收稿日期/修回日期；可标注引用本论文的参考文献格式 |
| 引言 | 研究背景、目的、理由、预期结果及意义价值；突出重点与创新点，客观评介前人研究 |
| 主体 | 由具有逻辑关系的多章构成，理论分析/材料与方法/结果和讨论等宜独立成章 |
| 结论 | 对研究结果和论点的提炼概括，非摘要或小结重复；推导不出结论时可写"结束语" |
| 致谢 | 排在结论或结束语之后，一般不编章编号 |
| 参考文献 | 符合 GB/T 7714；顺序编码制或著者-出版年制（全文统一） |
| 附录 | 一般不设；重要原始数据、推导、程序等可作附录 |

**与 1987 旧版的差异**（拆解时需注意新旧格式并存）：
- 题名 ≤20 字 → ≤25 字
- 摘要 200~300 字 → 按类型 150/300/400 字
- 新增基金项目、收稿日期、参考文献引用格式、增强出版元素等前置项目

### 10.2 中文学位论文结构（GB/T 7713.1-2022 + 高校惯例）

中文学位论文与期刊论文拆解字段应区分（document_type 字段作用在此）：

**标准结构（以艺术类博硕论文为例，北京电影学院等高校惯例）**：
1. 封面（学科/专业领域、方向、题目、作者、导师、完成时间）
2. 声明（独创性声明 + 学位论文使用授权书，须亲笔签名）
3. 标题（题文一致，一般 ≤20 字，可加副标题）
4. 中英文摘要和关键词（中文在前英文在后；硕士 1000~1500 字，博士 1500~2000 字；英文 ABSTRACT 1~2 页）
5. 目录（到三级标题）
6. 正文：**绪论（或前言）→ 本论 → 结语**
   - 绪论含：研究目的和范围、国内外研究现状及前人工作、主题和理论基础、研究方向路线和方法、理论与实践意义
   - 本论：理论分析、图片资料、调查对象、主要论点论据结论
7. 参考文献、附录、致谢

**艺术类学位论文特殊性**：艺术硕士（MFA）论文不少于 1 万字，是对学位作品的创作思考、理论阐释或理论批评——拆解字段应含"作品/创作说明"维度。

### 10.3 中英文学术话语体系差异（形式差异的内在动因）

根据英汉学术话语体系对比研究（International Journal of Education and Humanities）及北大跨文化学术写作访谈：

| 维度 | 英文论文 | 中文论文 |
|------|----------|----------|
| 语言特征 | 逻辑、简洁、客观，强调信息传递清晰与效率 | 全面、完整、文化深度，通过复杂句式与背景补充表现多维性 |
| 引言 | 直白，常含 roadmap（文章结构预告）；thesis 通常在引言出现 | 含蓄，常以"提出问题"方式开篇而非直接给出论点 |
| 论证 | 强调 argument（论点驱动），结构围绕 thesis 组织 | 提出问题→分析因果→比较各家观点→给出解决方案 |
| 文献综述 | Introduction 中简述，或独立 Literature Review 章节 | 期刊论文融入引言"研究现状"；学位论文在绪论中单列"国内外研究现状" |
| 数据使用 | 倾向用大数据/实证数据论证 | 传统上重经验知识与主观思辨，近年逐步引入实证数据 |
| 参考文献 | APA/MLA/Chicago/IEEE/Nature 等 | GB/T 7714（顺序编码制或著者-出版年制） |

### 10.4 语言识别技术选型（决定是否启动翻译流程）

**技术方案**：fastText LID（语言识别模型）

- **模型**：`lid.176.bin`（126MB，更快更准）/ `lid.176.ftz`（917KB 压缩版）；支持 176 种语言；开源（CC-BY-SA 3.0），Python `fasttext` 库或 Node.js `fasttext-lid` 包均可调用
- **工作原理**：对预处理文本（去空白、换行转空格）调用 `model.predict()`，返回语言标签（如 `__label__zh`/`__label__en`）与置信度
- **置信度阈值**：NVIDIA NeMo Curator 等生产管线默认 `min_langid_score=0.3`
- **HuggingFace 备选**：`facebook/fasttext-language-identification`（lid218e，217 语言，NLLB 项目发布）

**本项目的语言识别策略**：
1. **输入**：MinerU 解析后的正文前 N 字符（建议取摘要段 + 正文前 2000 字符，避免标题/摘要噪音）
2. **判定**：`fasttext` 预测 → `paper_language ∈ {中文, 英文, 其他}` + `confidence`
3. **触发翻译**：`paper_language != 中文` → 状态机进入 `translated` 前先标记 `needs_translation=true`（可在 F6 设置中覆盖，如"中文论文也生成英文摘要对照"）
4. **拆解影响**：语言识别结果进入字段组装器，注入 `language_dimension` 字段集（见 fields.yaml）

### 10.5 语言维度拆解字段（fields.yaml 新增）

新增 `language_dimension` 字段分类（14 项），关键字段：
- `paper_language` / `language_confidence` / `needs_translation`（语言识别与翻译触发）
- `document_type`（期刊论文 vs 学位论文 vs 会议论文 vs 预印本 vs 综述）
- `abstract_style`（报道性/报道指示性/指示性/结构化——中英摘要类型差异）
- `reference_standard`（GB/T 7714 vs APA/MLA/Chicago/IEEE/Nature）
- `has_english_abstract` / `has_english_keywords`（中文期刊论文双语文摘特有项）
- `funding_info` / `communication_author` / `received_date`（中文论文前置部分特有项）
- `roadmap_in_intro` / `literature_review_position`（中英引言与文献综述位置差异）