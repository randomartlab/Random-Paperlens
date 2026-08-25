# Rd学术阅读器

Rd学术阅读器（Rd Academic Reader）是一款本地优先的学术文献加工流水线桌面应用。导入 PDF 后自动完成版面解析、语言识别、翻译与学科范式拆解，帮助研究者快速吃透论文。文献库、笔记与全部密钥均保存在本机，不上传任何内容。

- 基于 Tauri 2 + React 19 + TypeScript
- 支持 macOS（Apple Silicon / Intel）与 Windows
- 内置免费方案：GLM Flash 系列 + MinerU 每日免费额度
- 内置五套主题（默认 + 四套 Obsidian 社区灵感主题，均支持深浅色）

## 核心功能

| 模块 | 说明 |
| --- | --- |
| 文档库 | PDF 导入（文件选择 / 拖拽 / 批量）、元数据编辑、搜索、筛选与阅读状态标记 |
| 版面解析 | 调用 MinerU 将 PDF 解析为结构化 Markdown，保留版面、公式、表格与图片 |
| AI 翻译 | 自动语言识别，中英互译，原文 / 译文 / 双语对照视图，支持术语表 |
| 范式拆解 | 识别学科与交叉类型，输出结构化研究范式字段；结论引用原文段落编号，禁止编造 |
| 笔记与摘录 | 本地 Markdown 笔记，编辑实时自动保存，可导出 |
| 导出 | 翻译与拆解结果导出为 Markdown、HTML 或 PDF |
| 主题 | 顶栏切换浅色 / 深色；设置 → 主题外观可选 Default、Minimal、Dracula、Blue Topaz、Catppuccin |

## 免费方案

零成本开始使用完整流水线，适合个人文献阅读：

1. 注册智谱开放平台（open.bigmodel.cn），创建 API Key。
2. 打开「设置 → 免费方案」，点击 GLM-4-Flash 卡片「填入模板」，粘贴 API Key 保存。翻译与拆解全部使用该模型，永久免费、30 并发。
3. 可选：将同一 Key 填入 GLM-4.6V-Flash，作为免费视觉模型，用于图注与图片内容识别。
4. 注册 mineru.net，在控制台复制 Token，粘贴到「设置 → 解析（MinerU）配置」。每日 2000 页免费额度。
5. 导入第一篇 PDF，等待解析完成后即可阅读、翻译与拆解。

免费方案的政策与额度以各平台官方页面为准，应用内帮助文档已提供对应注册入口。

## API 配置

- 默认 API：OpenAI 兼容格式，配置 Base URL / API Key / 模型名，可不限使用免费 GLM 或任意兼容服务。
- 视觉模型：独立配置，用于图片识别；不配置则跳过。
- MinerU Token：在 mineru.net 控制台获取，用于版面解析。
- 所有密钥仅保存在本机应用数据库中，不上传、不用于其他用途。

## 隐私与本地优先

文献文件、解析结果、笔记和密钥都保存在本地。应用仅在你主动触发时调用你配置的解析 / 翻译 / 视觉服务，不会收集使用数据。

## 开发与构建

环境要求：Node.js 20+、Rust stable、Tauri 2 CLI。

```bash
npm install
npm run tauri dev        # 本地开发
npm run build            # 前端 TypeScript 检查 + Vite 构建
npm run tauri build      # 桌面应用打包
```

macOS 双架构 DMG：

```bash
npm run tauri build -- --bundles dmg --target aarch64-apple-darwin
npm run tauri build -- --bundles dmg --target x86_64-apple-darwin
```

Windows NSIS 安装包由 GitHub Actions 在 `v*` tag 时自动构建。

## 仓库结构

```text
src/                React 前端（文献库、阅读、笔记、设置、帮助）
src-tauri/          Tauri 2 Rust 后端（MinerU、翻译、拆解、导出、数据库）
.github/workflows/  Windows CI 构建
PRD-文献阅读台.md   产品需求文档
开发计划-文献阅读台.md 开发计划
```