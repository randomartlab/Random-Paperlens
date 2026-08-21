import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import rehypeKatex from "rehype-katex";
import rehypeHighlight from "rehype-highlight";
import "katex/dist/katex.min.css";
import "highlight.js/styles/github.css";

interface Props {
  docId: string;
  title: string;
  onBack: () => void;
}

/** 解析结果预览：图文 Markdown 渲染（GFM + 公式 + 代码高亮） */
function ReaderView({ docId, title, onBack }: Props) {
  const [content, setContent] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    invoke<string>("read_parsed", { docId })
      .then((c) => {
        if (!cancelled) {
          setContent(c);
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
              rehypePlugins={[rehypeKatex, rehypeHighlight]}
            >
              {content}
            </ReactMarkdown>
          </div>
        )}
      </main>
    </div>
  );
}

export default ReaderView;
