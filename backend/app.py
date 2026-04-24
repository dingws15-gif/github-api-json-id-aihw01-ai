import json
import os
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from typing import Any
from urllib.parse import urljoin

import feedparser
import requests
from bs4 import BeautifulSoup
from dateutil import parser as date_parser
from fastapi import FastAPI, Query
from fastapi.middleware.cors import CORSMiddleware
from fastapi.staticfiles import StaticFiles

BASE_DIR = Path(__file__).resolve().parent
ROOT_DIR = BASE_DIR.parent
SOURCES_PATH = BASE_DIR / "news_sources.json"
FRONTEND_DIR = ROOT_DIR / "frontend"

TRANSLATE_URL = os.getenv("TRANSLATE_API_URL", "https://translate.argosopentech.com/translate")
TRANSLATE_TIMEOUT_SEC = float(os.getenv("TRANSLATE_TIMEOUT_SEC", "8"))
NEWS_TIMEOUT_SEC = float(os.getenv("NEWS_TIMEOUT_SEC", "10"))

USER_AGENT = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
    "AppleWebKit/537.36 (KHTML, like Gecko) "
    "Chrome/123.0.0.0 Safari/537.36"
)

KNOWN_FEEDS = {
    "https://www.tomshardware.com/": "https://www.tomshardware.com/feeds/all",
    "https://www.anandtech.com/": "https://www.anandtech.com/rss/",
    "https://semianalysis.com/": "https://semianalysis.com/feed/",
    "https://www.eetimes.com/": "https://www.eetimes.com/feed/",
    "https://www.servethehome.com/": "https://www.servethehome.com/feed/",
    "https://www.datacenterdynamics.com/": "https://www.datacenterdynamics.com/rss/",
    "https://www.nextplatform.com/": "https://www.nextplatform.com/feed/",
    "https://www.edge-ai-vision.com/": "https://www.edge-ai-vision.com/feed/",
    "https://www.embedded.com/": "https://www.embedded.com/feed/",
    "https://www.androidauthority.com/": "https://www.androidauthority.com/feed/",
    "https://blogs.nvidia.com/": "https://blogs.nvidia.com/feed/",
    "https://www.amd.com/en/newsroom": "https://www.amd.com/en/rss.xml",
    "https://www.qualcomm.com/news": "https://www.qualcomm.com/news/rss.xml",
    "https://news.ycombinator.com/": "https://news.ycombinator.com/rss",
    "https://www.reddit.com/r/hardware/": "https://www.reddit.com/r/hardware/.rss",
    "https://www.reddit.com/r/MachineLearning/": "https://www.reddit.com/r/MachineLearning/.rss",
    "https://www.technologyreview.com/topic/artificial-intelligence/": "https://www.technologyreview.com/topic/artificial-intelligence/feed/",
    "https://venturebeat.com/category/ai/": "https://venturebeat.com/category/ai/feed/",
    "https://www.wired.com/tag/artificial-intelligence/": "https://www.wired.com/feed/tag/artificial-intelligence/latest/rss",
    "https://www.artificialintelligence-news.com/": "https://www.artificialintelligence-news.com/feed/",
    "https://www.aitrends.com/": "https://www.aitrends.com/feed/",
    "https://openai.com/blog/": "https://openai.com/news/rss.xml",
    "https://www.anthropic.com/news": "https://www.anthropic.com/news/rss.xml",
    "https://www.marktechpost.com/": "https://www.marktechpost.com/feed/",
    "https://www.sciencedaily.com/news/computers_math/artificial_intelligence/": "https://www.sciencedaily.com/rss/computers_math/artificial_intelligence.xml",
    "https://www.reddit.com/r/ArtificialIntelligence/": "https://www.reddit.com/r/ArtificialIntelligence/.rss",
}

_translate_cache: dict[str, str] = {}


def load_sources() -> dict[str, Any]:
    with SOURCES_PATH.open("r", encoding="utf-8") as f:
        return json.load(f)


def flatten_sources(data: dict[str, Any]) -> list[dict[str, str]]:
    records: list[dict[str, str]] = []
    for root_key in ("ai_hardware_news", "ai_news_websites"):
        source_group = data.get(root_key, {})
        for category, source_list in source_group.items():
            for source in source_list:
                records.append(
                    {
                        "root": root_key,
                        "category": category,
                        "name": source["name"],
                        "focus": source["focus"],
                        "url": source["url"],
                    }
                )
    return records


def parse_date(value: str | None) -> str:
    if not value:
        return ""
    try:
        parsed = date_parser.parse(value)
        return parsed.isoformat()
    except Exception:
        return ""


def fetch_from_feed(source: dict[str, str], per_site_limit: int) -> list[dict[str, str]]:
    feed_url = KNOWN_FEEDS.get(source["url"], source["url"])
    try:
        response = requests.get(
            feed_url,
            timeout=NEWS_TIMEOUT_SEC,
            headers={"User-Agent": USER_AGENT},
        )
        response.raise_for_status()
    except Exception:
        return []

    parsed = feedparser.parse(response.content)
    if not parsed.entries:
        return []

    results = []
    for item in parsed.entries[:per_site_limit]:
        title = (item.get("title") or "").strip()
        link = (item.get("link") or source["url"]).strip()
        if not title:
            continue
        results.append(
            {
                "title": title,
                "url": link,
                "published": parse_date(item.get("published") or item.get("updated")),
                "source_name": source["name"],
                "source_url": source["url"],
                "source_category": source["category"],
                "source_focus": source["focus"],
            }
        )
    return results


def fetch_from_html(source: dict[str, str], per_site_limit: int) -> list[dict[str, str]]:
    try:
        response = requests.get(
            source["url"],
            timeout=NEWS_TIMEOUT_SEC,
            headers={"User-Agent": USER_AGENT},
        )
        response.raise_for_status()
    except Exception:
        return []

    soup = BeautifulSoup(response.text, "html.parser")
    results = []
    seen = set()
    for anchor in soup.select("a[href]"):
        title = anchor.get_text(strip=True)
        href = anchor.get("href") or ""
        if len(title) < 18:
            continue
        full_url = urljoin(source["url"], href)
        if not full_url.startswith("http"):
            continue
        key = (title, full_url)
        if key in seen:
            continue
        seen.add(key)
        results.append(
            {
                "title": title,
                "url": full_url,
                "published": "",
                "source_name": source["name"],
                "source_url": source["url"],
                "source_category": source["category"],
                "source_focus": source["focus"],
            }
        )
        if len(results) >= per_site_limit:
            break
    return results


def translate_to_chinese(text: str) -> str:
    if not text:
        return text
    if text in _translate_cache:
        return _translate_cache[text]

    payload = {
        "q": text,
        "source": "auto",
        "target": "zh",
        "format": "text",
    }
    try:
        response = requests.post(
            TRANSLATE_URL,
            json=payload,
            timeout=TRANSLATE_TIMEOUT_SEC,
            headers={"User-Agent": USER_AGENT},
        )
        response.raise_for_status()
        data = response.json()
        translated = data.get("translatedText", "").strip() or text
    except Exception:
        translated = text

    _translate_cache[text] = translated
    return translated


app = FastAPI(title="AI News Aggregator (aihw01)")
app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)


@app.get("/api/sources")
def get_sources() -> dict[str, Any]:
    data = load_sources()
    return {
        "id": data.get("id"),
        "total_sources": len(flatten_sources(data)),
        "sources": data,
    }


@app.get("/api/news")
def get_news(
    limit: int = Query(default=40, ge=1, le=200),
    per_site_limit: int = Query(default=3, ge=1, le=8),
    max_sources: int = Query(default=32, ge=1, le=100),
    translate: bool = Query(default=True),
) -> dict[str, Any]:
    start_time = time.time()
    sources = flatten_sources(load_sources())[:max_sources]

    collected = []
    with ThreadPoolExecutor(max_workers=8) as pool:
        futures = [pool.submit(fetch_from_feed, source, per_site_limit) for source in sources]
        for source, future in zip(sources, futures):
            try:
                rows = future.result()
            except Exception:
                rows = []
            if not rows:
                collected.extend(fetch_from_html(source, per_site_limit))
            else:
                collected.extend(rows)

    dedup = {}
    for row in collected:
        key = row["url"]
        if key not in dedup:
            dedup[key] = row
    items = list(dedup.values())

    for row in items:
        row["title_zh"] = translate_to_chinese(row["title"]) if translate else row["title"]

    def sort_key(news_item: dict[str, str]) -> float:
        published = news_item.get("published") or ""
        if not published:
            return -1.0
        try:
            return date_parser.parse(published).timestamp()
        except Exception:
            return -1.0

    items.sort(key=sort_key, reverse=True)
    items = items[:limit]

    return {
        "id": "aihw01",
        "translated": translate,
        "translate_api_url": TRANSLATE_URL,
        "total": len(items),
        "elapsed_seconds": round(time.time() - start_time, 3),
        "items": items,
    }


if FRONTEND_DIR.exists():
    app.mount("/", StaticFiles(directory=str(FRONTEND_DIR), html=True), name="frontend")
