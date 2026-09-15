import { memo, useEffect, useMemo, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import rehypeKatex from "rehype-katex";
import rehypeHighlight from "rehype-highlight";
import rehypeRaw from "rehype-raw";
import "katex/dist/katex.min.css";
import "highlight.js/styles/github.css";

interface Props {
  docId: string;
  title: string;
  onBack: () => void;
  initialMode?: Mode;
  onOpenNotes?: () => void;
}

type Mode = "original" | "translated" | "bilingual" | "digest";

interface DigestField {
  name: string;
  label: string;
  ftype: string;
  source: string;
  zh: string;
  en: string;
  table: string | null;
  failed: boolean;
}

interface DigestRecord {
  version: number;
  fields: DigestField[];
}

/** 从字符串中尽力提取 JSON 承载的文本（对象取 zh/text/…，数组逐项拼接） */
function extractJsonText(raw: string): string {
  const t = raw.trim();
  if (!t.startsWith("{") && !t.startsWith("[")) return raw;
  let v: unknown;
  try {
    v = JSON.parse(t);
  } catch {
    return raw;
  }
  const pick = (x: unknown): string | null => {
    if (typeof x === "string") {
      const s = x.trim();
      return s || null;
    }
    if (Array.isArray(x)) {
      const lines = x
        .map((it) => {
          if (typeof it === "string") return it.trim();
          if (it && typeof it === "object") {
            const o = it as Record<string, unknown>;
            const k = ["term", "text", "item", "label", "definition", "zh"].find(
              (kk) => typeof o[kk] === "string",
            );
            return k ? String(o[k]).trim() : JSON.stringify(it);
          }
          return "";
        })
        .filter((s) => s.length > 0);
      return lines.length ? lines.join("\n") : null;
    }
    return null;
  };
  if (v && typeof v === "object" && !Array.isArray(v)) {
    const o = v as Record<string, unknown>;
    for (const k of ["zh", "en", "text", "content", "term", "definition", "list"]) {
      const p = pick(o[k]);
      if (p) return p;
    }
  }
  const arr = pick(v);
  return arr || raw;
}

/** 对历史脏数据（LLM 曾返回 JSON 原文被整体存库）做兜底清洗 */
function cleanDigestRecord(r: DigestRecord | null): DigestRecord | null {
  if (!r) return r;
  return {
    ...r,
    fields: r.fields.map((f) => ({ ...f, zh: extractJsonText(f.zh), en: extractJsonText(f.en) })),
  };
}

/** 将 Markdown 表格转为 HTML（拆解视图的 table 字段用） */
function renderMdTable(content: string): string {
  const rows = content
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.startsWith("|") && !/^\|[\s:|-]+\|$/.test(l));
  if (rows.length < 2) return content;
  const make = (row: string, tag: "th" | "td") =>
    `<tr>${row
      .replace(/^\|/, "")
      .replace(/\|$/, "")
      .split("|")
      .map(
        (c) =>
          `<${tag} class="border border-black/10 px-2 py-1 align-top">${c.trim()}</${tag}>`,
      )
      .join("")}</tr>`;
  const thead = make(rows[0], "th");
  const tbody = rows
    .slice(1)
    .map((r) => make(r, "td"))
    .join("");
  return `<table class="w-full overflow-hidden rounded-lg border border-black/10 text-xs"><thead class="bg-black/5">${thead}</thead><tbody>${tbody}</tbody></table>`;
}

interface BilingualSegment {
  index: number;
  kind: string;
  original: string;
  translated: string;
}

interface TocItem {
  id: string;
  level: number;
  text: string;
}

/** 扫描 Markdown 标题（跳过围栏代码块），生成目录项 */
function buildToc(md: string): TocItem[] {
  const items: TocItem[] = [];
  let inCode = false;
  let n = 0;
  for (const line of md.split("\n")) {
    const t = line.trim();
    if (t.startsWith("```")) {
      inCode = !inCode;
      continue;
    }
    if (inCode) continue;
    const m = t.match(/^(#{1,4})\s+(.*?)\s*$/);
    if (m) {
      items.push({ id: `toc-${n}`, level: m[1].length, text: m[2] });
      n++;
    }
  }
  return items;
}

/** 在每个标题行末尾插入 <a id="toc-N"> 锚点（编号与 buildToc 一致，供目录点击跳转）。
 *  注意：锚点必须放在标题行末尾而非行首，否则该行不再以 # 开头，ATX 标题会退化为普通文字。 */
function injectHeadingAnchors(md: string): string {
  const out: string[] = [];
  let inCode = false;
  let n = 0;
  for (const line of md.split("\n")) {
    const t = line.trim();
    if (t.startsWith("```")) {
      inCode = !inCode;
      out.push(line);
      continue;
    }
    if (inCode) {
      out.push(line);
      continue;
    }
    if (/^(#{1,4})\s+/.test(t)) {
      out.push(`${line} <a id="toc-${n}"></a>`);
      n++;
    } else {
      out.push(line);
    }
  }
  return out.join("\n");
}

/**
 * 引用锚定预处理：
 * 1) 为文末参考文献条目（References 章节后的 [n] 行）添加 <a id="ref-n"> 锚点
 * 2) 将文中的 [n] / [n,m] / [n-m] 引用标记转为指向锚点的超链接
 */
function addCitationLinks(md: string): string {
  const lines = md.split("\n");
  let inRefs = false;
  const withAnchors = lines.map((line) => {
    const t = line.trim();
    // 参考文献章节标题（英/中）
    if (
      inRefs === false &&
      t.length < 30 &&
      /^(References|REFERENCES|Reference|Bibliography|参考文献|引用文献)\s*$/.test(t)
    ) {
      inRefs = true;
      return line;
    }
    if (inRefs) {
      const m = t.match(/^\[(\d+)\]([.:、．\s]|$)/);
      if (m) {
        return `<a id="ref-${m[1]}"></a>${line}`;
      }
    }
    return line;
  });

  // 文中引用 → 锚点链接（限 ≤8 个编号，避免误伤长编号列表）
  const linked = withAnchors.join("\n").replace(
    /\[(\d+(?:[\s,，\-–—]\s*\d+){0,7})\]/g,
    (match, body: string) => {
      const nums = body
        .split(/[\s,，\-–—]+/)
        .map((s: string) => parseInt(s, 10))
        .filter((n: number) => Number.isFinite(n));
      if (nums.length === 0) return match;
      return nums.map((n) => `[${n}](#ref-${n})`).join(", ");
    },
  );
  return linked;
}

/** 统一 Markdown 渲染体：图文 + 公式 + 代码高亮 + 引用锚点；showImages=false 时图片降级为占位文字（双语视图避免图片重复） */
function MarkdownBody({
  content,
  baseDir,
  showImages = true,
  imgNotes,
}: {
  content: string;
  baseDir: string;
  showImages?: boolean;
  imgNotes?: Record<string, string>;
}) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm, remarkMath]}
      rehypePlugins={[rehypeRaw, rehypeKatex, rehypeHighlight]}
      components={{
        // 各级标题大纲化：明显区分 h1-h4 层级（font-size 用 important 以覆盖 prose 默认）
        h1: ({ children }) => (
          <h1 className="mt-8 mb-4 border-b-2 border-primary/70 pb-2 text-[24px]! font-bold! tracking-tight">
            {children}
          </h1>
        ),
        h2: ({ children }) => (
          <h2 className="mt-6 mb-3 border-l-[3px] border-primary pl-3 text-[19px]! font-semibold!">
            {children}
          </h2>
        ),
        h3: ({ children }) => (
          <h3 className="mt-5 mb-2 border-l-2 border-primary/40 pl-3 text-[16px]! font-semibold!">
            {children}
          </h3>
        ),
        h4: ({ children }) => (
          <h4 className="mt-4 mb-2 text-[14px]! font-medium! text-primary/70">
            {children}
          </h4>
        ),
        img: ({ src, alt }) => {
          if (!showImages) {
            return (
              <span className="text-xs text-primary/35">{alt ?? "图片"}</span>
            );
          }
          // 相对路径（MinerU 产物：images/xxx.jpg）→ 本地绝对路径 → asset 协议 URL
          let resolved = src;
          if (src && !/^https?:\/\//.test(src)) {
            // Windows 下 baseDir 带反斜杠（C:\...\parsed），直接拼接会形成
            // 混合分隔符路径，导致 asset 协议（http://asset.localhost/...）解析失败、图片不显示
            const dir = baseDir.replace(/\\/g, "/").replace(/\/+$/, "");
            resolved = convertFileSrc(`${dir}/${src.replace(/^\.\//, "")}`);
          }
          // 视觉识别结果（如存在）：图片下方展示「类型 + 要点」
          const rel = src ? src.replace(/^\.\//, "") : "";
          const note = imgNotes?.[rel];
          return (
            <figure className="my-4">
              <img
                src={resolved}
                alt={alt ?? ""}
                className="max-w-full rounded-lg border border-divider"
              />
              {note && (
                <figcaption className="mt-1.5 rounded-md bg-primary/5 px-2.5 py-1.5 text-xs leading-relaxed text-primary/65">
                  图片识别：{note}
                </figcaption>
              )}
            </figure>
          );
        },
      }}
    >
      {addCitationLinks(content)}
    </ReactMarkdown>
  );
}

/** 行内 Markdown 渲染（句子级使用）：p 降级为 span 保持句间流式排版 */
function InlineMarkdown({ content }: { content: string }) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm, remarkMath]}
      rehypePlugins={[rehypeRaw, rehypeKatex, rehypeHighlight]}
      components={{
        p: ({ children }) => <span>{children}</span>,
        img: ({ alt }) => (
          <span className="text-xs text-primary/35">{alt ?? "图片"}</span>
        ),
      }}
    >
      {content}
    </ReactMarkdown>
  );
}

/** 中文：按句末标点切分 */
function splitOnPunct(text: string, puncts: string[]): string[] {
  const out: string[] = [];
  let buf = "";
  for (const ch of text) {
    buf += ch;
    if (puncts.includes(ch)) {
      if (buf.trim()) out.push(buf.trim());
      buf = "";
    }
  }
  if (buf.trim()) out.push(buf.trim());
  return out;
}

/**
 * 将段落文本切分为句子：
 * - 数学块（$...$ / $$...$$）视为不可拆分的整体（避免公式内句号误切）
 * - 中文按 。！？； 切分；英文按「句末标点 + 空格 + 大写字母」切分
 *   （避开 Fig. 1、et al.、小数等误切场景）
 */
function splitSentences(text: string): string[] {
  const sentences: string[] = [];
  const mathRe = /(\$\$[\s\S]*?\$\$|\$[^$\n]+\$)/g;
  const chunks: { math: boolean; text: string }[] = [];
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = mathRe.exec(text)) !== null) {
    if (m.index > last) chunks.push({ math: false, text: text.slice(last, m.index) });
    chunks.push({ math: true, text: m[0] });
    last = m.index + m[0].length;
  }
  if (last < text.length) chunks.push({ math: false, text: text.slice(last) });
  if (chunks.length === 0) chunks.push({ math: false, text });

  for (const c of chunks) {
    if (c.math) {
      sentences.push(c.text);
      continue;
    }
    const t = c.text.trim();
    if (!t) continue;
    if (/[\u4e00-\u9fff]/.test(t)) {
      for (const s of splitOnPunct(t, ["。", "！", "？", "；"])) sentences.push(s);
    } else {
      // 捕获分隔符重组的英文切句：tokens 为 [句, ".", 句, ".", ...]
      const tokens = t.split(/([.!?])\s+(?=[A-Z])/);
      for (let i = 0; i < tokens.length; i += 2) {
        const cur = tokens[i] + (i + 1 < tokens.length ? tokens[i + 1] : "");
        if (cur.trim()) sentences.push(cur.trim());
      }
    }
  }
  return sentences;
}

/** 原文句 ↔ 译文句对齐：数量相同按索引；否则超出部分就近归并到末句 */
function buildAlign(origLen: number, transLen: number) {
  if (origLen === transLen) {
    return { o2t: (i: number) => i, t2o: (j: number) => j };
  }
  const min = Math.min(origLen, transLen);
  return {
    o2t: (i: number) => (i < min ? i : transLen - 1),
    t2o: (j: number) => (j < min ? j : origLen - 1),
  };
}

/** Markdown 渲染记忆化：内容未变时跳过重解析（KaTeX/高亮开销大） */
const MarkdownBodyMemo = memo(MarkdownBody);

/** 段落级双语对：逐句切分渲染 + 鼠标悬停联动高亮对应句 */
function SentencePair({
  seg,
  baseDir,
  imgNotes,
}: {
  seg: BilingualSegment;
  baseDir: string;
  imgNotes?: Record<string, string>;
}) {
  const [hover, setHover] = useState<{ orig: number; trans: number } | null>(null);
  const origSents = useMemo(() => splitSentences(seg.original), [seg.original]);
  const transSents = useMemo(() => splitSentences(seg.translated), [seg.translated]);
  const canSync = origSents.length > 0 && transSents.length > 0;
  const align = useMemo(
    () => (canSync ? buildAlign(origSents.length, transSents.length) : null),
    [canSync, origSents.length, transSents.length],
  );

  return (
    <div className="grid grid-cols-2 gap-6 border-b border-divider py-6 first:pt-0">
      <div className="min-w-0 leading-relaxed">
        {canSync && align ? (
          origSents.map((s, i) => (
            <span
              key={i}
              onMouseEnter={() => setHover({ orig: i, trans: align.o2t(i) })}
              onMouseLeave={() => setHover(null)}
              className={`cursor-pointer rounded px-0.5 transition-colors ${
                hover?.orig === i ? "bg-mark" : ""
              }`}
            >
              <InlineMarkdown content={s} />
              {i < origSents.length - 1 ? " " : ""}
            </span>
          ))
        ) : (
          <MarkdownBody content={seg.original} baseDir={baseDir} imgNotes={imgNotes} />
        )}
      </div>
      <div className="min-w-0 border-l border-divider-strong pl-6 leading-relaxed">
        {canSync && align ? (
          transSents.map((s, j) => (
            <span
              key={j}
              onMouseEnter={() => setHover({ orig: align.t2o(j), trans: j })}
              onMouseLeave={() => setHover(null)}
              className={`cursor-pointer rounded px-0.5 transition-colors ${
                hover?.trans === j ? "bg-mark" : ""
              }`}
            >
              <InlineMarkdown content={s} />
              {j < transSents.length - 1 ? " " : ""}
            </span>
          ))
        ) : seg.translated ? (
          <MarkdownBody content={seg.translated} baseDir={baseDir} showImages={false} />
        ) : (
          <span className="text-xs text-primary/35">（翻译中…）</span>
        )}
      </div>
    </div>
  );
}

/** 双语段落记忆化：翻译进度事件频繁触发时避免整段重渲染 */
const SentencePairMemo = memo(SentencePair);

/** 阅读视图：原文 / 译文 / 双语对照 / 拆解（双语） */
function ReaderView({ docId, title, onBack, initialMode, onOpenNotes }: Props) {
  const [mode, setMode] = useState<Mode>(initialMode ?? "original");
  const [content, setContent] = useState("");
  const [baseDir, setBaseDir] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [liveSegs, setLiveSegs] = useState<Map<number, BilingualSegment>>(new Map());
  const [transState, setTransState] = useState<"idle" | "loading" | "done">("idle");
  const [transError, setTransError] = useState<string | null>(null);
  const [translatingNow, setTranslatingNow] = useState(false);
  const [imgNotes, setImgNotes] = useState<Record<string, string>>({});
  const [digest, setDigest] = useState<DigestRecord | null>(null);
  const [digestLoading, setDigestLoading] = useState(false);
  const [digestError, setDigestError] = useState<string | null>(null);
  const [digestVersions, setDigestVersions] = useState<{ version: number; created_at: string }[]>([]);
  const [editing, setEditing] = useState(false);
  const [editFields, setEditFields] = useState<DigestField[] | null>(null);
  const [savingDigest, setSavingDigest] = useState(false);
  const [exportMenu, setExportMenu] = useState(false);
  // 导出内容范围：默认「除原文外全部」—— 原文 PDF 本就在手边，全量导出价值不大
  const [exportScope, setExportScope] = useState("no-original");
  // 标注：value 与后端 ExportScope::parse 的取值一一对应
  const EXPORT_SCOPES = [
    { value: "translated", label: "中文译文" },
    { value: "bilingual", label: "双语对照" },
    { value: "digest", label: "拆解结果" },
    { value: "no-original", label: "除原文外全部" },
    { value: "all", label: "全部内容" },
  ];
  const [exportMsg, setExportMsg] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  // F8 摘录：自定义右键菜单（选中文本时出现）
  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number; text: string } | null>(null);

  const handleCtx = (e: React.MouseEvent) => {
    const sel = window.getSelection()?.toString().trim();
    if (sel) {
      e.preventDefault();
      setCtxMenu({ x: e.clientX, y: e.clientY, text: sel });
    }
  };

  const copyExcerpt = async () => {
    if (!ctxMenu) return;
    const excerpt = `> ${ctxMenu.text.replace(/\n+/g, "\n> ")}\n> ——《${title ?? "文献"}》`;
    try {
      await navigator.clipboard.writeText(excerpt);
      setExportMsg("已复制摘录，可粘贴到「笔记」中");
    } catch {
      setExportMsg("复制失败，请手动复制");
    }
    setCtxMenu(null);
  };

  // 导出（M4.1）：md / html（内嵌图片 + 主题），保存到用户选择路径
  const handleExport = async (format: "md" | "html", theme: string) => {
    setExportMenu(false);
    const ext = format === "html" ? "html" : "md";
    const path = await save({
      defaultPath: `${(title || "文献").replace(/[\\/:*?"<>|]/g, "_")}.${ext}`,
      filters:
        format === "html"
          ? [{ name: "HTML", extensions: ["html"] }]
          : [{ name: "Markdown", extensions: ["md"] }],
    });
    if (!path) return;
    setExporting(true);
    try {
      await invoke("export_document", { docId, format, theme, scope: exportScope, outPath: path });
      setExportMsg("导出成功");
      setTimeout(() => setExportMsg(null), 2500);
    } catch (e) {
      setExportMsg(`导出失败: ${String(e)}`);
      setTimeout(() => setExportMsg(null), 4000);
    } finally {
      setExporting(false);
    }
  };

  // 导出 PDF（M4.2）：HTML 中转 → 系统打印窗口（打印面板选「存储为 PDF」）
  const handlePrintPdf = async (theme: string) => {
    setExportMenu(false);
    setExporting(true);
    try {
      await invoke("print_document", { docId, theme, scope: exportScope });
      setExportMsg("打印窗口已打开，在打印面板中选择「存储为 PDF」");
      setTimeout(() => setExportMsg(null), 4000);
    } catch (e) {
      setExportMsg(`导出 PDF 失败: ${String(e)}`);
      setTimeout(() => setExportMsg(null), 4000);
    } finally {
      setExporting(false);
    }
  };

  // 拆解：进入「拆解」模式时读取最新版拆解结果 + 历史版本列表
  useEffect(() => {
    if (mode !== "digest") return;
    let cancelled = false;
    setDigestLoading(true);
    setDigestError(null);
    invoke<DigestRecord | null>("read_digest", { docId })
      .then((r) => {
        if (cancelled) return;
        setDigest(cleanDigestRecord(r));
        setDigestLoading(false);
      })
      .catch((e) => {
        if (!cancelled) {
          setDigestError(String(e));
          setDigestLoading(false);
        }
      });
    invoke<{ version: number; created_at: string }[]>("list_digest_versions", { docId })
      .then((r) => {
        if (!cancelled) setDigestVersions(r);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [mode, docId]);

  // 拆解：切换查看指定版本
  const loadDigestVersion = async (version: number) => {
    try {
      const r = await invoke<DigestRecord | null>("read_digest_version", { docId, version });
      if (r) setDigest(r);
    } catch (e) {
      setDigestError(String(e));
    }
  };

  // 拆解：进入在线编辑
  const startDigestEdit = () => {
    if (!digest) return;
    setEditFields(digest.fields.map((f) => ({ ...f })));
    setEditing(true);
  };

  const updateEditField = (i: number, key: "zh" | "en" | "table", val: string) => {
    setEditFields((prev) =>
      prev ? prev.map((f, j) => (j === i ? { ...f, [key]: val } : f)) : prev,
    );
  };

  // 拆解：保存编辑为新版本（保存即版本）
  const saveDigestEdit = async () => {
    if (!editFields) return;
    setSavingDigest(true);
    try {
      const r = await invoke<DigestRecord>("save_digest_edit", { docId, fields: editFields });
      setDigest(r);
      setEditing(false);
      setEditFields(null);
      const versions = await invoke<{ version: number; created_at: string }[]>(
        "list_digest_versions",
        { docId },
      );
      setDigestVersions(versions);
    } catch (e) {
      setDigestError(String(e));
    } finally {
      setSavingDigest(false);
    }
  };

  // 拆解：回滚到当前查看的旧版本（生成新最新版本，保留历史）
  const rollbackDigest = async (version: number) => {
    setSavingDigest(true);
    try {
      const r = await invoke<DigestRecord>("rollback_digest", { docId, version });
      setDigest(cleanDigestRecord(r));
      setEditing(false);
      setEditFields(null);
      const versions = await invoke<{ version: number; created_at: string }[]>(
        "list_digest_versions",
        { docId },
      );
      setDigestVersions(versions);
    } catch (e) {
      setDigestError(String(e));
    } finally {
      setSavingDigest(false);
    }
  };

  // 图片视觉识别结果（翻译时若配置了视觉模型，自动生成 images_analysis.json）
  useEffect(() => {
    let cancelled = false;
    invoke<Record<string, string>>("read_image_notes", { docId })
      .then((r) => {
        if (!cancelled) setImgNotes(r);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [docId]);

  // 原文：进入即加载
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    invoke<{ content: string; base_dir: string }>("read_parsed", { docId })
      .then((r) => {
        if (!cancelled) {
          setContent(r.content);
          setBaseDir(r.base_dir);
          setLoading(false);
        }
      })
      .catch((e) => {
        if (!cancelled) {
          setError(String(e));
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [docId]);

  // 译文 / 双语：进入即读 segments.json 作为种子（含断点续传中间态），后续靠事件增量更新
  useEffect(() => {
    if (mode === "original") return;
    let cancelled = false;
    setTransError(null);
    setTransState("loading");
    invoke<BilingualSegment[]>("read_bilingual", { docId })
      .then((r) => {
        if (cancelled) return;
        // 合并而非覆盖：事件可能已推送更新的段，避免竞态覆盖
        setLiveSegs((prev) => {
          const next = new Map(prev);
          for (const s of r) {
            const cur = next.get(s.index);
            if (!cur || (cur.translated === "" && s.translated !== "")) {
              next.set(s.index, s);
            }
          }
          return next;
        });
      })
      .catch((e) => {
        if (!cancelled) setTransError(String(e));
      })
      .finally(() => {
        if (!cancelled) setTransState("done");
      });
    return () => {
      cancelled = true;
    };
  }, [mode, docId]);

  // 翻译任务事件：段级实时推送（译文/双语逐段显示）+ 翻译中/完成/失败标记
  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    const register = async <T,>(event: string, handler: (p: T) => void) => {
      unlisteners.push(await listen<T>(event, (e) => handler(e.payload)));
    };
    register<{
      doc_id: string;
      index: number;
      kind: string;
      original: string;
      translated: string;
    }>("translate-segment", (p) => {
      if (p.doc_id !== docId) return;
      setTranslatingNow(true);
      setLiveSegs((prev) => {
        const next = new Map(prev);
        next.set(p.index, {
          index: p.index,
          kind: p.kind,
          original: p.original,
          translated: p.translated,
        });
        return next;
      });
    });
    register<{ doc_id: string }>("translate-progress", (p) => {
      if (p.doc_id === docId) setTranslatingNow(true);
    });
    register<{ doc_id: string }>("translate-done", (p) => {
      if (p.doc_id === docId) setTranslatingNow(false);
    });
    register<{ doc_id: string }>("translate-failed", (p) => {
      if (p.doc_id === docId) setTranslatingNow(false);
    });
    return () => {
      unlisteners.forEach((fn) => fn());
    };
  }, [docId]);

  const sortedSegs = useMemo(
    () => [...liveSegs.values()].sort((a, b) => a.index - b.index),
    [liveSegs],
  );
  const translatedContent = useMemo(
    () =>
      sortedSegs
        .filter((s) => s.translated !== "")
        .map((s) => s.translated)
        .join("\n\n"),
    [sortedSegs],
  );
  const hasTranslated = translatedContent.trim() !== "";

  // 目录：原文视图用原文标题；译文/双语视图用译文标题（未翻译则回退原文）
  const toc = useMemo(() => {
    if (mode === "original") return buildToc(content);
    const items: TocItem[] = [];
    let n = 0;
    for (const seg of sortedSegs) {
      if (seg.kind !== "heading") continue;
      const m = seg.original.trim().match(/^(#{1,4})\s+(.*?)\s*$/);
      if (!m) continue;
      const text = (seg.translated || seg.original)
        .trim()
        .replace(/^(#{1,4})\s+/, "")
        .replace(/\s*<a id="toc-\d+"><\/a>\s*$/, "")
        .trim();
      items.push({ id: `toc-${n++}`, level: m[1].length, text });
    }
    return items;
  }, [mode, content, sortedSegs]);
  const headingAnchorIds = useMemo(() => {
    const map = new Map<number, string>();
    let n = 0;
    for (const seg of sortedSegs) {
      if (seg.kind === "heading") map.set(seg.index, `toc-${n++}`);
    }
    return map;
  }, [sortedSegs]);
  const scrollToToc = (id: string) => {
    document.getElementById(id)?.scrollIntoView({ behavior: "smooth", block: "start" });
  };
  const scrollToTop = () => {
    document.getElementById("reader-scroll")?.scrollTo({ top: 0, behavior: "smooth" });
  };

  // 切换模式时回到顶部，避免保留上一视图的滚动位置
  useEffect(() => {
    document.getElementById("reader-scroll")?.scrollTo({ top: 0 });
  }, [mode]);

  // 注入标题锚点记忆化：内容未变时复用结果，避免每次渲染重扫全文
  const originalInjected = useMemo(() => injectHeadingAnchors(content), [content]);
  const translatedInjected = useMemo(
    () => injectHeadingAnchors(translatedContent),
    [translatedContent],
  );

  return (
    <div className="flex h-full flex-col">
      <header className="flex h-14 shrink-0 items-center gap-3 border-b border-divider bg-panel px-6">
        <button
          onClick={onBack}
          className="rounded-md px-2.5 py-1.5 text-sm text-primary/60 transition-colors hover:bg-hover"
        >
          ← 返回
        </button>
        <div className="min-w-0 flex-1">
          <div className="truncate text-[15px] font-semibold">{title}</div>
          <div className="text-xs text-primary/45">阅读视图</div>
        </div>
        <div className="relative shrink-0">
          <button
            onClick={() => setExportMenu((v) => !v)}
            disabled={exporting}
            className="rounded-md border border-divider-strong px-2.5 py-1 text-xs font-medium text-primary/70 transition-colors hover:bg-hover disabled:opacity-50"
          >
            {exporting ? "导出中…" : "导出"}
          </button>
          {exportMenu && (
            <div className="absolute right-0 top-9 z-20 w-56 overflow-hidden rounded-lg border border-divider-strong bg-panel py-1 shadow-lg">
              <div className="border-b border-divider px-3 py-2">
                <div className="mb-1.5 text-[11px] text-primary/45">导出内容</div>
                <div className="flex flex-wrap gap-1">
                  {EXPORT_SCOPES.map((s) => (
                    <button
                      key={s.value}
                      onClick={() => setExportScope(s.value)}
                      className={`rounded-md px-2 py-0.5 text-[11px] transition-colors ${
                        exportScope === s.value
                          ? "bg-primary/90 text-primary-inverse"
                          : "bg-hover text-primary/65 hover:text-primary/85"
                      }`}
                    >
                      {s.label}
                    </button>
                  ))}
                </div>
              </div>
              <button
                onClick={() => void handleExport("md", "light")}
                className="block w-full px-3 py-1.5 text-left text-xs text-primary/75 hover:bg-hover"
              >
                导出 Markdown
              </button>
              <button
                onClick={() => void handleExport("html", "light")}
                className="block w-full px-3 py-1.5 text-left text-xs text-primary/75 hover:bg-hover"
              >
                导出 HTML（浅色）
              </button>
              <button
                onClick={() => void handleExport("html", "sepia")}
                className="block w-full px-3 py-1.5 text-left text-xs text-primary/75 hover:bg-hover"
              >
                导出 HTML（米色）
              </button>
              <div className="mx-3 my-1 border-t border-divider-strong" />
              <button
                onClick={() => void handlePrintPdf("light")}
                className="block w-full px-3 py-1.5 text-left text-xs text-primary/75 hover:bg-hover"
              >
                导出 PDF（浅色）
              </button>
              <button
                onClick={() => void handlePrintPdf("sepia")}
                className="block w-full px-3 py-1.5 text-left text-xs text-primary/75 hover:bg-hover"
              >
                导出 PDF（米色）
              </button>
            </div>
          )}
        </div>
        {exportMsg && (
          <span className="anim-fade-in shrink-0 text-xs text-primary/50">{exportMsg}</span>
        )}
        <div className="flex shrink-0 items-center gap-1 rounded-lg bg-track p-0.5 text-xs">
          {(["original", "translated", "bilingual", "digest"] as const).map((m) => (
            <button
              key={m}
              onClick={() => setMode(m)}
              className={`rounded-md px-2.5 py-1 font-medium transition-colors ${
                mode === m
                  ? "bg-panel text-primary shadow-sm"
                  : "text-primary/55 hover:text-primary"
              }`}
            >
              {m === "original"
                ? "原文"
                : m === "translated"
                  ? "译文"
                  : m === "bilingual"
                    ? "双语"
                    : "拆解"}
            </button>
          ))}
        </div>
        {onOpenNotes && (
          <button
            onClick={onOpenNotes}
            title="打开笔记"
            aria-label="打开笔记"
            className="shrink-0 rounded-md border border-divider-strong px-2.5 py-1 text-xs font-medium text-primary/70 transition-colors hover:bg-hover"
          >
            笔记
          </button>
        )}
      </header>

      <main className="flex flex-1 overflow-hidden">
        {/* 左侧目录（快速链接定位）；拆解视图不显示目录 */}
        {mode !== "digest" && (
        <aside className="w-52 shrink-0 overflow-y-auto border-r border-divider bg-panel/70 px-3 py-4">
          <div className="mb-2 px-2 text-[11px] font-medium uppercase tracking-wide text-primary/40">
            目录
          </div>
          {toc.length === 0 ? (
            <div className="px-2 text-xs text-primary/35">暂无章节标题</div>
          ) : (
            <div className="space-y-0.5">
              <button
                onClick={scrollToTop}
                className="block w-full rounded-md px-2 py-1 text-left text-xs text-primary/55 transition-colors hover:bg-hover"
              >
                ↥ 回到顶部
              </button>
              {toc.map((item) => (
                <button
                  key={item.id}
                  onClick={() => scrollToToc(item.id)}
                  style={{ paddingLeft: `${8 + (item.level - 1) * 12}px` }}
                  className={`block w-full truncate rounded-md px-2 py-1 text-left text-xs transition-colors hover:bg-hover ${
                    item.level === 1
                      ? "font-medium text-primary"
                      : "text-primary/60"
                  }`}
                  title={item.text}
                >
                  {item.text}
                </button>
              ))}
            </div>
          )}
        </aside>
        )}

        {/* 内容滚动区 */}
        <div id="reader-scroll" className="flex-1 overflow-y-auto">
        <div key={mode} className="anim-fade-in h-full">
        {loading ? (
          <div className="flex h-full items-center justify-center gap-2 text-sm text-primary/45">
            <span className="spinner" />
            加载解析结果…
          </div>
        ) : error ? (
          <div className="flex h-full items-center justify-center">
            <div className="anim-fade-in rounded-lg border border-danger-border bg-danger-bg px-4 py-2.5 text-sm text-danger-fg">
              {error}
            </div>
          </div>
        ) : mode === "digest" ? (
          digestLoading ? (
            <div className="flex h-full items-center justify-center gap-2 text-sm text-primary/45">
              <span className="spinner" />
              加载拆解结果…
            </div>
          ) : digestError ? (
            <div className="flex h-full items-center justify-center">
              <div className="anim-fade-in rounded-lg border border-danger-border bg-danger-bg px-4 py-2.5 text-sm text-danger-fg">
                {digestError}
              </div>
            </div>
          ) : digest ? (
            <div className="mx-auto max-w-5xl px-8 py-8">
              {/* 版本工具栏：版本切换 / 在线编辑 / 回滚 */}
              <div className="mb-4 flex flex-wrap items-center gap-2">
                <span className="rounded-full bg-violet-600 px-2.5 py-0.5 text-xs font-medium text-white">
                  AI 拆解 · v{digest.version}
                </span>
                {digestVersions.length > 1 && (
                  <select
                    value={digest.version}
                    onChange={(e) => void loadDigestVersion(Number(e.target.value))}
                    className="rounded-md border border-divider-strong bg-panel px-2 py-1 text-xs text-primary/70"
                  >
                    {digestVersions.map((v) => (
                      <option key={v.version} value={v.version}>
                        v{v.version} · {new Date(Number(v.created_at)).toLocaleString()}
                      </option>
                    ))}
                  </select>
                )}
                <span className="text-xs text-primary/45">
                  {digest.fields.length} 个字段 · 中文 / English 对照
                </span>
                <div className="ml-auto flex items-center gap-1.5">
                  {digest.version < (digestVersions[0]?.version ?? digest.version) && (
                    <button
                      onClick={() => void rollbackDigest(digest.version)}
                      disabled={savingDigest}
                      className="rounded-lg border border-warning-border bg-warning-bg px-2.5 py-1 text-xs font-medium text-warning-fg transition-colors hover:bg-warning-border disabled:opacity-50"
                    >
                      回滚到此版本
                    </button>
                  )}
                  {!editing ? (
                    <button
                      onClick={startDigestEdit}
                      className="rounded-lg border border-divider-strong bg-panel px-2.5 py-1 text-xs font-medium text-primary/70 transition-colors hover:bg-hover"
                    >
                      编辑
                    </button>
                  ) : (
                    <>
                      <button
                        onClick={() => {
                          setEditing(false);
                          setEditFields(null);
                        }}
                        className="rounded-lg border border-divider-strong bg-panel px-2.5 py-1 text-xs font-medium text-primary/70 transition-colors hover:bg-hover"
                      >
                        取消
                      </button>
                      <button
                        onClick={() => void saveDigestEdit()}
                        disabled={savingDigest}
                        className="rounded-lg bg-violet-600 px-2.5 py-1 text-xs font-medium text-white transition-colors hover:bg-violet-500 disabled:opacity-50"
                      >
                        {savingDigest ? "保存中…" : "保存为新版本"}
                      </button>
                    </>
                  )}
                </div>
              </div>

              {/* 字段：编辑模式可编辑；展示模式双语对照 */}
              {(editing && editFields ? editFields : digest.fields).map((fld, i) =>
                editing ? (
                  <div
                    key={fld.name}
                    className="mb-5 rounded-xl border border-divider bg-surface p-5"
                  >
                    <div className="mb-2 flex flex-wrap items-center gap-1.5">
                      <span className="text-sm font-semibold">{fld.label}</span>
                      <span className="rounded border border-divider-strong px-1 text-[9px] text-primary/45">
                        {fld.ftype}
                      </span>
                    </div>
                    {fld.ftype === "table" ? (
                      <textarea
                        value={fld.table ?? ""}
                        onChange={(e) => updateEditField(i, "table", e.target.value)}
                        className="min-h-[140px] w-full rounded-lg border border-divider-strong bg-panel p-2 font-mono text-xs leading-relaxed"
                        placeholder="Markdown 表格"
                      />
                    ) : (
                      <div className="grid grid-cols-2 gap-6">
                        <div className="min-w-0">
                          <div className="mb-1 text-xs font-medium text-primary/45">中文</div>
                          <textarea
                            value={fld.zh}
                            onChange={(e) => updateEditField(i, "zh", e.target.value)}
                            className="min-h-[100px] w-full rounded-lg border border-divider-strong bg-panel p-2 text-xs leading-relaxed"
                          />
                        </div>
                        <div className="min-w-0 border-l border-divider-strong pl-6">
                          <div className="mb-1 text-xs font-medium text-primary/45">English</div>
                          <textarea
                            value={fld.en}
                            onChange={(e) => updateEditField(i, "en", e.target.value)}
                            className="min-h-[100px] w-full rounded-lg border border-divider-strong bg-panel p-2 text-xs leading-relaxed"
                          />
                        </div>
                      </div>
                    )}
                  </div>
                ) : (
                  <div
                    key={fld.name}
                    className="mb-5 rounded-xl border border-divider bg-surface p-5"
                  >
                    <div className="mb-3 flex flex-wrap items-center gap-1.5">
                      <span className="text-sm font-semibold">{fld.label}</span>
                      <span className="rounded border border-divider-strong px-1 text-[9px] text-primary/45">
                        {fld.ftype}
                      </span>
                      <span className="text-[10px] text-primary/40">{fld.source}</span>
                      {fld.failed && (
                        <span className="rounded bg-warning-bg px-1 text-[9px] text-warning-fg">
                          引用缺失/失败
                        </span>
                      )}
                    </div>
                    {fld.table ? (
                      <div dangerouslySetInnerHTML={{ __html: renderMdTable(fld.table) }} />
                    ) : (
                      <div className="grid grid-cols-2 gap-6">
                        <div className="min-w-0">
                          <div className="mb-1.5 text-xs font-medium text-primary/45">中文</div>
                          {fld.ftype === "list[string]" ? (
                            <ul className="list-disc space-y-0.5 pl-5 text-sm leading-relaxed text-primary/80">
                              {fld.zh
                                .split("\n")
                                .filter((l) => l.trim())
                                .map((line, li) => (
                                  <li key={li}>{line.replace(/^[-*•]\s*/, "")}</li>
                                ))}
                            </ul>
                          ) : (
                            <div className="whitespace-pre-wrap text-sm leading-relaxed text-primary/80">
                              {fld.zh}
                            </div>
                          )}
                        </div>
                        <div className="min-w-0 border-l border-divider-strong pl-6">
                          <div className="mb-1.5 text-xs font-medium text-primary/45">English</div>
                          {fld.en ? (
                            fld.ftype === "list[string]" ? (
                              <ul className="list-disc space-y-0.5 pl-5 text-sm leading-relaxed text-primary/70">
                                {fld.en
                                  .split("\n")
                                  .filter((l) => l.trim())
                                  .map((line, li) => (
                                    <li key={li}>{line.replace(/^[-*•]\s*/, "")}</li>
                                  ))}
                              </ul>
                            ) : (
                              <div className="whitespace-pre-wrap text-sm leading-relaxed text-primary/70">
                                {fld.en}
                              </div>
                            )
                          ) : (
                            <span className="text-xs text-primary/35">—</span>
                          )}
                        </div>
                      </div>
                    )}
                  </div>
                ),
              )}
            </div>
          ) : (
            <div className="flex h-full items-center justify-center text-sm text-primary/45">
              暂无拆解结果，请先在文献库执行「拆解」
            </div>
          )
        ) : mode === "original" ? (
          content.trim() === "" ? (
            <div className="flex h-full items-center justify-center text-sm text-primary/45">
              暂无原文内容
            </div>
          ) : (
            <div
              className="prose prose-slate mx-auto max-w-3xl px-8 py-8 prose-headings:tracking-tight prose-a:text-blue-700"
              onContextMenu={handleCtx}
            >
              <MarkdownBodyMemo content={originalInjected} baseDir={baseDir} imgNotes={imgNotes} />
            </div>
          )
        ) : transState === "loading" ? (
          <div className="flex h-full items-center justify-center gap-2 text-sm text-primary/45">
            <span className="spinner" />
            加载译文…
          </div>
        ) : mode === "translated" ? (
          hasTranslated ? (
            <div
              className="prose prose-slate mx-auto max-w-3xl px-8 py-8 prose-headings:tracking-tight prose-a:text-blue-700"
              onContextMenu={handleCtx}
            >
              <MarkdownBodyMemo content={translatedInjected} baseDir={baseDir} imgNotes={imgNotes} />
            </div>
          ) : translatingNow ? (
            <div className="flex h-full items-center justify-center text-sm text-primary/45">
              翻译进行中，译文将逐段显示…
            </div>
          ) : transError ? (
            <div className="flex h-full items-center justify-center">
              <div className="anim-fade-in rounded-lg border border-danger-border bg-danger-bg px-4 py-2.5 text-sm text-danger-fg">
                {transError}
              </div>
            </div>
          ) : (
            <div className="flex h-full items-center justify-center text-sm text-primary/45">
              暂无译文
            </div>
          )
        ) : (
          <div className="mx-auto max-w-5xl px-8 py-8">
            <div className="mb-3 grid grid-cols-2 gap-6 text-xs font-medium text-primary/45">
              <div>原文</div>
              <div>译文</div>
            </div>
            {sortedSegs.length === 0 ? (
              translatingNow ? (
                <div className="py-16 text-center text-sm text-primary/45">
                  翻译进行中，双语将逐段显示…
                </div>
              ) : transError ? (
                <div className="anim-fade-in py-16 text-center text-sm text-danger-fg">{transError}</div>
              ) : (
                <div className="py-16 text-center text-sm text-primary/45">暂无译文</div>
              )
            ) : (
              sortedSegs.map((seg) =>
                seg.kind === "paragraph" ? (
                  <SentencePairMemo key={seg.index} seg={seg} baseDir={baseDir} imgNotes={imgNotes} />
                ) : (
                  <div
                    key={seg.index}
                    className="grid grid-cols-2 gap-6 border-b border-divider py-6 first:pt-0"
                  >
                    <div className="min-w-0" onContextMenu={handleCtx}>
                      {seg.kind === "heading" && headingAnchorIds.get(seg.index) ? (
                        <a id={headingAnchorIds.get(seg.index)!} />
                      ) : null}
                      <MarkdownBodyMemo content={seg.original} baseDir={baseDir} imgNotes={imgNotes} />
                    </div>
                    <div className="min-w-0 border-l border-divider-strong pl-6">
                      {seg.translated ? (
                        <MarkdownBodyMemo
                          content={seg.translated}
                          baseDir={baseDir}
                          showImages={false}
                        />
                      ) : (
                        <span className="text-xs text-primary/35">（翻译中…）</span>
                      )}
                    </div>
                  </div>
                ),
              )
            )}
          </div>)}
          </div>
        </div>
      </main>
      {/* F8 摘录右键菜单（选中文本时出现） */}
      {ctxMenu && (
        <>
          <div
            className="fixed inset-0 z-40"
            onClick={() => setCtxMenu(null)}
            onContextMenu={(e) => {
              e.preventDefault();
              setCtxMenu(null);
            }}
          />
          <div
            className="fixed z-50 overflow-hidden rounded-lg border border-divider-strong bg-panel py-1 shadow-lg"
            style={{ left: Math.min(ctxMenu.x, window.innerWidth - 190), top: ctxMenu.y }}
          >
            <button
              onClick={copyExcerpt}
              className="block w-full px-3 py-1.5 text-left text-xs text-primary/75 hover:bg-hover"
            >
              复制为摘录（含来源）
            </button>
            <button
              onClick={() => setCtxMenu(null)}
              className="block w-full px-3 py-1.5 text-left text-xs text-primary/75 hover:bg-hover"
            >
              取消
            </button>
          </div>
        </>
      )}
    </div>
  );
}

export default ReaderView;
