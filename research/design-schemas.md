# 学科交叉拆解决策逻辑设计

生成日期：2026-08-21 ｜ 关联文档：《research/report.md》（范式知识库）、《PRD-文献阅读台.md》（模块 D）

---

## 1. 设计目标

将"学科交叉功能"从概念落地为 AI 可执行的三段式决策管线：

```
输入：已解析的文献（MinerU 输出）
  → ① 语言识别（fastText LID → 是否翻译）
  → ② 范式识别（AI 结构化输出）
  → ③ 字段集组装（路由 + 合并 + 语言维度注入）
  → ④ 逐字段拆解（引用锚定）→ 渲染
```

核心原则：**范式决定字段集，语言决定流程**——不同学科类型的论文用不同的拆解字段；语言识别结果决定是否启动翻译流程及是否注入中英文差异字段。

---

## 2. 语言识别与翻译触发（前置步骤）

### 2.1 语言识别器

**技术选型**：fastText LID（`lid.176.bin` / `lid.176.ftz` 压缩版，Python `fasttext` 库或 Node.js `fasttext-lid` 包）

| 项 | 配置 |
|----|------|
| 输入 | MinerU 解析后的摘要段 + 正文前 2000 字符 |
| 预处理 | 去空白、换行转空格（follow NeMo Curator 流程） |
| 判定 | `model.predict()` → `paper_language ∈ {中文, 英文, 其他}` + `confidence` |
| 置信度阈值 | ≥ 0.3 采纳（NVIDIA NeMo Curator 生产默认）；< 0.3 标记 `[语言待人工确认]` |

### 2.2 翻译触发规则

```
needs_translation = (paper_language != 中文) 默认 true
                  × 用户设置（F6 可覆盖：如"中文论文也生成英文对照"）
```

- 触发时机：语言识别完成 → `parsed` 状态 → 启动 F3 翻译（分段并发 + 断点续传）
- 翻译不改变拆解：拆解始终基于**原文**，翻译稿仅作阅读辅助（术语不二次翻译）
- 中文论文直接跳过翻译，进入范式识别

### 2.3 语言识别输出契约（JSON）

```json
{
  "paper_language": "英文",
  "language_confidence": 0.96,
  "needs_translation": true,
  "document_type": "期刊论文",
  "reference_standard": "APA",
  "abstract_style": "结构化",
  "literature_review_position": "融入引言"
}
```

语言识别结果进入字段组装器，注入 `language_dimension` 字段集（14 项，见 fields.yaml）。

### 2.4 中英文差异的拆解影响

| 维度 | 英文论文处理 | 中文论文处理 |
|------|------------|------------|
| 前置字段 | 不查英文摘要（本身即英文）；查 ORCID/Author Contributions | 拆解 `has_english_abstract` / `has_english_keywords` / `funding_info` / `communication_author` / `received_date` |
| 引言信号 | 识别 roadmap（`roadmap_in_intro=true`）定位 thesis | 识别"提出问题"式开篇，thesis 需全文检索 |
| 文献综述 | 定位独立 Literature Review 或引言末段 | 期刊=引言"研究现状"；学位论文=绪论内嵌 |
| 参考文献 | APA/MLA/Chicago/IEEE/Nature | GB/T 7714（顺序编码制/著者-出版年制） |
| 学位论文 | IMRaD 五章变体（Intro/LR/Design/Results/Discussion） | 绪论→本论→结语 + 中英双语文摘 |

---

## 3. 范式识别器（第二步）

### 3.1 输入

AI 收到的信号源（从解析结果提取，按可信度排序）：

| 优先级 | 信号源 | 提取方式 |
|--------|--------|---------|
| P1 | 章节标题列表 | 解析 markdown 的 `#/##/###` 标题层级 |
| P2 | 摘要 + 关键词 | 元数据字段 |
| P3 | 正文高频术语 | 全文扫描关键词词典命中率 |
| P4 | 特殊对象检测 | 表格/公式/图片/代码块的存在性 |

### 2.2 识别信号词典（范式 → 信号）

| 范式 ID | 章节信号 | 关键词信号 | 特殊对象 |
|---------|---------|-----------|---------|
| paradigm-experimental-baseline | Experiments/Benchmark/Evaluation/Results | baseline, SOTA, state-of-the-art, ablation, dataset, metric, F1, accuracy | 指标对比表、消融表 |
| paradigm-rct | Methods(随机化/盲法)、Trial、CONSORT | RCT, randomized, placebo, blinding, trial registration, CONSORT | CONSORT 流程图 |
| paradigm-empirical-stat | Data, Empirical Strategy, Identification, Robustness | regression, endogeneity, instrument, DID, RDD, panel, econometric | 描述统计表、回归表 |
| paradigm-theoretical | 主题式章节（无标准方法章） | argue, concept, genealogy, critique, objection, discourse | 无表格/公式，长引文多 |
| paradigm-case-study | Case Study, Case Selection, Cross-Case | case, interview, ethnography, fieldwork, unit of analysis | 案例对比矩阵 |
| paradigm-systematic-review | Systematic Review, Meta-Analysis, Search Strategy, PRISMA | PRISMA, inclusion criteria, meta-analysis, forest plot, funnel | PRISMA 流程图、森林图 |
| paradigm-computational-sim | Model, Numerical Methods, Verification, Validation, Simulation | FEM, FVM, mesh, convergence, boundary condition, simulation | 控制方程公式、网格图、仿真图 |
| paradigm-design-science | Design, Implementation, Artifact, Evaluation | design science, prototype, user study, artifact, DSRM | 系统架构图、评估表 |

### 2.3 置信度计算

```
范式匹配得分 = 0.4 × 章节信号命中率 + 0.4 × 关键词命中率 + 0.2 × 特殊对象命中率
```

- 最高范式得分 ≥ 0.70 → **高置信**：采纳该范式为主范式
- 最高范式得分 0.45-0.70 → **中置信**：采纳为主范式，同时列出次候选
- 最高范式得分 < 0.45 → **低置信**：默认走通用字段集，提示人工选择

### 3.4 AI 结构化输出契约（JSON）

```json
{
  "paradigm_id": "paradigm-experimental-baseline",
  "paradigm_name": "实验对比型范式",
  "confidence": 0.85,
  "identification_basis": ["章节含Experiments/Results", "关键词命中baseline/SOTA/ablation"],
  "cross_type": "双学科交叉",
  "secondary_paradigms": [
    {"paradigm_id": "paradigm-design-science", "weight": 0.3, "basis": "章节含Design/Implementation"}
  ],
  "human_review_required": false
}
```

---

## 3. 字段集组装器（第二步）

### 3.1 路由规则

| 交叉类型 | 判定条件 | 字段集组合策略 |
|---------|---------|---------------|
| 单学科 | 主范式置信 ≥ 0.70 且次范式权重 < 0.25 | 通用字段 + 主范式特有字段 |
| 双学科交叉 | 主范式 0.45-0.70 且次范式权重 ≥ 0.25 | 通用字段 + 主范式特有字段 + 次范式特有字段 |
| 多学科交叉 | 两个以上次范式权重 ≥ 0.15 | 通用字段 + 各范式特有字段 + 交叉对话字段 |

### 3.2 字段集构成

```
字段集 = common_fields（固定 10 项）
       ∪ paradigm_specific_fields（按路由规则合并）
       ∪ cross_dialogue_fields（仅多学科交叉时追加）
```

cross_dialogue_fields（多学科交叉专用，当前未在 fields.yaml 中的补充设计）：

| 字段 | 类型 | 说明 |
|------|------|------|
| cross_points | list[string] | 学科间真正的交叉点/结合处 |
| integration_contribution | text | 交叉整合产生的增量贡献 |
| tension_points | list[string] | 学科间张力/矛盾处 |
| borrowed_methods | list[string] | 从其他学科借用的方法/概念 |

### 3.3 冲突解决优先级

当合并字段集出现同名/语义重叠时：

1. **人工修正 > 主范式定义 > 次范式定义**（权重自上而下）
2. 同名字段：优先采用主范式的类型与渲染规则
3. 语义相近字段（如实验对比的 `evaluation_metrics` 与计算模拟的 `validation_metrics`）：主范式保留原名，次范式字段保留但标注 `[次范式视角]` 前缀

---

## 5. 逐字段拆解 Prompt（第四步）

### 5.1 拆解执行 Prompt 骨架

```
你是一名 {范式名} 领域的论文拆解专家。请对以下文献按给定字段逐一拆解。
【文献内容】{已解析+已翻译的全文}
【字段集】{组装好的字段列表，含字段名/类型/说明}
【规则】
1. 每个字段的输出必须引用原文段落（给出段落编号/原文摘录）
2. 无法定位原文支撑的字段，输出"引用缺失"而非编造
3. 表格类字段（如 results_table）输出 Markdown 表格
4. 图片类字段（如 consort_flow）输出图片描述+所在位置引用
5. 保留原文术语，不做二次翻译
【输出格式】严格 JSON 或流式 Markdown（按字段分节）
```

### 4.2 范式专属拆解指令（差异化体现）

每种范式在 Prompt 中追加专属指令，确保拆解深度符合该领域评审标准：

| 范式 | 追加指令 |
|------|---------|
| 实验对比型 | "重点核对：基线是否公平（同设置/同资源）；指标是否齐备；消融是否解释了每个组件的贡献；SOTA 对比是否诚实" |
| 对照实验型 | "重点核对：随机化与盲法是否完备；样本量估算是否合理；主要结局是否注册一致；harm 报告是否完整" |
| 实证统计型 | "重点核对：识别策略是否可信；内生性处理是否充分；稳健性检验覆盖哪些维度；结果是否支持因果解释" |
| 理论论述型 | "重点核对：概念界定是否清晰；论证前提与推理是否自洽；反驳是否回应到位；思想谱系定位是否准确" |
| 案例研究型 | "重点核对：案例选择是否有依据；证据是否多源三角验证；分析技术是否与问题匹配；命题是否可推广" |
| 综述元分析型 | "重点核对：检索策略是否可复现；纳排标准是否明确；质量评估工具是否恰当；异质性与偏倚是否报告" |
| 计算模拟型 | "重点核对：Verification 与 Validation 是否严格区分；收敛性是否验证；不确定性是否量化；模型假设是否声明" |
| 设计与构建型 | "重点核对：问题-目标-设计是否对齐；评估方法是否匹配构件类型；设计理论贡献是否明确" |

---

## 5. 渲染规则

### 5.1 字段渲染类型映射

| 字段类型 | 渲染方式 | 示例 |
|---------|---------|------|
| text | 段落 | 理论框架、机制分析 |
| list[string] | 无序列表 | 基线方法、稳健性检验 |
| table | Markdown 表格 | 指标对比表、回归表、案例矩阵 |
| enum | 徽章/标签 | 知识贡献类型、合成方法 |
| image | 图片 + 图注（引用原图位置） | CONSORT 流程图、仿真结果 |
| code | 代码块（可选） | 实验超参数配置 |

### 6.2 范式化排版差异

| 范式 | 排版特征 |
|------|---------|
| 实验对比型 | 指标表首列加粗最优行；表格带数据集列 |
| 对照实验型 | 流程图区块 + 效应量（OR/RR/HR）+ 95% CI |
| 实证统计型 | 回归表带标准误括号、显著性星号、观测数/R² 脚注 |
| 理论论述型 | 概念界定引用块；论证结构可折叠导图 |
| 案例研究型 | 跨案例矩阵表（行=案例，列=维度） |
| 综述元分析型 | PRISMA 流程 + 森林图 + I² 警示色 |
| 计算模拟型 | 公式 KaTeX 渲染；V&V 分区卡片 |
| 设计与构建型 | 架构图优先 + 设计原则编号列表 |

---

## 6. 人工修正兜底

- 范式识别结果在 UI 展示"主范式 + 置信度 + 识别依据"，允许人工改选
- 字段方案预览允许人工增删字段（持久化为用户自定义模板）
- 置信度 < 0.45 时默认通用字段集并强提示人工介入
- 所有人工修正结果回写训练反馈（本地统计：修正率 = 人工修正数 / 总拆解数），用于迭代识别信号权重

---

## 7. 实现落地清单

| # | 组件 | 实现要点 | 对应开发计划 |
|---|------|---------|-------------|
| 1 | 范式识别器 | 信号词典 + 置信度计算 + JSON 输出契约 | M3-3.1 |
| 2 | 字段模板库 | 8 范式特有字段 + 通用字段 + 交叉对话字段（YAML 存储） | M3-3.2 |
| 3 | 路由引擎 | cross_type 判定 + 并集合并 + 冲突解决 | M3-3.2 |
| 4 | 拆解执行器 | 范式专属 Prompt + 引用锚定 | M3-3.3/3.4 |
| 5 | 渲染器 | 字段类型映射 + 范式化排版差异 | M3-3.3 |

---

## 9. 待确认项

| # | 项 | 状态 |
|---|----|------|
| 1 | 置信度阈值（0.70/0.45/0.25）需样本集实测校准 | [To be confirmed] |
| 2 | 关键词词典规模与权重（当前为初版） | [To be confirmed] |
| 3 | 8 范式是否覆盖全部目标学科（艺术学/数媒交叉需单独验证） | [To be confirmed] |
| 4 | cross_dialogue_fields 是否并入初版 | [To be confirmed] |
| 5 | 语言识别置信度阈值（0.3 为 fastText 默认，需按中英学术语料校准） | [To be confirmed] |
| 6 | 中文学位论文（GB/T 7713.1）与期刊论文（GB/T 7713.2）是否分别建模拆解字段 | [To be confirmed] |
| 7 | 艺术类学位论文（MFA）是否追加"作品/创作说明"维度字段 | [To be confirmed] |
