# aihw01 - AI 新闻抓取与中文翻译（Rust）

这个项目实现了你要的功能（Rust 后端）：
- 聚合 AI 硬件/AI 行业新闻网站（使用你给的 JSON 源）
- 优先抓取 RSS，失败时回退抓取网站首页标题
- 调用开源翻译 API（默认 LibreTranslate 兼容接口）翻译成中文
- 提供网页界面展示新闻列表

## 目录结构

```text
backend/
  news_sources.json
frontend/
  index.html
  app.js
src/
  main.rs
Cargo.toml
```

## 运行

1. 安装 Rust（如未安装）

```powershell
rustc --version
cargo --version
```

2. 启动服务

```powershell
cargo run
```

3. 打开浏览器

- [http://localhost:8000](http://localhost:8000)

## API

- `GET /api/sources` 查看所有新闻源
- `GET /api/news?translate=true&limit=40` 从本地缓存数据库读取新闻（前台不直连外网）
- `GET /api/news/detail?url=<article_url>` 获取站内详情（先查 SQL，未命中才爬取+翻译并落库）

## 后台定时更新与缓存

服务启动后会在后台定时抓取新闻并写入 SQLite：

- 数据库文件：`backend/news_cache.db`
- 前端列表请求 `/api/news`（读取数据库）
- 点击详情后请求 `/api/news/detail`（懒加载爬取，后续走缓存）

可通过环境变量调节刷新策略：

```powershell
$env:NEWS_REFRESH_SEC="600"             # 刷新周期（秒）
$env:NEWS_REFRESH_PER_SITE_LIMIT="4"    # 每站抓取条数
$env:NEWS_REFRESH_MAX_SOURCES="32"      # 每轮抓取站点数
```

## 开源翻译 API 配置

默认使用：

- `https://translate.argosopentech.com/translate`

可通过环境变量替换为你自己的 LibreTranslate 实例：

```powershell
$env:TRANSLATE_API_URL="https://your-libretranslate-instance/translate"
```

可选超时：

```powershell
$env:TRANSLATE_TIMEOUT_SEC="8"
$env:NEWS_TIMEOUT_SEC="10"
```

## 新建 GitHub 仓库并推送

如果你本机已安装并登录 `gh`：

```powershell
git init
git add .
git commit -m "feat: ai news fetch + chinese translation (aihw01)"
gh repo create aihw01 --public --source . --remote origin --push
```

如果你要私有仓库，把 `--public` 改为 `--private`。
