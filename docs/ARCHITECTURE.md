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
| `dv-egress` | שליחה ל-Anthropic: רשימה לבנה של endpoints ודגמים, streaming, retry, מגבלות | `reqwest` (rustls, בלי שורשי מערכת), `webpki-roots` | כל דבר חוץ מ-`ClearedPayload` |
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
סיסמה ──Argon2id (m=1GiB יעד / 256MiB רצפה, t=3, p=4, salt 32B)──► K_pw
סוד חומרה (256 ביט, עטוף במפתח TPM / Secure Enclave שלא ניתן לייצא) ──► K_dev
KEK_unlock   = HKDF-SHA256(K_pw ‖ K_dev, info="dv/kek/v1")
KEK_recovery = HKDF-SHA256(ערכת שחזור 256 ביט, info="dv/recovery/v1")

MK (256 ביט אקראי) ── עטוף ב-KEK_unlock, ובנפרד ב-KEK_recovery   (ב-vault.header)
   ├ עוטף ► DEK_main      (מפתח SQLCipher ל-main.db)
   ├ עוטף ► DEK_identity  (מפתח SQLCipher ל-identity.db)
   ├ עוטף ► DEK_audit     (SQLCipher ל-audit.db) + K_audit_mac
   ├ עוטף ► K_casewrap ── עוטף ► CaseKey לכל תיק  (AES-256-GCM ברמת שדה)
   └ עוטף ► K_backup
```
- **כל הקריפטו** חוץ מ-Argon2id רץ במודול FIPS 140-3 (`aws-lc-rs`): AES-256-GCM, HKDF, HMAC ו-DRBG.
- **AEAD לכל שדה:** nonce אקראי של 96 ביט, AAD = `"dv/v1" ‖ table ‖ column ‖ row_id ‖ case_id`.
- **header:** פרמטרי KDF, מפתחות עטופים ו-(seq, mac) של ראש היומן. כל ה-header מאומת ב-HMAC שנגזר מ-MK.
- **פתיחה ביומטרית (ממתין להחלטה P-01):** עותק של K_dev שמשוחרר רק בנוכחות ביומטרית. הסיסמה נדרשת בכל הפעלה ראשונה של היום.
- **שינוי סיסמה:** עטיפה מחדש של MK בלבד.
- **מחשב חדש, TPM שהתאפס, שכחת סיסמה:** פתיחה עם ערכת השחזור, ואחר כך רישום מחדש של סיסמה וסוד חומרה.
- **מחיקת תיק:** מחיקת CaseKey העטוף, ואז `secure_delete` ו-VACUUM. אירוע ביומן.
- **נעילה** (ידנית, אחרי 10 דקות, בנעילת מסך או בשינה): zeroize לכל המפתחות, סגירת חיבורים, טעינה מחדש של ה-webview.
- **ערכת שחזור:** 24 מילים (BIP-39, 256 ביט), הדפסה, ואימות של 3 מילים אקראיות לפני סיום ההתקנה.

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
- Host יחיד: `api.anthropic.com`. TLS 1.3 בלבד, שורשי `webpki-roots` (לא של המערכת).
- **Endpoints מותרים:** `POST /v1/messages` ו-`POST /v1/messages/count_tokens`. כל השאר לא ממומש.
- **דגמים מותרים:** רשימה לבנה בקוד של דגמים שזמינים תחת ZDR. דגמים מסוג Covered Models (Fable, Mythos) חסומים. הדגם נבחר בהגדרות מתוך הרשימה.
- **שדות אסורים:** `metadata`, `tools` מסוג server-tool (web_search, code_execution), Files, Batch.
- `inference_geo: "us"` קבוע.
- הסכמות (structured outputs / `strict`) קבועות בקוד. בדיקה אוטומטית שאין בהן תוכן מתיק.
- מגבלת גודל payload, מגבלת קצב, תקרת עלות חודשית ו-circuit breaker.
- ללא אינטרנט: עבודה מקומית ממשיכה, וה-AI מסומן כלא זמין.

## הקשחת Tauri
- CSP: `default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src ipc: http://ipc.localhost; object-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'`.
- Isolation pattern. capabilities: רק הפקודות שלנו. אין plugins של fs, shell או http. דיאלוגים לבחירת קבצים נפתחים מצד Rust.
- `contentProtected: true`, DevTools כבויים ב-release, ניווט חיצוני חסום.
- פונטים (Frank Ruhl Libre, Assistant, ברישיון OFL) ארוזים מקומית.

## סביבה
- **תיקייה מסונכרנת לענן:** סירוב לפעול.
- **הצפנת דיסק כבויה:** חסימה או אזהרה (ממתין להחלטה P-02).
- **אין:** קבצים זמניים לא מוצפנים, לוגים עם תוכן, טלמטריה, crash reporting חיצוני.
- **גיבוי:** `.vaultbak` = snapshot של ה-DB דרך SQLite backup API, מוצפן ב-K_backup (AES-256-GCM במקטעים). ניתן לשחזר עם הסיסמה במכשיר המקורי, או עם ערכת השחזור בכל מכשיר.
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
