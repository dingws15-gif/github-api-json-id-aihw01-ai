# aihw01 - AI 新闻抓取与中文翻译

这个项目实现了你要的功能：
- 聚合 AI 硬件/AI 行业新闻网站（使用你给的 JSON 源）
- 优先抓取 RSS，失败时回退抓取网站首页标题
- 调用开源翻译 API（默认 LibreTranslate 兼容接口）翻译成中文
- 提供网页界面展示新闻列表

## 目录结构

```text
backend/
  app.py
  news_sources.json
frontend/
  index.html
  app.js
requirements.txt
```

## 运行

1. 创建并激活虚拟环境

```powershell
python -m venv .venv
.venv\Scripts\Activate.ps1
```

2. 安装依赖

```powershell
pip install -r requirements.txt
```

3. 启动服务

```powershell
uvicorn backend.app:app --reload --host 0.0.0.0 --port 8000
```

4. 打开浏览器

- [http://localhost:8000](http://localhost:8000)

## API

- `GET /api/sources` 查看所有新闻源
- `GET /api/news?translate=true&per_site_limit=3&limit=40` 获取新闻并翻译中文

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

