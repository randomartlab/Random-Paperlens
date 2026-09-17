/**
 * 阅读器「摘录出处」逻辑的校验脚本。
 *
 * 为什么不放进现有测试：前端没有测试框架，而这段逻辑是纯函数，
 * Node 22 自带类型擦除就能直接跑，不必为它引入依赖。
 *
 *   node --experimental-strip-types scripts/check-reader-locator.ts
 *   node --experimental-strip-types scripts/check-reader-locator.ts --real <parsed/full.md>
 *
 * 断言分两部分：
 * 1) 内置样例：章节路径、段号计数、无标题降级、表格/图片穿透、摘录拼装格式；
 * 2) 真实文档（可选）：用实际解析出的 Markdown 检查定位是否落在合理章节上。
 */
import { readFileSync } from "node:fs";
import {
  buildExcerpt,
  buildLocatorList,
  locatorListFromBlocks,
  mdHeadingLevel,
  resolveExcerptTarget,
  splitInteractiveBlocks,
  stripMdInline,
} from "../src/readerText.ts";

let failed = 0;
function check(label: string, actual: unknown, expected: unknown) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a === e) {
    console.log(`  ✓ ${label}`);
  } else {
    failed += 1;
    console.log(`  ✗ ${label}\n      期望 ${e}\n      实际 ${a}`);
  }
}

console.log("内置样例：");

const sample = [
  "# 1. Introduction",
  "First paragraph.",
  "",
  "Second paragraph.",
  "",
  "## 1.1 Background",
  "| a | b |",
  "| - | - |",
  "",
  "Third paragraph.",
  "",
  "> a quote block",
  "",
  "![fig](images/a.jpg)",
  "",
  "Fourth paragraph.",
  "",
  "## 1.2 Prior work",
  "Fifth paragraph.",
].join("\n");

const blocks = splitInteractiveBlocks(sample);
const locs = locatorListFromBlocks(blocks);
const pairs = blocks.map((b, i) => [b.text.split("\n")[0].slice(0, 28), locs[i]]);

check("标题层级解析", mdHeadingLevel("### 3.1 x"), 3);
check("非标题返回 null", mdHeadingLevel("- 列表项"), null);
check("标题去符号", stripMdInline("## **1.1** Background"), "1.1 Background");

const bodyLocs = pairs.filter(([, l]) => l !== "").map(([, l]) => l);
check("章节路径 + 段号连续", bodyLocs, [
  "1. Introduction · 第 1 段",
  "1. Introduction · 第 2 段",
  "1. Introduction › 1.1 Background · 第 3 段",
  "1. Introduction › 1.1 Background · 第 4 段",
  "1. Introduction › 1.2 Prior work · 第 5 段",
]);
check("表格/图片/引用不计段也不改路径", locs[6], "");

// 无标题文档：退化成纯段号
check(
  "无标题时只给段号",
  buildLocatorList([{ kind: "body" }, { kind: "body" }]),
  ["第 1 段", "第 2 段"],
);

// 摘录拼装
check(
  "摘录带出处（多行引用统一加 >）",
  buildExcerpt("Line1\nLine2", "1. Introduction · 第 3 段"),
  "> Line1\n> Line2\n> —— 1. Introduction · 第 3 段",
);
check(
  "定位缺失时回退题目",
  buildExcerpt("Words", "", "Some Title"),
  "> Words\n> —— 《Some Title》",
);

// 一级标题就是题目时，路径里不应再出现题目
const titled = ["# Same As Title", "Body one.", "", "## 1.1 Sub", "Body two."].join("\n");
check(
  "一级标题等于题目时从路径剔除",
  locatorListFromBlocks(splitInteractiveBlocks(titled), "Same As Title"),
  ["", "第 1 段", "", "1.1 Sub · 第 2 段"],
);

// 右键落点解析：决定"这次摘整句还是整段"
type Node = { dataset: Record<string, string>; parent: Node | null };
/** 模仿真实 DOM：dataset.exFull 对应 data-ex-full */
function ds(obj: Record<string, string>): Record<string, string> {
  const kebab = (k: string) => k.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`);
  return new Proxy(obj, {
    get: (t, k) => (typeof k === "string" ? (t[k] ?? t[kebab(k)]) : undefined),
    has: (t, k) => t[k as string] !== undefined || kebab(String(k)) in t,
  });
}
function el(dataset: Record<string, string>, parent: Node | null = null): any {
  const self: any = { dataset: ds(dataset), parentElement: parent };
  self.closest = (sel: string) => {
    // 只支持 [data-xxx] 这种选择器，转成 dataset 的键名
    const key = sel.replace(/^\[data-/, "").replace(/\]$/, "");
    let cur: any = self;
    while (cur) {
      if (cur.dataset && key in cur.dataset) return cur;
      cur = cur.parentElement;
    }
    return null;
  };
  return self;
}

const pairHolder = el({
  loc: "2. Related studies › 2.1. Body movements · 第 12 段",
  "ex-full": "ORIGINAL PARAGRAPH\n\n译文整段",
});
const sentence = el({ ex: "ORIGINAL SENTENCE\n译文句子" }, pairHolder);
check("落在句子上 → 取整句（原文 + 译文）", resolveExcerptTarget(sentence)?.text, "ORIGINAL SENTENCE\n译文句子");
check(
  "落在句间空白 → 退为整段",
  resolveExcerptTarget(el({}, pairHolder))?.text,
  "ORIGINAL PARAGRAPH\n\n译文整段",
);
check(
  "出处随落点所在段落给出",
  resolveExcerptTarget(sentence)?.loc,
  "2. Related studies › 2.1. Body movements · 第 12 段",
);
check("无可摘内容 → 交回原生菜单", resolveExcerptTarget(el({}, pairHolder.parent)), null);
check("空文本不视为可摘", resolveExcerptTarget(el({ ex: "  " }, pairHolder)), null);

const realIdx = process.argv.indexOf("--real");
if (realIdx !== -1) {
  const path = process.argv[realIdx + 1];
  console.log(`\n真实文档：${path}`);
  const md = readFileSync(path, "utf8");
  // 应用里传的是文档标题（正文一级标题），这里照做，才能验证"题目不进路径"
  const titleLine = md.match(/^#\s+(.+)$/m);
  const docTitle = titleLine ? titleLine[1].trim() : undefined;
  const rb = splitInteractiveBlocks(md);
  const rl = locatorListFromBlocks(rb, docTitle);
  const nonEmpty = rl.filter((l) => l !== "");
  console.log(`  块数 ${rb.length}，可定位段 ${nonEmpty.length}`);
  const uniq = [...new Set(nonEmpty)];
  // 抽两头与中间各一段看看落点是否合理
  const picks = [uniq[0], uniq[Math.floor(uniq.length / 2)], uniq[uniq.length - 1]];
  for (const p of picks) console.log(`   · ${p}`);
  check("段号不重复", uniq.length, nonEmpty.length);
  check("每段都带段号", nonEmpty.every((l) => /第 \d+ 段$/.test(l)), true);
  // 注意：这篇论文的章节在解析结果里同属一级，路径只有一层（没有 › 嵌套），
  // 所以这里只断言"至少有段落带出了章节名"，不假设一定有两级。
  check(
    "至少部分段落带出章节名",
    uniq.some((l) => !/^第 \d+ 段$/.test(l)),
    true,
  );
  check(
    "路径里不应重复论文题目",
    uniq.some((l) => l.startsWith("Confiding to AI")),
    false,
  );
}

console.log(failed === 0 ? "\n全部通过" : `\n${failed} 项未通过`);
process.exit(failed === 0 ? 0 : 1);
