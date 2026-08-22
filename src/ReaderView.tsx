import { useEffect, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
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

/** 解析结果预览：图文 Markdown 渲染（GFM + 公式 + 代码高亮 + 本地图片 + 引用锚点） */
function ReaderView({ docId, title, onBack }: Props) {
  const [content, setContent] = useState("");
  const [baseDir, setBaseDir] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
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

  return (
    <div className="flex h-full flex-col">
      <header className="flex h-14 shrink-0 items-center gap-3 border-b border-black/5 bg-white px-6">
        <button
          onClick={onBack}
          className="rounded-md px-2.5 py-1.5 text-sm text-[#0b1326]/60 transition-colors hover:bg-black/5"
        >
          ← 返回
        </button>
        <div className="min-w-0 flex-1">
          <div className="truncate text-[15px] font-semibold">{title}</div>
          <div className="text-xs text-[#0b1326]/45">解析结果预览</div>
        </div>
      </header>

      <main className="flex-1 overflow-y-auto">
        {loading ? (
          <div className="flex h-full items-center justify-center text-sm text-[#0b1326]/45">
            加载解析结果…
          </div>
        ) : error ? (
          <div className="flex h-full items-center justify-center">
            <div className="rounded-lg border border-red-200 bg-red-50 px-4 py-2.5 text-sm text-red-700">
              {error}
            </div>
          </div>
        ) : (
          <div className="prose prose-slate mx-auto max-w-3xl px-8 py-8 prose-headings:tracking-tight prose-a:text-blue-700">
            <ReactMarkdown
              remarkPlugins={[remarkGfm, remarkMath]}
              rehypePlugins={[rehypeRaw, rehypeKatex, rehypeHighlight]}
              components={{
                img: ({ src, alt }) => {
                  // 相对路径（MinerU 产物：images/xxx.jpg）→ 本地绝对路径 → asset 协议 URL
                  let resolved = src;
                  if (src && !/^https?:\/\//.test(src)) {
                    resolved = convertFileSrc(
                      `${baseDir}/${src.replace(/^\.\//, "")}`,
                    );
                  }
                  return (
                    <img
                      src={resolved}
                      alt={alt ?? ""}
                      className="my-4 max-w-full rounded-lg border border-black/5"
                    />
                  );
                },
              }}
            >
              {addCitationLinks(content)}
            </ReactMarkdown>
          </div>
        )}
      </main>
    </div>
  );
}

export default ReaderView;
