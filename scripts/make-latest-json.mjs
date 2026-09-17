#!/usr/bin/env node
/**
 * 汇总各平台构建产物，生成 Tauri updater 所需的 latest.json。
 *
 * 输入：
 *   artifacts/               由 actions/download-artifact 展开的产物目录
 *   VERSION                  版本号（如 0.4.2）
 *   GITHUB_REPOSITORY        owner/repo
 *   GITHUB_REF_NAME          标签名（如 v0.4.2）
 *   RELEASE_NOTES            可选，更新说明
 * 输出：
 *   latest.json              各平台的下载地址与签名
 */
import { readFileSync, writeFileSync, readdirSync, statSync } from "node:fs";
import { join, basename } from "node:path";

function required(name) {
  const v = process.env[name];
  if (!v) {
    console.error(`缺少环境变量 ${name}`);
    process.exit(1);
  }
  return v;
}

const version = required("VERSION");
const repo = required("GITHUB_REPOSITORY");
const tag = required("GITHUB_REF_NAME");
const root = process.argv[2] ?? "artifacts";

/** 递归收集文件路径 */
function walk(dir) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    if (statSync(p).isDirectory()) out.push(...walk(p));
    else out.push(p);
  }
  return out;
}

const files = walk(root);

/** 读取某个产物对应的 .sig 签名（Tauri 签名为单行文本） */
function sigFor(file) {
  const sigPath = `${file}.sig`;
  if (!files.includes(sigPath)) return null;
  return readFileSync(sigPath, "utf8").trim();
}

const downloadBase = `https://github.com/${repo}/releases/download/${tag}`;
const platforms = {};

// macOS：更新产物为 .app.tar.gz（配合 createUpdaterArtifacts 生成）
for (const [key, archHint] of [
  ["darwin-aarch64", "aarch64"],
  ["darwin-x86_64", "x64"],
]) {
  const archive = files.find(
    (f) => f.endsWith(".app.tar.gz") && basename(f).includes(archHint),
  );
  if (!archive) continue;
  const signature = sigFor(archive);
  if (!signature) continue;
  platforms[key] = { signature, url: `${downloadBase}/${basename(archive)}` };
}

// Windows：Tauri v2 直接以 NSIS 安装包及其签名为更新产物（不再产出 .nsis.zip）
const winSetup = files.find((f) => f.endsWith("-setup.exe"));
if (winSetup) {
  const signature = sigFor(winSetup);
  if (signature) {
    platforms["windows-x86_64"] = {
      signature,
      url: `${downloadBase}/${basename(winSetup)}`,
    };
  }
}

if (Object.keys(platforms).length === 0) {
  console.error("未找到任何可用的更新产物（检查签名文件是否生成）");
  process.exit(1);
}

const manifest = {
  version,
  notes: process.env.RELEASE_NOTES ?? `Rd学术阅读器 v${version}`,
  pub_date: new Date().toISOString(),
  platforms,
};

writeFileSync("latest.json", JSON.stringify(manifest, null, 2));
console.log("已生成 latest.json：");
console.log(JSON.stringify(manifest, null, 2));
