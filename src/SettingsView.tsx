import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import type { ThemePreset } from "./App";

interface ApiConfig {
  id: string;
  name: string;
  base_url: string;
  model: string | null;
  params: string | null;
  key_masked: string;
  has_key: boolean;
  is_default: boolean;
}

interface GlossaryEntry {
  term: string;
  translation: string;
  created_at: string;
}

interface StatsEventRow {
  event: string;
  count: number;
  total_ms: number;
}

interface StatsSnapshot {
  enabled: boolean;
  events: StatsEventRow[];
}

const EVENT_LABELS: Record<string, string> = {
  import: "导入",
  parse: "解析",
  translate: "翻译",
  digest: "拆解",
  export: "导出",
  error: "错误",
};

const fmtMs = (ms: number) =>
  ms >= 1000 ? `${(ms / 1000).toFixed(1)} s` : `${ms} ms`;

const EMPTY_FORM = {
  id: "",
  name: "",
  base_url: "",
  model: "",
  params: "",
  key: "",
  is_default: false,
};

/** F9 免费方案模板（2026-08 检索核验，政策以官方为准） */
const FREE_TEMPLATES = {
  translate: {
    name: "智谱免费 GLM-4-Flash",
    base_url: "https://open.bigmodel.cn/api/paas/v4",
    model: "glm-4-flash",
    params: '{"temperature": 0.3}',
    key_page: "https://open.bigmodel.cn/usercenter/apikeys",
    doc_page: "https://docs.bigmodel.cn/cn/guide/models/free/glm-4-flash",
    desc: "翻译 / 拆解用，永久免费、30 并发",
  },
  vision: {
    name: "智谱免费视觉 GLM-4.6V-Flash",
    base_url: "https://open.bigmodel.cn/api/paas/v4",
    model: "glm-4.6v-flash",
    params: '{"temperature": 0.1}',
    key_page: "https://open.bigmodel.cn/usercenter/apikeys",
    doc_page: "https://docs.bigmodel.cn/cn/guide/models/free/glm-4.6v-flash",
    desc: "图注识别用（可选），免费视觉模型",
  },
} as const;

const THEME_OPTIONS: { id: ThemePreset; name: string; desc: string; swatches: string[] }[] = [
  { id: "default", name: "默认", desc: "Rd 原生配色", swatches: ["#0f172a", "#7c3aed", "#2563eb"] },
  { id: "minimal", name: "Minimal", desc: "中灰 + 蓝，清爽克制", swatches: ["#232324", "#3b6eeb", "#fafafa"] },
  { id: "dracula", name: "Dracula", desc: "紫调高对比", swatches: ["#282a36", "#bd93f9", "#8be9fd"] },
  { id: "blue-topaz", name: "Blue Topaz", desc: "蓝宝石智识感", swatches: ["#202020", "#4a9ade", "#ffffff"] },
  { id: "catppuccin", name: "Catppuccin", desc: "莫兰迪紫，低饱和", swatches: ["#1e1e2e", "#c6a0f6", "#89b4fa"] },
];

function SettingsView({ themePreset, onThemePreset }: { themePreset: ThemePreset; onThemePreset: (preset: ThemePreset) => void }) {
  const [configs, setConfigs] = useState<ApiConfig[]>([]);
  const [showForm, setShowForm] = useState(false);
  const [form, setForm] = useState({ ...EMPTY_FORM });
  const [testing, setTesting] = useState<Record<string, string>>({});
  const [notice, setNotice] = useState<string | null>(null);
  // MinerU Token（用户自填，settings 表持久化）
  const [mineruKey, setMineruKey] = useState("");
  const [mineruConfigured, setMineruConfigured] = useState(false);

  const [glossary, setGlossary] = useState<GlossaryEntry[]>([]);
  const [termInput, setTermInput] = useState("");
  const [transInput, setTransInput] = useState("");

  const [vision, setVision] = useState({ base_url: "", api_key: "", model: "" });
  const [visionTesting, setVisionTesting] = useState<string | null>(null);

  const [stats, setStats] = useState<StatsSnapshot>({ enabled: false, events: [] });

  const [diag, setDiag] = useState<Diagnostics>({
    log_path: null,
    last_crash: false,
  });
  const [mineruTesting, setMineruTesting] = useState<string | null>(null);
  /** 剪贴板不可用时的诊断文本降级展示（Windows WebView2 可能拒绝 clipboard API） */
  const [diagFallback, setDiagFallback] = useState<string | null>(null);

  // 软件更新：仅在用户点击时联网检查，确认后才下载，重启时机由用户决定
  const [updateState, setUpdateState] = useState<
    "idle" | "checking" | "latest" | "available" | "downloading" | "ready" | "error"
  >("idle");
  const [updateVersion, setUpdateVersion] = useState("");
  const [updateNotes, setUpdateNotes] = useState("");
  const [updateProgress, setUpdateProgress] = useState(0);
  const [updateError, setUpdateError] = useState("");
  const pendingUpdate = useRef<Update | null>(null);

  const refresh = useCallback(async () => {
    setConfigs(await invoke<ApiConfig[]>("list_api_configs"));
    setGlossary(await invoke<GlossaryEntry[]>("list_glossary"));
    setVision(await invoke<{ base_url: string; api_key: string; model: string }>("get_vision_config"));
    setStats(await invoke<StatsSnapshot>("get_stats"));
    setDiag(await invoke<Diagnostics>("get_diagnostics"));
  }, []);

  useEffect(() => {
    refresh().catch((e) => setNotice(String(e)));
  }, [refresh]);

  // 加载 MinerU Token 配置状态
  useEffect(() => {
    invoke<{ configured: boolean }>("get_mineru_key")
      .then((r) => setMineruConfigured(r.configured))
      .catch(() => {});
  }, []);

  const saveMineruKey = async () => {
    try {
      await invoke("set_mineru_key", { key: mineruKey });
      setMineruConfigured(mineruKey.trim() !== "");
      setNotice(
        mineruKey.trim()
          ? "MinerU Token 已保存，当前会话即可用于解析"
          : "已清除 MinerU Token 配置",
      );
      setMineruKey("");
      // 保存后后端已重建客户端，刷新诊断以反映「当前生效」状态
      setDiag(await invoke<Diagnostics>("get_diagnostics"));
    } catch (e) {
      setNotice(String(e));
    }
  };

  /** 测试 MinerU 连接：只申请上传链接、不上传文件，不消耗解析页数 */
  const testMineru = async () => {
    setMineruTesting("测试中…");
    try {
      setMineruTesting(await invoke<string>("test_mineru_connection"));
    } catch (e) {
      setMineruTesting(String(e));
    }
  };

  /** 汇总诊断信息到剪贴板：一次性回传环境、配置状态与最近日志 */
  const copyDiagnostics = async () => {
    const text = [
      `平台: ${diag.platform ?? "-"} / ${diag.arch ?? "-"}`,
      `应用版本: ${diag.version ?? "-"}`,
      `数据目录: ${diag.data_dir ?? "-"}`,
      `日志文件: ${diag.log_path ?? "-"}`,
      `上次异常退出: ${diag.last_crash ? "是" : "否"}`,
      `MinerU Token: ${diag.mineru_configured ? "已配置" : "未配置"}（当前生效: ${
        diag.mineru_active ? "是" : "否"
      }）`,
      `翻译模型: ${diag.default_api_model ?? "未配置"}`,
      `视觉模型: ${diag.vision_configured ? "已配置" : "未配置"}`,
      `文献数: ${diag.document_count ?? 0} ｜ 失败任务: ${diag.failed_task_count ?? 0}`,
      "",
      "---- 最近日志 ----",
      diag.log_tail ?? "(空)",
    ].join("\n");
    try {
      await navigator.clipboard.writeText(text);
      setDiagFallback(null);
      setNotice("诊断信息已复制到剪贴板，可直接粘贴回传");
    } catch {
      // Windows WebView2 下剪贴板 API 可能被拒绝，退回可手动全选的文本框
      setDiagFallback(text);
      setNotice("剪贴板不可用，请在下方文本框中全选复制");
    }
  };

  /** 手动检查更新：只在用户点击时请求，不做后台轮询 */
  const checkUpdate = async () => {
    setUpdateState("checking");
    setUpdateError("");
    try {
      const update = await check();
      if (!update) {
        setUpdateState("latest");
        return;
      }
      pendingUpdate.current = update;
      setUpdateVersion(update.version);
      setUpdateNotes(update.body ?? "");
      setUpdateState("available");
    } catch (e) {
      setUpdateError(String(e));
      setUpdateState("error");
    }
  };

  /** 用户确认后下载并安装（带进度）；重启由用户另行决定，避免打断手头工作 */
  const downloadUpdate = async () => {
    const update = pendingUpdate.current;
    if (!update) return;
    setUpdateState("downloading");
    setUpdateProgress(0);
    try {
      let total = 0;
      let received = 0;
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          received += event.data.chunkLength;
          if (total > 0) {
            setUpdateProgress(Math.min(100, Math.round((received / total) * 100)));
          }
        } else if (event.event === "Finished") {
          setUpdateProgress(100);
        }
      });
      setUpdateState("ready");
    } catch (e) {
      setUpdateError(String(e));
      setUpdateState("error");
    }
  };

  const saveConfig = async () => {
    try {
      await invoke("save_api_config", { config: form });
      setShowForm(false);
      setForm({ ...EMPTY_FORM });
      setNotice(null);
      await refresh();
    } catch (e) {
      setNotice(String(e));
    }
  };

  /** F9 免费方案：一键填入 Base URL / 模型名（Key 留空待用户注册后填写） */
  const applyFreeTemplate = (kind: "translate" | "vision") => {
    const t = FREE_TEMPLATES[kind];
    setForm({
      id: "",
      name: t.name,
      base_url: t.base_url,
      model: t.model,
      params: t.params,
      key: "",
      is_default: kind === "translate",
    });
    setShowForm(true);
    setNotice(null);
  };

  /** F10 帮助系统：系统浏览器打开官网 / 文档（不内嵌 WebView） */
  const openExternal = async (url: string) => {
    try {
      await openUrl(url);
    } catch (e) {
      setNotice(`无法打开链接（${url}）: ${e}`);
    }
  };

  const deleteConfig = async (id: string) => {
    try {
      await invoke("delete_api_config", { id });
      await refresh();
    } catch (e) {
      setNotice(String(e));
    }
  };

  const setDefault = async (config: ApiConfig) => {
    try {
      await invoke("save_api_config", {
        config: { ...config, is_default: true, key: "" },
      });
      await refresh();
    } catch (e) {
      setNotice(String(e));
    }
  };

  const testConfig = async (config: ApiConfig) => {
    setTesting((t) => ({ ...t, [config.id]: "测试中…" }));
    try {
      const msg = await invoke<string>("test_api_connection", {
        id: config.id,
        baseUrl: config.base_url,
        key: "",
      });
      setTesting((t) => ({ ...t, [config.id]: msg }));
    } catch (e) {
      setTesting((t) => ({ ...t, [config.id]: String(e) }));
    }
  };

  const addTerm = async () => {
    const term = termInput.trim();
    const translation = transInput.trim();
    if (!term) {
      setNotice("请输入术语");
      return;
    }
    try {
      await invoke("save_glossary_entry", { term, translation });
      setTermInput("");
      setTransInput("");
      setNotice(null);
      await refresh();
    } catch (e) {
      setNotice(String(e));
    }
  };

  const deleteTerm = async (term: string) => {
    try {
      await invoke("delete_glossary_entry", { term });
      await refresh();
    } catch (e) {
      setNotice(String(e));
    }
  };

  const visionEnabled = vision.base_url.trim() && vision.api_key.trim() && vision.model.trim();
  const saveVision = async () => {
    try {
      await invoke("save_vision_config", {
        baseUrl: vision.base_url,
        apiKey: vision.api_key,
        model: vision.model,
      });
      setNotice(null);
      await refresh();
    } catch (e) {
      setNotice(String(e));
    }
  };

  const testVision = async () => {
    setVisionTesting("测试中…");
    try {
      const msg = await invoke<string>("test_vision_connection", {
        baseUrl: vision.base_url,
        apiKey: vision.api_key,
        model: vision.model,
      });
      setVisionTesting(msg);
    } catch (e) {
      setVisionTesting(String(e));
    }
  };

  const toggleStats = async () => {
    try {
      await invoke("set_stats_enabled", { enabled: !stats.enabled });
      setStats(await invoke<StatsSnapshot>("get_stats"));
    } catch (e) {
      setNotice(String(e));
    }
  };

  const clearStats = async () => {
    try {
      await invoke("reset_stats");
      setStats(await invoke<StatsSnapshot>("get_stats"));
    } catch (e) {
      setNotice(String(e));
    }
  };

  const revealLog = async () => {
    if (!diag.log_path) return;
    try {
      await revealItemInDir(diag.log_path);
    } catch (e) {
      setNotice(String(e));
    }
  };

  const inputCls =
    "w-full rounded-lg border border-divider-strong bg-panel px-3 py-2 text-sm outline-none transition-colors focus:border-primary/50";

  return (
    <div className="mx-auto max-w-4xl">
      {notice && (
        <div className="mb-4 rounded-lg border border-danger-border bg-danger-bg px-4 py-2.5 text-sm text-danger-fg">
          {notice}
          <button
            className="ml-3 font-medium underline"
            onClick={() => setNotice(null)}
          >
            关闭
          </button>
        </div>
      )}

      {/* ============ 主题外观 ============ */}
      <section className="mb-8">
        <div className="mb-3">
          <h2 className="text-[15px] font-semibold">主题外观</h2>
          <p className="mt-0.5 text-xs text-primary/50">
            灵感来自 Obsidian 社区皮肤，每套主题均可配合顶栏的深浅色切换使用，选择会自动保存。
          </p>
        </div>
        <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {THEME_OPTIONS.map((t) => {
            const active = themePreset === t.id;
            return (
              <button
                key={t.id}
                type="button"
                aria-pressed={active}
                onClick={() => onThemePreset(t.id)}
                className={`rounded-xl border p-4 text-left transition-colors ${
                  active
                    ? "border-primary bg-primary/5"
                    : "border-divider bg-panel hover:bg-hover"
                }`}
              >
                <div className="flex h-12 items-end gap-1.5 rounded-lg border border-divider bg-canvas p-1.5">
                  {t.swatches.map((c) => (
                    <span
                      key={c}
                      className="h-6 flex-1 rounded-sm"
                      style={{ backgroundColor: c }}
                    />
                  ))}
                </div>
                <div className="mt-2.5 flex items-center justify-between">
                  <span className="text-sm font-medium">{t.name}</span>
                  {active && (
                    <span className="text-[11px] font-medium text-primary">使用中</span>
                  )}
                </div>
                <p className="mt-0.5 text-xs text-primary/50">{t.desc}</p>
              </button>
            );
          })}
        </div>
      </section>

      {/* ============ 免费方案（F9） ============ */}
      <section className="mb-8">
        <div className="mb-3">
          <h2 className="text-[15px] font-semibold">免费方案（开箱即用）</h2>
          <p className="mt-0.5 text-xs text-primary/50">
            无需付费即可体验完整流水线：一键填入模板后，仅需在各平台免费注册并粘贴 Key。
            免费政策可能调整，以官方为准。
          </p>
        </div>
        <div className="grid gap-3 sm:grid-cols-3">
          <div className="rounded-xl border border-divider bg-panel p-4">
            <div className="text-sm font-medium">翻译 / 拆解</div>
            <div className="mt-1 min-h-8 text-xs leading-relaxed text-primary/55">
              {FREE_TEMPLATES.translate.desc}
            </div>
            <div className="mt-3 flex flex-wrap gap-2">
              <button
                onClick={() => applyFreeTemplate("translate")}
                className="rounded-md bg-primary/90 px-2.5 py-1 text-xs font-medium text-primary-inverse hover:bg-primary"
              >
                一键填充
              </button>
              <button
                onClick={() => openExternal(FREE_TEMPLATES.translate.key_page)}
                className="rounded-md border border-divider-strong px-2.5 py-1 text-xs text-primary/70 hover:bg-hover"
              >
                免费注册获取 Key ↗
              </button>
            </div>
          </div>
          <div className="rounded-xl border border-divider bg-panel p-4">
            <div className="text-sm font-medium">视觉（可选）</div>
            <div className="mt-1 min-h-8 text-xs leading-relaxed text-primary/55">
              {FREE_TEMPLATES.vision.desc}
            </div>
            <div className="mt-3 flex flex-wrap gap-2">
              <button
                onClick={() => applyFreeTemplate("vision")}
                className="rounded-md bg-primary/90 px-2.5 py-1 text-xs font-medium text-primary-inverse hover:bg-primary"
              >
                一键填充
              </button>
              <button
                onClick={() => openExternal(FREE_TEMPLATES.vision.doc_page)}
                className="rounded-md border border-divider-strong px-2.5 py-1 text-xs text-primary/70 hover:bg-hover"
              >
                官方文档 ↗
              </button>
            </div>
          </div>
          <div className="rounded-xl border border-divider bg-panel p-4">
            <div className="text-sm font-medium">解析（MinerU）</div>
            <div className="mt-1 min-h-8 text-xs leading-relaxed text-primary/55">
              官方 API 注册即享每日 2000 页免费额度；Token 在下方「解析（MinerU）配置」自行填入
            </div>
            <div className="mt-3 flex flex-wrap gap-2">
              <button
                onClick={() => openExternal("https://mineru.net")}
                className="rounded-md border border-divider-strong px-2.5 py-1 text-xs text-primary/70 hover:bg-hover"
              >
                打开 mineru.net ↗
              </button>
              <button
                onClick={() => openExternal("https://mineru.net/apiManage/docs")}
                className="rounded-md border border-divider-strong px-2.5 py-1 text-xs text-primary/70 hover:bg-hover"
              >
                API 文档 ↗
              </button>
            </div>
          </div>
        </div>
      </section>

      {/* ============ MinerU 解析配置 ============ */}
      <section className="mb-8">
        <div className="mb-3">
          <h2 className="text-[15px] font-semibold">解析（MinerU）配置</h2>
          <p className="mt-0.5 text-xs text-primary/50">
            MinerU 精准解析使用<b>你自己的 Token</b>：到 mineru.net 免费注册，控制台「API」页获取。
            官方每日 2000 页免费额度。
          </p>
        </div>
        <div className="max-w-md rounded-xl border border-divider bg-panel p-4">
          <div className="mb-2 flex items-center gap-2">
            <span className="text-xs text-primary/55">Token</span>
            {mineruConfigured ? (
              <span className="rounded-full bg-success-bg px-2 py-0.5 text-[11px] text-success-fg">
                已配置
              </span>
            ) : (
              <span className="rounded-full bg-warning-bg px-2 py-0.5 text-[11px] text-warning-fg">
                未配置
              </span>
            )}
          </div>
          <div className="flex gap-2">
            <input
              type="password"
              className={inputCls}
              value={mineruKey}
              onChange={(e) => setMineruKey(e.target.value)}
              placeholder="粘贴你的 MinerU Token"
            />
            <button
              onClick={saveMineruKey}
              className="shrink-0 rounded-md bg-primary/90 px-3 py-2 text-xs font-medium text-primary-inverse hover:bg-primary"
            >
              保存
            </button>
            <button
              onClick={testMineru}
              disabled={mineruTesting === "测试中…"}
              className="shrink-0 rounded-md border border-divider-strong px-3 py-2 text-xs text-primary/70 transition-colors hover:bg-hover"
            >
              测试连接
            </button>
          </div>
          {mineruTesting && (
            <p className="mt-2 text-[11px] text-primary/60">{mineruTesting}</p>
          )}
          <p className="mt-2 text-[11px] text-primary/40">
            保存后当前会话即可生效，无需重启应用；留空保存可清除配置（回退到 .env / 环境变量）
          </p>
        </div>
      </section>

      {/* ============ API 配置 ============ */}
      <section className="mb-8">
        <div className="mb-3 flex items-center justify-between">
          <div>
            <h2 className="text-[15px] font-semibold">翻译 / 拆解 API 配置</h2>
            <p className="mt-0.5 text-xs text-primary/50">
              OpenAI 兼容接口；翻译与后续 AI 拆解共用默认配置
            </p>
          </div>
          <button
            onClick={() => {
              setShowForm((v) => !v);
              setForm({ ...EMPTY_FORM });
            }}
            className="rounded-lg bg-primary/90 px-3 py-1.5 text-xs font-medium text-primary-inverse transition-colors hover:bg-primary"
          >
            {showForm ? "取消" : "+ 新增配置"}
          </button>
        </div>

        {showForm && (
          <div className="mb-4 rounded-xl border border-divider bg-panel p-5 shadow-sm">
            <div className="grid grid-cols-2 gap-3">
              <div>
                <label className="mb-1 block text-xs text-primary/55">
                  名称
                </label>
                <input
                  className={inputCls}
                  value={form.name}
                  onChange={(e) => setForm({ ...form, name: e.target.value })}
                  placeholder="如：智谱 GLM / DeepSeek"
                />
              </div>
              <div>
                <label className="mb-1 block text-xs text-primary/55">
                  Base URL
                </label>
                <input
                  className={inputCls}
                  value={form.base_url}
                  onChange={(e) =>
                    setForm({ ...form, base_url: e.target.value })
                  }
                  placeholder="https://api.example.com/v1"
                />
              </div>
              <div>
                <label className="mb-1 block text-xs text-primary/55">
                  模型名
                </label>
                <input
                  className={inputCls}
                  value={form.model}
                  onChange={(e) => setForm({ ...form, model: e.target.value })}
                  placeholder="如 glm-4-plus"
                />
              </div>
              <div>
                <label className="mb-1 block text-xs text-primary/55">
                  额外参数（JSON，可选）
                </label>
                <input
                  className={inputCls}
                  value={form.params}
                  onChange={(e) => setForm({ ...form, params: e.target.value })}
                  placeholder='{"temperature": 0.3}'
                />
              </div>
              <div className="col-span-2">
                <label className="mb-1 block text-xs text-primary/55">
                  API Key
                  {form.id && (
                    <span className="ml-2 text-primary/35">
                      （留空则保留已保存的 Key）
                    </span>
                  )}
                </label>
                <input
                  type="password"
                  className={inputCls}
                  value={form.key}
                  onChange={(e) => setForm({ ...form, key: e.target.value })}
                  placeholder="sk-..."
                />
                <div className="mt-1.5 flex items-center gap-3 text-[11px] text-primary/45">
                  <span>在对应平台注册后获取 Key</span>
                  <button
                    onClick={() => openExternal("https://open.bigmodel.cn/usercenter/apikeys")}
                    className="text-primary/60 underline hover:text-primary"
                  >
                    智谱 API Key 获取 ↗
                  </button>
                  <button
                    onClick={() => openExternal("https://docs.bigmodel.cn")}
                    className="text-primary/60 underline hover:text-primary"
                  >
                    官方文档 ↗
                  </button>
                </div>
              </div>
            </div>
            <div className="mt-4 flex items-center gap-3">
              <label className="flex items-center gap-2 text-xs text-primary/60">
                <input
                  type="checkbox"
                  checked={form.is_default}
                  onChange={(e) =>
                    setForm({ ...form, is_default: e.target.checked })
                  }
                  className="accent-primary"
                />
                设为默认（翻译使用）
              </label>
              <div className="ml-auto flex gap-2">
                <button
                  onClick={() => {
                    setShowForm(false);
                    setForm({ ...EMPTY_FORM });
                  }}
                  className="rounded-lg border border-divider-strong bg-panel px-3 py-1.5 text-xs font-medium text-primary/70 hover:bg-hover"
                >
                  取消
                </button>
                <button
                  onClick={saveConfig}
                  className="rounded-lg bg-primary px-3 py-1.5 text-xs font-medium text-primary-inverse"
                >
                  保存
                </button>
              </div>
            </div>
          </div>
        )}

        <div className="space-y-2">
          {configs.length === 0 && !showForm && (
            <div className="rounded-xl border border-dashed border-divider-strong bg-panel/60 px-4 py-6 text-center text-sm text-primary/45">
              尚未配置 API。新增一个 OpenAI 兼容接口后即可启用翻译。
            </div>
          )}
          {configs.map((c) => (
            <div
              key={c.id}
              className="flex items-center gap-4 rounded-xl border border-divider bg-panel px-5 py-3.5"
            >
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="text-sm font-medium">{c.name}</span>
                  {c.is_default && (
                    <span className="rounded-full bg-success-bg px-2 py-0.5 text-[11px] text-success-fg">
                      默认
                    </span>
                  )}
                </div>
                <div className="mt-0.5 truncate text-xs text-primary/50">
                  {c.base_url}
                  {c.model ? ` · ${c.model}` : ""}
                  {c.has_key ? ` · ${c.key_masked}` : " · 未配置 Key"}
                </div>
                {testing[c.id] && (
                  <div
                    className={`mt-1 text-xs ${
                      testing[c.id].startsWith("连接")
                        ? "text-success-fg"
                        : "text-danger-fg"
                    }`}
                  >
                    {testing[c.id]}
                  </div>
                )}
              </div>
              <div className="flex shrink-0 items-center gap-1.5">
                <button
                  onClick={() => testConfig(c)}
                  className="rounded-md border border-divider-strong px-2.5 py-1 text-xs text-primary/70 hover:bg-hover"
                >
                  测试
                </button>
                {!c.is_default && (
                  <button
                    onClick={() => setDefault(c)}
                    className="rounded-md border border-divider-strong px-2.5 py-1 text-xs text-primary/70 hover:bg-hover"
                  >
                    设为默认
                  </button>
                )}
                <button
                  onClick={() => {
                    setForm({
                      id: c.id,
                      name: c.name,
                      base_url: c.base_url,
                      model: c.model ?? "",
                      params: c.params ?? "",
                      key: "",
                      is_default: c.is_default,
                    });
                    setShowForm(true);
                  }}
                  className="rounded-md border border-divider-strong px-2.5 py-1 text-xs text-primary/70 hover:bg-hover"
                >
                  编辑
                </button>
                <button
                  onClick={() => deleteConfig(c.id)}
                  className="rounded-md border border-danger-border px-2.5 py-1 text-xs text-danger-fg hover:bg-danger-bg"
                >
                  删除
                </button>
              </div>
            </div>
          ))}
        </div>
      </section>

      {/* ============ 视觉模型（可选） ============ */}
      <section className="mb-8">
        <div className="mb-3">
          <h2 className="text-[15px] font-semibold">视觉模型（可选外挂）</h2>
          <p className="mt-0.5 text-xs text-primary/50">
            翻译时用于识别文中图片类型（数据图表 / 示意图 / 案例照片 / 地图等）并辅助图注翻译。
            需为支持图片输入（多模态）的 OpenAI 兼容接口；主翻译模型本身无需识图。留空则不启用。
          </p>
        </div>
        <div className="rounded-xl border border-divider bg-panel p-5 shadow-sm">
          <div className="grid grid-cols-2 gap-3">
            <div className="col-span-2">
              <label className="mb-1 block text-xs text-primary/55">
                Base URL
              </label>
              <input
                className={inputCls}
                value={vision.base_url}
                onChange={(e) => setVision({ ...vision, base_url: e.target.value })}
                placeholder="https://api.example.com/v1"
              />
            </div>
            <div>
              <label className="mb-1 block text-xs text-primary/55">
                模型名
              </label>
              <input
                className={inputCls}
                value={vision.model}
                onChange={(e) => setVision({ ...vision, model: e.target.value })}
                placeholder="如 glm-4v / qwen-vl-max"
              />
            </div>
            <div>
              <label className="mb-1 block text-xs text-primary/55">
                API Key
              </label>
              <input
                type="password"
                className={inputCls}
                value={vision.api_key}
                onChange={(e) => setVision({ ...vision, api_key: e.target.value })}
                placeholder="sk-..."
              />
            </div>
          </div>
          <div className="mt-4 flex items-center justify-end gap-3">
            <span
              className={`text-xs ${
                visionEnabled ? "text-success-fg" : "text-primary/40"
              }`}
            >
              {visionEnabled
                ? "已启用（翻译时自动分析文中图片）"
                : "未启用（图注按纯文本翻译）"}
            </span>
            {visionTesting && (
              <span
                className={`max-w-[320px] truncate text-xs ${
                  visionTesting.startsWith("连接正常")
                    ? "text-success-fg"
                    : visionTesting === "测试中…"
                      ? "text-primary/45"
                      : "text-danger-fg"
                }`}
                title={visionTesting}
              >
                {visionTesting}
              </span>
            )}
            <button
              onClick={testVision}
              disabled={!visionEnabled}
              className={`rounded-lg border px-3 py-1.5 text-xs font-medium transition-colors ${
                visionEnabled
                  ? "border-divider-strong text-primary/70 hover:bg-hover"
                  : "cursor-not-allowed border-divider text-primary/30"
              }`}
            >
              测试连接
            </button>
            <button
              onClick={saveVision}
              className="rounded-lg bg-primary px-4 py-1.5 text-xs font-medium text-primary-inverse hover:bg-primary/90"
            >
              保存
            </button>
          </div>
        </div>
      </section>

      {/* ============ 术语表 ============ */}
      <section>
        <div className="mb-3">
          <h2 className="text-[15px] font-semibold">术语表</h2>
          <p className="mt-0.5 text-xs text-primary/50">
            翻译时注入 system prompt，强制术语一致性（如：mind-wandering =
            心智游移）
          </p>
        </div>

        <div className="mb-3 flex items-center gap-2">
          <input
            className={inputCls}
            value={termInput}
            onChange={(e) => setTermInput(e.target.value)}
            placeholder="原文术语"
            onKeyDown={(e) => e.key === "Enter" && addTerm()}
          />
          <input
            className={inputCls}
            value={transInput}
            onChange={(e) => setTransInput(e.target.value)}
            placeholder="建议译文"
            onKeyDown={(e) => e.key === "Enter" && addTerm()}
          />
          <button
            onClick={addTerm}
            className="shrink-0 rounded-lg bg-primary/90 px-4 py-2 text-xs font-medium text-primary-inverse hover:bg-primary"
          >
            添加
          </button>
        </div>

        <div className="overflow-hidden rounded-xl border border-divider bg-panel">
          {glossary.length === 0 ? (
            <div className="px-4 py-6 text-center text-sm text-primary/45">
              术语表为空。添加术语可提升跨文献翻译一致性。
            </div>
          ) : (
            glossary.map((g) => (
              <div
                key={g.term}
                className="flex items-center gap-3 border-b border-divider px-5 py-2.5 last:border-b-0"
              >
                <span className="w-1/3 truncate text-sm font-medium">
                  {g.term}
                </span>
                <span className="w-1/3 truncate text-sm text-primary/70">
                  {g.translation || "—"}
                </span>
                <span className="flex-1" />
                <button
                  onClick={() => deleteTerm(g.term)}
                  className="rounded-md px-2 py-1 text-xs text-danger-fg hover:bg-danger-bg"
                >
                  删除
                </button>
              </div>
            ))
          )}
        </div>
      </section>

      {/* ============ 本地统计（M5.1） ============ */}
      <section className="mt-8">
        <div className="mb-3 flex items-center justify-between">
          <div>
            <h2 className="text-[15px] font-semibold">本地统计</h2>
            <p className="mt-0.5 text-xs text-primary/50">
              事件计数与耗时统计，数据仅保存在本机。默认关闭，开启后完全可随时关闭。
            </p>
          </div>
          <label className="flex cursor-pointer items-center gap-2">
            <span className="text-xs text-primary/60">
              {stats.enabled ? "已开启" : "已关闭"}
            </span>
            <input
              type="checkbox"
              checked={stats.enabled}
              onChange={toggleStats}
              className="accent-primary"
            />
          </label>
        </div>

        <div className="overflow-hidden rounded-xl border border-divider bg-panel">
          {!stats.enabled ? (
            <div className="px-4 py-6 text-center text-sm text-primary/45">
              统计已关闭。开启后开始记录导入、解析、翻译、拆解、导出等事件。
            </div>
          ) : stats.events.length === 0 ? (
            <div className="px-4 py-6 text-center text-sm text-primary/45">
              暂无统计记录。
            </div>
          ) : (
            <div>
              {stats.events.map((e) => {
                const avg = e.count > 0 ? e.total_ms / e.count : 0;
                return (
                  <div
                    key={e.event}
                    className="flex items-center gap-4 border-b border-divider px-5 py-2.5 last:border-b-0"
                  >
                    <span className="w-16 text-sm font-medium">
                      {EVENT_LABELS[e.event] ?? e.event}
                    </span>
                    <span className="w-14 text-sm text-primary/70">
                      {e.count} 次
                    </span>
                    <span className="w-24 text-xs text-primary/50">
                      总 {fmtMs(e.total_ms)}
                    </span>
                    <span className="flex-1 text-xs text-primary/50">
                      平均 {fmtMs(Math.round(avg))}
                    </span>
                  </div>
                );
              })}
            </div>
          )}
          {stats.enabled && (
            <div className="flex items-center justify-end gap-3 border-t border-divider px-5 py-2.5">
              <button
                onClick={clearStats}
                className="rounded-md border border-divider-strong px-2.5 py-1 text-xs text-primary/70 hover:bg-hover"
              >
                清空统计
              </button>
            </div>
          )}
        </div>
      </section>

      {/* ============ 软件更新 ============ */}
      <section className="mt-8">
        <div className="mb-3">
          <h2 className="text-[15px] font-semibold">软件更新</h2>
          <p className="mt-0.5 text-xs text-primary/50">
            仅在你点击时检查，不会后台联网。更新包经签名校验，下载完成后重启应用生效。
          </p>
        </div>
        <div className="rounded-xl border border-divider bg-panel p-5 shadow-sm">
          <div className="flex flex-wrap items-center gap-3">
            <span className="text-xs text-primary/55">当前版本</span>
            <code className="rounded bg-hover px-2 py-0.5 text-xs text-primary/80">
              v{diag.version ?? "…"}
            </code>
            <button
              onClick={checkUpdate}
              disabled={updateState === "checking" || updateState === "downloading"}
              className="rounded-md border border-divider-strong px-2.5 py-1 text-xs text-primary/70 transition-colors hover:bg-hover disabled:cursor-not-allowed disabled:opacity-50"
            >
              {updateState === "checking" ? "检查中…" : "检查更新"}
            </button>
            {updateState === "latest" && (
              <span className="text-xs text-success-fg">已是最新版本</span>
            )}
          </div>

          {updateState === "error" && (
            <p className="mt-3 text-xs text-warning-fg">检查更新失败：{updateError}</p>
          )}

          {updateState === "available" && (
            <div className="mt-3 rounded-lg border border-divider-strong bg-hover p-3">
              <p className="text-xs text-primary/80">
                发现新版本 <strong>v{updateVersion}</strong>
              </p>
              {updateNotes && (
                <pre className="mt-2 max-h-40 overflow-auto whitespace-pre-wrap text-[11px] leading-relaxed text-primary/60">
                  {updateNotes}
                </pre>
              )}
              <button
                onClick={downloadUpdate}
                className="mt-3 rounded-md bg-primary/90 px-3 py-1.5 text-xs font-medium text-primary-inverse transition-colors hover:bg-primary"
              >
                下载并安装
              </button>
            </div>
          )}

          {updateState === "downloading" && (
            <div className="mt-3">
              <div className="h-1.5 w-full overflow-hidden rounded-full bg-track">
                <div
                  className="h-full bg-primary/70 transition-all"
                  style={{ width: `${updateProgress}%` }}
                />
              </div>
              <p className="mt-1.5 text-[11px] text-primary/50">正在下载 {updateProgress}%</p>
            </div>
          )}

          {updateState === "ready" && (
            <div className="mt-3 rounded-lg border border-divider-strong bg-hover p-3">
              <p className="text-xs text-primary/80">
                更新已下载并安装完成，重启应用后生效。
              </p>
              <button
                onClick={() => void relaunch()}
                className="mt-3 rounded-md bg-primary/90 px-3 py-1.5 text-xs font-medium text-primary-inverse transition-colors hover:bg-primary"
              >
                立即重启
              </button>
            </div>
          )}
        </div>
      </section>

      {/* ============ 诊断信息（M5.2） ============ */}
      <section className="mt-8">
        <div className="mb-3">
          <h2 className="text-[15px] font-semibold">诊断信息</h2>
          <p className="mt-0.5 text-xs text-primary/50">
            遇到问题时点「复制诊断信息」，可把环境、配置状态与最近日志一次性回传（错误信息含 [E-xxxx] 错误码）。
          </p>
        </div>
        <div className="rounded-xl border border-divider bg-panel p-5 shadow-sm">
          {diag.last_crash && (
            <div className="mb-3 rounded-lg border border-warning-border bg-warning-bg px-4 py-2.5 text-sm text-warning-fg">
              检测到上次应用异常退出，未完成的任务已中断，可重新执行。
            </div>
          )}

          <div className="mb-4 grid grid-cols-2 gap-x-6 gap-y-2.5 text-xs sm:grid-cols-3">
            <DiagItem label="平台 / 架构" value={`${diag.platform ?? "-"} / ${diag.arch ?? "-"}`} />
            <DiagItem label="应用版本" value={diag.version ?? "-"} />
            <DiagItem label="运行位置" value={diag.exe_path ?? "-"} />
            <DiagItem
              label="文献数 / 失败任务"
              value={`${diag.document_count ?? 0} / ${diag.failed_task_count ?? 0}`}
              warn={(diag.failed_task_count ?? 0) > 0}
            />
            <DiagItem
              label="MinerU Token"
              value={
                diag.mineru_configured
                  ? `已配置（生效: ${diag.mineru_active ? "是" : "否"}）`
                  : "未配置"
              }
              warn={!diag.mineru_active}
            />
            <DiagItem
              label="翻译 / 拆解模型"
              value={diag.default_api_model ?? "未配置"}
              warn={!diag.default_api_model}
            />
            <DiagItem label="视觉模型" value={diag.vision_configured ? "已配置" : "未配置"} />
          </div>

          <div className="flex items-center gap-3">
            <span className="text-xs text-primary/55">日志文件：</span>
            <code className="min-w-0 flex-1 truncate rounded bg-hover px-2 py-1 text-xs text-primary/80">
              {diag.log_path ?? "未初始化"}
            </code>
            <button
              onClick={revealLog}
              disabled={!diag.log_path}
              className={`shrink-0 rounded-md border px-2.5 py-1 text-xs transition-colors ${
                diag.log_path
                  ? "border-divider-strong text-primary/70 hover:bg-hover"
                  : "cursor-not-allowed border-divider text-primary/30"
              }`}
            >
              打开日志目录
            </button>
            <button
              onClick={copyDiagnostics}
              className="shrink-0 rounded-md border border-divider-strong px-2.5 py-1 text-xs text-primary/70 transition-colors hover:bg-hover"
            >
              复制诊断信息
            </button>
          </div>

          {diagFallback && (
            <textarea
              readOnly
              value={diagFallback}
              onFocus={(e) => e.currentTarget.select()}
              className="mt-3 h-48 w-full rounded-lg border border-divider-strong bg-panel p-3 font-mono text-[11px] leading-relaxed text-primary/80 outline-none"
            />
          )}

          {diag.log_tail && (
            <pre className="mt-3 max-h-64 overflow-auto rounded-lg bg-hover p-3 text-[11px] leading-relaxed text-primary/70">
              {diag.log_tail}
            </pre>
          )}
        </div>
      </section>
    </div>
  );
}

/** 后端 get_diagnostics 返回的诊断快照 */
type Diagnostics = {
  log_path: string | null;
  last_crash: boolean;
  platform?: string;
  arch?: string;
  version?: string;
  exe_path?: string;
  data_dir?: string | null;
  mineru_configured?: boolean;
  mineru_active?: boolean;
  default_api_model?: string | null;
  vision_configured?: boolean;
  document_count?: number;
  failed_task_count?: number;
  log_tail?: string;
  error?: string;
};

/** 诊断项：标签 + 取值，未配置时以警示色提示；长值悬停可看全文 */
function DiagItem({ label, value, warn }: { label: string; value: string; warn?: boolean }) {
  return (
    <div className="flex min-w-0 flex-col">
      <span className="text-primary/45">{label}</span>
      <span className={`truncate ${warn ? "text-warning-fg" : "text-primary/80"}`} title={value}>
        {value}
      </span>
    </div>
  );
}

export default SettingsView;
