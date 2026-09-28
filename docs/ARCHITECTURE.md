# ארכיטקטורה (מוצעת – לסקירה בשלב 0)

```
┌──────────────────────── המחשב של עינת ────────────────────────┐
│  Tauri 2 (מעטפת דסקטופ)                                        │
│  ┌──────────────────────────┐   JSON-RPC על stdio              │
│  │ React + TypeScript (UI)  │◄────────────────────────┐        │
│  │ ללא גישה לרשת (CSP)      │   (אין פורט פתוח)       ▼        │
│  └──────────────────────────┘         ┌──────────────────────┐ │
│                                       │ Python Core (sidecar)│ │
│                                       │ vault/ privacy/ ai/  │ │
│                                       │ domain/ ingest/      │ │
│                                       │ export/ rpc/         │ │
│                                       └───┬──────────┬───────┘ │
│   vault/ (תיקייה מקומית, לא בענן)◄────────┘          │         │
│   ├ vault.header  מפתחות עטופים + פרמטרי KDF         │         │
│   ├ main.db       SQLCipher                          │         │
│   ├ identity.db   SQLCipher, מפתח נפרד               │         │
│   └ audit.db      append-only, hash-chain            │         │
└──────────────────────────────────────────────────────┼─────────┘
                                   רק טקסט עם תגיות    │ HTTPS, host יחיד
                                                       ▼
                                             api.anthropic.com
```

## רכיבים
| רכיב | טכנולוגיה | למה |
|---|---|---|
| מעטפת | Tauri 2 | קל, מערכת הרשאות מובנית, מתקינים ל-Windows ו-Mac |
| UI | React + TS + Vite, RTL | ממשק מודרני; CSP חוסם כל רשת |
| Core | Python 3.12, PyInstaller sidecar | הכלים הטובים ביותר לעברית, Word, PDF ו-OCR |
| IPC | JSON-RPC 2.0 על stdio, סכמות pydantic | בלי פורט רשת פתוח |
| DB | SQLCipher (`sqlcipher3`) | הצפנת קובץ מלאה |
| קריפטו | `cryptography` (AES-256-GCM), `argon2-cffi` | סטנדרטי ומבוקר |
| Keychain | `keyring` | מפתח API וביומטריה |
| NER | `dicta-il/dictabert-ner`, onnxruntime, quantized | עברית, מקומי |
| PDF / OCR | `pymupdf`, Tesseract + heb | מקומי |
| Word | `docxtpl`, `msoffcrypto-tool` | תבנית + הצפנה |
| AI | Anthropic Python SDK | streaming, tool use, prompt caching |

**חלופה לבדיקה:** Electron + Node/TS. יתרון: שפה אחת. חיסרון: NER עברי ו-OCR פחות בשלים.

## מבנה הריפו
```
diagnostic-vault/
├ CLAUDE.md  START_HERE.md
├ apps/desktop/
│  ├ src/             screens/, components/, rpc-client.ts, i18n/he.ts
│  └ src-tauri/       config, capabilities, sidecar
├ core/
│  ├ rpc/             server.py, schemas.py
│  ├ vault/           crypto.py, keys.py, db.py, audit.py, lock.py, env_checks.py
│  ├ privacy/         pipeline.py, stages/, gate.py, variants_he.py, review.py
│  ├ ai/              client.py, context_builder.py, prompts/, schemas.py, actions.py
│  ├ domain/          cases.py, sections.py, scores.py, interpretation.py, dsm.py
│  ├ ingest/          pdf.py, ocr.py, docx_in.py, metadata_strip.py
│  └ export/          docx_export.py, restore.py
├ knowledge/          wppsi.yaml, ados2.yaml, asrs.yaml, abas.yaml, ... (status: pending|approved)
├ templates/report.docx
├ tests/              unit/, adversarial/, canary/, e2e/, fixtures/
└ docs/
```

## מפתחות
```
סיסמת-על ──Argon2id (m≥256MB, t≥3)──► KEK (בזיכרון בלבד)
   ├ עוטף ► DEK_main  ├ עוטף ► DEK_identity  └ עוטף ► DEK_audit
DEK_main ─ עוטף ► CaseKey לכל תיק (AES-256-GCM ברמת שדה)
```
- שינוי סיסמה = עטיפה מחדש של המפתחות בלבד.
- מחיקת תיק = מחיקת ה-CaseKey (crypto-shredding) + `PRAGMA secure_delete=ON`.
- נעילה (ידנית, או אחרי 10 דקות) = מחיקת המפתחות מהזיכרון וסגירת החיבורים.
- שחזור: כברירת מחדל אין. אופציונלי: מפתח שחזור מודפס (24 מילים).

## סכמה – main.db (`_enc` = מוצפן ב-CaseKey)
```sql
cases(id PK, code TEXT UNIQUE, status, current_section,
      created_at, updated_at, retention_until, wrapped_case_key BLOB)
inputs(id PK, case_id FK, section_key, source_type, content_enc BLOB, created_at)
      -- source_type: intake|prior_report|professional|kindergarten|observation|free_text
attachments(id PK, case_id FK, filename_enc, mime, blob_enc, extracted_text_enc,
            ocr_used, metadata_stripped, approved, created_at)
scores(id PK, case_id FK, test, test_version, subtest, raw, scaled, index_value,
       t_score, computed_range, interpretation_table_version, note_enc, created_at)
conversations(id PK, case_id FK, section_key, created_at)
messages(id PK, conversation_id FK, role, content_tagged_enc,
         filter_report_json, tokens_in, tokens_out, created_at)
drafts(id PK, case_id FK, section_key, version, text_tagged_enc,
       status,          -- proposed|approved|rejected|superseded
       created_by,      -- ai|user
       source_refs_json, approved_at)
style_profile(id PK, content_enc, version, updated_at)
kb_overrides(id PK, test, key, value, source, approved, updated_at)
settings(key PK, value)
```
**ההודעות והטיוטות נשמרות רק עם תגיות.** השמות חוזרים לטקסט בתצוגה ובייצוא בלבד.

## identity.db
```sql
identities(id PK, case_id, tag, role, value_enc, variants_enc, created_at)
decisions(id PK, case_id, token_hash, decision, tag, created_at)  -- hide|not_a_name
```

## audit.db
```sql
audit(id PK, ts, event, case_code, meta_json, prev_hash, hash)
-- event: unlock|lock|case_open|send|blocked|export|delete|settings_change
-- meta_json: מטא-דאטה בלבד, אף פעם לא תוכן. שלמות ה-hash-chain נבדקת בכל פתיחה.
```

## סביבה
- **תיקיית vault מסונכרנת לענן** (Dropbox, OneDrive, iCloud, Google Drive): התוכנה מסרבת לפעול.
- **BitLocker / FileVault כבוי:** אזהרה.
- **אין:** קבצים זמניים לא מוצפנים, לוגים עם תוכן, טלמטריה.
- **גיבוי:** קובץ `.vaultbak` מוצפן, לכונן חיצוני.
- **Clipboard:** "העתק" מנקה את הלוח אחרי 60 שניות.
- **ללא אינטרנט:** הזנה ועריכה ממשיכות לעבוד, וה-AI מסומן כלא זמין.
