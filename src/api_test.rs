// API 连接测试模块
use reqwest::{Client, Proxy, header::{HeaderMap, HeaderValue}};

/// 安全截断字符串（按字符边界，避免 UTF-8 multi-byte panic）
fn trunc(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes { return s; }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) { end -= 1; }
    &s[..end]
}

pub async fn test_anthropic(
    base_url: &str,
    key: &str,
    key_type: &str,
    proxy: Option<&str>,
    model: Option<&str>,
    timeout_secs: u64,
) -> Result<String, String> {
    let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));

    let mut headers = HeaderMap::new();
    if !key.is_empty() {
        if key_type == "api_key" {
            headers.insert("x-api-key", HeaderValue::from_str(key).map_err(|_| "Invalid key")?);
        } else {
            headers.insert("Authorization", HeaderValue::from_str(&format!("Bearer {}", key)).map_err(|_| "Invalid key")?);
        }
    }
    headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));

    let model_id = model.unwrap_or("claude-haiku-4-5");
    let data = serde_json::json!({
        "model": model_id,
        "max_tokens": 1,
        "messages": [{"role": "user", "content": "hi"}],
    });

    let client = build_client(proxy, timeout_secs)?;
    let response = client
        .post(&url)
        .headers(headers)
        .json(&data)
        .send()
        .await
        .map_err(|e| format!("请求失败：{}", e))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if status.is_success() {
        Ok(format!("HTTP {} - API 正常响应", status))
    } else if status.as_u16() == 401 {
        Err(format!("认证失败 (HTTP 401): Key 无效或已过期\n{}", trunc(&body, 200)))
    } else if status.as_u16() == 403 {
        Err(format!("权限不足 (HTTP 403)\n{}", trunc(&body, 200)))
    } else if [400, 422, 429].contains(&status.as_u16()) {
        Ok(format!("HTTP {} - API 可达，Key 已验证 ({})", status, extract_error(&body)))
    } else {
        Err(format!("HTTP {}\n{}", status, trunc(&body, 300)))
    }
}

pub async fn test_openai(
    base_url: &str,
    key: &str,
    proxy: Option<&str>,
    timeout_secs: u64,
) -> Result<String, String> {
    let stripped = base_url.trim_end_matches('/');
    let url = if stripped.ends_with("/v1") {
        format!("{}/models", stripped)
    } else {
        format!("{}/v1/models", stripped)
    };

    let mut headers = HeaderMap::new();
    if !key.is_empty() {
        headers.insert("Authorization", HeaderValue::from_str(&format!("Bearer {}", key)).map_err(|_| "Invalid key")?);
    }

    let client = build_client(proxy, timeout_secs)?;
    let response = client
        .get(&url)
        .headers(headers)
        .send()
        .await
        .map_err(|e| format!("请求失败：{}", e))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if status.is_success() {
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&body) {
            let count = data.get("data").map(|d| d.as_array().map(|a| a.len()).unwrap_or(0)).unwrap_or(0);
            Ok(format!("HTTP {} - 获取到 {} 个模型", status, count))
        } else {
            Ok(format!("HTTP {} - 获取到模型列表", status))
        }
    } else if status.as_u16() == 401 {
        Err(format!("认证失败 (HTTP 401): Key 无效\n{}", trunc(&body, 200)))
    } else {
        Err(format!("HTTP {}\n{}", status, trunc(&body, 300)))
    }
}

pub async fn test_gemini(
    base_url: &str,
    key: &str,
    proxy: Option<&str>,
    timeout_secs: u64,
) -> Result<String, String> {
    let stripped = base_url.trim_end_matches('/');
    let url = if stripped.ends_with("/models") {
        stripped.to_string()
    } else {
        format!("{}/v1beta/models", stripped)
    };

    let url = if key.is_empty() {
        url
    } else {
        format!("{}?key={}", url, key)
    };

    let client = build_client(proxy, timeout_secs)?;
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("请求失败：{}", e))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if status.is_success() {
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&body) {
            let count = data.get("models").map(|m| m.as_array().map(|a| a.len()).unwrap_or(0)).unwrap_or(0);
            Ok(format!("HTTP {} - 获取到 {} 个模型", status, count))
        } else {
            Ok(format!("HTTP {} - 获取到模型列表", status))
        }
    } else if [400, 401, 403].contains(&status.as_u16()) {
        Err(format!("认证失败 (HTTP {})\n{}", status, trunc(&body, 200)))
    } else {
        Err(format!("HTTP {}\n{}", status, trunc(&body, 300)))
    }
}

pub async fn test_generic(
    base_url: &str,
    proxy: Option<&str>,
    timeout_secs: u64,
) -> Result<String, String> {
    let client = build_client(proxy, timeout_secs)?;
    let response = client
        .get(base_url)
        .send()
        .await
        .map_err(|e| format!("请求失败：{}", e))?;

    let status = response.status();
    if status.is_success() {
        Ok(format!("HTTP {} - 地址可达", status))
    } else {
        Ok(format!("HTTP {} - 地址可达（服务器有响应）", status))
    }
}

fn build_client(proxy: Option<&str>, timeout_secs: u64) -> Result<Client, String> {
    let mut builder = Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs));
    if let Some(p) = proxy {
        let proxy = Proxy::all(p).map_err(|e| format!("代理配置失败：{}", e))?;
        builder = builder.proxy(proxy);
    }
    builder.build().map_err(|e| format!("客户端创建失败：{}", e))
}

fn extract_error(body: &str) -> String {
    if let Ok(data) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(error) = data.get("error") {
            if let Some(msg) = error.get("message").and_then(|m| m.as_str()) {
                return msg[..msg.len().min(80)].to_string();
            }
            if let Some(type_) = error.get("type").and_then(|t| t.as_str()) {
                return type_[..type_.len().min(80)].to_string();
            }
        }
    }
    trunc(body, 80).to_string()
}
