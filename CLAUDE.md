# כספת האבחון – כללי הפרויקט

תוכנת דסקטופ מקומית לעינת, פסיכולוגית התפתחותית, לכתיבת דוחות אבחון לילדים, בשיחה רציפה ומסוננת מול Claude.
המפתח: אור. המשתמשת: עינת (לא טכנית, ממשק בעברית RTL).
הטכנולוגיה: Tauri 2 + Rust (בלי Python בזמן ריצה) + React/TS. ראו `docs/ARCHITECTURE.md`.

## כללי ברזל
- **אין בריפו שום מידע אמיתי על מטופלים**, גם לא מטושטש. רק נתונים בדויים (`tests/fixtures/`). לעולם לא לבקש מאור או מעינת דוח אמיתי.
- **Fail-closed:** כל ספק בסינון = חסימה. שום טקסט לא יוצא ל-API בלי `ClearedPayload` מ-`dv-privacy::gate`.
- **הרשת קיימת רק ב-`dv-egress`.** אסור להוסיף ספריית רשת לשום crate אחר (נאכף ב-CI).
- **ה-AI מנסח, עינת מאבחנת.** פרשנות ציונים בקוד דטרמיניסטי. אין המצאת ממצאים.
- **פלט של מודל או של מסמך מוצג כטקסט בלבד**, אף פעם לא כ-HTML.
- **יציאה רק במסגרת ZDR:** `POST /v1/messages` (ו-`count_tokens`), דגמים מהרשימה הלבנה, בלי Files/Batch/כלי שרת, בלי מידע בסכמות JSON. חריג יחיד: ChatGPT, Gemini, Mistral או מודל מקומי כשעינת בחרה בהם במפורש ואישרה מה הם שומרים (D-040), דרך אותו שער ובלי כלים.
- **התקן המחמיר ביותר** בכל החלטה, עם נימוק (`docs/STANDARDS.md`). החלטה שלא מוגדרת: האפשרות המחמירה מבחינת פרטיות. החלטה ארכיטקטונית: מתייעצים, עם חלופות מדורגות, ומתעדים ב-`docs/DECISIONS.md`.
- `#![forbid(unsafe_code)]`. חריג רק ב-FFI לחומרה, במודול מבודד שעבר ביקורת.

## מסמכים (לקרוא לפי הצורך, לא הכל בכל משימה)
- `docs/SPEC.md` – מה בונים ולמי
- `docs/STANDARDS.md` – תקנים, רגולציה ומטריצת בקרות
- `docs/THREAT_MODEL.md` – נכסים, תוקפים, STRIDE, LINDDUN וסיכוני שארית
- `docs/DECISIONS.md` – החלטות שהתקבלו, והחלטות שממתינות
- `docs/ARCHITECTURE.md` – רכיבים, תהליכים, crates, מפתחות, סכמות
- `docs/ARCHITECTURE_REVIEW.md` – למה הארכיטקטורה נראית כך
- `docs/PRIVACY_PIPELINE.md` – צנרת הסינון (הלב של הפרויקט)
- `docs/AI_LAYER.md` – בניית הקשר, פלט מובנה, כללי המודל
- `docs/SCREENS.md` + `docs/design/` – המסכים שאושרו
- `docs/REPORT_STRUCTURE.md` – מבנה הדוח וסגנון הכתיבה
- `docs/ROADMAP.md` – סדר העבודה ותנאי מעבר · `docs/PHASE1_PLAN.md` – השלב הנוכחי

## שגרת עבודה
- משימה אחת בכל פעם, עם תוכנית קצרה לפני קוד.
- כל שינוי ב-`crates/dv-privacy`, `dv-egress` או `dv-vault`: להריץ את החבילה האדוורסריאלית ואת ה-canary (`cargo test -p dv-privacy`). תנאי למיזוג: **0 דליפות**.
- בדיקות לא קוראות ל-API אמיתי (mock).
- `cargo fmt`, `cargo clippy -- -D warnings`, `cargo xtask check-invariants`, `cargo deny check`. ב-UI: `tsc --strict`, eslint.
- תלויות מינימליות ונעולות (`Cargo.lock`, `pnpm-lock.yaml`). כל תלות חדשה מקבלת נימוק ב-PR.
- `.gitignore` חוסם `vault/`, `*.db`, `*.vaultbak`, `.env`, ארכיונים. pre-commit (`cargo xtask scan --staged`) חוסם ת.ז. תקינה, מיילים, טלפונים וקבצים בינאריים.
