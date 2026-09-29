# תוכנית שלב 1 – שלד מאובטח

> **המטרה:** שלד שבו כל כללי האבטחה כבר נאכפים מהיום הראשון (CI, אינווריאנטים, pre-commit, הקשחת webview), לפני שנכתבת שורת קוד אחת שנוגעת במידע.
> **בשלב הזה אין:** קריפטו, בסיס נתונים, AI או מידע.
> **תנאי מעבר:** האפליקציה נפתחת ב-Windows, ב-macOS וב-Linux (CI), מדברת עם ליבת ה-Rust דרך `ping`, וכל בדיקות ה-CI ירוקות.

## משימות

| # | משימה | תוצר | בדיקה / קבלה |
|---|---|---|---|
| 1.1 | **Rust workspace** | `Cargo.toml` (workspace), `rust-toolchain.toml` (גרסה נעולה), 8 crates ריקים (`dv-*`), `#![forbid(unsafe_code)]`, `[workspace.lints]`: clippy `-D warnings` + `pedantic` נבחרים | `cargo build`, `cargo clippy` נקיים |
| 1.2 | **אפליקציית Tauri 2** | `apps/desktop/src-tauri`: חלון יחיד 1280×800, בלי plugins | `cargo tauri dev` פותח חלון |
| 1.3 | **UI בסיסי ב-React + TS** | Vite, `tsc --strict`, RTL, `lang="he"`, טוקנים לצבעים ולצורות מ-`SCREENS.md`, פונטים ארוזים מקומית (OFL), מסך נעילה סטטי (דמה) לפי `Lock.dc.html`, ErrorBoundary | Vitest + Testing Library: המסך מרונדר ב-`dir="rtl"`, והכפתורים נגישים לפי role ו-label |
| 1.4 | **IPC מוקלד** | `dv-ipc`: פקודה `ping` ⇒ `{core_version, build_commit, fips_active:false, platform}`. טיפוסי TS נוצרים עם `tauri-specta`. ה-UI מציג "הליבה מחוברת". | בדיקת יחידה ל-`ping` + בדיקה עם mock runtime של Tauri |
| 1.5 | **הקשחת webview** | CSP מלא (`ARCHITECTURE.md`), isolation pattern, capabilities רק ל-`ping`, `contentProtected: true`, בלי DevTools ב-release, ניווט חיצוני חסום, בלי remote URLs | בדיקה שמנתחת את `tauri.conf.json` ואת קובצי ה-capabilities, ונכשלת אם CSP נחלש או אם נוסף plugin |
| 1.6 | **xtask: אינווריאנטים** | `cargo xtask check-invariants`: (א) ספריות רשת רק ב-`dv-egress` (דרך `cargo metadata`); (ב) `forbid(unsafe_code)` בכל crate; (ג) אין `dangerouslySetInnerHTML` ואין `eval` ב-UI; (ד) בלי plugins אסורים ב-Tauri | **בדיקות שליליות:** fixture עם `reqwest` ב-`dv-vault` ⇒ הבדיקה נכשלת |
| 1.7 | **xtask: סורק pre-commit** | `cargo xtask scan --staged`: ת.ז. עם ספרת ביקורת תקינה, טלפון ישראלי, מייל, ארכיונים, `.docx` מחוץ ל-`templates/`, קובץ בינארי מעל 1MB, ומילות מפתח (`real_data`, `vault/`). רשימת היתר: `tests/fixtures/` (ת.ז. הבדויה `000000018`). | בדיקות יחידה לכל כלל, כולל false positive מוכר |
| 1.8 | **git hooks** | `.githooks/pre-commit` ⇒ `cargo xtask scan --staged`. `CONTRIBUTING.md` עם `git config core.hooksPath .githooks`. | קומיט עם ת.ז. תקינה נחסם |
| 1.9 | **CI (GitHub Actions)** | `ci.yml`: **lint** (fmt, clippy), **test** (`cargo test --workspace`), **supply-chain** (`cargo deny check` לרישיונות, אזהרות, bans ומקורות; `cargo audit`), **frontend** (`pnpm install --frozen-lockfile`, tsc, eslint עם `react/no-danger=error`, vitest), **invariants**, **secrets** (gitleaks עם כללים מותאמים), **build** (מטריצה: windows-latest, macos-latest, ubuntu-latest; Tauri build לא חתום). כל ה-actions נעולים לפי SHA, `permissions: contents: read`, בלי סודות. | כל ה-jobs ירוקים ב-PR |
| 1.10 | **`deny.toml`** | רשימת רישיונות מותרים (MIT, Apache-2.0, BSD-2/3, ISC, Zlib, Unicode-3.0, MPL-2.0), איסור GPL/AGPL, רק crates.io, איסור crates של טלמטריה (sentry וכו') | `cargo deny check` |
| 1.11 | **ממשל הריפו** | `CODEOWNERS` (`crates/dv-privacy`, `dv-egress`, `dv-vault`, `.github/` ⇒ אור), `SECURITY.md` ראשוני (איך מדווחים על חולשה), `docs/REPO_SETTINGS.md`: הגדרות שאור מפעיל ב-GitHub (branch protection, required checks, חתימת קומיטים, איסור force-push, secret scanning ו-push protection) | סקירה |
| 1.12 | **תיעוד** | עדכון `CLAUDE.md` עם פקודות אמיתיות, `CONTRIBUTING.md` (התקנת toolchain בכל מערכת הפעלה) | — |

## תלויות שיתווספו בשלב 1 (כל אחת עם נימוק)
| תלות | למה | הערה |
|---|---|---|
| `tauri` 2.x, `tauri-build` | המעטפת | בלי features מיותרים |
| `serde`, `serde_json` | סריאליזציה ל-IPC | |
| `specta`, `tauri-specta` | טיפוסי TS אוטומטיים, בלי כפילות ידנית | |
| `thiserror` | שגיאות מוקלדות | |
| React, ReactDOM, Vite, TypeScript | UI | |
| ESLint + `eslint-plugin-react` + `@typescript-eslint` | כללי אבטחה ל-UI | |
| Vitest, `@testing-library/react`, jsdom | בדיקות UI | dev בלבד |
| `cargo_metadata` (ב-xtask) | בדיקת עץ התלויות | dev בלבד |

## לא בשלב 1
קריפטו, SQLCipher, קשירה לחומרה, NER, AI, ייצוא, חתימת קוד. אלה בשלבים 2–8 לפי `ROADMAP.md`.

## סיכונים ידועים
| סיכון | צמצום |
|---|---|
| WebView2 ב-Windows | Evergreen (מתעדכן אוטומטית עם תיקוני אבטחה). בודקים שהוא קיים בהתקנה. |
| תאימות של isolation pattern עם `tauri-specta` | בדיקה ב-1.4. אם יש בעיה, מתעדים ובוחרים חלופה באישור. |
| `aws-lc-rs` ב-FIPS דורש CMake, Go ו-NASM ב-build | מכינים את ה-toolchain ב-CI כבר בשלב 1, כדי ששלב 2 לא ייתקע. |

## הדגמה בסוף השלב
1. צילומי מסך מ-CI של האפליקציה בשלוש מערכות ההפעלה: מסך נעילה (דמה) בעברית, ו"הליבה מחוברת".
2. הדגמה שקומיט עם ת.ז. תקינה נחסם.
3. הדגמה ש-PR שמוסיף `reqwest` ל-`dv-vault` נכשל ב-CI.
