# יומן לילה · 22–23.11.2026

ענף: `night-2026-11-22`, מעל `main` (0.9.16) + PR #212 (סעיפי הדוח, D-209..D-213) + PR #213 (הקליניקה, D-207/D-208).
שום דבר לא מוזג ל-main. הכל בדוי; שום מידע של מטופלים.

## BASELINE (לפני שינוי אחד שלי, אחרי מיזוג שני ה-PRs)

| בדיקה | תוצאה |
|---|---|
| `cargo test --workspace --exclude dv-desktop` | 1,964 עברו, 0 נכשלו, 13 מדולגות (מסומנות `ignore` בריפו) |
| `cargo fmt --check` | נקי |
| `cargo clippy --workspace --exclude dv-desktop --all-targets -- -D warnings` | נקי |
| `cargo xtask check-invariants` | ok |
| `cargo deny check` | לא מותקן במכונה הזו; רץ ב-CI |
| ממשק: `tsc --noEmit` | נקי |
| ממשק: `eslint --max-warnings 0` | נקי |
| ממשק: `vitest` | 110 קבצים, 1,225 בדיקות, כולן עברו |
| ממשק: `vite build` (הדמיה בדפדפן) | נבנה |
| `dv-desktop` (Tauri) | לא נבנה כאן (אין GTK במכונה); נבנה ב-CI |
| נגישות, axe-core (WCAG 2.1 A+AA) על 25 מסכים בהדמיה | הפרה אחת: ניגודיות בסימן אזהרה ב"הגדרות › סיסמה ואבטחה" |
| נגישות, axe "best-practice" | בכל מסך: תוכן מחוץ ל-landmark; במסך התיק אין `main`; בחלון פתוח שני `banner` |
| נגישות, מה ש-axe לא בודק | אין קישור "דילוג לתוכן"; כותרת החלון (`document.title`) זהה בכל המסכים |

