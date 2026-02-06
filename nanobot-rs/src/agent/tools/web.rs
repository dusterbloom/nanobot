//! Web tools: web_search and web_fetch.

use std::collections::HashMap;

use async_trait::async_trait;
use regex::Regex;
use reqwest::Client;
use url::Url;

use super::base::Tool;

/// Shared user-agent string.
const USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_7_2) AppleWebKit/537.36";

/// Maximum number of redirects to follow.
const MAX_REDIRECTS: usize = 5;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Remove HTML tags and decode entities.
fn strip_tags(text: &str) -> String {
    // Remove script and style blocks.
    let re_script = Regex::new(r"(?is)<script[\s\S]*?</script>").unwrap();
    let text = re_script.replace_all(text, "");
    let re_style = Regex::new(r"(?is)<style[\s\S]*?</style>").unwrap();
    let text = re_style.replace_all(&text, "");
    // Remove remaining tags.
    let re_tags = Regex::new(r"<[^>]+>").unwrap();
    let text = re_tags.replace_all(&text, "");
    html_escape::decode_html_entities(&text).trim().to_string()
}

/// Normalize whitespace: collapse runs of spaces/tabs, limit consecutive newlines.
fn normalize_whitespace(text: &str) -> String {
    let re_spaces = Regex::new(r"[ \t]+").unwrap();
    let text = re_spaces.replace_all(text, " ");
    let re_newlines = Regex::new(r"\n{3,}").unwrap();
    re_newlines.replace_all(&text, "\n\n").trim().to_string()
}

/// Validate a URL: must be http(s) with a valid domain.
fn validate_url(url_str: &str) -> Result<(), String> {
    let parsed = Url::parse(url_str).map_err(|e| format!("Invalid URL: {}", e))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(format!(
                "Only http/https allowed, got '{}'",
                other
            ))
        }
    }
    if parsed.host_str().is_none() {
        return Err("Missing domain".to_string());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// WebSearchTool
// ---------------------------------------------------------------------------

/// Search the web using Brave Search API.
pub struct WebSearchTool {
    api_key: String,
    max_results: u32,
    client: Client,
}

impl WebSearchTool {
    /// Create a new web search tool.
    ///
    /// If `api_key` is empty/None, the `BRAVE_API_KEY` environment variable is
    /// checked.
    pub fn new(api_key: Option<String>, max_results: u32) -> Self {
        let resolved_key = api_key
            .filter(|k| !k.is_empty())
            .or_else(|| std::env::var("BRAVE_API_KEY").ok())
            .unwrap_or_default();

        Self {
            api_key: resolved_key,
            max_results,
            client: Client::new(),
        }
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web. Returns titles, URLs, and snippets."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "count": {
                    "type": "integer",
                    "description": "Results (1-10)",
                    "minimum": 1,
                    "maximum": 10
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, params: HashMap<String, serde_json::Value>) -> String {
        let query = match params.get("query").and_then(|v| v.as_str()) {
            Some(q) => q,
            None => return "Error: 'query' parameter is required".to_string(),
        };

        if self.api_key.is_empty() {
            return "Error: BRAVE_API_KEY not configured".to_string();
        }

        let count = params
            .get("count")
            .and_then(|v| v.as_u64())
            .map(|n| n.min(10).max(1) as u32)
            .unwrap_or(self.max_results);

        match self
            .client
            .get("https://api.search.brave.com/res/v1/web/search")
            .query(&[("q", query), ("count", &count.to_string())])
            .header("Accept", "application/json")
            .header("X-Subscription-Token", &self.api_key)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
        {
            Ok(response) => {
                if !response.status().is_success() {
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();
                    return format!("Error: Brave Search returned HTTP {}: {}", status, body);
                }

                match response.json::<serde_json::Value>().await {
                    Ok(data) => {
                        let results = data
                            .get("web")
                            .and_then(|w| w.get("results"))
                            .and_then(|r| r.as_array())
                            .cloned()
                            .unwrap_or_default();

                        if results.is_empty() {
                            return format!("No results for: {}", query);
                        }

                        let mut lines = vec![format!("Results for: {}\n", query)];
                        for (i, item) in results.iter().take(count as usize).enumerate() {
                            let title = item
                                .get("title")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let url = item
                                .get("url")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            lines.push(format!("{}. {}\n   {}", i + 1, title, url));

                            if let Some(desc) = item.get("description").and_then(|v| v.as_str()) {
                                lines.push(format!("   {}", desc));
                            }
                        }
                        lines.join("\n")
                    }
                    Err(e) => format!("Error parsing search results: {}", e),
                }
            }
            Err(e) => format!("Error: {}", e),
        }
    }
}

// ---------------------------------------------------------------------------
// WebFetchTool
// ---------------------------------------------------------------------------

/// Fetch and extract content from a URL.
pub struct WebFetchTool {
    max_chars: usize,
    client: Client,
}

impl WebFetchTool {
    /// Create a new web fetch tool.
    pub fn new(max_chars: usize) -> Self {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
            .user_agent(USER_AGENT)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self { max_chars, client }
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetch URL and extract readable content (HTML -> text)."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL to fetch"
                },
                "extractMode": {
                    "type": "string",
                    "enum": ["markdown", "text"],
                    "default": "markdown"
                },
                "maxChars": {
                    "type": "integer",
                    "minimum": 100
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, params: HashMap<String, serde_json::Value>) -> String {
        let url = match params.get("url").and_then(|v| v.as_str()) {
            Some(u) => u,
            None => return serde_json::json!({"error": "url parameter is required"}).to_string(),
        };

        let extract_mode = params
            .get("extractMode")
            .and_then(|v| v.as_str())
            .unwrap_or("markdown");

        let max_chars = params
            .get("maxChars")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(self.max_chars);

        // Validate URL.
        if let Err(e) = validate_url(url) {
            return serde_json::json!({
                "error": format!("URL validation failed: {}", e),
                "url": url
            })
            .to_string();
        }

        match self.client.get(url).send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                let final_url = response.url().to_string();
                let content_type = response
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();

                match response.text().await {
                    Ok(body) => {
                        let (text, extractor) = if content_type.contains("application/json") {
                            // Pretty-print JSON.
                            let formatted = match serde_json::from_str::<serde_json::Value>(&body)
                            {
                                Ok(v) => {
                                    serde_json::to_string_pretty(&v).unwrap_or_else(|_| body.clone())
                                }
                                Err(_) => body.clone(),
                            };
                            (formatted, "json")
                        } else if content_type.contains("text/html")
                            || body.trim_start().to_lowercase().starts_with("<!doctype")
                            || body.trim_start().to_lowercase().starts_with("<html")
                        {
                            // Extract text from HTML using scraper.
                            let extracted = extract_html_content(&body, extract_mode);
                            (extracted, "scraper")
                        } else {
                            (body, "raw")
                        };

                        let truncated = text.len() > max_chars;
                        let final_text = if truncated {
                            text[..max_chars].to_string()
                        } else {
                            text.clone()
                        };

                        serde_json::json!({
                            "url": url,
                            "finalUrl": final_url,
                            "status": status,
                            "extractor": extractor,
                            "truncated": truncated,
                            "length": final_text.len(),
                            "text": final_text
                        })
                        .to_string()
                    }
                    Err(e) => serde_json::json!({
                        "error": format!("Failed to read response body: {}", e),
                        "url": url
                    })
                    .to_string(),
                }
            }
            Err(e) => serde_json::json!({
                "error": e.to_string(),
                "url": url
            })
            .to_string(),
        }
    }
}

/// Extract readable content from HTML using the `scraper` crate.
///
/// This is a simplified readability extraction: we look for the `<body>` or
/// `<main>` or `<article>` element and extract text, falling back to the whole
/// document if those are not found.
fn extract_html_content(html: &str, mode: &str) -> String {
    use scraper::{Html, Selector};

    let document = Html::parse_document(html);

    // Try to extract title.
    let title = Selector::parse("title")
        .ok()
        .and_then(|sel| document.select(&sel).next())
        .map(|el| el.text().collect::<String>())
        .unwrap_or_default();

    // Try progressively narrower selectors.
    let selectors = ["article", "main", "[role=\"main\"]", "body"];
    let mut body_text = String::new();

    for sel_str in &selectors {
        if let Ok(sel) = Selector::parse(sel_str) {
            if let Some(el) = document.select(&sel).next() {
                body_text = if mode == "markdown" {
                    html_to_markdown_simple(&el.html())
                } else {
                    el.text().collect::<Vec<_>>().join(" ")
                };
                if !body_text.trim().is_empty() {
                    break;
                }
            }
        }
    }

    if body_text.trim().is_empty() {
        // Fallback: extract all text from the document.
        body_text = document.root_element().text().collect::<Vec<_>>().join(" ");
    }

    let result = normalize_whitespace(&body_text);

    if title.is_empty() {
        result
    } else {
        format!("# {}\n\n{}", title.trim(), result)
    }
}

/// Very simple HTML-to-markdown converter.
fn html_to_markdown_simple(html: &str) -> String {
    // Convert links.
    let re_links =
        Regex::new(r#"(?is)<a\s+[^>]*href=["']([^"']+)["'][^>]*>([\s\S]*?)</a>"#).unwrap();
    let text = re_links.replace_all(html, |caps: &regex::Captures| {
        let href = &caps[1];
        let inner = strip_tags(&caps[2]);
        format!("[{}]({})", inner, href)
    });

    // Convert headings.
    let re_headings = Regex::new(r"(?is)<h([1-6])[^>]*>([\s\S]*?)</h\1>").unwrap();
    let text = re_headings.replace_all(&text, |caps: &regex::Captures| {
        let level: usize = caps[1].parse().unwrap_or(1);
        let inner = strip_tags(&caps[2]);
        format!("\n{} {}\n", "#".repeat(level), inner)
    });

    // Convert list items.
    let re_li = Regex::new(r"(?is)<li[^>]*>([\s\S]*?)</li>").unwrap();
    let text = re_li.replace_all(&text, |caps: &regex::Captures| {
        let inner = strip_tags(&caps[1]);
        format!("\n- {}", inner)
    });

    // Convert block-end tags to newlines.
    let re_block = Regex::new(r"(?i)</(p|div|section|article)>").unwrap();
    let text = re_block.replace_all(&text, "\n\n");

    // Convert br/hr.
    let re_br = Regex::new(r"(?i)<(br|hr)\s*/?>").unwrap();
    let text = re_br.replace_all(&text, "\n");

    normalize_whitespace(&strip_tags(&text))
}
