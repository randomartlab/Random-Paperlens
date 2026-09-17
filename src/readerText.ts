/**
 * 阅读器的纯文本处理与「摘录出处」推导。
 *
 * 这里刻意不依赖 React / Tauri：切句、分块、段落定位、摘录组装都是纯函数，
 * 便于单独验证（见 scripts/check-reader-locator.ts）。
 */

/** 中文：按句末标点切分 */
export function splitOnPunct(text: string, puncts: string[]): string[] {
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
export function splitSentences(text: string): string[] {
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

/**
 * 把解析 Markdown 切成「可交互段落」与「原样块」。
 *
 * 普通段落按句拆分，以便悬停高亮与单击复制；
 * 标题、表格、代码块、图片、引用保持 Markdown 原样渲染，避免破坏结构
 * （公式、表格若被逐句拆开会散架）。
 */
export function splitInteractiveBlocks(
  md: string,
): { interactive: boolean; text: string }[] {
  const blocks: { interactive: boolean; text: string }[] = [];
  const lines = md.split("\n");
  let buf: string[] = [];
  let interactive = false;
  let inCode = false;

  const flush = () => {
    const text = buf.join("\n").trim();
    if (text) blocks.push({ interactive, text });
    buf = [];
  };

  for (const line of lines) {
    const t = line.trim();
    if (t.startsWith("```")) {
      if (inCode) {
        buf.push(line);
        inCode = false;
        flush();
      } else {
        flush();
        interactive = false;
        inCode = true;
        buf.push(line);
      }
      continue;
    }
    if (inCode) {
      buf.push(line);
      continue;
    }
    if (!t) {
      flush();
      continue;
    }
    // 结构块：标题 / 表格行 / 图片 / 引用
    const isBlock =
      /^#{1,6}\s/.test(t) || t.startsWith("|") || t.startsWith("![") || t.startsWith("> ");
    const want = !isBlock;
    if (buf.length && want !== interactive) flush();
    interactive = want;
    buf.push(line);
  }
  flush();
  return blocks;
}

/** 取 Markdown 标题的层级（`## x` → 2）；非标题返回 null */
export function mdHeadingLevel(md: string): number | null {
  const m = md.match(/^(#{1,6})\s/);
  return m ? m[1].length : null;
}

/** 去掉 Markdown 标记，取纯文本（用于标题路径展示） */
export function stripMdInline(md: string): string {
  return md
    .replace(/^#{1,6}\s*/, "")
    .replace(/<a id="[^"]*"><\/a>/g, "")
    .replace(/\*\*|__|`|\*|~~/g, "")
    .trim();
}

/** 定位推导的输入块：标题负责维护层级，正文计段，其它块穿透 */
export type LocEntry =
  | { kind: "heading"; level: number; text: string }
  | { kind: "body" }
  | { kind: "other" };

/** 两段标题是否其实是同一个（忽略大小写与标点空格差异） */
function sameTitle(a: string, b: string): boolean {
  const norm = (s: string) => s.toLowerCase().replace(/[\s\p{P}\p{S}]/gu, "");
  const x = norm(a);
  return x !== "" && x === norm(b);
}

/**
 * 摘录出处定位：把文档顺序上的一串块，推导成「章节路径 › 小节 · 第 N 段」。
 *
 * 段号只统计正文段（body），标题负责维护层级栈，表格/图片等其它块穿过不影响计数。
 * 这样贴进笔记的出处是"哪一章哪一节第几段"，而不是重复一遍笔记名里已有的题目。
 *
 * @param docTitle 论文题目：一级标题若就是题目本身则不进路径（笔记名里已经有了，重复只是噪音）
 */
export function buildLocatorList(entries: LocEntry[], docTitle?: string): string[] {
  const stack: { level: number; text: string }[] = [];
  const out: string[] = [];
  let para = 0;
  for (const e of entries) {
    if (e.kind === "heading") {
      // 同级或更深的后出现标题会替换掉旧路径
      while (stack.length && stack[stack.length - 1].level >= e.level) stack.pop();
      const text = e.text.trim();
      stack.push({
        level: e.level,
        text: docTitle && sameTitle(text, docTitle) ? "" : text,
      });
      out.push("");
    } else if (e.kind === "body") {
      para += 1;
      const path = stack
        .map((s) => s.text)
        .filter(Boolean)
        .join(" › ");
      out.push(`${path ? `${path} · ` : ""}第 ${para} 段`);
    } else {
      out.push("");
    }
  }
  return out;
}

/** 从 Markdown 块序列推导每块的定位（供原文视图使用） */
export function locatorListFromBlocks(
  blocks: { interactive: boolean; text: string }[],
  docTitle?: string,
): string[] {
  return buildLocatorList(
    blocks.map((b) => {
      if (b.interactive) return { kind: "body" };
      // 结构块会把相邻的表行/图片并进同一块，标题只取首行，否则路径里会混进表格
      const firstLine = b.text.split("\n")[0];
      const level = mdHeadingLevel(firstLine);
      return level === null
        ? { kind: "other" }
        : { kind: "heading", level, text: stripMdInline(firstLine) };
    }),
    docTitle,
  );
}

/**
 * 组装摘录文本：引用块 + 出处行。
 *
 * 出处用「章节路径 › 小节 · 第 N 段」而不是题目——题目已经写在笔记名与正文首行，
 * 重复一遍没有信息量，缺的恰恰是"这段话在论文的哪个位置"。
 * 定位不到时（如译文视图的零散选区）才回退成题目。
 */
export function buildExcerpt(text: string, loc?: string, title?: string): string {
  const source = loc?.trim() || `《${title ?? "文献"}》`;
  return `> ${text.replace(/\n+/g, "\n> ")}\n> —— ${source}`;
}

/**
 * 从右键落点解析"这次要摘什么"。
 *
 * 落点可能是句子（data-ex）、段落/整条（data-ex-full），也可能正好在句间空白上，
 * 所以要沿 DOM 往上找最近的候选，而不是只看最内层元素。
 * 返回 null 表示这里没有可摘的内容，交回浏览器原生菜单。
 */
export function resolveExcerptTarget(start: Element | null): {
  text: string;
  loc: string;
  el: HTMLElement;
} | null {
  const sentence = start?.closest?.("[data-ex]") as HTMLElement | null;
  const holder =
    sentence ?? (start?.closest?.("[data-ex-full]") as HTMLElement | null);
  if (!holder) return null;
  const text = sentence?.dataset.ex || holder.dataset.exFull;
  if (!text || !text.trim()) return null;
  const locHolder = start?.closest?.("[data-loc]") as HTMLElement | null;
  return { text, loc: locHolder?.dataset.loc ?? "", el: holder };
}
