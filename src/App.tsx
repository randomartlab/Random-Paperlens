import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { listen } from "@tauri-apps/api/event";
import ReaderView from "./ReaderView";

interface Doc {
  id: string;
  title: string;
  authors: string | null;
  year: number | null;
  journal: string | null;
  tags: string | null;
  file_path: string;
  status: string;
  language: string | null;
  created_at: string;
}

const STATUS_LABEL: Record<string, string> = {
  pending: "待解析",
  parsed: "已解析",
  translated: "已翻译",
  digested: "已拆解",
};

function App() {
  const [docs, setDocs] = useState<Doc[]>([]);
  const [notice, setNotice] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const [parsing, setParsing] = useState<Record<string, { stage: string; progress: number }>>({});
  const [view, setView] = useState<{ type: "list" } | { type: "reader"; doc: Doc }>({
    type: "list",
  });

  const openReader = (doc: Doc) => {
    if (doc.status === "parsed") setView({ type: "reader", doc });
  };

  const refresh = async () => {
    setDocs(await invoke<Doc[]>("list_documents"));
  };

  const handleImport = async (path: string) => {
    try {
      await invoke("import_document", { path });
      setNotice(null);
      await refresh();
    } catch (e) {
      setNotice(String(e));
    }
  };

  const handleParse = async (doc: Doc) => {
    try {
      setParsing((p) => ({ ...p, [doc.id]: { stage: "启动中", progress: 0 } }));
      await invoke("start_parse", { docId: doc.id });
    } catch (e) {
      setNotice(String(e));
    }
  };

  const pickFile = async () => {
    const path = await open({
      multiple: false,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (typeof path === "string") await handleImport(path);
  };

  useEffect(() => {
    refresh();
    // 解析任务事件：进度 / 完成 / 失败
    const unlisteners: Array<() => void> = [];
    const register = async <T,>(event: string, handler: (payload: T) => void) => {
      unlisteners.push(await listen<T>(event, (e) => handler(e.payload)));
    };
    register<{ doc_id: string; stage: string; progress: number }>("parse-progress", (p) => {
      setParsing((prev) => ({ ...prev, [p.doc_id]: { stage: p.stage, progress: p.progress } }));
    });
    register("parse-done", async () => {
      setParsing({});
      setNotice(null);
      await refresh();
    });
    register<{ doc_id: string; error: string }>("parse-failed", (p) => {
      setParsing((prev) => {
        const { [p.doc_id]: _drop, ...rest } = prev;
        return rest;
      });
      setNotice(`解析失败: ${p.error}`);
    });

    // 拖拽导入：窗口级文件拖放事件
    let unlisten: (() => void) | undefined;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "over") setDragging(true);
        else if (event.payload.type === "leave") setDragging(false);
        else if (event.payload.type === "drop") {
          setDragging(false);
          for (const p of event.payload.paths) handleImport(p);
        }
      })
      .then((fn) => {
        unlisten = fn;
      });
    return () => {
      unlisteners.forEach((fn) => fn());
      unlisten?.();
    };
  }, []);

  return (
    <div
      className={`flex h-full flex-col bg-[#f7f8fa] text-[#0b1326] ${
        dragging ? "ring-2 ring-inset ring-[#0b1326]/40" : ""
      }`}
    >
      {view.type === "reader" ? (
        <ReaderView
          docId={view.doc.id}
          title={view.doc.title}
          onBack={() => setView({ type: "list" })}
        />
      ) : (
        <>
          {/* 顶栏 */}
      <header className="flex h-14 shrink-0 items-center justify-between border-b border-black/5 bg-white px-6">
        <div className="flex items-center gap-3">
          <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-[#0b1326] text-sm font-semibold text-white">
            文
          </div>
          <h1 className="text-[15px] font-semibold tracking-tight">
            文献阅读台
          </h1>
          <span className="rounded-full bg-[#0b1326]/5 px-2.5 py-0.5 text-xs text-[#0b1326]/60">
            v0.1.0
          </span>
        </div>
        <div className="flex items-center gap-2 text-xs text-[#0b1326]/50">
          本地优先 · 学术文献加工流水线
        </div>
      </header>

      {/* 主导航 */}
      <nav className="flex h-11 shrink-0 items-center gap-1 border-b border-black/5 bg-white px-4 text-sm">
        {["文献库", "任务中心", "设置"].map((item, i) => (
          <button
            key={item}
            className={`rounded-md px-3 py-1.5 font-medium transition-colors ${
              i === 0
                ? "bg-[#0b1326] text-white"
                : "text-[#0b1326]/60 hover:bg-black/5"
            }`}
          >
            {item}
          </button>
        ))}
        <div className="ml-auto">
          <button
            onClick={pickFile}
            className="rounded-md bg-[#0b1326]/90 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-[#0b1326]"
          >
            + 导入 PDF
          </button>
        </div>
      </nav>

      {/* 通知 */}
      {notice && (
        <div className="mx-4 mt-3 rounded-lg border border-red-200 bg-red-50 px-4 py-2.5 text-sm text-red-700">
          {notice}
          <button
            className="ml-3 font-medium underline"
            onClick={() => setNotice(null)}
          >
            关闭
          </button>
        </div>
      )}

      {/* 内容区 */}
      <main className="flex-1 overflow-y-auto p-6">
        {docs.length === 0 ? (
          /* 空库引导 */
          <div className="flex h-full items-center justify-center">
            <div className="flex max-w-md flex-col items-center text-center">
              <div className="mb-5 flex h-16 w-16 items-center justify-center rounded-2xl border border-dashed border-black/20 bg-white text-2xl">
                📄
              </div>
              <h2 className="mb-2 text-lg font-semibold">导入第一篇文献</h2>
              <p className="mb-6 text-sm leading-relaxed text-[#0b1326]/55">
                支持 PDF 格式，导入后将自动解析版面、识别语言，
                完成翻译与学科范式拆解。
              </p>
              <div className="flex gap-3">
                <button
                  onClick={pickFile}
                  className="rounded-lg bg-[#0b1326] px-4 py-2 text-sm font-medium text-white transition-transform hover:scale-[1.02]"
                >
                  选择文件
                </button>
                <span className="rounded-lg border border-black/10 bg-white px-4 py-2 text-sm font-medium text-[#0b1326]/70">
                  或将 PDF 拖入窗口
                </span>
              </div>
            </div>
          </div>
        ) : (
          /* 文献列表 */
          <div className="mx-auto max-w-4xl">
            <div className="mb-4 text-sm text-[#0b1326]/50">
              共 {docs.length} 篇文献
            </div>
            <div className="space-y-2">
              {docs.map((d) => (
                <div
                  key={d.id}
                  onClick={() => openReader(d)}
                  className={`flex items-center gap-4 rounded-xl border border-black/5 bg-white px-5 py-4 transition-shadow hover:shadow-sm ${
                    d.status === "parsed" ? "cursor-pointer" : ""
                  }`}
                >
                  <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-[#0b1326]/5 text-lg">
                    📄
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-[15px] font-medium">
                      {d.title}
                    </div>
                    <div className="mt-0.5 truncate text-xs text-[#0b1326]/50">
                      {d.authors ?? "作者未知"}
                      {d.year ? ` · ${d.year}` : ""}
                    </div>
                  </div>
                  {d.language && (
                    <span className="rounded-full bg-blue-50 px-2 py-0.5 text-xs text-blue-700">
                      {d.language === "中文" ? "中" : "英"}
                    </span>
                  )}
                  <span
                    className={`rounded-full px-2.5 py-0.5 text-xs ${
                      d.status === "pending"
                        ? "bg-amber-50 text-amber-700"
                        : "bg-emerald-50 text-emerald-700"
                    }`}
                  >
                    {STATUS_LABEL[d.status] ?? d.status}
                  </span>
                  {parsing[d.id] ? (
                    <div className="flex w-40 flex-col items-end gap-1">
                      <span className="text-xs text-[#0b1326]/60">
                        {parsing[d.id].stage}{" "}
                        {Math.round(parsing[d.id].progress * 100)}%
                      </span>
                      <div className="h-1.5 w-full overflow-hidden rounded-full bg-black/5">
                        <div
                          className="h-full rounded-full bg-[#0b1326]/70 transition-all"
                          style={{ width: `${parsing[d.id].progress * 100}%` }}
                        />
                      </div>
                    </div>
                  ) : (
                    d.status === "pending" && (
                      <button
                        onClick={() => handleParse(d)}
                        className="rounded-lg bg-[#0b1326]/90 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-[#0b1326]"
                      >
                        解析
                      </button>
                    )
                  )}
                </div>
              ))}
            </div>
          </div>
        )}
        </main>
          </>
        )}
    </div>
  );
}

export default App;
