function App() {
  return (
    <div className="flex h-full flex-col bg-[#f7f8fa] text-[#0b1326]">
      {/* 顶栏：应用标识 */}
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
      </nav>

      {/* 内容区：空库引导 */}
      <main className="flex flex-1 items-center justify-center p-8">
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
            <button className="rounded-lg bg-[#0b1326] px-4 py-2 text-sm font-medium text-white transition-transform hover:scale-[1.02]">
              选择文件
            </button>
            <button className="rounded-lg border border-black/10 bg-white px-4 py-2 text-sm font-medium text-[#0b1326]/70 transition-colors hover:bg-black/5">
              拖拽导入
            </button>
          </div>
        </div>
      </main>
    </div>
  );
}

export default App;
