const refreshBtn = document.getElementById("refreshBtn");
const newsList = document.getElementById("newsList");
const meta = document.getElementById("meta");

const CACHE_KEY = "ai_news_cache";
const zhTranslateCache = new Map();

function safeHttpUrl(raw) {
    if (!raw) return "#";
    try {
        const u = new URL(raw, window.location.origin);
        if (u.protocol === "http:" || u.protocol === "https:") {
            return u.href;
        }
    } catch (e) {
        console.warn("Invalid URL:", raw, e);
    }
    return "#";
}

function renderItem(item) {
    const a = document.createElement("a");
    a.className = "news-item";
    a.href = safeHttpUrl(item.url);
    a.target = "_blank";
    a.rel = "noopener noreferrer";

    const h3 = document.createElement("h3");
    h3.textContent = item.title_zh || item.title || "";

    const originalTitle = document.createElement("div");
    originalTitle.className = "original-title";
    originalTitle.textContent = item.title || "";

    const info = document.createElement("div");
    info.className = "info";

    const source = document.createElement("span");
    source.textContent = item.source_name || "";

    const date = document.createElement("span");
    date.textContent = item.published ? new Date(item.published).toLocaleString() : "最近";

    const details = document.createElement("div");
    details.className = "details";
    details.textContent = `分类: ${item.source_category || "-"} ｜ 关注: ${item.source_focus || "-"}`;

    const link = document.createElement("div");
    link.className = "link";
    link.textContent = item.url || "";

    info.appendChild(source);
    info.appendChild(date);

    a.appendChild(h3);
    a.appendChild(originalTitle);
    a.appendChild(info);
    a.appendChild(details);
    a.appendChild(link);
    return a;
}

async function translateTitleFallback(text) {
    if (!text) return text;
    if (zhTranslateCache.has(text)) return zhTranslateCache.get(text);
    try {
        const url = `https://translate.googleapis.com/translate_a/single?client=gtx&sl=auto&tl=zh-CN&dt=t&q=${encodeURIComponent(text)}`;
        const res = await fetch(url);
        if (!res.ok) return text;
        const data = await res.json();
        const translated = data?.[0]?.[0]?.[0] || text;
        zhTranslateCache.set(text, translated);
        return translated;
    } catch {
        return text;
    }
}

async function fillMissingTranslations(items) {
    const tasks = items.slice(0, 40).map(async (item) => {
        if (!item?.title) return;
        if (item.title_zh && item.title_zh !== item.title) return;
        item.title_zh = await translateTitleFallback(item.title);
    });
    await Promise.all(tasks);
}

async function fetchNews(isInitial = false) {
    refreshBtn.disabled = true;
    refreshBtn.textContent = "抓取中...";
    
    if (isInitial) {
        const cached = localStorage.getItem(CACHE_KEY);
        if (cached) {
            try {
                const data = JSON.parse(cached);
                displayNews(data, true);
            } catch (e) {
                console.error("Cache parse error", e);
            }
        }
    }

    try {
        const res = await fetch("/api/news?translate=true&limit=40");
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const data = await res.json();

        await fillMissingTranslations(data.items || []);
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
    data.items.forEach(item => {
        fragment.appendChild(renderItem(item));
    });
    
    if (fromCache) {
        newsList.innerHTML = ""; // Clear for final update if needed
    }
    newsList.appendChild(fragment);
    
    if (fromCache) {
        meta.textContent = "正在同步最新资讯...";
    }
}

refreshBtn.addEventListener("click", () => fetchNews());

// Start
fetchNews(true);
