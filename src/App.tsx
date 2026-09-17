import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { ask, message, open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { listen } from "@tauri-apps/api/event";
import logo from "./assets/logo.png";
import ReaderView from "./ReaderView";
import SettingsView from "./SettingsView";
import NotesView from "./NotesView";
import TaskCenterView from "./TaskCenterView";
import HelpView from "./HelpView";

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
  read_status: string;
  created_at: string;
}

export type ThemePreset =
  | "default"
  | "minimal"
  | "dracula"
  | "blue-topaz"
  | "catppuccin"
  | "neon"
  | "twilight";

/** 带主页氛围层的主题：只在文献库页铺装饰，其它栏目仅继承色调 token */
const AMBIENT_THEMES: ThemePreset[] = ["neon", "twilight"];

const THEME_PRESETS: { id: ThemePreset; name: string }[] = [
  { id: "default", name: "默认" },
  { id: "minimal", name: "Minimal" },
  { id: "dracula", name: "Dracula" },
  { id: "blue-topaz", name: "Blue Topaz" },
  { id: "catppuccin", name: "Catppuccin" },
  { id: "neon", name: "Neon" },
  { id: "twilight", name: "Twilight" },
];

const STATUS_LABEL: Record<string, string> = {
  pending: "待解析",
  parsed: "已解析",
  translated: "已翻译",
  digested: "已拆解",
};

/** F7 阅读状态"左右扳机"开关：两段式，左=未读完，右=已读完，点击即时切换 */
function ReadToggle({
  value,
  onChange,
}: {
  value: string;
  onChange: (v: "unread" | "read") => void;
}) {
  return (
    <div className="flex items-center rounded-full border border-divider-strong bg-panel/70 p-0.5 text-[11px] leading-none">
      <button
        onClick={(e) => {
          e.stopPropagation();
          onChange("unread");
        }}
        className={`rounded-full px-2.5 py-1 transition-colors ${
          value === "unread"
            ? "bg-warning-bg font-medium text-warning-fg"
            : "text-primary/40 hover:text-primary/70"
        }`}
        title="标记为未读完"
      >
        未读
      </button>
      <button
        onClick={(e) => {
          e.stopPropagation();
          onChange("read");
        }}
        className={`rounded-full px-2.5 py-1 transition-colors ${
          value === "read"
            ? "bg-success-bg font-medium text-success-fg"
            : "text-primary/40 hover:text-primary/70"
        }`}
        title="标记为已读完"
      >
        已读
      </button>
    </div>
  );
}

interface TranslateErrorInfo {
  category: string;
  message: string;
  hint: string;
}

interface ParadigmResult {
  paradigm_id: string;
  paradigm_name: string;
  confidence: number;
  identification_basis: string[];
  cross_type: string;
  secondary_paradigms: {
    paradigm_id: string;
    paradigm_name: string;
    weight: number;
    basis: string[];
  }[];
  human_review_required: boolean;
  fallback_to_common: boolean;
  domestic: {
    category: string;
    code: string;
    discipline: string;
    confidence: number;
    basis: string[];
    secondary: { name: string; weight: number }[];
  };
  international: {
    supergroup: string;
    code: string;
    discipline: string;
    confidence: number;
    basis: string[];
    secondary: { name: string; weight: number }[];
  };
  directions: { name: string; weight: number }[];
}

interface FieldPlanResult {
  paradigm_id: string;
  paradigm_name: string;
  cross_type: string;
  combine_strategy: string;
  count: number;
  notes: string[];
  fields: {
    name: string;
    label: string;
    ftype: string;
    description: string;
    required: boolean;
    enum_values: string[];
    source: string;
  }[];
}

// 翻译失败统一弹「确定」对话框，附可能的问题解释
const showTranslateErrorDialog = (err: TranslateErrorInfo) => {
  const titleMap: Record<string, string> = {
    network: "网络连接失败",
    llm: "翻译失败（API 返回错误）",
    config: "翻译未启动",
    internal: "翻译失败",
  };
  return message(`${err.message}\n\n${err.hint}`, {
    title: titleMap[err.category] ?? "翻译失败",
    kind: "error",
    buttons: { ok: "确定" },
  }).catch(() => {});
};

function App() {
  const [docs, setDocs] = useState<Doc[]>([]);
  // 真实运行版本：从应用元数据读取。此前硬编码 "v0.3.1"，
  // 换版本后界面永远显示同一个号，无法确认实际在跑哪一份构建
  const [appVersion, setAppVersion] = useState("");
  useEffect(() => {
    getVersion()
      .then(setAppVersion)
      .catch(() => setAppVersion(""));
  }, []);
  const [readFilter, setReadFilter] = useState<"all" | "unread" | "read">("all");
  const [hasApi, setHasApi] = useState(true);
  const [loadingLib, setLoadingLib] = useState(true);
  const [libError, setLibError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const [parsing, setParsing] = useState<Record<string, { stage: string; progress: number }>>({});
  const [translating, setTranslating] = useState<
    Record<string, { stage: string; progress: number; detail: string; paused: boolean }>
  >({});
  const [tab, setTab] = useState<"文献库" | "任务中心" | "笔记" | "设置">("文献库");
  const [view, setView] = useState<
    | { type: "list" }
    | {
        type: "reader";
        doc: Doc;
        initialMode?: "original" | "translated" | "bilingual" | "digest";
      }
  >({ type: "list" });
  const [showHelp, setShowHelp] = useState(false);
  const [recog, setRecog] = useState<{ doc: Doc; result: ParadigmResult } | null>(null);
  const [recognizing, setRecognizing] = useState<string | null>(null);
  const [digesting, setDigesting] = useState<string | null>(null);
  const [digestProgress, setDigestProgress] = useState<
    Record<string, { stage: string; progress: number; detail: string }>
  >({});
  const [fieldPlan, setFieldPlan] = useState<FieldPlanResult | null>(null);
  const [showPlan, setShowPlan] = useState(false);
  const [planLoading, setPlanLoading] = useState(false);
  const [planError, setPlanError] = useState<string | null>(null);
  // M4.4 主题：优先读 localStorage，其次跟随系统，默认浅色
  const [theme, setTheme] = useState<"light" | "dark">(() => {
    const saved = localStorage.getItem("theme");
    if (saved === "light" || saved === "dark") return saved;
    return window.matchMedia?.("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  });
  const [themePreset, setThemePreset] = useState<ThemePreset>(() => {
    const saved = localStorage.getItem("theme-preset");
    return THEME_PRESETS.some((p) => p.id === saved) ? (saved as ThemePreset) : "default";
  });

  const openReader = (doc: Doc) => {
    if (doc.status === "parsed" || doc.status === "translated") setView({ type: "reader", doc });
  };

  const refresh = async () => {
    try {
      setLibError(null);
      setDocs(await invoke<Doc[]>("list_documents"));
    } catch (e) {
      setLibError(String(e));
    } finally {
      setLoadingLib(false);
    }
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

  // 清除某篇文献的处理缓存（解析 / 翻译 / 拆解产物），保留原始 PDF，可重新处理
  const handleClearCache = async (doc: Doc) => {
    const ok = await ask(
      `将清除《${doc.title}》的解析、翻译与拆解产物（原始 PDF 保留）。\n\n清除后需要重新解析与拆解，确定继续？`,
      { title: "清除处理缓存", kind: "warning" },
    );
    if (!ok) return;
    try {
      const r = await invoke<{ cleared: string[] }>("clear_document_cache", {
        docId: doc.id,
      });
      setNotice(
        r.cleared.length > 0
          ? `已清除《${doc.title}》的${r.cleared.join("、")}`
          : `《${doc.title}》没有可清除的缓存`,
      );
      await refresh();
    } catch (e) {
      setNotice(String(e));
    }
  };

  const handleToggleRead = async (doc: Doc, next: "unread" | "read") => {
    try {
      await invoke("set_read_status", { docId: doc.id, readStatus: next });
      setDocs((prev) =>
        prev.map((d) => (d.id === doc.id ? { ...d, read_status: next } : d)),
      );
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

  const handleTranslate = async (doc: Doc) => {
    // 中文论文的翻译方向是「中 → 英」，与直觉相反（通常期待译成中文），先确认再执行
    if (doc.language === "中文") {
      const ok = await ask(
        "该文献为中文论文，翻译将生成英文译文（中文 → 英文）。\n\n中文文献通常无需翻译即可直接「识别范式」与「拆解」，是否仍要翻译？",
        { title: "确认翻译方向", kind: "info" },
      );
      if (!ok) return;
    }
    try {
      setTranslating((t) => ({
        ...t,
        [doc.id]: { stage: "启动中", progress: 0, detail: "", paused: false },
      }));
      await invoke("start_translate", {
        docId: doc.id,
        direction: doc.language === "中文" ? "zh_to_en" : "en_to_zh",
      });
    } catch (e) {
      const msg = String(e);
      setTranslating((prev) => {
        const { [doc.id]: _drop, ...rest } = prev;
        return rest;
      });
      setNotice(`翻译失败: ${msg}`);
      void showTranslateErrorDialog({
        category: "config",
        message: msg,
        hint: "可能的原因：未配置默认 API，或 Base URL / API Key / 模型名填写不完整。请到「设置 → API 配置」检查后重试",
      });
    }
  };

  const handlePauseResume = async (doc: Doc, paused: boolean) => {
    try {
      await invoke(paused ? "resume_translate" : "pause_translate", {
        docId: doc.id,
      });
    } catch (e) {
      setNotice(String(e));
    }
  };

  // 获取字段拆解方案（M3.2 路由引擎），识别成功后联动调用，也可手动重试
  const fetchFieldPlan = async (doc: Doc) => {
    setPlanLoading(true);
    setPlanError(null);
    setFieldPlan(null);
    try {
      const plan = await invoke<FieldPlanResult>("get_field_plan", {
        docId: doc.id,
      });
      setFieldPlan(plan);
    } catch (e) {
      setPlanError(String(e));
    } finally {
      setPlanLoading(false);
    }
  };

  const handleRecognize = async (doc: Doc) => {
    setRecognizing(doc.id);
    try {
      const result = await invoke<ParadigmResult>("recognize_paradigm", {
        docId: doc.id,
      });
      setRecog({ doc, result });
      setShowPlan(false);
      void fetchFieldPlan(doc);
    } catch (e) {
      setNotice(String(e));
    } finally {
      setRecognizing(null);
    }
  };

  // 启动拆解（M3.3）：识别 → 字段方案 → 逐字段 LLM 拆解（引用锚定）
  const handleDigest = async (doc: Doc) => {
    setDigesting(doc.id);
    setDigestProgress((prev) => ({
      ...prev,
      [doc.id]: { stage: "准备", progress: 0, detail: "" },
    }));
    try {
      await invoke("start_digest", { docId: doc.id });
    } catch (e) {
      setDigesting(null);
      setDigestProgress((prev) => {
        const { [doc.id]: _drop, ...rest } = prev;
        return rest;
      });
      setNotice(String(e));
    }
  };

  // 打开拆解视图（阅读视图内「拆解」栏目，与原文/译文/双语同级）
  const handleOpenDigest = (doc: Doc) => {
    setView({ type: "reader", doc, initialMode: "digest" });
  };

  const pickFile = async () => {
    const path = await open({
      multiple: false,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (typeof path === "string") await handleImport(path);
  };

  // F9 首次启动引导：检测是否已配置 API（无配置时在空库页提示免费方案）
  useEffect(() => {
    invoke<unknown[]>("list_api_configs")
      .then((c) => setHasApi(c.length > 0))
      .catch(() => setHasApi(true));
  }, []);

  // M4.4 主题切换：挂载 .dark / data-theme、短暂过渡动画、写入 localStorage
  useEffect(() => {
    const root = document.documentElement;
    root.classList.add("theme-transition");
    root.classList.toggle("dark", theme === "dark");
    root.dataset.theme = themePreset;
    try {
      localStorage.setItem("theme", theme);
      localStorage.setItem("theme-preset", themePreset);
    } catch {
      // localStorage 不可用时静默降级（如无痕模式）
    }
    const timer = window.setTimeout(() => root.classList.remove("theme-transition"), 200);
    return () => window.clearTimeout(timer);
  }, [theme, themePreset]);

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

    // 翻译任务事件：进度 / 完成 / 失败
    register<{ doc_id: string; stage: string; progress: number; detail: string }>(
      "translate-progress",
      (p) => {
        setTranslating((prev) => {
          const cur = prev[p.doc_id];
          return {
            ...prev,
            [p.doc_id]: {
              stage: p.stage,
              progress: p.progress,
              detail: p.detail,
              paused: cur?.paused ?? false,
            },
          };
        });
      },
    );
    register<{ doc_id: string }>("translate-paused", (p) => {
      setTranslating((prev) =>
        prev[p.doc_id]
          ? { ...prev, [p.doc_id]: { ...prev[p.doc_id], paused: true } }
          : prev,
      );
    });
    register<{ doc_id: string }>("translate-resumed", (p) => {
      setTranslating((prev) =>
        prev[p.doc_id]
          ? { ...prev, [p.doc_id]: { ...prev[p.doc_id], paused: false } }
          : prev,
      );
    });
    register("translate-done", async () => {
      setTranslating({});
      setNotice(null);
      await refresh();
    });
    register<{
      doc_id: string;
      error: TranslateErrorInfo | string;
    }>("translate-failed", (p) => {
      setTranslating((prev) => {
        const { [p.doc_id]: _drop, ...rest } = prev;
        return rest;
      });
      const err: TranslateErrorInfo =
        typeof p.error === "string"
          ? { category: "internal", message: p.error, hint: "请稍后重试" }
          : p.error;
      setNotice(`翻译失败: ${err.message}`);
      void showTranslateErrorDialog(err);
    });

    // 拆解任务事件：进度 / 完成 / 失败
    register<{ doc_id: string; stage: string; progress: number; detail: string }>(
      "digest-progress",
      (p) => {
        setDigestProgress((prev) => ({
          ...prev,
          [p.doc_id]: { stage: p.stage, progress: p.progress, detail: p.detail },
        }));
      },
    );
    register("digest-done", async () => {
      setDigesting(null);
      setDigestProgress({});
      setNotice(null);
      await refresh();
    });
    register<{ doc_id: string; error: TranslateErrorInfo | string }>(
      "digest-failed",
      (p) => {
        setDigesting(null);
        setDigestProgress((prev) => {
          const { [p.doc_id]: _drop, ...rest } = prev;
          return rest;
        });
        const err: TranslateErrorInfo =
          typeof p.error === "string"
            ? { category: "internal", message: p.error, hint: "请稍后重试" }
            : p.error;
        setNotice(`拆解失败: ${err.message}`);
        void showTranslateErrorDialog(err);
      },
    );

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
      className={`flex h-full flex-col bg-canvas text-primary ${
        dragging ? "ring-2 ring-inset ring-primary/40" : ""
      }`}
    >
      {view.type === "reader" ? (
        <ReaderView
          docId={view.doc.id}
          title={view.doc.title}
          onBack={() => setView({ type: "list" })}
          initialMode={view.initialMode}
        />
      ) : (
        <>
          {/* 顶栏 */}
      <header className="flex h-14 shrink-0 items-center justify-between border-b border-divider bg-panel px-6">
        <div className="flex items-center gap-3">
          <img
            src={logo}
            alt="Paperlens"
            className="h-8 w-auto select-none"
            draggable={false}
          />
          <div className="flex flex-col leading-tight">
            <h1 className="text-[15px] font-semibold tracking-tight">
              Rd学术阅读器
            </h1>
            <span className="text-[10px] tracking-wide text-primary/40">
              Random Paperlens
            </span>
          </div>
          <span
            className="rounded-full bg-primary/5 px-2.5 py-0.5 text-xs text-primary/60"
            title="当前运行版本（取自应用元数据，非硬编码）"
          >
            v{appVersion || "…"}
          </span>
        </div>
        <div className="flex items-center gap-3 text-xs text-primary/50">
          <span className="hidden sm:inline">本地优先 · 学术文献加工流水线</span>
          <button
            onClick={() => setShowHelp(true)}
            title="使用帮助"
            aria-label="使用帮助"
            className="flex h-7 w-7 items-center justify-center rounded-md border border-divider-strong bg-panel/70 text-primary/60 transition-colors hover:bg-hover"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <circle cx="12" cy="12" r="10" />
              <path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3" />
              <path d="M12 17h.01" />
            </svg>
          </button>
          <button
            onClick={() => setTheme((t) => (t === "dark" ? "light" : "dark"))}
            title={theme === "dark" ? "切换到浅色模式" : "切换到深色模式"}
            aria-label="切换主题"
            className="flex h-7 w-7 items-center justify-center rounded-md border border-divider-strong bg-panel/70 text-primary/60 transition-colors hover:bg-hover"
          >
            {theme === "dark" ? (
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <circle cx="12" cy="12" r="4" />
                <path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41" />
              </svg>
            ) : (
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
              </svg>
            )}
          </button>
        </div>
      </header>

      {/* 主导航 */}
      <nav className="flex h-11 shrink-0 items-center gap-1 border-b border-divider bg-panel px-4 text-sm">
        {(["文献库", "任务中心", "笔记", "设置"] as const).map((item) => (
          <button
            key={item}
            onClick={() => setTab(item)}
            className={`rounded-md px-3 py-1.5 font-medium transition-colors ${
              tab === item
                ? "bg-primary text-primary-inverse"
                : "text-primary/60 hover:bg-hover"
            }`}
          >
            {item}
          </button>
        ))}
        <div className="ml-auto">
          <button
            onClick={pickFile}
            className="rounded-md bg-primary/90 px-3 py-1.5 text-xs font-medium text-primary-inverse transition-colors hover:bg-primary"
          >
            + 导入 PDF
          </button>
        </div>
      </nav>

      {/* 通知 */}
      {notice && (
        <div className="anim-slide-down mx-4 mt-3 rounded-lg border border-danger-border bg-danger-bg px-4 py-2.5 text-sm text-danger-fg">
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
        {/* 氛围层：只作用于文献库（列表）页，仅带氛围的主题会铺。
            阅读、笔记、设置等栏目只继承主题的色调 token，不铺这层装饰 */}
        {tab === "文献库" && AMBIENT_THEMES.includes(themePreset) && (
          <div className="pointer-events-none fixed inset-0 -z-10">
            <div
              className={`absolute inset-0 ${
                themePreset === "neon" ? "neon-aurora" : "twilight-aurora"
              }`}
            />
            <div
              className={`absolute inset-0 ${
                themePreset === "neon" ? "neon-grid" : "twilight-dots"
              }`}
            />
          </div>
        )}
        <div key={tab} className="anim-fade-in h-full">
        {tab === "设置" ? (
          <SettingsView themePreset={themePreset} onThemePreset={setThemePreset} />
        ) : tab === "笔记" ? (
          <NotesView />
        ) : tab === "任务中心" ? (
          <TaskCenterView />
        ) : loadingLib && docs.length === 0 ? (
          /* 文献库加载态 */
          <div className="flex h-full items-center justify-center gap-2.5 text-sm text-primary/45">
            <span className="spinner" />
            加载文献库…
          </div>
        ) : libError && docs.length === 0 ? (
          /* 文献库加载失败 */
          <div className="flex h-full items-center justify-center">
            <div className="rounded-xl border border-danger-border bg-danger-bg px-5 py-4 text-center">
              <div className="text-sm font-medium text-danger-fg">文献库加载失败</div>
              <div className="mt-1 max-w-md text-xs leading-relaxed text-danger-fg/80">
                {libError}
              </div>
              <button
                onClick={() => {
                  setLoadingLib(true);
                  void refresh();
                }}
                className="mt-3 rounded-lg bg-red-600/90 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-red-600"
              >
                重试
              </button>
            </div>
          </div>
        ) : docs.length === 0 ? (
          /* 空库引导 */
          <div className="flex h-full items-center justify-center">
            <div className="anim-slide-up flex max-w-md flex-col items-center text-center">
              <div className="mb-5 flex h-16 w-16 items-center justify-center rounded-2xl border border-dashed border-divider-bold bg-panel text-2xl">
                📄
              </div>
              <h2 className="mb-2 text-lg font-semibold">导入第一篇文献</h2>
              <p className="mb-6 text-sm leading-relaxed text-primary/55">
                支持 PDF 格式，导入后将自动解析版面、识别语言，
                完成翻译与学科范式拆解。
              </p>
              <div className="flex gap-3">
                <button
                  onClick={pickFile}
                  className="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-inverse transition-transform hover:scale-[1.02]"
                >
                  选择文件
                </button>
                <span className="rounded-lg border border-divider-strong bg-panel px-4 py-2 text-sm font-medium text-primary/70">
                  或将 PDF 拖入窗口
                </span>
              </div>
              {!hasApi && (
                <button
                  onClick={() => setTab("设置")}
                  className="mt-4 text-xs text-primary/55 underline transition-colors hover:text-primary"
                >
                  还没有配置 API？前往「设置 → 免费方案」免费开始 ↗
                </button>
              )}
            </div>
          </div>
        ) : (
          /* 文献列表 */
          <div className="mx-auto max-w-4xl">
            <div className="mb-4 flex items-center justify-between">
              <div className="text-sm text-primary/50">
                共 {docs.length} 篇文献
              </div>
              <div className="flex items-center gap-1 rounded-full border border-divider bg-panel p-0.5 text-xs">
                {(
                  [
                    ["all", "全部"],
                    ["unread", "未读完"],
                    ["read", "已读完"],
                  ] as const
                ).map(([k, label]) => (
                  <button
                    key={k}
                    onClick={() => setReadFilter(k)}
                    className={`rounded-full px-3 py-1 transition-colors ${
                      readFilter === k
                        ? "bg-primary text-primary-inverse"
                        : "text-primary/55 hover:bg-hover"
                    }`}
                  >
                    {label}
                  </button>
                ))}
              </div>
            </div>
            <div className="space-y-2">
              {docs
                .filter((d) => readFilter === "all" || d.read_status === readFilter)
                .map((d) => (
                <div
                  key={d.id}
                  onClick={() => openReader(d)}
                  onDoubleClick={() =>
                    setView({ type: "reader", doc: d, initialMode: "original" })
                  }
                  title="双击直接打开原文"
                  className={`anim-fade-in flex items-center gap-4 rounded-xl border border-divider bg-panel px-5 py-4 transition-shadow hover:shadow-sm ${
                    d.status === "parsed" || d.status === "translated"
                      ? "cursor-pointer"
                      : ""
                  }`}
                >
                  <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-primary/5 text-lg">
                    📄
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-[15px] font-medium">
                      {d.title}
                    </div>
                    <div className="mt-0.5 truncate text-xs text-primary/50">
                      {d.authors ?? "作者未知"}
                      {d.year ? ` · ${d.year}` : ""}
                    </div>
                  </div>
                  {d.language && (
                    <span className="rounded-full bg-info-bg px-2 py-0.5 text-xs text-info-fg">
                      {d.language === "中文" ? "中" : "英"}
                    </span>
                  )}
                  <span
                    className={`rounded-full px-2.5 py-0.5 text-xs ${
                      d.status === "pending"
                        ? "bg-warning-bg text-warning-fg"
                        : d.status === "translated"
                          ? "bg-info-bg text-info-fg"
                          : "bg-success-bg text-success-fg"
                    }`}
                  >
                    {STATUS_LABEL[d.status] ?? d.status}
                  </span>
                  {d.read_status === "unread" && (
                    <span className="rounded-full bg-warning-bg px-2 py-0.5 text-[11px] text-warning-fg">
                      未读完
                    </span>
                  )}
                  <ReadToggle
                    value={d.read_status}
                    onChange={(v) => handleToggleRead(d, v)}
                  />
                  {translating[d.id] ? (
                    <div className="flex w-52 flex-col items-end gap-1">
                      <div className="flex w-full items-center justify-between gap-2">
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            handlePauseResume(d, translating[d.id].paused);
                          }}
                          className={`rounded-md border px-2 py-0.5 text-[11px] transition-colors ${
                            translating[d.id].paused
                              ? "border-warning-border bg-warning-bg text-warning-fg hover:bg-warning-border"
                              : "border-divider-strong text-primary/70 hover:bg-hover"
                          }`}
                        >
                          {translating[d.id].paused ? "继续" : "暂停"}
                        </button>
                        <span
                          className={`text-xs ${
                            translating[d.id].paused
                              ? "text-warning-fg"
                              : "text-primary/60"
                          }`}
                        >
                          {translating[d.id].paused
                            ? "已暂停"
                            : `${translating[d.id].stage} ${Math.round(translating[d.id].progress * 100)}%`}
                        </span>
                      </div>
                      <div className="h-1.5 w-full overflow-hidden rounded-full bg-track">
                        <div
                          className={`h-full rounded-full transition-all ${
                            translating[d.id].paused
                              ? "bg-warning-bg0/70"
                              : "bg-sky-600/70"
                          }`}
                          style={{
                            width: `${translating[d.id].progress * 100}%`,
                          }}
                        />
                      </div>
                      <span className="text-[11px] text-primary/40">
                        {translating[d.id].detail}
                      </span>
                    </div>
                  ) : parsing[d.id] ? (
                    <div className="flex w-40 flex-col items-end gap-1">
                      <span className="text-xs text-primary/60">
                        {parsing[d.id].stage}{" "}
                        {Math.round(parsing[d.id].progress * 100)}%
                      </span>
                      <div className="h-1.5 w-full overflow-hidden rounded-full bg-track">
                        <div
                          className="h-full rounded-full bg-primary/70 transition-all"
                          style={{ width: `${parsing[d.id].progress * 100}%` }}
                        />
                      </div>
                    </div>
                  ) : digestProgress[d.id] ? (
                    <div className="flex w-52 flex-col items-end gap-1">
                      <span className="text-xs text-primary/60">
                        {digestProgress[d.id].stage}{" "}
                        {Math.round(digestProgress[d.id].progress * 100)}%
                      </span>
                      <div className="h-1.5 w-full overflow-hidden rounded-full bg-track">
                        <div
                          className="h-full rounded-full bg-violet-600/70 transition-all"
                          style={{ width: `${digestProgress[d.id].progress * 100}%` }}
                        />
                      </div>
                      <span className="max-w-[200px] truncate text-[11px] text-primary/40">
                        {digestProgress[d.id].detail}
                      </span>
                    </div>
                  ) : d.status === "pending" ? (
                    <button
                      onClick={() => handleParse(d)}
                      className="rounded-lg bg-primary/90 px-3 py-1.5 text-xs font-medium text-primary-inverse transition-colors hover:bg-primary"
                    >
                      解析
                    </button>
                  ) : d.status === "parsed" ? (
                    <div className="flex shrink-0 items-center gap-1.5">
                      <button
                        onClick={() => handleTranslate(d)}
                        title={
                          d.language === "中文" ? "中文论文将翻译为英文" : "翻译为中文"
                        }
                        className="rounded-lg bg-sky-600/90 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-sky-600"
                      >
                        {d.language === "中文" ? "翻译为英文" : "翻译"}
                      </button>
                      <button
                        onClick={() => handleRecognize(d)}
                        className="rounded-lg border border-divider-strong bg-panel px-3 py-1.5 text-xs font-medium text-primary/70 transition-colors hover:bg-hover"
                      >
                        {recognizing === d.id ? "识别中…" : "识别范式"}
                      </button>
                      <button
                        onClick={() => handleDigest(d)}
                        disabled={digesting === d.id}
                        className="rounded-lg border border-violet-200 bg-violet-50 px-3 py-1.5 text-xs font-medium text-violet-700 transition-colors hover:bg-violet-100 disabled:opacity-50"
                      >
                        拆解
                      </button>
                      <ClearCacheButton doc={d} onClick={() => handleClearCache(d)} />
                    </div>
                  ) : d.status === "translated" || d.status === "digested" ? (
                    <div className="flex shrink-0 items-center gap-1.5">
                      <button
                        onClick={() => handleRecognize(d)}
                        className="rounded-lg border border-divider-strong bg-panel px-3 py-1.5 text-xs font-medium text-primary/70 transition-colors hover:bg-hover"
                      >
                        {recognizing === d.id ? "识别中…" : "识别范式"}
                      </button>
                      <button
                        onClick={() => handleDigest(d)}
                        disabled={digesting === d.id}
                        className="rounded-lg border border-violet-200 bg-violet-50 px-3 py-1.5 text-xs font-medium text-violet-700 transition-colors hover:bg-violet-100 disabled:opacity-50"
                      >
                        拆解
                      </button>
                      {d.status === "digested" && (
                        <button
                          onClick={() => handleOpenDigest(d)}
                          className="rounded-lg bg-violet-600/90 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-violet-600"
                        >
                          查看拆解
                        </button>
                      )}
                      <ClearCacheButton doc={d} onClick={() => handleClearCache(d)} />
                    </div>
                  ) : null}
                </div>
              ))}
            </div>
          </div>
        )}
        </div>
        </main>
          </>
        )}

      {/* 范式识别结果弹窗 */}
      {recog && (
        <div
          className="anim-fade-in fixed inset-0 z-50 flex items-center justify-center bg-black/30 p-6"
          onClick={() => setRecog(null)}
        >
          <div
            className="anim-scale-in max-h-[85vh] w-full max-w-lg overflow-y-auto rounded-2xl bg-panel p-6 shadow-2xl"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-start justify-between gap-4">
              <div className="min-w-0">
                <div className="truncate text-sm font-semibold">{recog.doc.title}</div>
                <div className="mt-0.5 text-xs text-primary/50">范式识别结果</div>
              </div>
              <button
                onClick={() => setRecog(null)}
                className="shrink-0 rounded-md px-2 py-1 text-xs text-primary/50 hover:bg-hover"
              >
                ✕
              </button>
            </div>

            {/* 学科识别：国内 + 国际两套口径 */}
            <div className="mt-5 grid grid-cols-1 gap-3 sm:grid-cols-2">
              {/* 国内口径（2022 研究生目录） */}
              <div className="rounded-xl border border-info-border bg-info-bg/50 p-3.5">
                <div className="flex items-center gap-1.5">
                  <span className="text-[10px] font-medium text-info-fg">国内</span>
                  <span className="truncate rounded-full bg-blue-600 px-2.5 py-0.5 text-xs font-medium text-white">
                    {recog.result.domestic.discipline}
                  </span>
                </div>
                <div className="mt-1.5 text-[11px] text-primary/55">
                  {recog.result.domestic.category} · {recog.result.domestic.code}
                </div>
                <div className="mt-2 flex items-center gap-2 text-[11px] text-primary/55">
                  <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-info-border">
                    <div
                      className="h-full rounded-full bg-info-bg0/70"
                      style={{ width: `${recog.result.domestic.confidence * 100}%` }}
                    />
                  </div>
                  <span className="shrink-0">
                    {Math.round(recog.result.domestic.confidence * 100)}%
                  </span>
                </div>
                {recog.result.domestic.secondary.length > 0 && (
                  <div className="mt-2 flex flex-wrap gap-1">
                    {recog.result.domestic.secondary.map((s) => (
                      <span
                        key={s.name}
                        className="rounded-full border border-info-border bg-info-bg px-1.5 py-0.5 text-[10px] text-info-fg"
                      >
                        {s.name}
                      </span>
                    ))}
                  </div>
                )}
              </div>
              {/* 国际口径（ASJC / WoS） */}
              <div className="rounded-xl border border-violet-100 bg-violet-50/50 p-3.5">
                <div className="flex items-center gap-1.5">
                  <span className="text-[10px] font-medium text-violet-500">国际</span>
                  <span className="truncate rounded-full bg-violet-600 px-2.5 py-0.5 text-xs font-medium text-white">
                    {recog.result.international.discipline}
                  </span>
                </div>
                <div className="mt-1.5 text-[11px] text-primary/55">
                  {recog.result.international.supergroup} · {recog.result.international.code}
                </div>
                <div className="mt-2 flex items-center gap-2 text-[11px] text-primary/55">
                  <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-violet-100">
                    <div
                      className="h-full rounded-full bg-violet-500/70"
                      style={{ width: `${recog.result.international.confidence * 100}%` }}
                    />
                  </div>
                  <span className="shrink-0">
                    {Math.round(recog.result.international.confidence * 100)}%
                  </span>
                </div>
                {recog.result.international.secondary.length > 0 && (
                  <div className="mt-2 flex flex-wrap gap-1">
                    {recog.result.international.secondary.map((s) => (
                      <span
                        key={s.name}
                        className="rounded-full border border-violet-200 bg-violet-50 px-1.5 py-0.5 text-[10px] text-violet-700"
                      >
                        {s.name}
                      </span>
                    ))}
                  </div>
                )}
              </div>
            </div>

            {/* 识别依据（国内/国际命中词） */}
            {(recog.result.domestic.basis.length > 0 ||
              recog.result.international.basis.length > 0) && (
              <div className="mt-3 rounded-xl border border-divider bg-surface p-3.5 text-xs leading-relaxed text-primary/55">
                <div className="font-medium text-primary/70">识别依据</div>
                <div className="mt-1">
                  国内：{recog.result.domestic.basis.join(" / ")}
                </div>
                <div className="mt-0.5">
                  国际：{recog.result.international.basis.join(" / ")}
                </div>
              </div>
            )}

            {/* 研究方向 */}
            {recog.result.directions.length > 0 && (
              <div className="mt-3 rounded-xl border border-divider bg-surface p-3.5">
                <div className="mb-1.5 text-xs font-medium text-primary/70">研究方向</div>
                <div className="flex flex-wrap gap-1.5">
                  {recog.result.directions.map((d) => (
                    <span
                      key={d.name}
                      className="rounded-full border border-warning-border bg-warning-bg px-2 py-0.5 text-xs text-warning-fg"
                    >
                      {d.name} · {Math.round(d.weight * 100)}%
                    </span>
                  ))}
                </div>
              </div>
            )}

            {/* 字段拆解方案（M3.2 路由引擎） */}
            <div className="mt-3 rounded-xl border border-divider bg-surface p-3.5">
              <button
                onClick={() => setShowPlan(!showPlan)}
                className="flex w-full items-center justify-between text-left"
              >
                <span className="text-xs font-medium text-primary/70">
                  字段拆解方案
                  {fieldPlan && (
                    <span className="ml-2 rounded-full bg-primary px-2 py-0.5 text-[10px] font-medium text-primary-inverse">
                      {fieldPlan.count} 项
                    </span>
                  )}
                </span>
                <span className="text-xs text-primary/40">
                  {showPlan ? "收起 ▲" : "展开 ▼"}
                </span>
              </button>
              {showPlan &&
                (planLoading ? (
                  <div className="mt-2.5 flex items-center gap-2 text-[11px] text-primary/40">
                    <span className="spinner" />
                    方案生成中…
                  </div>
                ) : planError ? (
                  <div className="mt-2.5 rounded-lg border border-danger-border bg-danger-bg p-2.5 text-[11px] text-danger-fg">
                    <div className="leading-relaxed">方案获取失败：{planError}</div>
                    <button
                      onClick={() => void fetchFieldPlan(recog.doc)}
                      className="mt-1.5 rounded-md bg-red-600/90 px-2.5 py-1 text-[10px] font-medium text-white transition-colors hover:bg-red-600"
                    >
                      重试
                    </button>
                  </div>
                ) : fieldPlan ? (
                  <div className="mt-2.5 space-y-3">
                    <div className="text-[11px] text-primary/55">
                      合并策略：{fieldPlan.combine_strategy}（{fieldPlan.cross_type}）
                    </div>
                    {fieldPlan.notes.length > 0 && (
                      <div className="rounded-lg bg-warning-bg p-2 text-[11px] text-warning-fg">
                        {fieldPlan.notes.map((n, i) => (
                          <div key={i}>{n}</div>
                        ))}
                      </div>
                    )}
                    <div className="max-h-64 space-y-2 overflow-y-auto pr-1">
                      {fieldPlan.fields.map((fld) => (
                        <div key={fld.name} className="rounded-lg bg-panel p-2.5">
                          <div className="flex items-center gap-1.5">
                            <span className="text-xs font-medium">{fld.label}</span>
                            {fld.required && (
                              <span className="rounded bg-danger-bg px-1 text-[9px] text-danger-fg">
                                必填
                              </span>
                            )}
                            <span className="ml-auto shrink-0 rounded border border-divider-strong px-1 text-[9px] text-primary/45">
                              {fld.ftype}
                            </span>
                          </div>
                          <div className="mt-0.5 text-[10px] text-primary/45">{fld.source}</div>
                          {fld.description && (
                            <div className="mt-0.5 text-[11px] text-primary/60">
                              {fld.description}
                            </div>
                          )}
                          {fld.enum_values.length > 0 && (
                            <div className="mt-1 flex flex-wrap gap-1">
                              {fld.enum_values.map((v) => (
                                <span
                                  key={v}
                                  className="rounded bg-hover px-1.5 py-0.5 text-[9px] text-primary/55"
                                >
                                  {v}
                                </span>
                              ))}
                            </div>
                          )}
                        </div>
                      ))}
                    </div>
                  </div>
                ) : (
                  <div className="mt-2 text-[11px] text-primary/40">暂无方案</div>
                ))}
            </div>

            <div className="mt-4 rounded-xl border border-divider bg-surface p-4">
              <div className="flex flex-wrap items-center gap-1.5">
                <span className="rounded-full bg-primary px-2.5 py-0.5 text-xs font-medium text-primary-inverse">
                  {recog.result.paradigm_name}
                </span>
                <span className="rounded-full bg-info-bg px-2 py-0.5 text-xs text-info-fg">
                  {recog.result.cross_type}
                </span>
                {recog.result.human_review_required && (
                  <span className="rounded-full bg-warning-bg px-2 py-0.5 text-xs text-warning-fg">
                    需人工确认
                  </span>
                )}
                {recog.result.fallback_to_common && (
                  <span className="rounded-full bg-warning-bg px-2 py-0.5 text-xs text-warning-fg">
                    回退通用字段
                  </span>
                )}
              </div>
              <div className="mt-3">
                <div className="mb-1 flex items-center justify-between text-xs text-primary/55">
                  <span>识别置信度</span>
                  <span>{Math.round(recog.result.confidence * 100)}%</span>
                </div>
                <div className="h-2 w-full overflow-hidden rounded-full bg-track">
                  <div
                    className="h-full rounded-full bg-sky-600/70"
                    style={{ width: `${recog.result.confidence * 100}%` }}
                  />
                </div>
              </div>
            </div>

            <div className="mt-4">
              <div className="mb-1.5 text-xs font-medium text-primary/55">识别依据</div>
              <ul className="space-y-1.5">
                {recog.result.identification_basis.map((b, i) => (
                  <li key={i} className="flex items-start gap-2 text-sm text-primary/75">
                    <span className="mt-1.5 h-1 w-1 shrink-0 rounded-full bg-primary/40" />
                    {b}
                  </li>
                ))}
              </ul>
            </div>

            {recog.result.cross_type !== "单学科" ? (
              <div className="mt-4 rounded-xl border border-info-border bg-info-bg/60 p-4">
                <div className="mb-2 text-xs font-medium text-info-fg">
                  交叉构成（{recog.result.cross_type}）
                </div>
                <div className="flex flex-wrap items-center gap-x-2 gap-y-1.5 text-sm">
                  <span className="rounded-md bg-panel px-2 py-0.5 font-medium text-primary">
                    {recog.result.paradigm_name}
                  </span>
                  {recog.result.secondary_paradigms.map((s) => (
                    <span key={s.paradigm_id} className="flex items-center gap-x-2">
                      <span className="text-primary/40">×</span>
                      <span className="rounded-md bg-panel px-2 py-0.5 text-primary/70">
                        {s.paradigm_name}（{Math.round(s.weight * 100)}%）
                      </span>
                    </span>
                  ))}
                </div>
              </div>
            ) : recog.result.secondary_paradigms.length > 0 ? (
              <div className="mt-4">
                <div className="mb-1.5 text-xs font-medium text-primary/55">
                  次要范式（交叉候选）
                </div>
                <div className="flex flex-wrap gap-1.5">
                  {recog.result.secondary_paradigms.map((s) => (
                    <span
                      key={s.paradigm_id}
                      className="rounded-full border border-divider-strong px-2.5 py-0.5 text-xs text-primary/65"
                    >
                      {s.paradigm_name} · {Math.round(s.weight * 100)}%
                    </span>
                  ))}
                </div>
              </div>
            ) : null}

            <div className="mt-5 flex justify-end">
              <button
                onClick={() => setRecog(null)}
                className="rounded-lg bg-primary px-4 py-1.5 text-xs font-medium text-primary-inverse hover:bg-primary/90"
              >
                知道了
              </button>
            </div>
          </div>
        </div>
      )}

      {/* 使用帮助弹窗 */}
      {showHelp && <HelpView onClose={() => setShowHelp(false)} />}
    </div>
  );
}

/** 清除处理缓存按钮：把已解析/翻译/拆解的文献重置回可重新处理的状态 */
function ClearCacheButton({ doc, onClick }: { doc: Doc; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      title={`清除《${doc.title}》的解析/翻译/拆解产物，可重新处理（保留原始 PDF）`}
      className="rounded-lg border border-divider-strong px-2.5 py-1.5 text-xs text-primary/45 transition-colors hover:bg-hover hover:text-primary/75"
    >
      重置
    </button>
  );
}

export default App;
