import { openUrl } from "@tauri-apps/plugin-opener";
import logo from "./assets/logo.png";

interface Props {
  onClose: () => void;
}

const PIPELINE = [
  {
    icon: "📄",
    title: "导入 PDF",
    desc: "点击右上角「+ 导入 PDF」或直接将文件拖入窗口，文献进入本地文献库。",
  },
  {
    icon: "🔍",
    title: "版面解析（MinerU）",
    desc: "自动识别版面布局、公式、图片与语言，输出结构化 Markdown，为后续步骤打底。",
  },
  {
    icon: "🌐",
    title: "翻译",
    desc: "基于默认 API（如免费的 GLM-4-Flash）自动翻译，支持中英互译，可在原文 / 译文 / 双语对照三种模式间切换。",
  },
  {
    icon: "🧬",
    title: "范式拆解",
    desc: "识别学科研究范式（实验对比、实证统计、理论论述、案例研究、综述元分析、计算模拟、设计与构建等），按范式模板拆解字段；所有结论引用原文段落编号，禁止编造。",
  },
  {
    icon: "📤",
    title: "导出",
    desc: "将翻译 / 拆解结果导出为 Markdown、HTML 或 PDF，方便引用与存档。",
  },
  {
    icon: "📝",
    title: "笔记",
    desc: "内置本地 Markdown 笔记，编辑实时自动保存，支持渲染预览。",
  },
];

const FREE_STEPS = [
  {
    title: "注册智谱开放平台，获取 API Key",
    desc: "访问 open.bigmodel.cn 注册账号，在「API 密钥」页面创建密钥。",
    url: "https://open.bigmodel.cn/usercenter/apikeys",
  },
  {
    title: "填入 GLM-4-Flash 免费模板",
    desc: "打开「设置 → 免费方案」，点击 GLM-4-Flash 卡片上的「填入模板」，再粘贴 API Key 保存。翻译与拆解即可全部使用，永久免费、30 并发。",
  },
  {
    title: "（可选）添加视觉模型",
    desc: "同样的 Key 再填入 GLM-4.6V-Flash 模板，用于图注 / 图片内容识别，同样是免费视觉模型。",
  },
  {
    title: "注册 MinerU 获取 Token",
    desc: "访问 mineru.net 注册，在控制台「API」页复制 Token，粘贴到「设置 → 解析（MinerU）配置」。每日 2000 页免费额度，个人阅读完全够用。",
    url: "https://mineru.net",
  },
  {
    title: "回到文献库开始使用",
    desc: "导入第一篇 PDF，等待解析完成即可阅读、翻译与拆解。",
  },
];

function SectionTitle({ title, desc }: { title: string; desc?: string }) {
  return (
    <div className="mb-3">
      <h2 className="text-sm font-semibold">{title}</h2>
      {desc && <p className="mt-0.5 text-xs text-primary/50">{desc}</p>}
    </div>
  );
}

function HelpView({ onClose }: Props) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4 sm:p-8">
      <div className="anim-slide-up flex h-full max-h-[720px] w-full max-w-3xl flex-col overflow-hidden rounded-2xl border border-divider bg-canvas shadow-2xl">
        <header className="flex shrink-0 items-center justify-between border-b border-divider bg-panel px-6 py-4">
          <div className="flex items-center gap-3">
            <img
              src={logo}
              alt="Paperlens"
              className="h-8 w-auto select-none"
              draggable={false}
            />
            <div>
              <h1 className="text-[15px] font-semibold tracking-tight">使用帮助</h1>
              <p className="text-[11px] text-primary/45">Rd学术阅读器 · 快速上手与配置指南</p>
            </div>
          </div>
          <button
            onClick={onClose}
            title="关闭帮助"
            aria-label="关闭帮助"
            className="flex h-7 w-7 items-center justify-center rounded-md border border-divider-strong bg-panel/70 text-primary/60 transition-colors hover:bg-hover"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </button>
        </header>

        <div className="flex-1 overflow-y-auto p-6">
          {/* 简介 */}
          <section className="mb-8">
            <SectionTitle title="这是什么？" />
            <div className="rounded-xl border border-divider bg-panel p-4 text-xs leading-relaxed text-primary/70">
              Rd学术阅读器是一款
              <span className="mx-1 rounded bg-primary/5 px-1.5 py-0.5 font-medium text-primary">
                本地优先
              </span>
              的学术文献加工流水线：导入 PDF 后自动完成版面解析、语言识别、翻译与学科范式拆解，帮助你快速吃透一篇论文。文献库、笔记与全部密钥均保存在本机，不上传任何内容。
            </div>
          </section>

          {/* 核心功能 */}
          <section className="mb-8">
            <SectionTitle title="核心功能：一条完整的加工流水线" desc="从 PDF 到结构化理解，六步一气呵成" />
            <div className="grid gap-3 sm:grid-cols-2">
              {PIPELINE.map((s, i) => (
                <div key={s.title} className="rounded-xl border border-divider bg-panel p-4">
                  <div className="flex items-center gap-2.5">
                    <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-primary/10 text-[11px] font-semibold text-primary">
                      {i + 1}
                    </span>
                    <span className="text-sm">{s.icon}</span>
                    <span className="text-sm font-medium">{s.title}</span>
                  </div>
                  <p className="mt-2 text-xs leading-relaxed text-primary/55">{s.desc}</p>
                </div>
              ))}
            </div>
          </section>

          {/* 免费方案 */}
          <section className="mb-8">
            <SectionTitle
              title="免费方案：5 步免费开始"
              desc="全程零成本，无需付费订阅，适合个人文献阅读"
            />
            <div className="space-y-3">
              {FREE_STEPS.map((s, i) => (
                <div key={s.title} className="rounded-xl border border-divider bg-panel p-4">
                  <div className="flex items-start gap-3">
                    <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-primary text-[11px] font-semibold text-primary-inverse">
                      {i + 1}
                    </span>
                    <div>
                      <div className="text-sm font-medium">{s.title}</div>
                      <p className="mt-1 text-xs leading-relaxed text-primary/55">{s.desc}</p>
                      {s.url && (
                        <button
                          onClick={() => void openUrl(s.url!)}
                          className="mt-2 inline-flex items-center gap-1 text-xs text-primary underline decoration-primary/30 underline-offset-2 transition-colors hover:text-primary/80"
                        >
                          {s.url.replace("https://", "")} ↗
                        </button>
                      )}
                    </div>
                  </div>
                </div>
              ))}
            </div>
            <p className="mt-3 rounded-lg border border-success-border bg-success-bg px-3 py-2 text-[11px] leading-relaxed text-success-fg">
              提示：免费方案的政策与额度以各平台官方页面为准，本说明仅为引导，如有变动请以官方公告为准。
            </p>
          </section>

          {/* Key 配置详解 */}
          <section className="mb-8">
            <SectionTitle title="Key 配置详解" desc="所有密钥仅保存在本机应用数据库中，不会上传或用于其他用途" />
            <div className="grid gap-3 sm:grid-cols-3">
              <div className="rounded-xl border border-divider bg-panel p-4">
                <div className="text-sm font-medium">默认 API</div>
                <div className="mt-1 text-[11px] leading-relaxed text-primary/45">翻译 / 拆解共用</div>
                <div className="mt-2 text-xs leading-relaxed text-primary/55">
                  采用 OpenAI 兼容格式（Base URL / API Key / 模型名），可用免费的 GLM-4-Flash，也可换成任意 OpenAI 兼容服务。
                </div>
              </div>
              <div className="rounded-xl border border-divider bg-panel p-4">
                <div className="text-sm font-medium">视觉模型</div>
                <div className="mt-1 text-[11px] leading-relaxed text-primary/45">可选，独立配置</div>
                <div className="mt-2 text-xs leading-relaxed text-primary/55">
                  用于图注 / 图片内容识别。<span className="font-medium text-primary/70">若主模型本身支持视觉会自动复用，无需另配</span>；主模型是纯文本模型时才需单独填写（免费方案推荐
                  GLM-4.6V-Flash），否则跳过图片识别，不影响解析、翻译与拆解。
                </div>
              </div>
              <div className="rounded-xl border border-divider bg-panel p-4">
                <div className="text-sm font-medium">MinerU Token</div>
                <div className="mt-1 text-[11px] leading-relaxed text-primary/45">版面解析</div>
                <div className="mt-2 text-xs leading-relaxed text-primary/55">
                  在 mineru.net 控制台「API」页获取，填入「设置 → 解析（MinerU）配置」。每日 2000 页免费额度。
                </div>
              </div>
            </div>
          </section>

          {/* 常用功能 */}
          <section className="mb-4">
            <SectionTitle title="还有这些常用功能" />
            <div className="grid gap-3 sm:grid-cols-2">
              <div className="rounded-xl border border-divider bg-panel p-4">
                <div className="text-sm font-medium">阅读模式</div>
                <p className="mt-1.5 text-xs leading-relaxed text-primary/55">
                  在阅读页可切换「原文 / 译文 / 双语对照 / 拆解」四种视图，边读边对照。
                </p>
              </div>
              <div className="rounded-xl border border-divider bg-panel p-4">
                <div className="text-sm font-medium">双击直达原文</div>
                <p className="mt-1.5 text-xs leading-relaxed text-primary/55">
                  在文献库里<span className="font-medium text-primary/70">双击</span>任一卡片，直接打开该篇的原文视图，跳过译文与拆解；单击仍是常规打开。
                </p>
              </div>
              <div className="rounded-xl border border-divider bg-panel p-4">
                <div className="text-sm font-medium">引用跳转</div>
                <p className="mt-1.5 text-xs leading-relaxed text-primary/55">
                  正文里的 [1]、[2,3] 这类引用标记可点击，直接跳到文末参考文献的对应条目，核对出处不用手动翻。
                </p>
              </div>
              <div className="rounded-xl border border-divider bg-panel p-4">
                <div className="text-sm font-medium">阅读状态标记</div>
                <p className="mt-1.5 text-xs leading-relaxed text-primary/55">
                  文献卡片上的「未读 / 已读」开关，按状态筛选文献列表。
                </p>
              </div>
              <div className="rounded-xl border border-divider bg-panel p-4">
                <div className="text-sm font-medium">术语表</div>
                <p className="mt-1.5 text-xs leading-relaxed text-primary/55">
                  在「设置」中自定义术语翻译，保证全文译文用词统一。
                </p>
              </div>
              <div className="rounded-xl border border-divider bg-panel p-4">
                <div className="text-sm font-medium">主题外观</div>
                <p className="mt-1.5 text-xs leading-relaxed text-primary/55">
                  顶栏按钮切换浅色 / 深色；「设置 → 主题外观」可选七套主题，偏好会记住。其中
                  <span className="font-medium text-primary/70">Twilight / Amber 带主页氛围图</span>
                  ：文献库（列表）页整页铺氛围层、顶栏与导航吃氛围色，点进去的阅读、笔记、设置等栏目只继承色调。
                </p>
              </div>
              <div className="rounded-xl border border-divider bg-panel p-4">
                <div className="text-sm font-medium">单击复制</div>
                <p className="mt-1.5 text-xs leading-relaxed text-primary/55">
                  双语视图与原文视图里，鼠标指向某句会高亮，单击即复制该句（双语会连对应译文一起复制）；拆解栏单击整条复制。若你正在拖选文字，则不会触发复制，选取照常可用。
                </p>
              </div>
              <div className="rounded-xl border border-divider bg-panel p-4">
                <div className="text-sm font-medium">添加到笔记</div>
                <p className="mt-1.5 text-xs leading-relaxed text-primary/55">
                  选中文本后右键「添加到笔记」：首次会按「月-日-题目-一作-年份-阅读笔记」新建笔记，之后同一篇文献再添加就追加到同一份笔记末尾。
                </p>
              </div>
              <div className="rounded-xl border border-divider bg-panel p-4">
                <div className="text-sm font-medium">笔记管理</div>
                <p className="mt-1.5 text-xs leading-relaxed text-primary/55">
                  「笔记」页可浏览、编辑、重命名、批量删除与跨笔记查找替换。笔记是本地 Markdown 文件，也可单篇或批量导出为 Markdown / HTML / PDF。
                </p>
              </div>
              <div className="rounded-xl border border-divider bg-panel p-4">
                <div className="text-sm font-medium">任务中心</div>
                <p className="mt-1.5 text-xs leading-relaxed text-primary/55">
                  正在解析 / 翻译 / 拆解的文献都会在「任务中心」显示进度与状态，长文处理时不必盯着某一页等结果。
                </p>
              </div>
              <div className="rounded-xl border border-divider bg-panel p-4">
                <div className="text-sm font-medium">按需导出</div>
                <p className="mt-1.5 text-xs leading-relaxed text-primary/55">
                  导出前可选内容范围：中文译文 / 双语对照 / 拆解结果 / 除原文外全部 / 全部内容；格式支持 Markdown、HTML（图片内嵌、可离线打开）与 PDF。
                </p>
              </div>
              <div className="rounded-xl border border-divider bg-panel p-4">
                <div className="text-sm font-medium">重置与更新</div>
                <p className="mt-1.5 text-xs leading-relaxed text-primary/55">
                  文献卡片的「重置」可清除该篇的解析 / 翻译 / 拆解产物（原始 PDF 保留）以便重跑；「设置 → 软件更新」可手动检查新版本，确认后下载、重启生效。
                </p>
              </div>
            </div>
          </section>

          {/* 篇幅提醒 */}
          <section className="mb-4">
            <SectionTitle title="关于篇幅：适合单篇论文" />
            <p className="rounded-lg border border-warning-border bg-warning-bg px-3 py-2 text-[11px] leading-relaxed text-warning-fg">
              翻译与拆解都是<span className="font-medium">全量处理</span>：翻译按段落逐段调用，拆解按字段逐个请求。token 消耗随篇幅增长很快——请勿导入专著、论文集或上百页的学位论文，一次全量处理可能吃掉可观额度。超长文档建议拆成几份分别导入。
            </p>
          </section>
        </div>
      </div>
    </div>
  );
}

export default HelpView;
