import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import {
  buildExcerpt,
  buildLocatorList,
  locatorListFromBlocks,
  mdHeadingLevel,
  resolveExcerptTarget,
  splitInteractiveBlocks,
  splitSentences,
  stripMdInline,
} from "./readerText";
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
  /** 失败原因分类：config（凭据/配置）/ network / llm / internal；旧数据为空 */
  error_category?: string;
  /** 失败原因原文（说明是哪个环节出问题） */
  error_message?: string;
}

/** 把字段级失败翻译成"是哪个环节出问题"的人话 */
function upstreamFailureLabel(f: DigestField): string | null {
  const cat = f.error_category ?? "";
  const msg = f.error_message ?? "";
  if (cat === "config" || /401|403|API Key|鉴权|未授权|Unauthorized|无效/.test(msg)) {
    return "接口鉴权失败";
  }
  if (cat === "network") return "网络不可达";
  if (cat === "llm") return "模型返回异常";
  if (cat === "internal") return "处理出错";
  return "拆解失败";
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
    // 参考文献章节标题（英/中）：先剥掉 # 与 ** 标记再判断，
    // 否则 `## References` 这类常见写法永远匹配不上，整段参考文献都不会有锚点
    const plain = t.replace(/^#+\s*/, "").replace(/\*\*/g, "").trim();
    if (
      inRefs === false &&
      plain.length < 30 &&
      /^(References|REFERENCES|Reference|Bibliography|参考文献|引用文献)$/i.test(plain)
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
  // 负向断言 `(?!\()`：跳过已经是链接的 [n](...) 形式，
  // 否则会命中 MinerU 已生成脚注链接里的 [n]，替换出 [n](#ref-n)(#ref1) 这类畸形结果
  const linked = withAnchors.join("\n").replace(
    /\[(\d+(?:[\s,，\-–—]\s*\d+){0,7})\](?!\()/g,
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

/**
 * 当前是否已有用户选区。
 *
 * 单击复制前先判断：若用户正在拖选文字，说明他想精确选中某个词或片段，
 * 此时不触发复制，从而保留手动选取词语/句子的能力。
 */
function hasTextSelection(): boolean {
  const sel = window.getSelection();
  return !!sel && sel.toString().trim().length > 0;
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
  locator,
  onCopy,
}: {
  seg: BilingualSegment;
  baseDir: string;
  imgNotes?: Record<string, string>;
  /** 出处定位（章节路径 · 第 N 段），供右键摘录标注来源 */
  locator?: string;
  /** 单击复制回调：传入文本与鼠标位置，用于就地给出反馈 */
  onCopy?: (text: string, x: number, y: number) => void;
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
    <div
      data-loc={locator}
      // 落点在句间空白时的兜底：整段（原文 + 译文）
      data-ex-full={[seg.original, seg.translated].filter(Boolean).join("\n\n")}
      className="grid grid-cols-2 gap-6 border-b border-divider py-6 first:pt-0"
    >
      <div className="min-w-0 leading-relaxed">
        {canSync && align ? (
          origSents.map((s, i) => (
            <span
              key={i}
              // 无选区右键时按整句（原文 + 对应译文）摘录
              data-ex={transSents[align.o2t(i)] ? `${s}\n${transSents[align.o2t(i)]}` : s}
              onMouseEnter={() => setHover({ orig: i, trans: align.o2t(i) })}
              onMouseLeave={() => setHover(null)}
              onClick={(e) => {
                // 有选区说明用户在手动选字，让位给原生选择行为
                if (hasTextSelection()) return;
                const t = transSents[align.o2t(i)];
                onCopy?.(t ? `${s}\n${t}` : s, e.clientX, e.clientY);
              }}
              title="单击复制该句（原文 + 对应译文）"
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
              data-ex={origSents[align.t2o(j)] ? `${origSents[align.t2o(j)]}\n${s}` : s}
              onMouseEnter={() => setHover({ orig: align.t2o(j), trans: j })}
              onMouseLeave={() => setHover(null)}
              onClick={(e) => {
                if (hasTextSelection()) return;
                const o = origSents[align.t2o(j)];
                onCopy?.(o ? `${o}\n${s}` : s, e.clientX, e.clientY);
              }}
              title="单击复制该句（原文 + 对应译文）"
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

/** 原文视图的逐句交互渲染：段落内每句可悬停高亮、单击复制 */
function MarkdownSentences({
  content,
  baseDir,
  imgNotes,
  title,
  onCopy,
}: {
  content: string;
  baseDir: string;
  imgNotes?: Record<string, string>;
  /** 论文题目：用于把一级标题（题目本身）从出处路径里去掉 */
  title?: string;
  onCopy?: (text: string, x: number, y: number) => void;
}) {
  const [hover, setHover] = useState<string | null>(null);
  const blocks = useMemo(() => splitInteractiveBlocks(content), [content]);
  // 每个块的出处定位：标题维护章节路径，正文块依次计段，表格/图片等穿透
  const locators = useMemo(() => locatorListFromBlocks(blocks, title), [blocks, title]);

  return (
    <>
      {blocks.map((b, i) => {
        if (!b.interactive) {
          return (
            <div key={i}>
              <MarkdownBodyMemo content={b.text} baseDir={baseDir} imgNotes={imgNotes} />
            </div>
          );
        }
        const sents = splitSentences(b.text);
        return (
          <p
            key={i}
            data-loc={locators[i]}
            // 落点在句间空白时的兜底：整段
            data-ex-full={sents.join(" ")}
          >
            {sents.map((s, j) => {
              const key = `${i}-${j}`;
              return (
                <span
                  key={j}
                  data-ex={s}
                  onMouseEnter={() => setHover(key)}
                  onMouseLeave={() => setHover(null)}
                  onClick={(e) => {
                    // 有选区时让位给手动选取，不触发复制
                    if (hasTextSelection()) return;
                    onCopy?.(s, e.clientX, e.clientY);
                  }}
                  title="单击复制该句"
                  className={`cursor-pointer rounded px-0.5 transition-colors ${
                    hover === key ? "bg-mark" : ""
                  }`}
                >
                  <InlineMarkdown content={s} />
                  {j < sents.length - 1 ? " " : ""}
                </span>
              );
            })}
          </p>
        );
      })}
    </>
  );
}

const MarkdownSentencesMemo = memo(MarkdownSentences);

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
  // 拆解/识别任务运行中：进入拆解栏时显示实时进度，而不是「暂无拆解结果」这类误导提示
  const [digestRun, setDigestRun] = useState<{
    stage: string;
    detail: string;
    progress: number;
  } | null>(null);
  // 解析任务运行中：原文栏同理
  const [parseRun, setParseRun] = useState<{ stage: string; detail: string } | null>(null);
  // 拆解完成事件到达后自增，用于重新拉取拆解结果
  const [digestReload, setDigestReload] = useState(0);
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
  // F8 摘录：自定义右键菜单（选区或整句）
  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number; text: string; loc: string } | null>(
    null,
  );
  /**
   * 是否由「拖拽」产生了选区。
   *
   * 只用「有没有选区」判断用户意图是不够的：双击选词、或上一次拖选留下的残留选区，
   * 都会让右键摘录只拿到几个字。这里以鼠标是否真的移动过来区分：
   * 拖选 → 用户只要选中的片段；否则 → 以光标所在的整句为准。
   */
  const dragSelRef = useRef(false);
  const downPosRef = useRef<{ x: number; y: number } | null>(null);
  const trackMouseDown = (e: React.MouseEvent) => {
    // 只认左键：右键（摘录菜单）不能把拖拽标记冲掉，否则拖选后右键会退回"整句"
    if (e.button !== 0) return;
    downPosRef.current = { x: e.clientX, y: e.clientY };
    dragSelRef.current = false;
  };
  const trackMouseMove = (e: React.MouseEvent) => {
    const d = downPosRef.current;
    if (!d || e.buttons !== 1) return;
    if (Math.abs(e.clientX - d.x) + Math.abs(e.clientY - d.y) > 6) dragSelRef.current = true;
  };
  const trackMouseUp = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    downPosRef.current = null;
  };
  // 单击复制的就地反馈：记录鼠标位置，短暂显示「已复制」
  const [copyToast, setCopyToast] = useState<{ x: number; y: number; label?: string } | null>(
    null,
  );
  const copyToastTimer = useRef<number | null>(null);
  // 拆解栏当前悬停的条目（整条高亮，提示可整条复制）
  const [hoverField, setHoverField] = useState<number | null>(null);

  /** 复制文本并在鼠标位置给出「已复制」反馈（约 0.9 秒后自动消失） */
  const copyWithFeedback = useCallback(async (text: string, x: number, y: number) => {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      return; // 剪贴板不可用时静默跳过，避免给出错误的成功反馈
    }
    setCopyToast({ x, y });
    if (copyToastTimer.current !== null) window.clearTimeout(copyToastTimer.current);
    copyToastTimer.current = window.setTimeout(() => setCopyToast(null), 900);
  }, []);

  /** 就地反馈的统一出口：在鼠标位置短暂显示一行提示 */
  const flashToast = useCallback((label: string, x: number, y: number) => {
    setCopyToast({ x, y, label });
    if (copyToastTimer.current !== null) window.clearTimeout(copyToastTimer.current);
    copyToastTimer.current = window.setTimeout(() => setCopyToast(null), 1400);
  }, []);

  /** 摘录文本组装：引用块 + 出处行（章节路径 · 第 N 段），见 readerText.buildExcerpt */
  const makeExcerpt = useCallback(
    (text: string, loc?: string) => buildExcerpt(text, loc, title),
    [title],
  );

  /**
   * 把选中的文本加入该文献对应的笔记。
   * 首次会按命名规则新建笔记并建立关联，之后追加到同一份笔记末尾。
   */
  const addToNote = async (x: number, y: number) => {
    if (!ctxMenu) return;
    const excerpt = makeExcerpt(ctxMenu.text, ctxMenu.loc);
    const loc = ctxMenu.loc;
    setCtxMenu(null);
    try {
      const r = await invoke<{ note_name: string; created: boolean }>("append_to_note", {
        docId,
        text: excerpt,
      });
      flashToast(
        r.created ? `已新建笔记 · ${loc || "已摘录"}` : `已追加到笔记 · ${loc || "已摘录"}`,
        x,
        y,
      );
    } catch (e) {
      flashToast(`添加失败：${String(e)}`, x, y);
    }
  };

  /**
   * 右键菜单的入口判定：
   * - 拖拽产生的选区 → 摘录所选片段
   * - 否则（仅悬停、双击选词、残留选区）→ 摘录光标所在的整句；
   *   若落点正好在句间空白（不是任何句子的 DOM 子节点），退为整段
   * - 两者都不成立 → 不接管，交回原生菜单
   */
  const handleCtx = (e: React.MouseEvent) => {
    const target = e.target as Element | null;
    const hit = resolveExcerptTarget(target);
    const selText = window.getSelection()?.toString().trim() ?? "";
    if (selText && dragSelRef.current) {
      e.preventDefault();
      setCtxMenu({
        x: e.clientX,
        y: e.clientY,
        text: selText,
        loc: (target?.closest?.("[data-loc]") as HTMLElement | null)?.dataset.loc ?? "",
      });
      return;
    }
    if (hit) {
      e.preventDefault();
      // 清掉点击/双击留下的残留选区，避免高亮的片段与即将摘录的内容不一致
      window.getSelection()?.removeAllRanges();
      setCtxMenu({ x: e.clientX, y: e.clientY, text: hit.text, loc: hit.loc });
    }
  };

  const copyExcerpt = async () => {
    if (!ctxMenu) return;
    const excerpt = makeExcerpt(ctxMenu.text, ctxMenu.loc);
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
  }, [mode, docId, digestReload]);

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
    // 解析进度：让原文栏在任务进行中显示状态，而不是空白或报错
    register<{ doc_id: string; stage: string; progress: number; detail?: string }>(
      "parse-progress",
      (p) => {
        if (p.doc_id !== docId) return;
        setParseRun({ stage: p.stage, detail: p.detail ?? "" });
      },
    );
    register<{ doc_id: string }>("parse-done", (p) => {
      if (p.doc_id === docId) setParseRun(null);
    });
    register<{ doc_id: string }>("parse-failed", (p) => {
      if (p.doc_id === docId) setParseRun(null);
    });
    // 拆解进度（含范式识别阶段）：进入拆解栏时应看到实时阶段，而不是「暂无拆解结果」
    register<{ doc_id: string; stage: string; progress: number; detail?: string }>(
      "digest-progress",
      (p) => {
        if (p.doc_id !== docId) return;
        setDigestRun({ stage: p.stage, detail: p.detail ?? "", progress: p.progress });
      },
    );
    register<{ doc_id: string }>("digest-done", (p) => {
      if (p.doc_id !== docId) return;
      setDigestRun(null);
      setDigestReload((n) => n + 1); // 结果就绪，重新拉取
    });
    register<{ doc_id: string }>("digest-failed", (p) => {
      if (p.doc_id !== docId) return;
      setDigestRun(null);
      setDigestReload((n) => n + 1);
    });
    return () => {
      unlisteners.forEach((fn) => fn());
    };
  }, [docId]);

  const sortedSegs = useMemo(
    () => [...liveSegs.values()].sort((a, b) => a.index - b.index),
    [liveSegs],
  );
  // 双语视图每段的出处定位（与 sortedSegs 同序），供右键摘录标注来源
  const segLocators = useMemo(
    () =>
      buildLocatorList(
        sortedSegs.map((seg) => {
          if (seg.kind === "paragraph") return { kind: "body" as const };
          const firstLine = seg.original.split("\n")[0];
          const level = mdHeadingLevel(firstLine);
          return level === null
            ? { kind: "other" as const }
            : { kind: "heading" as const, level, text: stripMdInline(firstLine) };
        }),
        title,
      ),
    [sortedSegs, title],
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
        <div
          id="reader-scroll"
          className="flex-1 overflow-y-auto"
          onMouseDown={trackMouseDown}
          onMouseMove={trackMouseMove}
          onMouseUp={trackMouseUp}
        >
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
          ) : digestRun ? (
            <div className="flex h-full flex-col items-center justify-center gap-3">
              <span className="spinner" />
              <div className="text-sm text-primary/60">
                {digestRun.stage || "处理中"}
                {digestRun.detail ? ` · ${digestRun.detail}` : ""}
              </div>
              <div className="h-1.5 w-64 overflow-hidden rounded-full bg-track">
                <div
                  className="h-full bg-violet-500/70 transition-all"
                  style={{ width: `${Math.round((digestRun.progress || 0) * 100)}%` }}
                />
              </div>
              <div className="text-[11px] text-primary/40">
                完成后结果会自动出现，期间可先看原文或译文
              </div>
            </div>
          ) : digestError ? (
            <div className="flex h-full items-center justify-center">
              <div className="anim-fade-in rounded-lg border border-danger-border bg-danger-bg px-4 py-2.5 text-sm text-danger-fg">
                {digestError}
              </div>
            </div>
          ) : digest ? (
            <div className="mx-auto max-w-5xl px-8 py-8" onContextMenu={handleCtx}>
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

              {/* 上游故障统一横幅：把"软件坏了"和"接口/网络出问题"分开说清楚 */}
              {(() => {
                const pool = editing && editFields ? editFields : digest.fields;
                const failed = pool.filter((f) => f.failed);
                if (failed.length === 0) return null;
                const cred = failed.filter((f) => upstreamFailureLabel(f) === "接口鉴权失败").length;
                const net = failed.filter((f) => upstreamFailureLabel(f) === "网络不可达").length;
                const llm = failed.filter((f) => upstreamFailureLabel(f) === "模型返回异常").length;
                const missing = failed.length - cred - net - llm;
                const parts: string[] = [];
                if (cred) parts.push(`${cred} 项因接口鉴权失败`);
                if (net) parts.push(`${net} 项因网络不可达`);
                if (llm) parts.push(`${llm} 项因模型返回异常`);
                if (missing) parts.push(`${missing} 项未找到可用出处`);
                const isUpstream = cred + net + llm > 0;
                return (
                  <div
                    className={`mb-5 rounded-xl border p-4 text-sm leading-relaxed ${
                      isUpstream
                        ? "border-warning-border bg-warning-bg text-warning-fg"
                        : "border-divider bg-surface text-primary/70"
                    }`}
                  >
                    <div className="font-semibold">
                      {isUpstream
                        ? `本页有 ${failed.length} 个字段没能完成，其中 ${parts.filter((x) => !x.includes("未找到")).join("、")}——问题出在上游接口，不是软件本身`
                        : `本页有 ${failed.length} 个字段未能定位到原文出处，已在对应条目上标出`}
                    </div>
                    {isUpstream && (
                      <div className="mt-1.5">
                        请到「设置 → API 配置」检查 Base URL、API Key 与模型名，可先用「测试连接」验证；
                        解析走的是 MinerU 那套配置，不受影响。已完成的字段仍然保留。
                      </div>
                    )}
                    <div className="mt-1.5 opacity-80">明细：{parts.join("、")}</div>
                  </div>
                );
              })()}

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
                    data-loc={`拆解 · ${fld.label}`}
                    data-ex={[`【${fld.label}】`, fld.zh, fld.en, fld.table]
                      .filter(Boolean)
                      .join("\n\n")}
                    onMouseEnter={() => setHoverField(i)}
                    onMouseLeave={() => setHoverField(null)}
                    onClick={(e) => {
                      // 有选区时让位给原生文本选择，不触发整条复制
                      if (hasTextSelection()) return;
                      const parts = [`【${fld.label}】`, fld.zh];
                      if (fld.en) parts.push(fld.en);
                      if (fld.table) parts.push(fld.table);
                      void copyWithFeedback(
                        parts.filter(Boolean).join("\n\n"),
                        e.clientX,
                        e.clientY,
                      );
                    }}
                    title="单击复制该条目（含中英对照）"
                    className={`mb-5 cursor-pointer rounded-xl border p-5 transition-colors ${
                      hoverField === i
                        ? "border-primary/25 bg-primary/[0.03]"
                        : "border-divider bg-surface"
                    }`}
                  >
                    <div className="mb-3 flex flex-wrap items-center gap-1.5">
                      <span className="text-sm font-semibold">{fld.label}</span>
                      <span className="rounded border border-divider-strong px-1 text-[9px] text-primary/45">
                        {fld.ftype}
                      </span>
                      <span className="text-[10px] text-primary/40">{fld.source}</span>
                      {fld.failed && (
                        <span
                          className="rounded bg-warning-bg px-1 text-[9px] text-warning-fg"
                          title={fld.error_message || undefined}
                        >
                          {upstreamFailureLabel(fld)}
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
          parseRun ? (
            <div className="flex h-full flex-col items-center justify-center gap-3">
              <span className="spinner" />
              <div className="text-sm text-primary/60">
                解析中 · {parseRun.stage}
                {parseRun.detail ? ` · ${parseRun.detail}` : ""}
              </div>
              <div className="text-[11px] text-primary/40">完成后原文会自动显示</div>
            </div>
          ) : content.trim() === "" ? (
            <div className="flex h-full items-center justify-center text-sm text-primary/45">
              暂无原文内容
            </div>
          ) : (
            <div
              className="prose mx-auto max-w-3xl px-8 py-8 prose-headings:tracking-tight"
              onContextMenu={handleCtx}
            >
              <MarkdownSentencesMemo
                content={originalInjected}
                baseDir={baseDir}
                imgNotes={imgNotes}
                title={title}
                onCopy={copyWithFeedback}
              />
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
              className="prose mx-auto max-w-3xl px-8 py-8 prose-headings:tracking-tight"
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
          <div className="mx-auto max-w-5xl px-8 py-8" onContextMenu={handleCtx}>
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
              sortedSegs.map((seg, si) =>
                seg.kind === "paragraph" ? (
                  <SentencePairMemo
                    key={seg.index}
                    seg={seg}
                    baseDir={baseDir}
                    imgNotes={imgNotes}
                    locator={segLocators[si]}
                    onCopy={copyWithFeedback}
                  />
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
      {/* 单击复制的就地反馈 */}
      {copyToast && (
        <div
          className="anim-fade-in pointer-events-none fixed z-50 rounded-md bg-primary/90 px-2 py-0.5 text-[11px] font-medium text-primary-inverse shadow-lg"
          style={{ left: copyToast.x + 10, top: copyToast.y + 14 }}
        >
          {copyToast.label ?? "已复制"}
        </div>
      )}
      {/* F8 摘录右键菜单（选中片段或整句时出现） */}
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
            className="fixed z-50 w-64 overflow-hidden rounded-lg border border-divider-strong bg-panel py-1 shadow-lg"
            style={{ left: Math.min(ctxMenu.x, window.innerWidth - 268), top: ctxMenu.y }}
          >
            {/* 摘录预览：先让使用者看清"这次会摘到哪一句、标的是哪一段" */}
            <div className="border-b border-divider px-3 py-1.5">
              <div className="text-[10px] leading-snug text-primary/45">
                {ctxMenu.loc || "未定位到章节"}
              </div>
              <div className="mt-0.5 line-clamp-2 text-[11px] leading-snug text-primary/70">
                {ctxMenu.text}
              </div>
            </div>
            <button
              onClick={copyExcerpt}
              className="block w-full px-3 py-1.5 text-left text-xs text-primary/75 hover:bg-hover"
            >
              复制为摘录（含来源）
            </button>
            <button
              onClick={() => void addToNote(ctxMenu.x, ctxMenu.y)}
              className="block w-full px-3 py-1.5 text-left text-xs text-primary/75 hover:bg-hover"
            >
              添加到笔记
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
