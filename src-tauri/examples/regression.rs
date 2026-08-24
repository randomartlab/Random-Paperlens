//! M5.3 样本集回归（MinerU 解析成功率）
//! 用法：cargo run --manifest-path src-tauri/Cargo.toml --example regression -- <样本目录> [输出目录]
//! 样本目录需含 ≥10 篇多学科英文 PDF；MinerU Key 从环境变量 MINERU_API_KEY 或项目根 .env 读取。
//! 验收线：解析成功率 ≥95%（对应 PRD/开发计划 M5.3）。

use litdesk_lib::mineru::MinerUClient;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn main() {
    let mut args = std::env::args();
    args.next();
    let samples_dir = args.next().unwrap_or_else(|| "samples".to_string());
    let out_dir = args.next().unwrap_or_else(|| "target/regression-out".to_string());

    let Some(key) = load_mineru_key() else {
        eprintln!("未找到 MINERU_API_KEY（环境变量或项目根 .env）");
        std::process::exit(2);
    };
    let client = MinerUClient::new(key);

    let mut pdfs: Vec<PathBuf> = std::fs::read_dir(&samples_dir)
        .unwrap_or_else(|e| {
            eprintln!("无法读取样本目录 {samples_dir}: {e}");
            std::process::exit(2);
        })
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("pdf"))
                .unwrap_or(false)
        })
        .collect();
    pdfs.sort();
    if pdfs.is_empty() {
        eprintln!("样本目录 {samples_dir} 下未找到 PDF（要求 ≥10 篇多学科英文文献）");
        std::process::exit(2);
    }

    std::fs::create_dir_all(&out_dir).ok();

    println!("========== MinerU 样本集回归（{samples_dir}） ==========");
    println!("样本数: {}", pdfs.len());
    println!();

    let mut ok = 0usize;
    let mut fail: Vec<(String, String)> = Vec::new();
    let mut total_ms: u128 = 0;

    for (i, pdf) in pdfs.iter().enumerate() {
        let name = pdf
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let t = Instant::now();
        let dest = Path::new(&out_dir).join(format!("{i:02}"));
        let r = run_one(&client, pdf, &dest);
        let ms = t.elapsed().as_millis();
        total_ms += ms;
        match r {
            Ok(size) => {
                ok += 1;
                println!(
                    "[{:>2}/{:<2}] OK   {name}  {:.1}s  产物 {size} 字节",
                    i + 1,
                    pdfs.len(),
                    ms as f64 / 1000.0
                );
            }
            Err(e) => {
                fail.push((name.clone(), e.clone()));
                println!(
                    "[{:>2}/{:<2}] FAIL {name}  {:.1}s  {e}",
                    i + 1,
                    pdfs.len(),
                    ms as f64 / 1000.0
                );
            }
        }
    }

    let n = pdfs.len();
    let rate = if n > 0 {
        ok as f64 / n as f64 * 100.0
    } else {
        0.0
    };
    let avg = if n > 0 {
        total_ms as f64 / n as f64 / 1000.0
    } else {
        0.0
    };
    println!();
    println!("========== 汇总 ==========");
    println!("成功 {ok}/{n}，解析成功率 {rate:.1}%（验收线 ≥95%）");
    println!("平均单篇耗时 {avg:.1}s");
    if !fail.is_empty() {
        println!("失败明细:");
        for (name, e) in &fail {
            println!("  - {name}: {e}");
        }
    }
    if rate < 95.0 {
        println!("结论: 未达标（<95%），需排查失败样本");
        std::process::exit(1);
    } else {
        println!("结论: 达标");
    }
}

fn run_one(client: &MinerUClient, pdf: &Path, dest: &Path) -> Result<u64, String> {
    // MinerU 服务端偶发临时失败（parsing failed / please try again later），自动重试一次
    let mut last_err = String::new();
    for attempt in 0..2 {
        match attempt_once(client, pdf, dest) {
            Ok(s) => return Ok(s),
            Err(e)
                if e.contains("parsing failed") || e.contains("please try again later") =>
            {
                last_err = e.clone();
                eprintln!("      临时失败，重试({}/2): {e}", attempt + 1);
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_err)
}

fn attempt_once(client: &MinerUClient, pdf: &Path, dest: &Path) -> Result<u64, String> {
    let batch = client.submit_file(pdf)?;
    let result = client.poll_batch(&batch, 900, None)?;
    let md = client.download_extract(&result, dest)?;
    let size = std::fs::metadata(&md).map(|m| m.len()).unwrap_or(0);
    if size == 0 {
        return Err("解析产物为空".into());
    }
    Ok(size)
}

fn load_mineru_key() -> Option<String> {
    if let Ok(k) = std::env::var("MINERU_API_KEY") {
        if !k.is_empty() {
            return Some(k);
        }
    }
    for path in [".env", "src-tauri/.env"] {
        if let Ok(content) = std::fs::read_to_string(path) {
            for line in content.lines() {
                if let Some(v) = line.trim().strip_prefix("MINERU_API_KEY=") {
                    let v = v.trim().trim_matches('"').trim_matches('\'');
                    if !v.is_empty() {
                        return Some(v.to_string());
                    }
                }
            }
        }
    }
    None
}
