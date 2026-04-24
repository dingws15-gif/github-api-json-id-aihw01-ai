# ai_news_app

Flutter client for the AI news backend.

## Run

Use `--dart-define` to point to your backend:

```bash
flutter run --dart-define=API_BASE_URL=http://10.0.2.2:8000
```

Notes:
- Android emulator usually uses `10.0.2.2` to access the host machine.
- On iOS simulator you can often use `http://localhost:8000`.
- On web, the app uses `window.location.origin` automatically.
