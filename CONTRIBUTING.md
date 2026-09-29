# פיתוח – התקנה והרצה

## דרישות
- Rust (הגרסה נעולה ב-`rust-toolchain.toml`, ו-rustup מתקין אותה לבד)
- Node 22 + pnpm 10 (`corepack enable`)
- ב-Linux, ספריות WebView: `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev libxdo-dev`
- ב-Windows: Microsoft C++ Build Tools ו-WebView2 (מותקן ב-Windows 10/11)
- ב-macOS: Xcode Command Line Tools

## פעם אחת אחרי clone
```sh
git config core.hooksPath .githooks   # סורק לפני כל commit: ת.ז., טלפונים, מיילים, ארכיונים
cd apps/desktop && pnpm install
```

## הרצה
```sh
cd apps/desktop && pnpm tauri dev      # האפליקציה במצב פיתוח
```

## בדיקות (מה ש-CI מריץ)
```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test                              # כולל יצירה מחדש של טיפוסי ה-IPC ל-TypeScript
cargo xtask check-invariants            # רשת רק ב-dv-egress, unsafe, CSP, capabilities
cargo xtask scan --all                  # אין מידע אישי או קבצים בינאריים בריפו
cargo deny check                        # רישיונות, אזהרות אבטחה, מקורות
cd apps/desktop && pnpm typecheck && pnpm lint && pnpm test && pnpm build
```

## כללים
ראו `CLAUDE.md`. בקצרה: אין מידע אמיתי, הרשת רק ב-`dv-egress`, הפלט מוצג כטקסט בלבד, ולכל תלות חדשה צריך לכתוב נימוק ב-PR.
