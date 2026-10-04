# ארכיטקטורה (אושרה בשלב 0 – גרסה 2)

> גרסה 1 (Tauri + Python sidecar) נסקרה ב-`ARCHITECTURE_REVIEW.md` והוחלפה. ההחלטות מתועדות ב-`DECISIONS.md`, והתקנים ב-`STANDARDS.md`.

```
┌──────────────────────────── המחשב של עינת ─────────────────────────────┐
│                                                                        │
│  ┌──────────────────────────┐  Tauri IPC (פקודות ברשימה לבנה,           │
│  │ React + TS (UI, RTL)     │  isolation pattern, אימות סכמה)           │
│  │ CSP: אין רשת, טקסט בלבד  │◄───────────────────┐                       │
│  └──────────────────────────┘                    ▼                       │
│                             ┌───────────────────────────────────────┐    │
│                             │ תהליך ראשי – Rust (בינארי חתום אחד)   │    │
│                             │  vault · privacy · domain · ai ·      │    │
│                             │  export · ipc                         │    │
│                             │        │ ClearedPayload בלבד          │    │
│                             │        ▼                              │    │
│                             │  egress  ← ה-crate היחיד עם רשת        │────┼──► api.anthropic.com
│                             └──┬──────────────┬─────────────────────┘    │   TLS1.3, שורשי Mozilla,
│          stdin/stdout, bytes   │              │                          │   ZDR, inference_geo=us
│  ┌─────────────────────────────▼──┐     ┌─────▼─────────────────────┐    │
│  │ ingest worker (אותו בינארי,     │     │ OS: TPM / Secure Enclave, │    │
│  │ תהליך נפרד): בלי מפתחות, בלי DB, │     │ Keychain / Cred. Manager  │    │
│  │ PDF (pdfium) · DOCX · OCR       │     └───────────────────────────┘    │
│  └────────────────────────────────┘                                     │
│   vault/                                                                │
│   ├ vault.header   פרמטרי KDF, מפתחות עטופים, עוגן יומן (מאומת ב-MAC)  │
│   ├ main.db        SQLCipher + AES-256-GCM לכל שדה של תיק               │
│   ├ identity.db    SQLCipher, מפתח נפרד (סוד הקישור)                    │
│   └ audit.db       שרשרת HMAC, מטא-דאטה בלבד                            │
└────────────────────────────────────────────────────────────────────────┘
```

## תהליכים
| תהליך | מחזיק מפתחות | רשת | קורא קבצים לא מהימנים | הערה |
|---|---|---|---|---|
| ראשי (Rust + webview) | כן, רק כשהכספת פתוחה | רק דרך `egress` | לא | |
| ingest worker | **לא** | **לא** | כן | מופעל לכל מסמך, עם מגבלות זמן וזיכרון, ונהרג בסוף |
| tesseract helper | לא | לא | כן (תמונה) | בינארי חתום שנקרא מה-worker בלבד |

## Crates (Rust workspace)
| Crate | תפקיד | תלויות רגישות | אסור |
|---|---|---|---|
| `dv-vault` | KDF, מפתחות, SQLCipher, AEAD, נעילה, יומן, env_checks, קשירה לחומרה | `aws-lc-rs` (fips), `argon2`, `rusqlite` (sqlcipher), `zeroize`, `secrecy`, `keyring` | רשת |
| `dv-privacy` | שכבות 1–7, `variants_he`, NER (`ort` + `tokenizers`), **gate** ⇒ `ClearedPayload` | `ort`, `tokenizers`, `aho-corasick`, `regex` | רשת |
| `dv-ai` | context builder, פרומפטים, סכמות, פעולות מהירות, פענוח פלט | `serde` | רשת. **בונה בקשות בלבד** |
| `dv-egress` | שליחה ל-AI שנבחר (Anthropic כברירת מחדל, או OpenAI / Google לפי D-040): רשימה לבנה של endpoints ודגמים, streaming, retry, מגבלות | `reqwest` (rustls, בלי שורשי מערכת), `webpki-roots` | כל דבר חוץ מ-`ClearedPayload` |
| `dv-domain` | תיקים, סעיפים, ציונים, `interpretation`, DSM | — | רשת |
| `dv-ingest` | ה-worker: pdfium, DOCX (`zip` + `quick-xml`), OCR, חילוץ מטא-דאטה | `pdfium-render` | רשת, מפתחות |
| `dv-export` | DOCX מתבנית, הצפנת ECMA-376 Agile, `restore` | `zip`, `quick-xml`, `cfb` | רשת |
| `dv-ipc` | הגדרות פקודות, סכמות, יצירת טיפוסי TS (`specta`) | `serde`, `specta` | |
| `apps/desktop/src-tauri` | מעטפת דקה: חלון, CSP, capabilities, העברת פקודות ל-crates | `tauri` | plugins של fs/shell/http |

**אינווריאנטים שנאכפים ב-CI (`xtask check-invariants`):**
1. `reqwest`, `hyper`, `ureq` ודומיהם מופיעים בעץ התלויות **רק** של `dv-egress`.
2. `#![forbid(unsafe_code)]` בכל ה-crates. חריגים (FFI לחומרה) מבודדים במודול אחד ועוברים ביקורת.
3. אין `dangerouslySetInnerHTML` ב-UI.
4. הבנאי של `ClearedPayload` זמין רק בתוך `dv-privacy::gate`.

## מפתחות

```
MK (256 ביט אקראי) – עטוף בנפרד בכל "חריץ" (slot) ב-vault.header, כמו LUKS:
   slot password      : KEK = HKDF(Argon2id(סיסמה, salt 32B, m=256MiB–1GiB, t=3, p=4))
   slot recovery      : KEK = HKDF(ערכת שחזור 256 ביט)
   slot windows_hello : KEK משוחרר ע"י מפתח TPM של Windows Hello (PIN / פנים)     [שלב 2ב]
   slot macos         : KEK ב-Keychain עם access control של Touch ID / סיסמת Mac    [שלב 2ב]

MK ─ HKDF ─► K_header_mac, K_audit_mac, K_index (HMAC לחיפוש)
MK ─ עוטף ─► DEK_main, DEK_identity, DEK_audit (מפתחות SQLite), K_casewrap, K_backup
K_casewrap ─ עוטף ─► CaseKey לכל תיק  (AES-256-GCM ברמת שדה)
```
- **כל הקריפטו** חוץ מ-Argon2id רץ במודול FIPS 140-3 (`aws-lc-rs`): AES-256-GCM, HKDF, HMAC ו-DRBG.
- **AEAD לכל שדה:** nonce אקראי של 96 ביט, AAD = `"dv/v1" ‖ table ‖ column ‖ row_id ‖ case_id`.
- **header:** פרמטרי KDF, מפתחות עטופים ו-(seq, mac) של ראש היומן. כל ה-header מאומת ב-HMAC שנגזר מ-MK.
- **כניסה (D-012):** Windows Hello / Touch ID ביומיום, סיסמה כגיבוי, ערכת שחזור מודפסת. כל slot הוא מימוש של `UnlockProvider`.
- **שינוי סיסמה:** עטיפה מחדש של MK בלבד.
- **מחשב חדש, TPM שהתאפס, שכחת סיסמה:** פתיחה עם ערכת השחזור, ואחר כך רישום מחדש של slots.
- **מחיקת תיק:** מחיקת CaseKey העטוף, ואז `secure_delete` ו-VACUUM. אירוע ביומן.
- **נעילה** (ידנית, אחרי 10 דקות, בנעילת מסך או בשינה): zeroize לכל המפתחות, סגירת חיבורים, טעינה מחדש של ה-webview.
- **ערכת שחזור (D-016):** 256 ביט ב-Crockford Base32 (13×4 + ביקורת), הדפסה, ואימות של קבוצה אקראית לפני סיום ההתקנה.

## סכמה – main.db
`_enc` = מוצפן ב-CaseKey עם AAD. **כל** מידע ששייך לתיק מוצפן, כולל ציונים ותאריכים.
```sql
cases(id PK /* UUIDv4 */, code_enc, status, current_section,
      created_at_enc, updated_at_enc, retention_until_enc,
      consent_json_enc,            -- תאריך, גרסת טופס, מי חתם (תפקיד)
      wrapped_case_key BLOB)
inputs(id PK, case_id FK, section_key, source_type, content_tagged_enc, created_at_enc)
attachments(id PK, case_id FK, filename_enc, mime, blob_enc, extracted_text_tagged_enc,
            metadata_report_enc, ocr_used, approved, created_at_enc)
scores(id PK, case_id FK, test, test_version, payload_enc)   -- subtest, raw, scaled, index, t, range, table_version, note
conversations(id PK, case_id FK, section_key, created_at_enc)
messages(id PK, conversation_id FK, role, content_tagged_enc,
         filter_report_enc, payload_sha256, tokens_in, tokens_out, created_at_enc)
transmissions(id PK, case_id FK, payload_sha256, payload_tagged_enc,   -- בדיוק מה שנשלח
              approved_by_user INTEGER, model, request_id_enc, sent_at_enc)
drafts(id PK, case_id FK, section_key, version, text_tagged_enc,
       status, created_by, source_refs_enc, approved_at_enc)
style_profile(id PK, content_enc /* ב-K_style */, version, updated_at)
kb_overrides(id PK, test, key, value, source, approved, updated_at)
settings(key PK, value)            -- בלי מידע אישי
```
**ההודעות, הטיוטות והשליחות נשמרות עם תגיות בלבד.** השמות חוזרים רק בתצוגה ובייצוא.

## identity.db (סוד הקישור)
```sql
identities(id PK, case_id, tag, role, value_enc, variants_enc, source /* form|metadata|review */, created_at_enc)
decisions(id PK, case_id, token_hmac, decision /* hide|not_a_name */, tag, created_at_enc)
practitioner(id PK, value_enc, variants_enc)   -- שם עינת, שם הקליניקה, מספר רישיון
```
`token_hmac` הוא HMAC ולא hash רגיל, כדי שאי אפשר יהיה לנחש שמות מתוך רשימת hash-ים.

## audit.db
```sql
audit(seq PK, ts, event, case_ref /* HMAC של case_id */, meta_json_enc, prev_mac, mac)
-- event: unlock|unlock_failed|lock|case_open|send|blocked|override|export|delete|backup|restore|settings_change|integrity_fail
-- שמירה: 24 חודשים לפחות. תוכן אף פעם לא נרשם.
```

## Egress – כללי יציאה (`dv-egress`)
- Host ל-AI: `api.anthropic.com` כברירת מחדל. אם עינת בחרה בהגדרות ChatGPT או Gemini (D-040): `api.openai.com` (`POST /v1/chat/completions`), `generativelanguage.googleapis.com` (`POST /v1beta/models/{model}:generateContent`) או `api.mistral.ai` (`POST /v1/chat/completions`), רק לדגמים מהרשימה הלבנה. מודל מקומי: `http://127.0.0.1:11434/api/chat` בלבד (Ollama, בלי TLS כי זה בתוך המחשב). TLS 1.3 בלבד, שורשי `webpki-roots` (לא של המערכת), אותו client לכל הספקים.
- **ספקים אחרים (D-040, `providers.rs`):** השער מאשר תמיד את אותו גוף בקשה (בצורה של Messages API). אחרי השער, `dv-egress` מעביר את אותן מחרוזות לשדות של הספק, בלי להוסיף שום טקסט חוץ משמות שדות והגדרות קבועות (נבדק בבדיקה). מה שאין לו מקבילה מדויקת (כלים, תמונות, thinking, שדה לא מוכר) נחסם. התשובה מומרת חזרה לאותה צורה, כך שהבדיקות המקומיות של התשובה זהות לכל ספק. ל-OpenAI נשלח `store: false`.
- **עדכוני תוכנה (D-033, `update.rs`):** GET בלבד ל-hosts של GitHub מהרשימה הלבנה (גם ב-redirect), בלי נתונים מהכספת. הודעת גרסה חתומה ב-Ed25519 במפתח ציבורי מקובע, ו-SHA-256 של המתקין. כל ספק = אין עדכון. גרסה חדשה יורדת ונבדקת ברקע, ומותקנת רק בלחיצה (D-038).
- **Endpoints מותרים:** `POST /v1/messages` ו-`POST /v1/messages/count_tokens`. כל השאר לא ממומש.
- **דגמים מותרים:** רשימה לבנה בקוד של דגמים שזמינים תחת ZDR. דגמים מסוג Covered Models (Fable, Mythos) חסומים. הדגם נבחר בהגדרות מתוך הרשימה.
- **שדות אסורים:** `metadata`, Files, Batch, code_execution. **חריג יחיד:** `web_search_20250305` (הגרסה הבסיסית, זכאית ל-ZDR), ורק בקריאות של מצב מחקר (D-014), בלי תוכן מהתיק ועם `allowed_domains`.
- `inference_geo: "us"` קבוע.
- הסכמות (structured outputs / `strict`) קבועות בקוד. בדיקה אוטומטית שאין בהן תוכן מתיק.
- מגבלת גודל payload, מגבלת קצב, תקרת עלות חודשית ו-circuit breaker.
- ללא אינטרנט: עבודה מקומית ממשיכה, וה-AI מסומן כלא זמין.

## הקשחת Tauri
- CSP: `default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src ipc: http://ipc.localhost; object-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'`.
- Isolation pattern. capabilities: רק הפקודות שלנו. אין plugins של fs, shell או http. דיאלוגים לבחירת קבצים נפתחים מצד Rust.
- `contentProtected: true` (מתג בהגדרות, רק כשהכספת פתוחה; D-037), DevTools כבויים ב-release, ניווט חיצוני חסום.
- פונטים (Frank Ruhl Libre, Assistant, ברישיון OFL) ארוזים מקומית.

## סביבה
- **תיקייה מסונכרנת לענן:** סירוב לפעול.
- **הצפנת דיסק כבויה:** תזכורת בלבד (D-013).
- **אין:** קבצים זמניים לא מוצפנים, לוגים עם תוכן, טלמטריה, crash reporting חיצוני.
- **גיבוי (D-024):** `.vaultbak` = `"DVBAK\0\0\x01"` · אורך הכותרת · `vault.header` · זמן הגיבוי · מכל AES-256-GCM (מפתח `K_backup`, AAD = `dv/backup/v1|vault_id|created_at`) ובו שלושת מסדי הנתונים כ-snapshot של `sqlcipher_export` (מוצפנים באותם מפתחות, אף פעם לא גלויים). נפתח בסיסמה או בערכת השחזור שהיו ביום הגיבוי, בכל מחשב. שחזור רק לתיקייה בלי כספת; תרגול שחזור פותח לתיקייה זמנית ומוחק.
- **Clipboard:** ניקוי אחרי 60 שניות, והחרגה מהיסטוריית הלוח.

## מבנה הריפו
```
Cargo.toml                 (workspace)
crates/  dv-vault/ dv-privacy/ dv-ai/ dv-egress/ dv-domain/ dv-ingest/ dv-export/ dv-ipc/
apps/desktop/  src/ (React+TS+Vite)   src-tauri/
xtask/                     בדיקות אינווריאנטים, סריקת pre-commit (Rust, בלי Python)
knowledge/                 *.yaml (status: pending|approved)
templates/report.docx
tests/fixtures/            נתונים בדויים בלבד
tools/dev-only/            כלי אימות לפיתוח (למשל msoffcrypto). לא נארזים.
docs/
```
