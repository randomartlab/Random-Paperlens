# Nowledge 待推送归档（全量、不加工）

> 用途：Nowledge MCP 服务当前不可用（`list tools failed`，持续于 2026-08-24 会话）。
> 此文件按"全量、不自行拆分加工"原则归档本会话应推送的记忆与资料，服务恢复后据此执行推送。
> 生成时间：2026-08-24 ｜ 归属项目：文献阅读台（Tauri 2 + React 19）

## 一、应推送的项目资料（文件，作为 source/原文）

| 资料 | 路径 | 说明 |
|------|------|------|
| PRD | `PRD-文献阅读台.md` | 含 F7 阅读状态 / F8 笔记与摘录 / F9 免费初始配置 / F10 帮助系统 / F11 Web 迁移 / F12 Windows 迁移，里程碑 M1-M9 |
| 开发计划 | `开发计划-文献阅读台.md` | M1-M5 任务与验收；M5 已全部完成 |
| 范式研究报告 | `research/report.md` | 范式知识库（8 种研究范式 + 中英文形式差异） |
| 设计 Schema | `research/design-schemas.md` | 字段模板库设计 |

## 二、应推送的记忆条目（完整原文）

### 条目 1：免费大模型/解析 API 现状调研（2026-08，用于 F9 免费初始配置）

## 免费模型/API 现状（2026-08 检索核验，政策以官方为准）

### 翻译/拆解（OpenAI 兼容，国内直连）
1. 智谱 GLM-4-Flash：永久免费、128K 上下文、30 并发。Base URL https://open.bigmodel.cn/api/paas/v4，模型 glm-4-flash。官方文档 https://docs.bigmodel.cn/cn/guide/models/free/glm-4-flash-250414
2. 智谱 GLM-4.7-Flash：永久免费、200K 上下文，免费版并发=1。模型 glm-4.7-flash。
3. 硅基流动 SiliconFlow：9B 以下模型永久免费、新用户送 2000 万 Token。Base URL https://api.siliconflow.cn/v1。
4. 百度千帆：ERNIE-Speed-8K 永久免费。Base URL https://qianfan.baidubce.com/v2。
5. 海外（需代理）：Google Gemini 免费层、Groq 免费层。

### 视觉模型
- 智谱 GLM-4.6V-Flash：免费视觉模型，128K。

### 解析（MinerU）
- MinerU 官方精准解析 API：注册 mineru.net 后每日 2000 页免费高优先级额度。官网 https://mineru.net/apiManage/limit。另有免 Token 的 Agent 轻量解析 API（≤10MB、≤20 页、仅 Markdown）。

### 结论（用于 F9 预置模板）
- 翻译/拆解默认 GLM-4-Flash（30 并发利于并发翻译）；拆解可换 GLM-4.7-Flash（200K）。
- 解析用 MinerU 官方 API（每日 2000 页免费）；视觉用 GLM-4.6V-Flash。
- 关键约束：免费方案均需用户免费注册自取 Key/Token，应用只预置配置骨架。

[source_thread_id: 文献阅读台项目-M5-20260824] [retrieval_tool: webserch]
[query_string: 永久免费大模型API 2026 / MinerU 免费额度] [timestamp: 2026-08-24]

### 条目 2：Web/Windows 端迁移调研结论（2026-08，F11/F12）

## Web 端迁移调研结论（2026-08，文献阅读台 F11）

### 纯静态托管（GitHub Pages 等）保持全部功能 → 不可行
1. CORS：MinerU 官方 API（mineru.net）与 OpenAI 兼容 LLM API 未对任意浏览器来源开放跨域，纯前端直连被拦截。
2. 沙箱文件系统：浏览器无法访问本地任意目录；OPFS 为站点私有沙箱、用户文件管理器不可见；File System Access API 磁盘访问仅 Chromium 桌面支持，Firefox/Safari 不实现（MDN OPFS 文档，Baseline 2023-03）。
3. 长任务：翻译/拆解为分钟级任务，标签页关闭即中断；Service Worker 仅部分缓解。
4. API Key：无法用系统钥匙串。

### 落地方案
- 方案 A（推荐）：serverless 代理（Cloudflare Pages Functions / Vercel Functions）+ 前端适配；前端复用 80-90%；F8 笔记失去文件系统可见性，降级浏览器存储+导出。
- 方案 B：Rust 编译 wasm32（成本高，文件系统/API 直连问题依旧）。
- 方案 C：只读演示版。

### Windows 端迁移（F12）
- Tauri 2 原生支持 Windows（WebView2，Win10/11 预装）；NSIS/MSI 打包；适配路径分隔符、长路径、中文字体、notify 差异；回归 M1-M7 全功能。

[source_thread_id: 文献阅读台项目-M5-20260824] [retrieval_tool: webserch]
[query_string: Tauri 迁移 Web 静态托管可行性 / OPFS File System Access] [timestamp: 2026-08-24]

### 条目 3：M5 完成记录

## M5 完成记录（2026-08-24）

- 5.1 本地统计（默认关闭，事件计数+耗时，可完全关闭）：stats.rs + lib.rs 埋点（导入/解析/翻译/拆解/导出/错误）+ 设置页统计区块 + get_stats/set_stats_enabled/reset_stats。
- 5.2 错误码体系 + 日志 + 崩溃恢复：error_code.rs（E-xxxx 分组：导入 1000/解析 2000/翻译 3000/拆解 4000/系统 9000）；logging.rs（纯 std 文件日志 litdesk.log，5MB 轮转）；session.lock 崩溃检测 + get_diagnostics + 设置页诊断区（日志路径/上次崩溃/打开目录）。
- 5.3 样本集回归：examples/regression.rs + scripts/fetch_regression_samples.sh；实测 20 篇多学科英文 PDF（arXiv 经典 + 物理/生物/经济/数学最新），首次 18/20=90%，MinerU 服务端临时失败（parsing failed）自动重试后 20/20=100%，平均单篇 27.8s。
- 5.4 打包：tauri build 成功产出 .dmg（文献阅读台_0.1.0_aarch64.dmg，5.9MB）；未签名/公证（需 Apple Developer 证书）；tauri.conf.json identifier `com.litdesk.app` 以 .app 结尾，建议发布前改为如 com.litdesk.reader。

[source_thread_id: 文献阅读台项目-M5-20260824] [timestamp: 2026-08-24]

### 条目 4：M6 完成记录

## M6 完成记录（文献阅读台，2026-08-24）

### F7 阅读状态标记
- DB：documents 表新增 read_status 列（v4 迁移，默认 unread），db 测试更新到 version 4。
- 后端：set_read_status 命令（校验 unread/read），list_documents/导入返回 read_status。
- 前端：文献列表行左右扳机开关（未读/已读两段式），未读完徽章，全部/未读完/已读完筛选。

### F9 免费初始配置
- 设置页免费方案卡片：智谱 GLM-4-Flash（翻译/拆解，永久免费）、GLM-4.6V-Flash（视觉）、MinerU（每日 2000 页）。一键填充 Base URL/模型名，Key 留空待用户注册。
- 首次启动引导：空库且无 API 配置时提示前往免费方案。

### F10 帮助系统
- Key 输入框下方超链接（智谱 Key 获取/官方文档），免费方案卡片各链接用系统浏览器打开（tauri-plugin-opener openUrl）。

[source_thread_id: 文献阅读台项目-M6-20260824] [timestamp: 2026-08-24]

### 条目 5：M7 完成记录

## M7 完成记录：笔记与摘录（F8，文献阅读台，2026-08-24）

- 后端 notes.rs：笔记以纯 .md 文件存于 app_data_dir/notes/（用户可在文件系统直接管理）。命令：list_notes / read_note / save_note / delete_note / export_note（md/html）/ print_note（PDF 走系统打印窗口）。提取 spawn_print_html 供文献打印与笔记打印共用；命令跨模块注册（pub + #[macro_use] + use 引入，run() 加 allow(dependency_on_unit_never_type_fallback) 抑制宏展开 lint）。
- 前端 NotesView.tsx：左列表（新建/删除/打开目录 revealItemInDir）+ 右侧 Notion 式编辑区（textarea Markdown 编辑 / react-markdown 预览切换、600ms 防抖自动保存、导出 md/html/PDF）。
- App.tsx：主导航新增「笔记」tab。
- 摘录联动：ReaderView 原文/译文/双语原文列选中文本 → 右键「复制为摘录（含来源）」，剪贴板生成 `> 摘录\n> ——《文献标题》`，供粘贴进笔记。
- 编译验证：cargo check + tsc/vite build 通过。

[source_thread_id: 文献阅读台项目-M7-20260824] [timestamp: 2026-08-24]

### 条目 6：Windows 端迁移（F12）完成记录

## Windows 端迁移（F12，文献阅读台，2026-08-24）

- tauri.conf.json：identifier 改为 com.litdesk.reader（消除 .app 结尾警告，规范双平台标识）；bundle targets 明确 ["app","dmg","nsis","msi"]；新增 bundle.windows.nsis 配置（installMode currentUser 免 UAC + SimpChinese/English 语言）。
- index.css：字体栈加 Windows 中文字体 fallback（Microsoft YaHei、Segoe UI）。
- 排查结论：Rust 侧无任何平台特定代码（grep cfg(target_os) 无匹配）；依赖全跨平台（rusqlite bundled、reqwest native-tls、whatlang 等）；图标 icon.ico 已存在；打印 window.print() 在 WebView2 支持。
- 交叉编译验证（macOS 上 cargo check --target x86_64-pc-windows-msvc）：纯 Rust 层（tauri/windows-sys/wry/webview2-com 等）全部编译通过；仅 SQLite C 库编译失败（缺 Windows SDK 头文件，纯环境问题）。真实构建/打包由 CI 承担。
- 新增 .github/workflows/windows-build.yml：windows-latest 上 npm ci + tauri build --bundles nsis,msi，上传 .exe/.msi artifact；触发：手动 workflow_dispatch 或推送 v* tag。
- 本机回归：cargo check + tsc/vite build 通过。

[source_thread_id: 文献阅读台项目-M9-20260824] [timestamp: 2026-08-24]
