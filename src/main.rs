use std::collections::{HashMap, HashSet};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::DateTime;
use feed_rs::parser;
use futures::stream::{self, StreamExt};
use once_cell::sync::Lazy;
use reqwest::Client;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;
use tower_http::services::{ServeDir, ServeFile};
use url::Url;

const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) ",
    "AppleWebKit/537.36 (KHTML, like Gecko) ",
    "Chrome/123.0.0.0 Safari/537.36"
);

static KNOWN_FEEDS: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    HashMap::from([
        (
            "https://www.tomshardware.com/",
            "https://www.tomshardware.com/feeds/all",
        ),
        (
            "https://www.anandtech.com/",
            "https://www.anandtech.com/rss/",
        ),
        (
            "https://semianalysis.com/",
            "https://semianalysis.com/feed/",
        ),
        ("https://www.eetimes.com/", "https://www.eetimes.com/feed/"),
        (
            "https://www.servethehome.com/",
            "https://www.servethehome.com/feed/",
        ),
        (
            "https://www.datacenterdynamics.com/",
            "https://www.datacenterdynamics.com/rss/",
        ),
        (
            "https://www.nextplatform.com/",
            "https://www.nextplatform.com/feed/",
        ),
        (
            "https://www.edge-ai-vision.com/",
            "https://www.edge-ai-vision.com/feed/",
        ),
        (
            "https://www.embedded.com/",
            "https://www.embedded.com/feed/",
        ),
        (
            "https://www.androidauthority.com/",
            "https://www.androidauthority.com/feed/",
        ),
        (
            "https://blogs.nvidia.com/",
            "https://blogs.nvidia.com/feed/",
        ),
        (
            "https://www.amd.com/en/newsroom",
            "https://www.amd.com/en/rss.xml",
        ),
        (
            "https://www.qualcomm.com/news",
            "https://www.qualcomm.com/news/rss.xml",
        ),
        (
            "https://news.ycombinator.com/",
            "https://news.ycombinator.com/rss",
        ),
        (
            "https://www.reddit.com/r/hardware/",
            "https://www.reddit.com/r/hardware/.rss",
        ),
        (
            "https://www.reddit.com/r/MachineLearning/",
            "https://www.reddit.com/r/MachineLearning/.rss",
        ),
        (
            "https://www.technologyreview.com/topic/artificial-intelligence/",
            "https://www.technologyreview.com/topic/artificial-intelligence/feed/",
        ),
        (
            "https://venturebeat.com/category/ai/",
            "https://venturebeat.com/category/ai/feed/",
        ),
        (
            "https://www.wired.com/tag/artificial-intelligence/",
            "https://www.wired.com/feed/tag/artificial-intelligence/latest/rss",
        ),
        (
            "https://www.artificialintelligence-news.com/",
            "https://www.artificialintelligence-news.com/feed/",
        ),
        (
            "https://www.aitrends.com/",
            "https://www.aitrends.com/feed/",
        ),
        (
            "https://openai.com/blog/",
            "https://openai.com/news/rss.xml",
        ),
        (
            "https://www.anthropic.com/news",
            "https://www.anthropic.com/news/rss.xml",
        ),
        (
            "https://www.marktechpost.com/",
            "https://www.marktechpost.com/feed/",
        ),
        (
            "https://www.sciencedaily.com/news/computers_math/artificial_intelligence/",
            "https://www.sciencedaily.com/rss/computers_math/artificial_intelligence.xml",
        ),
        (
            "https://www.reddit.com/r/ArtificialIntelligence/",
            "https://www.reddit.com/r/ArtificialIntelligence/.rss",
        ),
    ])
});

#[derive(Clone)]
struct AppState {
    sources_path: PathBuf,
    translate_url: String,
    news_client: Client,
    translate_client: Client,
    translate_cache: Arc<RwLock<HashMap<String, String>>>,
}

#[derive(Debug, Clone, Serialize)]
struct SourceRecord {
    root: String,
    category: String,
    name: String,
    focus: String,
    url: String,
}

#[derive(Debug, Clone, Serialize)]
struct NewsItem {
    title: String,
    url: String,
    published: String,
    source_name: String,
    source_url: String,
    source_category: String,
    source_focus: String,
    title_zh: String,
}

#[derive(Debug, Deserialize)]
struct NewsQuery {
    limit: Option<usize>,
    per_site_limit: Option<usize>,
    max_sources: Option<usize>,
    translate: Option<bool>,
}

#[derive(Debug, Serialize)]
struct NewsResponse {
    id: &'static str,
    translated: bool,
    translate_api_url: String,
    total: usize,
    elapsed_seconds: f64,
    items: Vec<NewsItem>,
}

#[derive(Debug, Serialize)]
struct SourcesResponse {
    id: Option<String>,
    total_sources: usize,
    sources: Value,
}

#[derive(Debug, Deserialize)]
struct TranslateResponse {
    #[serde(rename = "translatedText")]
    translated_text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MyMemoryResponseData {
    #[serde(rename = "translatedText")]
    translated_text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MyMemoryResponse {
    #[serde(rename = "responseData")]
    response_data: Option<MyMemoryResponseData>,
}

#[derive(Debug)]
struct AppError(String, StatusCode);

impl AppError {
    fn internal(message: impl Into<String>) -> Self {
        Self(message.into(), StatusCode::INTERNAL_SERVER_ERROR)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.1, self.0).into_response()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root_dir = env::current_dir()?;
    let frontend_dir = root_dir.join("frontend");
    let sources_path = root_dir.join("backend").join("news_sources.json");

    let translate_url = env::var("TRANSLATE_API_URL")
        .unwrap_or_else(|_| "https://translate.argosopentech.com/translate".to_string());
    let translate_timeout = env::var("TRANSLATE_TIMEOUT_SEC")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(8.0);
    let news_timeout = env::var("NEWS_TIMEOUT_SEC")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(10.0);

    let state = AppState {
        sources_path,
        translate_url,
        news_client: Client::builder()
            .user_agent(USER_AGENT)
            .timeout(std::time::Duration::from_secs_f64(news_timeout))
            .build()?,
        translate_client: Client::builder()
            .user_agent(USER_AGENT)
            .timeout(std::time::Duration::from_secs_f64(translate_timeout))
            .build()?,
        translate_cache: Arc::new(RwLock::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/api/sources", get(get_sources))
        .route("/api/news", get(get_news))
        .nest_service(
            "/",
            ServeDir::new(frontend_dir.clone())
                .append_index_html_on_directories(true)
                .fallback(ServeFile::new(frontend_dir.join("index.html"))),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await?;
    println!("Server running on http://127.0.0.1:8000");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn get_sources(State(state): State<AppState>) -> Result<Json<SourcesResponse>, AppError> {
    let data = load_sources(&state.sources_path).await?;
    let total = flatten_sources(&data).len();
    Ok(Json(SourcesResponse {
        id: data
            .get("id")
            .and_then(|v| v.as_str())
            .map(std::string::ToString::to_string),
        total_sources: total,
        sources: data,
    }))
}

async fn get_news(
    State(state): State<AppState>,
    Query(query): Query<NewsQuery>,
) -> Result<Json<NewsResponse>, AppError> {
    let start = Instant::now();
    let limit = query.limit.unwrap_or(40).clamp(1, 200);
    let per_site_limit = query.per_site_limit.unwrap_or(3).clamp(1, 8);
    let max_sources = query.max_sources.unwrap_or(32).clamp(1, 100);
    let translate = query.translate.unwrap_or(true);

    let all_sources = flatten_sources(&load_sources(&state.sources_path).await?);
    let sources: Vec<SourceRecord> = all_sources.into_iter().take(max_sources).collect();

    let mut collected = Vec::new();
    let mut rows_stream = stream::iter(sources.into_iter().map(|source| {
        let state = state.clone();
        async move {
            let feed_rows = fetch_from_feed(&state, &source, per_site_limit).await;
            if feed_rows.is_empty() {
                fetch_from_html(&state, &source, per_site_limit).await
            } else {
                feed_rows
            }
        }
    }))
    .buffer_unordered(8);

    while let Some(rows) = rows_stream.next().await {
        collected.extend(rows);
    }

    let mut dedup = HashMap::new();
    for item in collected {
        dedup.entry(item.url.clone()).or_insert(item);
    }
    let mut items: Vec<NewsItem> = dedup.into_values().collect();

    if translate {
        for item in &mut items {
            item.title_zh = translate_to_chinese(&state, &item.title).await;
        }
    } else {
        for item in &mut items {
            item.title_zh = item.title.clone();
        }
    }

    items.sort_by(|a, b| {
        timestamp_of(&b.published)
            .partial_cmp(&timestamp_of(&a.published))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    items.truncate(limit);

    Ok(Json(NewsResponse {
        id: "aihw01",
        translated: translate,
        translate_api_url: state.translate_url.clone(),
        total: items.len(),
        elapsed_seconds: (start.elapsed().as_secs_f64() * 1000.0).round() / 1000.0,
        items,
    }))
}

async fn load_sources(path: &Path) -> Result<Value, AppError> {
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| AppError::internal(format!("failed to read news_sources.json: {e}")))?;
    serde_json::from_str(&content)
        .map_err(|e| AppError::internal(format!("invalid news_sources.json: {e}")))
}

fn flatten_sources(data: &Value) -> Vec<SourceRecord> {
    let mut records = Vec::new();
    for root_key in ["ai_hardware_news", "ai_news_websites"] {
        let Some(source_group) = data.get(root_key).and_then(|v| v.as_object()) else {
            continue;
        };
        for (category, source_list) in source_group {
            let Some(items) = source_list.as_array() else {
                continue;
            };
            for source in items {
                let name = source
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let focus = source
                    .get("focus")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let url = source
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if name.is_empty() || url.is_empty() {
                    continue;
                }
                records.push(SourceRecord {
                    root: root_key.to_string(),
                    category: category.clone(),
                    name: name.to_string(),
                    focus: focus.to_string(),
                    url: url.to_string(),
                });
            }
        }
    }
    records
}

async fn fetch_from_feed(
    state: &AppState,
    source: &SourceRecord,
    per_site_limit: usize,
) -> Vec<NewsItem> {
    let feed_url = KNOWN_FEEDS
        .get(source.url.as_str())
        .copied()
        .unwrap_or(source.url.as_str());
    let Ok(resp) = state.news_client.get(feed_url).send().await else {
        return Vec::new();
    };
    let Ok(resp) = resp.error_for_status() else {
        return Vec::new();
    };
    let Ok(bytes) = resp.bytes().await else {
        return Vec::new();
    };
    let Ok(feed) = parser::parse(&bytes[..]) else {
        return Vec::new();
    };

    feed.entries
        .iter()
        .take(per_site_limit)
        .filter_map(|entry| {
            let title = entry
                .title
                .as_ref()
                .map(|t| t.content.trim().to_string())
                .unwrap_or_default();
            if title.is_empty() {
                return None;
            }
            let link = entry
                .links
                .first()
                .map(|l| l.href.trim().to_string())
                .filter(|l| !l.is_empty())
                .filter(|l| is_allowed_http_url(l))
                .unwrap_or_else(|| source.url.clone());
            let published = entry
                .published
                .as_ref()
                .or(entry.updated.as_ref())
                .map(|d| d.to_rfc3339())
                .unwrap_or_default();
            Some(NewsItem {
                title,
                url: link,
                published,
                source_name: source.name.clone(),
                source_url: source.url.clone(),
                source_category: source.category.clone(),
                source_focus: source.focus.clone(),
                title_zh: String::new(),
            })
        })
        .collect()
}

async fn fetch_from_html(
    state: &AppState,
    source: &SourceRecord,
    per_site_limit: usize,
) -> Vec<NewsItem> {
    let Ok(resp) = state.news_client.get(&source.url).send().await else {
        return Vec::new();
    };
    let Ok(resp) = resp.error_for_status() else {
        return Vec::new();
    };
    let Ok(body) = resp.text().await else {
        return Vec::new();
    };
    let base = Url::parse(&source.url).ok();
    let document = Html::parse_document(&body);
    let selector = Selector::parse("a[href]").expect("valid css selector");
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut results = Vec::new();

    for anchor in document.select(&selector) {
        let title = anchor
            .text()
            .collect::<Vec<_>>()
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if title.chars().count() < 18 {
            continue;
        }
        let Some(href) = anchor.value().attr("href") else {
            continue;
        };
        let Some(url) = normalize_url(base.as_ref(), href) else {
            continue;
        };
        let key = (title.clone(), url.clone());
        if !seen.insert(key) {
            continue;
        }

        results.push(NewsItem {
            title,
            url,
            published: String::new(),
            source_name: source.name.clone(),
            source_url: source.url.clone(),
            source_category: source.category.clone(),
            source_focus: source.focus.clone(),
            title_zh: String::new(),
        });

        if results.len() >= per_site_limit {
            break;
        }
    }
    results
}

fn normalize_url(base: Option<&Url>, href: &str) -> Option<String> {
    if href.starts_with("http://") || href.starts_with("https://") {
        return is_allowed_http_url(href).then(|| href.to_string());
    }
    let base = base?;
    let joined = base.join(href).ok()?;
    is_allowed_http_url(joined.as_str()).then(|| joined.to_string())
}

async fn translate_to_chinese(state: &AppState, text: &str) -> String {
    if text.is_empty() {
        return text.to_string();
    }

    {
        let cache = state.translate_cache.read().await;
        if let Some(found) = cache.get(text) {
            return found.clone();
        }
    }

    let payload = serde_json::json!({
        "q": text,
        "source": "auto",
        "target": "zh",
        "format": "text"
    });

    let primary = match state
        .translate_client
        .post(&state.translate_url)
        .json(&payload)
        .send()
        .await
    {
        Ok(resp) => match resp.error_for_status() {
            Ok(ok_resp) => match ok_resp.json::<TranslateResponse>().await {
                Ok(data) => data
                    .translated_text
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| text.to_string()),
                Err(_) => text.to_string(),
            },
            Err(_) => text.to_string(),
        },
        Err(_) => text.to_string(),
    };

    let translated = if primary != text {
        primary
    } else if let Some(mm) = translate_with_mymemory(state, text).await {
        mm
    } else {
        translate_with_google_gtx(state, text)
            .await
            .unwrap_or_else(|| text.to_string())
    };

    let mut cache = state.translate_cache.write().await;
    cache.insert(text.to_string(), translated.clone());
    translated
}

async fn translate_with_mymemory(state: &AppState, text: &str) -> Option<String> {
    let resp = state
        .translate_client
        .get("https://api.mymemory.translated.net/get")
        .query(&[("q", text), ("langpair", "auto|zh-CN")])
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;

    let data = resp.json::<MyMemoryResponse>().await.ok()?;
    let translated = data.response_data?.translated_text?.trim().to_string();
    if translated.is_empty() || translated == text {
        None
    } else {
        Some(translated)
    }
}

async fn translate_with_google_gtx(state: &AppState, text: &str) -> Option<String> {
    let resp = state
        .translate_client
        .get("https://translate.googleapis.com/translate_a/single")
        .query(&[
            ("client", "gtx"),
            ("sl", "auto"),
            ("tl", "zh-CN"),
            ("dt", "t"),
            ("q", text),
        ])
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;

    let data = resp.json::<Value>().await.ok()?;
    let translated = data.get(0)?.get(0)?.get(0)?.as_str()?.trim().to_string();

    if translated.is_empty() || translated == text {
        None
    } else {
        Some(translated)
    }
}

fn timestamp_of(published: &str) -> f64 {
    if published.is_empty() {
        return -1.0;
    }
    DateTime::parse_from_rfc3339(published)
        .map(|dt| dt.timestamp() as f64)
        .unwrap_or(-1.0)
}

fn is_allowed_http_url(url: &str) -> bool {
    Url::parse(url)
        .ok()
        .map(|u| u.scheme() == "http" || u.scheme() == "https")
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{is_allowed_http_url, normalize_url, timestamp_of};
    use url::Url;

    #[test]
    fn test_allowed_http_url() {
        assert!(is_allowed_http_url("https://example.com/a"));
        assert!(is_allowed_http_url("http://example.com/a"));
        assert!(!is_allowed_http_url("javascript:alert(1)"));
        assert!(!is_allowed_http_url("data:text/html,abc"));
    }

    #[test]
    fn test_normalize_url_keeps_http_https_only() {
        let base = Url::parse("https://example.com/news/").expect("base url");
        assert_eq!(
            normalize_url(Some(&base), "/item/1"),
            Some("https://example.com/item/1".to_string())
        );
        assert_eq!(normalize_url(Some(&base), "javascript:alert(1)"), None);
    }

    #[test]
    fn test_timestamp_of_rfc3339() {
        assert!(timestamp_of("2026-04-24T12:00:00+00:00") > 0.0);
        assert_eq!(timestamp_of("invalid"), -1.0);
    }

    #[test]
    fn test_allow_relative_then_joined_as_http() {
        let base = Url::parse("https://example.com/").expect("base url");
        assert_eq!(
            normalize_url(Some(&base), "article/123"),
            Some("https://example.com/article/123".to_string())
        );
    }
}
