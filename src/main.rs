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
use rusqlite::{params, Connection};
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
    db_path: PathBuf,
    translate_url: String,
    refresh_interval_sec: u64,
    refresh_per_site_limit: usize,
    refresh_max_sources: usize,
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

#[derive(Debug, Deserialize)]
struct DetailQuery {
    url: String,
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

#[derive(Debug, Clone)]
struct NewsItemSummary {
    title: String,
    published: String,
    source_name: String,
    source_url: String,
}

#[derive(Debug, Clone, Serialize)]
struct NewsDetail {
    url: String,
    title: String,
    title_zh: String,
    source_name: String,
    source_url: String,
    published: String,
    content: String,
    content_zh: String,
    fetched_at: String,
}

#[derive(Debug, Serialize)]
struct NewsDetailResponse {
    id: &'static str,
    from_cache: bool,
    detail: NewsDetail,
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

    fn bad_request(message: impl Into<String>) -> Self {
        Self(message.into(), StatusCode::BAD_REQUEST)
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
    let db_path = root_dir.join("backend").join("news_cache.db");

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
    let refresh_interval_sec = env::var("NEWS_REFRESH_SEC")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(600);
    let refresh_per_site_limit = env::var("NEWS_REFRESH_PER_SITE_LIMIT")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(4)
        .clamp(1, 8);
    let refresh_max_sources = env::var("NEWS_REFRESH_MAX_SOURCES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(32)
        .clamp(1, 100);

    let state = AppState {
        sources_path,
        db_path,
        translate_url,
        refresh_interval_sec,
        refresh_per_site_limit,
        refresh_max_sources,
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

    init_db(&state.db_path)
        .await
        .map_err(|e| std::io::Error::other(e.0))?;
    tokio::spawn({
        let state = state.clone();
        async move {
            if let Err(e) = refresh_news_cache(state).await {
                eprintln!("initial refresh failed: {}", e.0);
            }
        }
    });
    start_background_refresh(state.clone());

    let app = Router::new()
        .route("/api/sources", get(get_sources))
        .route("/api/news", get(get_news))
        .route("/api/news/detail", get(get_news_detail))
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
    let _per_site_limit = query.per_site_limit.unwrap_or(3).clamp(1, 8);
    let _max_sources = query.max_sources.unwrap_or(32).clamp(1, 100);
    let translate = query.translate.unwrap_or(true);

    let mut items = read_news_from_db(&state.db_path, limit).await?;
    if !translate {
        for item in &mut items {
            item.title_zh = item.title.clone();
        }
    }

    Ok(Json(NewsResponse {
        id: "aihw01",
        translated: translate,
        translate_api_url: state.translate_url.clone(),
        total: items.len(),
        elapsed_seconds: (start.elapsed().as_secs_f64() * 1000.0).round() / 1000.0,
        items,
    }))
}

async fn get_news_detail(
    State(state): State<AppState>,
    Query(query): Query<DetailQuery>,
) -> Result<Json<NewsDetailResponse>, AppError> {
    let normalized_url = normalize_detail_url(&query.url)
        .ok_or_else(|| AppError::bad_request("invalid detail url"))?;

    if let Some(detail) = read_news_detail_from_db(&state.db_path, &normalized_url).await? {
        return Ok(Json(NewsDetailResponse {
            id: "aihw01",
            from_cache: true,
            detail,
        }));
    }

    let summary = read_news_summary_by_url(&state.db_path, &normalized_url).await?;
    let content = fetch_article_content(&state, &normalized_url).await;

    let title = summary
        .as_ref()
        .map(|s| s.title.clone())
        .unwrap_or_else(|| normalized_url.clone());
    let content = if content.is_empty() {
        format!(
            "暂时无法自动抓取到该页面的正文内容。\n\n你可以通过下方原文链接查看完整文章：\n{}",
            normalized_url
        )
    } else {
        content
    };
    let title_zh = translate_to_chinese(&state, &title).await;
    let content_zh = translate_long_text(&state, &content).await;
    let fetched_at = chrono::Utc::now().to_rfc3339();

    let detail = NewsDetail {
        url: normalized_url.clone(),
        title,
        title_zh,
        source_name: summary
            .as_ref()
            .map(|s| s.source_name.clone())
            .unwrap_or_else(|| "Unknown".to_string()),
        source_url: summary
            .as_ref()
            .map(|s| s.source_url.clone())
            .unwrap_or_else(|| normalized_url.clone()),
        published: summary
            .as_ref()
            .map(|s| s.published.clone())
            .unwrap_or_default(),
        content,
        content_zh,
        fetched_at,
    };

    write_news_detail_to_db(&state.db_path, &detail).await?;

    Ok(Json(NewsDetailResponse {
        id: "aihw01",
        from_cache: false,
        detail,
    }))
}

fn start_background_refresh(state: AppState) {
    tokio::spawn(async move {
        let interval = std::time::Duration::from_secs(state.refresh_interval_sec.max(30));
        loop {
            tokio::time::sleep(interval).await;
            if let Err(e) = refresh_news_cache(state.clone()).await {
                eprintln!("background refresh failed: {}", e.0);
            }
        }
    });
}

async fn refresh_news_cache(state: AppState) -> Result<(), AppError> {
    let all_sources = flatten_sources(&load_sources(&state.sources_path).await?);
    let sources: Vec<SourceRecord> = all_sources
        .into_iter()
        .take(state.refresh_max_sources)
        .collect();

    let mut collected = Vec::new();
    let mut rows_stream = stream::iter(sources.into_iter().map(|source| {
        let state = state.clone();
        async move {
            let feed_rows = fetch_from_feed(&state, &source, state.refresh_per_site_limit).await;
            if feed_rows.is_empty() {
                fetch_from_html(&state, &source, state.refresh_per_site_limit).await
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
    for item in &mut items {
        item.title_zh = translate_to_chinese(&state, &item.title).await;
    }

    write_news_to_db(&state.db_path, &items).await
}

async fn load_sources(path: &Path) -> Result<Value, AppError> {
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| AppError::internal(format!("failed to read news_sources.json: {e}")))?;
    serde_json::from_str(&content)
        .map_err(|e| AppError::internal(format!("invalid news_sources.json: {e}")))
}

async fn init_db(db_path: &Path) -> Result<(), AppError> {
    let db_path = db_path.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS news_items (
                url TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                title_zh TEXT NOT NULL,
                published TEXT NOT NULL,
                published_ts REAL NOT NULL,
                source_name TEXT NOT NULL,
                source_url TEXT NOT NULL,
                source_category TEXT NOT NULL,
                source_focus TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_news_items_published_ts
            ON news_items(published_ts DESC);
            CREATE TABLE IF NOT EXISTS news_details (
                url TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                title_zh TEXT NOT NULL,
                source_name TEXT NOT NULL,
                source_url TEXT NOT NULL,
                published TEXT NOT NULL,
                content TEXT NOT NULL,
                content_zh TEXT NOT NULL,
                fetched_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_news_details_fetched_at
            ON news_details(fetched_at DESC);
            ",
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::internal(format!("db init join error: {e}")))?
    .map_err(|e| AppError::internal(format!("db init error: {e}")))?;
    Ok(())
}

async fn write_news_to_db(db_path: &Path, items: &[NewsItem]) -> Result<(), AppError> {
    let db_path = db_path.to_path_buf();
    let items = items.to_vec();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut conn = Connection::open(db_path).map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let mut stmt = tx
            .prepare(
                "
                INSERT INTO news_items (
                    url, title, title_zh, published, published_ts,
                    source_name, source_url, source_category, source_focus, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ON CONFLICT(url) DO UPDATE SET
                    title=excluded.title,
                    title_zh=excluded.title_zh,
                    published=excluded.published,
                    published_ts=excluded.published_ts,
                    source_name=excluded.source_name,
                    source_url=excluded.source_url,
                    source_category=excluded.source_category,
                    source_focus=excluded.source_focus,
                    updated_at=excluded.updated_at
                ",
            )
            .map_err(|e| e.to_string())?;

        let now = chrono::Utc::now().to_rfc3339();
        for item in &items {
            stmt.execute(params![
                item.url,
                item.title,
                item.title_zh,
                item.published,
                timestamp_of(&item.published),
                item.source_name,
                item.source_url,
                item.source_category,
                item.source_focus,
                now,
            ])
            .map_err(|e| e.to_string())?;
        }
        drop(stmt);
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::internal(format!("db write join error: {e}")))?
    .map_err(|e| AppError::internal(format!("db write error: {e}")))
}

async fn read_news_from_db(db_path: &Path, limit: usize) -> Result<Vec<NewsItem>, AppError> {
    let db_path = db_path.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<Vec<NewsItem>, String> {
        let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "
                SELECT title, url, published, source_name, source_url, source_category, source_focus, title_zh
                FROM news_items
                ORDER BY published_ts DESC, updated_at DESC
                LIMIT ?1
                ",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(NewsItem {
                    title: row.get(0)?,
                    url: row.get(1)?,
                    published: row.get(2)?,
                    source_name: row.get(3)?,
                    source_url: row.get(4)?,
                    source_category: row.get(5)?,
                    source_focus: row.get(6)?,
                    title_zh: row.get(7)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut items = Vec::new();
        for row in rows {
            items.push(row.map_err(|e| e.to_string())?);
        }
        Ok(items)
    })
    .await
    .map_err(|e| AppError::internal(format!("db read join error: {e}")))?
    .map_err(|e| AppError::internal(format!("db read error: {e}")))
}

async fn read_news_summary_by_url(
    db_path: &Path,
    url: &str,
) -> Result<Option<NewsItemSummary>, AppError> {
    let db_path = db_path.to_path_buf();
    let url = url.to_string();
    tokio::task::spawn_blocking(move || -> Result<Option<NewsItemSummary>, String> {
        let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "
                SELECT title, published, source_name, source_url
                FROM news_items
                WHERE url = ?1
                LIMIT 1
                ",
            )
            .map_err(|e| e.to_string())?;

        let mut rows = stmt.query(params![url]).map_err(|e| e.to_string())?;
        let row_opt = rows.next().map_err(|e| e.to_string())?;
        let Some(row) = row_opt else {
            return Ok(None);
        };

        Ok(Some(NewsItemSummary {
            title: row.get(0).map_err(|e| e.to_string())?,
            published: row.get(1).map_err(|e| e.to_string())?,
            source_name: row.get(2).map_err(|e| e.to_string())?,
            source_url: row.get(3).map_err(|e| e.to_string())?,
        }))
    })
    .await
    .map_err(|e| AppError::internal(format!("db summary join error: {e}")))?
    .map_err(|e| AppError::internal(format!("db summary error: {e}")))
}

async fn read_news_detail_from_db(
    db_path: &Path,
    url: &str,
) -> Result<Option<NewsDetail>, AppError> {
    let db_path = db_path.to_path_buf();
    let url = url.to_string();
    tokio::task::spawn_blocking(move || -> Result<Option<NewsDetail>, String> {
        let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "
                SELECT url, title, title_zh, source_name, source_url, published, content, content_zh, fetched_at
                FROM news_details
                WHERE url = ?1
                LIMIT 1
                ",
            )
            .map_err(|e| e.to_string())?;

        let mut rows = stmt.query(params![url]).map_err(|e| e.to_string())?;
        let row_opt = rows.next().map_err(|e| e.to_string())?;
        let Some(row) = row_opt else {
            return Ok(None);
        };

        Ok(Some(NewsDetail {
            url: row.get(0).map_err(|e| e.to_string())?,
            title: row.get(1).map_err(|e| e.to_string())?,
            title_zh: row.get(2).map_err(|e| e.to_string())?,
            source_name: row.get(3).map_err(|e| e.to_string())?,
            source_url: row.get(4).map_err(|e| e.to_string())?,
            published: row.get(5).map_err(|e| e.to_string())?,
            content: row.get(6).map_err(|e| e.to_string())?,
            content_zh: row.get(7).map_err(|e| e.to_string())?,
            fetched_at: row.get(8).map_err(|e| e.to_string())?,
        }))
    })
    .await
    .map_err(|e| AppError::internal(format!("db detail read join error: {e}")))?
    .map_err(|e| AppError::internal(format!("db detail read error: {e}")))
}

async fn write_news_detail_to_db(db_path: &Path, detail: &NewsDetail) -> Result<(), AppError> {
    let db_path = db_path.to_path_buf();
    let detail = detail.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
        conn.execute(
            "
            INSERT INTO news_details (
                url, title, title_zh, source_name, source_url, published, content, content_zh, fetched_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(url) DO UPDATE SET
                title=excluded.title,
                title_zh=excluded.title_zh,
                source_name=excluded.source_name,
                source_url=excluded.source_url,
                published=excluded.published,
                content=excluded.content,
                content_zh=excluded.content_zh,
                fetched_at=excluded.fetched_at
            ",
            params![
                detail.url,
                detail.title,
                detail.title_zh,
                detail.source_name,
                detail.source_url,
                detail.published,
                detail.content,
                detail.content_zh,
                detail.fetched_at
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::internal(format!("db detail write join error: {e}")))?
    .map_err(|e| AppError::internal(format!("db detail write error: {e}")))
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

fn normalize_detail_url(raw: &str) -> Option<String> {
    let parsed = Url::parse(raw).ok()?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return None;
    }
    Some(parsed.to_string())
}

async fn fetch_article_content(state: &AppState, url: &str) -> String {
    let Ok(resp) = state.news_client.get(url).send().await else {
        return String::new();
    };
    let Ok(resp) = resp.error_for_status() else {
        return String::new();
    };
    let Ok(body) = resp.text().await else {
        return String::new();
    };

    let doc = Html::parse_document(&body);
    let selector =
        Selector::parse("article p, main p, .article p, .post-content p, .entry-content p, p")
            .expect("valid article selector");
    let mut seen = HashSet::new();
    let mut lines = Vec::new();
    let mut total_chars = 0usize;

    for p in doc.select(&selector) {
        let text = p
            .text()
            .collect::<Vec<_>>()
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if text.len() < 40 {
            continue;
        }
        if !seen.insert(text.clone()) {
            continue;
        }
        total_chars += text.len();
        lines.push(text);
        if lines.len() >= 80 || total_chars >= 16_000 {
            break;
        }
    }

    if lines.is_empty() {
        let body_selector = Selector::parse("article, main, body").expect("valid body selector");
        if let Some(node) = doc.select(&body_selector).next() {
            let text = node
                .text()
                .collect::<Vec<_>>()
                .join(" ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if text.len() > 120 {
                return text.chars().take(12_000).collect();
            }
        }

        let meta_selector =
            Selector::parse("meta[name='description'], meta[property='og:description']")
                .expect("valid meta selector");
        for node in doc.select(&meta_selector) {
            if let Some(content) = node.value().attr("content") {
                let t = content.trim();
                if t.len() > 30 {
                    return t.to_string();
                }
            }
        }
    }

    lines.join("\n\n")
}

async fn translate_long_text(state: &AppState, text: &str) -> String {
    if text.trim().is_empty() {
        return String::new();
    }
    let chunks = split_text_chunks(text, 900);
    let mut translated = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        translated.push(translate_to_chinese(state, &chunk).await);
    }
    translated.join("\n\n")
}

fn split_text_chunks(text: &str, max_len: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    for line in text.lines() {
        if buf.len() + line.len() + 1 > max_len && !buf.is_empty() {
            out.push(buf.trim().to_string());
            buf.clear();
        }
        if !buf.is_empty() {
            buf.push('\n');
        }
        buf.push_str(line);
    }
    if !buf.trim().is_empty() {
        out.push(buf.trim().to_string());
    }
    out
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
