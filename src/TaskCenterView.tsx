import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface TaskItem {
  id: string;
  doc_id: string;
  title: string;
  type: string;
  status: "running" | "done" | "failed";
  progress: number;
  stage: string;
  detail: string;
  error: string;
  created_at: string;
  updated_at: string;
}

type TaskFilter = "all" | "running" | "done" | "failed";

const TYPE_LABEL: Record<string, string> = {
  parse: "解析",
  translate: "翻译",
  digest: "拆解",
};

const TYPE_ICON: Record<string, string> = {
  parse: "📄",
  translate: "🌐",
  digest: "🧩",
};

const STATUS_LABEL: Record<string, string> = {
  running: "进行中",
  done: "已完成",
  failed: "失败",
};

function formatTime(raw: string): string {
  if (!raw) return "-";
  let ms = Number(raw);
  if (Number.isNaN(ms) || raw.length < 12) {
    const parsed = Date.parse(raw);
    if (!Number.isNaN(parsed)) ms = parsed;
    else return raw.slice(0, 19).replace("T", " ");
  }
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export default function TaskCenterView() {
  const [tasks, setTasks] = useState<TaskItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [filter, setFilter] = useState<TaskFilter>("all");
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setError(null);
      setTasks(await invoke<TaskItem[]>("list_tasks"));
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const unlisteners: Array<() => void> = [];
    const events = [
      "parse-progress",
      "parse-done",
      "parse-failed",
      "translate-progress",
      "translate-done",
      "translate-failed",
      "digest-progress",
      "digest-done",
      "digest-failed",
    ];
    events.forEach((name) => {
      listen(name, () => void refresh()).then((fn) => unlisteners.push(fn));
    });
    const timer = window.setInterval(() => void refresh(), 4000);
    return () => {
      unlisteners.forEach((fn) => fn());
      window.clearInterval(timer);
    };
  }, [refresh]);

  const hasRunning = tasks.some((t) => t.status === "running");

  const handleClearFinished = async () => {
    try {
      await invoke("clear_finished_tasks");
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  };

  const handleDelete = async (task: TaskItem) => {
    try {
      await invoke("delete_task", { taskId: task.id });
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  };

  const filtered = tasks.filter((t) => filter === "all" || t.status === filter);

  const statusBadge = (status: string) => {
    const cls =
      status === "running"
        ? "bg-info-bg text-info-fg"
        : status === "done"
          ? "bg-success-bg text-success-fg"
          : "bg-danger-bg text-danger-fg";
    return (
      <span className={`rounded-full px-2.5 py-0.5 text-xs ${cls}`}>
        {STATUS_LABEL[status] ?? status}
      </span>
    );
  };

  const progressBar = (task: TaskItem) => {
    const pct = Math.min(100, Math.max(0, Math.round(task.progress * 100)));
    const color =
      task.status === "done"
        ? "bg-success-fg/80"
        : task.status === "failed"
          ? "bg-danger-fg/80"
          : "bg-sky-600/70";
    return (
      <div className="flex w-40 flex-col items-end gap-1">
        <div className="flex w-full items-center justify-between gap-2 text-xs">
          <span className="truncate text-primary/60">{task.stage || "-"}</span>
          <span className="shrink-0 text-primary/70">{pct}%</span>
        </div>
        <div className="h-1.5 w-full overflow-hidden rounded-full bg-track">
          <div
            className={`h-full rounded-full transition-all ${color}`}
            style={{ width: `${pct}%` }}
          />
        </div>
        {task.detail && (
          <span className="max-w-full truncate text-[11px] text-primary/40">
            {task.detail}
          </span>
        )}
      </div>
    );
  };

  return (
    <div className="mx-auto max-w-4xl">
      <div className="mb-4 flex items-center justify-between">
        <div>
          <div className="text-base font-semibold">任务中心</div>
          <div className="mt-0.5 text-xs text-primary/50">
            解析 / 翻译 / 拆解任务统一管理，记录保存在本地数据库
          </div>
        </div>
        <div className="flex items-center gap-2">
          {hasRunning && (
            <span className="flex items-center gap-1.5 text-xs text-primary/50">
              <span className="spinner" />
              有任务进行中
            </span>
          )}
          <button
            onClick={() => void handleClearFinished()}
            disabled={!tasks.some((t) => t.status === "done" || t.status === "failed")}
            className="rounded-md border border-divider-strong bg-panel px-3 py-1.5 text-xs font-medium text-primary/70 transition-colors hover:bg-hover disabled:cursor-not-allowed disabled:opacity-40"
          >
            清空已完成 / 失败
          </button>
        </div>
      </div>

      <div className="mb-3 flex items-center gap-1 rounded-full border border-divider bg-panel p-0.5 text-xs">
        {(
          [
            ["all", "全部"],
            ["running", "进行中"],
            ["done", "已完成"],
            ["failed", "失败"],
          ] as [TaskFilter, string][]
        ).map(([k, label]) => {
          const count =
            k === "all"
              ? tasks.length
              : tasks.filter((t) => t.status === k).length;
          return (
            <button
              key={k}
              onClick={() => setFilter(k)}
              className={`flex items-center gap-1.5 rounded-full px-3 py-1 transition-colors ${
                filter === k
                  ? "bg-primary text-primary-inverse"
                  : "text-primary/55 hover:bg-hover"
              }`}
            >
              {label}
              <span
                className={`rounded-full px-1.5 text-[10px] ${
                  filter === k ? "bg-primary-inverse/20" : "bg-track"
                }`}
              >
                {count}
              </span>
            </button>
          );
        })}
      </div>

      {error && (
        <div className="mb-3 rounded-lg border border-danger-border bg-danger-bg px-4 py-2.5 text-sm text-danger-fg">
          {error}
          <button
            className="ml-3 font-medium underline"
            onClick={() => setError(null)}
          >
            关闭
          </button>
        </div>
      )}

      {loading ? (
        <div className="flex h-40 items-center justify-center gap-2.5 text-sm text-primary/45">
          <span className="spinner" />
          加载任务…
        </div>
      ) : filtered.length === 0 ? (
        <div className="flex h-56 items-center justify-center rounded-xl border border-dashed border-divider-bold bg-panel/60">
          <div className="flex max-w-sm flex-col items-center text-center">
            <div className="mb-3 flex h-12 w-12 items-center justify-center rounded-2xl border border-dashed border-divider-bold bg-panel text-xl">
              🗂️
            </div>
            <div className="text-sm font-medium text-primary/60">暂无任务</div>
            <div className="mt-1 text-xs leading-relaxed text-primary/45">
              {filter === "all"
                ? "在文献库中导入 PDF 并执行解析、翻译或拆解后，任务会显示在这里"
                : "当前筛选下没有任务"}
            </div>
          </div>
        </div>
      ) : (
        <div className="space-y-2">
          {filtered.map((task) => (
            <div
              key={task.id}
              className="anim-fade-in flex items-center gap-4 rounded-xl border border-divider bg-panel px-5 py-4"
            >
              <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-primary/5 text-lg">
                {TYPE_ICON[task.type] ?? "📋"}
              </div>
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="truncate text-[15px] font-medium">
                    {task.title}
                  </span>
                  <span className="shrink-0 rounded-full bg-primary/5 px-2 py-0.5 text-[11px] text-primary/55">
                    {TYPE_LABEL[task.type] ?? task.type}
                  </span>
                </div>
                <div className="mt-0.5 flex items-center gap-2 text-xs text-primary/50">
                  {statusBadge(task.status)}
                  <span>更新于 {formatTime(task.updated_at)}</span>
                </div>
                {task.status === "failed" && task.error && (
                  <div className="mt-1.5 truncate text-xs text-danger-fg">
                    {task.error}
                  </div>
                )}
              </div>
              {progressBar(task)}
              {(task.status === "done" || task.status === "failed") && (
                <button
                  onClick={() => void handleDelete(task)}
                  title="删除此记录"
                  className="rounded-md border border-divider-strong px-2.5 py-1 text-[11px] text-primary/50 transition-colors hover:border-danger-border hover:bg-danger-bg hover:text-danger-fg"
                >
                  删除
                </button>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
