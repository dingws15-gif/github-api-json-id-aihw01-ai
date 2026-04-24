const refreshBtn = document.getElementById("refreshBtn");
const newsList = document.getElementById("newsList");
const meta = document.getElementById("meta");
const detailView = document.getElementById("detailView");
const detailTitle = document.getElementById("detailTitle");
const detailMeta = document.getElementById("detailMeta");
const detailContentZh = document.getElementById("detailContentZh");
const detailContent = document.getElementById("detailContent");
const detailLink = document.getElementById("detailLink");
const backBtn = document.getElementById("backBtn");

const CACHE_KEY = "ai_news_cache";

function safeHttpUrl(raw) {
    if (!raw) return "#";
    try {
        const u = new URL(raw, window.location.origin);
        if (u.protocol === "http:" || u.protocol === "https:") return u.href;
    } catch (e) {
        console.warn("Invalid URL:", raw, e);
    }
    return "#";
}

function createMetaText(item) {
    const published = item.published ? new Date(item.published).toLocaleString() : "最近";
    return `${item.source_name || "-"} ｜ ${published}`;
}

function renderItem(item) {
    const card = document.createElement("button");
    card.className = "news-item";
    card.type = "button";

    const h3 = document.createElement("h3");
    h3.textContent = item.title_zh || item.title || "";

    const origin = document.createElement("div");
    origin.className = "original-title";
    origin.textContent = item.title || "";

    const info = document.createElement("div");
    info.className = "info";
    info.textContent = createMetaText(item);

    const details = document.createElement("div");
    details.className = "details";
    details.textContent = `分类: ${item.source_category || "-"} ｜ 关注: ${item.source_focus || "-"}`;

    card.appendChild(h3);
    card.appendChild(origin);
    card.appendChild(info);
    card.appendChild(details);
    card.addEventListener("click", () => openDetail(item));
    return card;
}

async function fetchNews(isInitial = false) {
    refreshBtn.disabled = true;
    refreshBtn.textContent = "抓取中...";

    if (isInitial) {
        const cached = localStorage.getItem(CACHE_KEY);
        if (cached) {
            try {
                displayNews(JSON.parse(cached), true);
            } catch (e) {
                console.error("Cache parse error", e);
            }
        }
    }

    try {
        const res = await fetch("/api/news?translate=true&limit=40");
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const data = await res.json();
        displayNews(data);
        localStorage.setItem(CACHE_KEY, JSON.stringify(data));
        meta.textContent = `更新于 ${new Date().toLocaleTimeString()} · 共 ${data.total} 条`;
    } catch (err) {
        console.error(err);
        meta.textContent = `加载失败: ${err.message}`;
    } finally {
        refreshBtn.disabled = false;
        refreshBtn.textContent = "刷新";
    }
}

function displayNews(data, fromCache = false) {
    if (!fromCache) newsList.innerHTML = "";
    const fragment = document.createDocumentFragment();
    (data.items || []).forEach((item) => fragment.appendChild(renderItem(item)));
    if (fromCache) newsList.innerHTML = "";
    newsList.appendChild(fragment);
    if (fromCache) meta.textContent = "正在同步最新资讯...";
}

function setDetailLoading(item) {
    detailTitle.textContent = item.title_zh || item.title || "详情";
    detailMeta.textContent = "加载中...";
    detailContentZh.textContent = "正在抓取并翻译全文，请稍候...";
    detailContent.textContent = "";
    detailLink.href = safeHttpUrl(item.url);
    detailLink.textContent = item.url || "";
}

function showDetailView() {
    newsList.style.display = "none";
    detailView.style.display = "block";
}

function showListView() {
    detailView.style.display = "none";
    newsList.style.display = "grid";
}

async function openDetail(item) {
    showDetailView();
    setDetailLoading(item);
    try {
        const url = encodeURIComponent(item.url || "");
        const res = await fetch(`/api/news/detail?url=${url}`);
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const data = await res.json();
        const detail = data.detail;

        detailTitle.textContent = detail.title_zh || detail.title || "详情";
        detailMeta.textContent = `${detail.source_name || "-"} ｜ ${detail.published ? new Date(detail.published).toLocaleString() : "最近"} ｜ ${data.from_cache ? "缓存命中" : "新抓取"}`;
        detailContentZh.textContent = detail.content_zh || "暂无翻译内容";
        detailContent.textContent = detail.content || "暂无原文内容";
        detailLink.href = safeHttpUrl(detail.url);
        detailLink.textContent = detail.url || "";
    } catch (err) {
        detailMeta.textContent = `加载失败: ${err.message}`;
        detailContentZh.textContent = "";
        detailContent.textContent = "";
    }
}

refreshBtn.addEventListener("click", () => fetchNews());
backBtn.addEventListener("click", showListView);

fetchNews(true);
