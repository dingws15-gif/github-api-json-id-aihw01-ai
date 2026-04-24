const loadBtn = document.getElementById("loadBtn");
const newsList = document.getElementById("newsList");
const meta = document.getElementById("meta");
const siteLimit = document.getElementById("siteLimit");
const totalLimit = document.getElementById("totalLimit");

function createItem(item) {
  const div = document.createElement("div");
  div.className = "item";
  div.innerHTML = `
    <h3>${item.title_zh || item.title}</h3>
    <p>原文: ${item.title}</p>
    <p>来源: ${item.source_name} | 分类: ${item.source_category}</p>
    <p>发布时间: ${item.published || "未知"}</p>
    <p><a href="${item.url}" target="_blank" rel="noreferrer">查看原文</a></p>
  `;
  return div;
}

async function loadNews() {
  loadBtn.disabled = true;
  loadBtn.textContent = "加载中...";
  newsList.innerHTML = "";
  meta.textContent = "正在抓取并翻译...";
  try {
    const url = `/api/news?translate=true&per_site_limit=${siteLimit.value}&limit=${totalLimit.value}`;
    const res = await fetch(url);
    if (!res.ok) {
      throw new Error(`HTTP ${res.status}`);
    }
    const data = await res.json();
    meta.textContent = `共 ${data.total} 条，耗时 ${data.elapsed_seconds}s，翻译端点: ${data.translate_api_url}`;
    data.items.forEach((item) => newsList.appendChild(createItem(item)));
  } catch (err) {
    meta.textContent = `加载失败: ${err.message}`;
  } finally {
    loadBtn.disabled = false;
    loadBtn.textContent = "刷新新闻";
  }
}

loadBtn.addEventListener("click", loadNews);
loadNews();

