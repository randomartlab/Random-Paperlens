use std::path::{Path, PathBuf};

/// MinerU 云端解析 API 客户端（Precision Extract API, v4）
///
/// 流程：申请上传链接 → PUT 上传文件 → 轮询批量任务 → 下载并解压结果 zip。
/// 文档参考：https://mineru.net/doc/docs/index_en/
#[derive(Clone)]
pub struct MinerUClient {
    key: String,
    base: String,
}

impl MinerUClient {
    pub fn new(key: String) -> Self {
        Self {
            key,
            base: "https://mineru.net/api/v4".to_string(),
        }
    }

    fn headers(&self) -> reqwest::header::HeaderMap {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", self.key).parse().unwrap(),
        );
        h
    }

    /// 构建 HTTP 客户端：显式加载代理（环境变量 HTTPS_PROXY/HTTP_PROXY 优先）
    /// 当前启用了不安全证书验证（用于诊断代理中断问题），生产环境建议移除
    fn http_client() -> Result<reqwest::blocking::Client, reqwest::Error> {
        let mut builder = reqwest::blocking::Client::builder();
        let proxy_url = std::env::var("HTTPS_PROXY")
            .or_else(|_| std::env::var("https_proxy"))
            .or_else(|_| std::env::var("HTTP_PROXY"))
            .or_else(|_| std::env::var("http_proxy"))
            .ok()
            .filter(|u| !u.is_empty());
        if let Some(url) = proxy_url {
            if let Ok(p) = reqwest::Proxy::all(&url) {
                builder = builder.proxy(p);
            }
        }
        builder.build()
    }

    /// 提交本地 PDF：申请上传链接 → PUT 上传 → 返回 batch_id
    pub fn submit_file(&self, file_path: &Path) -> Result<String, String> {
        let file_name = file_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file.pdf")
            .to_string();

        // 1. 申请上传链接
        let client = Self::http_client().map_err(|e| e.to_string())?;
        let resp = client
            .post(format!("{}/file-urls/batch", self.base))
            .headers(self.headers())
            .json(&serde_json::json!({
                "files": [{ "name": file_name }],
                "model_version": "vlm"
            }))
            .send()
            .map_err(|e| format!("申请上传链接失败: {e}"))?;
        let body: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
        if body["code"] != 0 {
            return Err(format!("MinerU 错误: {}", body["msg"]));
        }
        let batch_id = body["data"]["batch_id"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let upload_url = body["data"]["file_urls"][0].as_str().unwrap_or("").to_string();
        if batch_id.is_empty() || upload_url.is_empty() {
            return Err("上传信息缺失（请检查 API Key 与配额）".into());
        }

        // 2. PUT 上传（MinerU 预签名 URL 签名不含 Content-Type，勿添加自定义 header）
        let file_bytes = std::fs::read(file_path).map_err(|e| e.to_string())?;
        let up = client
            .put(&upload_url)
            .body(file_bytes)
            .send()
            .map_err(|e| format!("文件上传失败: {e}"))?;
        if !up.status().is_success() {
            return Err(format!("文件上传失败 HTTP {}", up.status()));
        }
        Ok(batch_id)
    }

    /// 轮询批量任务直至完成或超时（timeout_secs 秒）
    pub fn poll_batch(
        &self,
        batch_id: &str,
        timeout_secs: u64,
        on_progress: Option<&dyn Fn(&str)>,
    ) -> Result<serde_json::Value, String> {
        let client = Self::http_client().map_err(|e| e.to_string())?;
        let start = std::time::Instant::now();

        loop {
            if start.elapsed().as_secs() > timeout_secs {
                return Err("解析超时".into());
            }
            let resp = client
                .get(format!("{}/extract-results/batch/{}", self.base, batch_id))
                .headers(self.headers())
                .send()
                .map_err(|e| e.to_string())?;
            let body: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
            if body["code"] != 0 {
                return Err(format!("MinerU 错误: {}", body["msg"]));
            }
            let file = &body["data"]["extract_result"][0];
            let state = file["state"].as_str().unwrap_or("");
            match state {
                "done" => return Ok(file.clone()),
                "failed" => {
                    return Err(format!("解析失败: {}", file["err_msg"].as_str().unwrap_or("未知错误")))
                }
                _ => {
                    if let Some(cb) = on_progress {
                        let progress = &file["extract_progress"];
                        let cur = progress["extracted_pages"].as_u64().unwrap_or(0);
                        let total = progress["total_pages"].as_u64().unwrap_or(0);
                        cb(&format!("{cur}/{total}"));
                    }
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
            }
        }
    }

    /// 下载结果 zip 并解压到 dest，返回解压后的 Markdown 文件路径
    /// 使用系统 curl 下载（对 CDN 兼容性最佳，行为与命令行一致）
    pub fn download_extract(
        &self,
        result: &serde_json::Value,
        dest: &Path,
    ) -> Result<PathBuf, String> {
        let zip_url = result["full_zip_url"]
            .as_str()
            .ok_or("结果下载链接缺失")?;

        std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
        let zip_path = dest.join("result.zip");

        // 用系统 curl 下载（macOS 必定自带）。
        // 策略：结果 CDN 为国内节点，优先直连；失败后回退走系统代理。
        let proxy = std::env::var("HTTPS_PROXY")
            .or_else(|_| std::env::var("https_proxy"))
            .ok()
            .filter(|u| !u.is_empty());

        let mut direct = std::process::Command::new("curl");
        direct.args(["-sS", "-L", "--max-time", "600", "--noproxy", "*", "-o"]);
        direct.arg(&zip_path).arg(zip_url);
        let out = match direct.output() {
            Ok(o) if o.status.success() => o,
            _ => {
                // 直连失败 → 走代理重试
                let mut proxied = std::process::Command::new("curl");
                proxied.args(["-sS", "-L", "--max-time", "600", "-o"]);
                proxied.arg(&zip_path);
                if let Some(p) = &proxy {
                    proxied.args(["-x", p]);
                }
                proxied
                    .arg(zip_url)
                    .output()
                    .map_err(|e| format!("curl 调用失败: {e}"))?
            }
        };
        if !out.status.success() {
            return Err(format!(
                "结果下载失败: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }

        // 解压
        let file = std::fs::File::open(&zip_path).map_err(|e| e.to_string())?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
        let md_name = {
            let mut found: Option<String> = None;
            for i in 0..archive.len() {
                let entry = archive.by_index(i).map_err(|e| e.to_string())?;
                if entry.name().ends_with(".md") {
                    found = Some(entry.name().to_string());
                    break;
                }
            }
            found.ok_or("结果中未找到 Markdown 文件")?
        };
        archive.extract(dest).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&zip_path);

        Ok(dest.join(md_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 生成最小合法 PDF（无 xref 表，主流解析器可容忍重建）
    fn minimal_test_pdf() -> Vec<u8> {
        let mut s = String::new();
        s.push_str("%PDF-1.4\n");
        s.push_str("1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n");
        s.push_str("2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n");
        s.push_str("3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Contents 4 0 R/Resources<</Font<</F1 5 0 R>>>>>>endobj\n");
        s.push_str("4 0 obj<</Length 42>>stream\nBT/F1 12 Tf 72 720 Td(Literature Reading Desk MinerU Test) Tj ET\nendstream endobj\n");
        s.push_str("5 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>endobj\n");
        s.push_str("trailer<</Size 6/Root 1 0 R>>\n%%EOF\n");
        s.into_bytes()
    }

    /// 端到端真实解析验证（需要 MINERU_API_KEY 环境变量与网络，默认忽略）
    #[test]
    #[ignore = "需要真实 MINERU_API_KEY 与网络"]
    fn real_parse_flow() {
        let key = std::env::var("MINERU_API_KEY").expect("MINERU_API_KEY 未设置");
        let client = MinerUClient::new(key);

        // 本地生成测试 PDF（避免海外 CDN 访问受限）
        let path = std::env::temp_dir().join("litdesk_m1_test.pdf");
        std::fs::write(&path, minimal_test_pdf()).expect("写临时文件失败");

        // 全流程：上传 → 轮询 → 下载解压
        let batch_id = client.submit_file(&path).expect("提交失败");
        let result = client.poll_batch(&batch_id, 180, None).expect("轮询失败");
        let dest = std::env::temp_dir().join(format!(
            "litdesk_extract_{}",
            uuid::Uuid::new_v4()
        ));
        let md = client.download_extract(&result, &dest).expect("下载解压失败");
        assert!(md.exists(), "Markdown 文件应存在: {}", md.display());
        let content = std::fs::read_to_string(&md).expect("读取 Markdown 失败");
        assert!(
            !content.trim().is_empty(),
            "Markdown 不应为空: {}",
            content
        );

        println!(
            "端到端解析成功: {} ({} bytes)\n--- 内容预览 ---\n{}",
            md.display(),
            content.len(),
            content.chars().take(300).collect::<String>()
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dest);
    }
}
