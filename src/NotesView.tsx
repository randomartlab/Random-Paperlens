import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { appDataDir, join } from "@tauri-apps/api/path";
import { open, save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

interface NoteMeta {
  name: string;
  updated_at: string;
  size: number;
}

function NotesView() {
  const [notes, setNotes] = useState<NoteMeta[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [content, setContent] = useState("");
  const [dirty, setDirty] = useState(false);
  const [mode, setMode] = useState<"edit" | "preview">("edit");
  const [notice, setNotice] = useState<string | null>(null);
  const [newName, setNewName] = useState("");
  const [saving, setSaving] = useState(false);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // 批量管理状态
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [showReplace, setShowReplace] = useState(false);
  const [replaceSearch, setReplaceSearch] = useState("");
  const [replaceWith, setReplaceWith] = useState("");
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");

  const refresh = useCallback(async () => {
    try {
      setNotes(await invoke<NoteMeta[]>("list_notes"));
    } catch (e) {
      setNotice(String(e));
    }
  }, []);

  useEffect(() => {
    refresh().catch(() => {});
  }, [refresh]);

  // 离开当前笔记或组件卸载前自动保存
  const persist = useCallback(
    async (name: string, body: string) => {
      if (!name || body === undefined) return;
      try {
        await invoke("save_note", { name, content: body });
        setDirty(false);
        await refresh();
      } catch (e) {
        setNotice(String(e));
      }
    },
    [refresh],
  );

  const openNote = async (name: string) => {
    if (dirty && selected && saveTimer.current) {
      clearTimeout(saveTimer.current);
      await persist(selected, content);
    }
    try {
      const r = await invoke<{ name: string; content: string }>("read_note", { name });
      setSelected(r.name);
      setContent(r.content);
      setDirty(false);
      setMode("edit");
      setNotice(null);
    } catch (e) {
      setNotice(String(e));
    }
  };

  const handleChange = (v: string) => {
    setContent(v);
    setDirty(true);
    if (saveTimer.current) clearTimeout(saveTimer.current);
    // 600ms 防抖自动保存（纯本地文件，随时可落盘）
    saveTimer.current = setTimeout(() => {
      if (selected) {
        void persist(selected, v);
      }
    }, 600);
  };

  const createNote = async () => {
    const name = newName.trim();
    if (!name) {
      setNotice("请输入笔记名称");
      return;
    }
    try {
      const n = await invoke<string>("save_note", { name, content: `# ${name}\n\n` });
      setNewName("");
      setNotice(null);
      await refresh();
      await openNote(n);
    } catch (e) {
      setNotice(String(e));
    }
  };

  const removeNote = async (name: string) => {
    if (!window.confirm(`确定删除笔记「${name}」？`)) return;
    try {
      await invoke("delete_note", { name });
      if (selected === name) {
        setSelected(null);
        setContent("");
        setDirty(false);
      }
      setNotice(null);
      await refresh();
    } catch (e) {
      setNotice(String(e));
    }
  };

  const exportNote = async (format: "md" | "html") => {
    if (!selected) return;
    try {
      const out = await save({
        defaultPath: `${selected}.${format}`,
        filters: [
          { name: format === "md" ? "Markdown" : "HTML", extensions: [format] },
        ],
      });
      if (!out) return; // 用户取消
      await invoke("export_note", { name: selected, format, outPath: out });
      setNotice(`已导出：${out}`);
    } catch (e) {
      setNotice(String(e));
    }
  };

  const printPdf = async () => {
    if (!selected) return;
    try {
      await invoke("print_note", { name: selected });
      setNotice("已打开打印窗口，可选择「存储为 PDF」");
    } catch (e) {
      setNotice(String(e));
    }
  };

  const revealDir = async () => {
    try {
      // 用 Tauri 的 join 按平台生成分隔符（Windows 需原生反斜杠，explorer 对正斜杠不友好）
      const dir = await join(await appDataDir(), "notes");
      await revealItemInDir(dir);
    } catch (e) {
      setNotice(String(e));
    }
  };

  // —— 批量管理 ——
  const toggleChecked = (name: string) => {
    setChecked((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  };

  const toggleAll = () => {
    setChecked((prev) =>
      prev.size === notes.length && notes.length > 0
        ? new Set()
        : new Set(notes.map((n) => n.name)),
    );
  };

  const removeChecked = async () => {
    const names = [...checked];
    if (names.length === 0) return;
    if (!window.confirm(`确定删除选中的 ${names.length} 篇笔记？此操作不可恢复。`)) return;
    setBusy(true);
    try {
      await invoke("delete_notes_batch", { names });
      if (selected && checked.has(selected)) {
        setSelected(null);
        setContent("");
        setDirty(false);
      }
      setChecked(new Set());
      setNotice(null);
      await refresh();
    } catch (e) {
      setNotice(String(e));
    } finally {
      setBusy(false);
    }
  };

  const exportChecked = async (format: "md" | "html") => {
    const names = [...checked];
    if (names.length === 0) return;
    const outDir = await open({
      directory: true,
      title: `选择导出目录（${names.length} 篇 · ${format.toUpperCase()}）`,
    });
    if (typeof outDir !== "string") return; // 用户取消
    setBusy(true);
    try {
      const files = await invoke<string[]>("export_notes_batch", { names, format, outDir });
      setNotice(`已导出 ${files.length} 篇到所选目录`);
    } catch (e) {
      setNotice(String(e));
    } finally {
      setBusy(false);
    }
  };

  const replaceChecked = async () => {
    const names = [...checked];
    if (names.length === 0) return;
    const search = replaceSearch.trim();
    if (!search) {
      setNotice("查找内容不能为空");
      return;
    }
    setBusy(true);
    try {
      const results = await invoke<{ name: string; count: number }[]>("replace_in_notes", {
        names,
        search,
        replace: replaceWith,
      });
      const total = results.reduce((s, r) => s + r.count, 0);
      setReplaceSearch("");
      setReplaceWith("");
      setShowReplace(false);
      setNotice(`替换完成：共 ${total} 处（${results.filter((r) => r.count > 0).length} 篇受影响）`);
      await refresh();
      if (selected) {
        const r = await invoke<{ name: string; content: string }>("read_note", {
          name: selected,
        });
        setContent(r.content);
        setDirty(false);
      }
    } catch (e) {
      setNotice(String(e));
    } finally {
      setBusy(false);
    }
  };

  const startRename = (name: string) => {
    setRenaming(name);
    setRenameValue(name);
  };

  const commitRename = async () => {
    if (!renaming) return;
    const oldName = renaming;
    const target = renameValue.trim();
    setRenaming(null);
    if (!target || target === oldName) return;
    try {
      const n = await invoke<string>("rename_note", { oldName, newName: target });
      if (selected === oldName) setSelected(n);
      setNotice(null);
      await refresh();
    } catch (e) {
      setNotice(String(e));
    }
  };

  const btnCls =
    "rounded-md border border-divider-strong px-2.5 py-1 text-xs text-primary/70 hover:bg-hover";
  const primaryBtnCls =
    "rounded-md bg-primary/90 px-3 py-1 text-xs font-medium text-primary-inverse hover:bg-primary";

  return (
    <div className="mx-auto flex h-full max-w-5xl gap-4">
      {/* 左侧列表 */}
      <div className="flex w-60 shrink-0 flex-col rounded-xl border border-divider bg-panel">
        <div className="flex items-center justify-between border-b border-divider px-3 py-2.5">
          <span className="text-sm font-semibold">笔记</span>
          <button onClick={revealDir} className="text-[11px] text-primary/55 hover:text-primary" title="在文件系统中打开笔记目录">
            打开目录
          </button>
        </div>
        <div className="border-b border-divider p-2">
          <div className="flex gap-1.5">
            <input
              className="w-full min-w-0 rounded-md border border-divider-strong bg-panel px-2 py-1 text-xs outline-none focus:border-primary/50"
              placeholder="新笔记名称"
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && createNote()}
            />
            <button
              onClick={createNote}
              className="shrink-0 rounded-md bg-primary/90 px-2 py-1 text-xs font-medium text-primary-inverse hover:bg-primary"
            >
              +
            </button>
          </div>
        </div>
        {/* 批量操作栏 */}
        <div className="flex items-center gap-1.5 border-b border-divider px-2 py-1.5">
          <button
            onClick={toggleAll}
            className="shrink-0 rounded px-1.5 py-0.5 text-[11px] text-primary/60 hover:bg-hover hover:text-primary"
          >
            {checked.size === notes.length && notes.length > 0 ? "取消全选" : "全选"}
          </button>
          {checked.size > 0 && (
            <span className="shrink-0 text-[11px] text-primary/45">已选 {checked.size}</span>
          )}
          <div className="ml-auto flex items-center gap-1">
            <button
              onClick={() => void exportChecked("md")}
              disabled={checked.size === 0 || busy}
              className="rounded border border-divider-strong px-1.5 py-0.5 text-[11px] text-primary/70 hover:bg-hover disabled:opacity-40"
              title="批量导出 Markdown 到目录"
            >
              导出 md
            </button>
            <button
              onClick={() => void exportChecked("html")}
              disabled={checked.size === 0 || busy}
              className="rounded border border-divider-strong px-1.5 py-0.5 text-[11px] text-primary/70 hover:bg-hover disabled:opacity-40"
              title="批量导出 HTML 到目录"
            >
              导出 html
            </button>
            <button
              onClick={() => setShowReplace(!showReplace)}
              disabled={checked.size === 0 || busy}
              className={`rounded border px-1.5 py-0.5 text-[11px] hover:bg-hover disabled:opacity-40 ${
                showReplace
                  ? "border-warning-border bg-warning-bg text-warning-fg"
                  : "border-divider-strong text-warning-fg"
              }`}
              title="对选中笔记批量查找替换"
            >
              替换
            </button>
            <button
              onClick={() => void removeChecked()}
              disabled={checked.size === 0 || busy}
              className="rounded border border-divider-strong px-1.5 py-0.5 text-[11px] text-danger-fg hover:bg-danger-bg disabled:opacity-40"
              title="批量删除选中笔记"
            >
              删除
            </button>
          </div>
        </div>
        {/* 批量替换面板 */}
        {showReplace && (
          <div className="border-b border-divider bg-warning-bg/25 p-2">
            <div className="flex gap-1.5">
              <input
                className="w-full min-w-0 rounded-md border border-divider-strong bg-panel px-2 py-1 text-xs outline-none focus:border-warning-border"
                placeholder="查找内容"
                value={replaceSearch}
                onChange={(e) => setReplaceSearch(e.target.value)}
              />
              <input
                className="w-full min-w-0 rounded-md border border-divider-strong bg-panel px-2 py-1 text-xs outline-none focus:border-warning-border"
                placeholder="替换为"
                value={replaceWith}
                onChange={(e) => setReplaceWith(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && void replaceChecked()}
              />
            </div>
            <div className="mt-1.5 flex items-center justify-between">
              <span className="text-[11px] text-primary/45">将对选中的 {checked.size} 篇执行纯文本替换</span>
              <div className="flex gap-1.5">
                <button
                  onClick={() => setShowReplace(false)}
                  className="rounded border border-divider-strong px-2 py-0.5 text-[11px] text-primary/70 hover:bg-hover"
                >
                  取消
                </button>
                <button
                  onClick={() => void replaceChecked()}
                  disabled={busy}
                  className="rounded bg-warning-fg px-2 py-0.5 text-[11px] font-medium text-warning-bg hover:opacity-90 disabled:opacity-40"
                >
                  应用替换
                </button>
              </div>
            </div>
          </div>
        )}
        <div className="flex-1 overflow-y-auto p-1.5">
          {notes.length === 0 ? (
            <div className="px-2 py-6 text-center text-xs text-primary/45">
              还没有笔记。新建一篇，或从阅读器中复制摘录粘贴进来。
            </div>
          ) : (
            notes.map((n) => (
              <div
                key={n.name}
                className={`group flex cursor-pointer items-center gap-1.5 rounded-lg px-2 py-2 transition-colors ${
                  selected === n.name ? "bg-primary/10" : "hover:bg-hover"
                }`}
              >
                <input
                  type="checkbox"
                  checked={checked.has(n.name)}
                  onChange={() => toggleChecked(n.name)}
                  onClick={(e) => e.stopPropagation()}
                  className="h-3.5 w-3.5 shrink-0 accent-[var(--color-primary)]"
                  title="选择以批量操作"
                />
                {renaming === n.name ? (
                  <input
                    autoFocus
                    value={renameValue}
                    onChange={(e) => setRenameValue(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") void commitRename();
                      if (e.key === "Escape") setRenaming(null);
                    }}
                    onBlur={() => void commitRename()}
                    onClick={(e) => e.stopPropagation()}
                    className="w-full min-w-0 rounded border border-primary/50 bg-panel px-1.5 py-0.5 text-[13px] font-medium outline-none"
                  />
                ) : (
                  <button onClick={() => openNote(n.name)} className="min-w-0 flex-1 text-left">
                    <div className="truncate text-[13px] font-medium">{n.name}</div>
                    <div className="mt-0.5 text-[11px] text-primary/40">
                      {new Date(n.updated_at).toLocaleString()}
                    </div>
                  </button>
                )}
                {renaming !== n.name && (
                  <div className="hidden shrink-0 items-center gap-0.5 group-hover:flex">
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        startRename(n.name);
                      }}
                      className="rounded px-1 py-0.5 text-[11px] text-primary/60 hover:bg-hover hover:text-primary"
                      title="重命名笔记"
                    >
                      改名
                    </button>
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        removeNote(n.name);
                      }}
                      className="rounded px-1 py-0.5 text-[11px] text-danger-fg hover:bg-danger-bg"
                      title="删除笔记"
                    >
                      删
                    </button>
                  </div>
                )}
              </div>
            ))
          )}
        </div>
      </div>

      {/* 右侧编辑区 */}
      <div className="flex min-w-0 flex-1 flex-col rounded-xl border border-divider bg-panel">
        {!selected ? (
          <div className="flex h-full items-center justify-center">
            <div className="max-w-sm text-center">
              <div className="mb-3 text-2xl">📝</div>
              <div className="text-sm text-primary/50">
                选择左侧笔记开始编辑，或在阅读器中高亮文字后右键「复制为摘录」粘贴进来。
              </div>
            </div>
          </div>
        ) : (
          <>
            <div className="flex items-center justify-between border-b border-divider px-4 py-2">
              <div className="min-w-0">
                <span className="truncate text-sm font-semibold">{selected}</span>
                <span className="ml-2 text-[11px] text-primary/40">.md · 本地文件</span>
                {dirty && (
                  <span className="ml-2 text-[11px] text-warning-fg">未保存…</span>
                )}
              </div>
              <div className="flex shrink-0 items-center gap-1.5">
                <button
                  onClick={() => setMode("edit")}
                  className={`rounded-md px-2.5 py-1 text-xs ${
                    mode === "edit"
                      ? "bg-primary/10 font-medium text-primary"
                      : "text-primary/55 hover:bg-hover"
                  }`}
                >
                  编辑
                </button>
                <button
                  onClick={() => {
                    if (dirty && selected) void persist(selected, content);
                    setMode("preview");
                  }}
                  className={`rounded-md px-2.5 py-1 text-xs ${
                    mode === "preview"
                      ? "bg-primary/10 font-medium text-primary"
                      : "text-primary/55 hover:bg-hover"
                  }`}
                >
                  预览
                </button>
                <div className="mx-1 h-4 w-px bg-divider-strong" />
                <button onClick={() => exportNote("md")} className={btnCls}>
                  导出 md
                </button>
                <button onClick={() => exportNote("html")} className={btnCls}>
                  导出 html
                </button>
                <button onClick={printPdf} className={btnCls}>
                  导出 PDF
                </button>
                <button
                  onClick={async () => {
                    if (!selected) return;
                    setSaving(true);
                    await persist(selected, content);
                    setSaving(false);
                    setNotice("已保存");
                  }}
                  disabled={saving}
                  className={primaryBtnCls}
                >
                  {saving ? "保存中…" : "保存"}
                </button>
              </div>
            </div>
            {notice && (
              <div className="border-b border-divider bg-warning-bg px-4 py-1.5 text-xs text-warning-fg">
                {notice}
                <button className="ml-2 underline" onClick={() => setNotice(null)}>
                  关闭
                </button>
              </div>
            )}
            {mode === "edit" ? (
              <textarea
                value={content}
                onChange={(e) => handleChange(e.target.value)}
                spellCheck={false}
                className="min-h-0 flex-1 resize-none bg-panel p-4 font-mono text-[13px] leading-relaxed text-primary outline-none placeholder:text-primary/30"
                placeholder={"支持 Markdown：标题 #、列表 -、引用 >、待办 [ ]、代码 ```\n\n可在阅读器中选中文字 → 右键「复制为摘录」后粘贴到这里。"}
              />
            ) : (
              <div className="min-h-0 flex-1 overflow-y-auto p-5">
                <article className="prose prose-sm max-w-none dark:prose-invert">
                  <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
                </article>
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}

export default NotesView;
