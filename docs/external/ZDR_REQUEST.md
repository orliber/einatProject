# בקשת ZDR מ-Anthropic

**מה זה:** Zero Data Retention. אחרי שהתשובה חוזרת, Anthropic לא שומרת את מה שנשלח. בלי אישור בכתב, **אסור** לשלוח מידע אמיתי (`STANDARDS.md` §4, §6). זה הפריט הראשון ברשימה "מוכנה לתיקים אמיתיים?" בהגדרות (`zdr`).

**עודכן 26.10.2026 (D-115):** המייל נשלח עכשיו **מאור**, בשם הקליניקה, ועינת בהעתק כבעלת הארגון. הפרטים הטכניים נבדקו מול הקוד: endpoint אחד, אילו שדות יוצאים בפועל, ורשימת הדגמים כפי שהיא ב-`ALLOWED_MODELS`. מה שהתוכנה **לא** עושה היום (`count_tokens`, מצב המחקר, `thinking`) כתוב ככזה, ומבקשים אישור מראש רק כדי שאפשר יהיה להוסיף אותו בלי בקשה חדשה.
**3.10.2026:** נוספו לשאלה 2 הדגמים `claude-opus-5-5` ו-`claude-sonnet-5-5` (AI-2). לספקים האחרים (ChatGPT, Gemini, Mistral): `ZDR_OTHER_PROVIDERS.md`.

## מה התוכנה שולחת בפועל (נבדק מול הקוד, 26.10.2026)
| | מה | איפה בקוד |
|---|---|---|
| כתובת | `POST https://api.anthropic.com/v1/messages` בלבד. אין קריאה אחרת ל-`api.anthropic.com`. | `crates/dv-egress/src/lib.rs` (`MESSAGES_URL`) |
| `count_tokens` | **לא ממומש.** מותר לפי הכללים (`CLAUDE.md`, D-009), ולכן מבקשים שהאישור יכסה אותו. | – |
| headers | `content-type`, `x-api-key`, `anthropic-version: 2023-06-01`. בלי `anthropic-beta`. | `dv-egress/src/lib.rs` |
| שדות בבקשה | רשימה לבנה: `model`, `max_tokens`, `system`, `messages`, `output_config`, `inference_geo`, `tools`, `thinking`, `stream`. כל שדה אחר נדחה, כולל `metadata`. | `ALLOWED_FIELDS` |
| בשימוש היום | `output_config` (`effort`, ופלט מובנה בסכמה קבועה `format`), `cache_control: ephemeral` על ההוראות הקבועות ועל החומרים המסוננים (D-058), `stream: true` (D-035), `inference_geo: "us"` בכל בקשה (נאכף). | `dv-ai/src/request.rs` |
| לא בשימוש היום | `thinking` (מותר ברשימה, לא נשלח). `tools`: רק `web_search_20250305` עם `allowed_domains`, למצב מחקר (D-014) שנבנה ב-`dv-ai` אבל **לא מחובר** לתוכנה. | `dv-ai/src/request.rs` |
| דגמים | `claude-opus-5`, `claude-sonnet-5`, `claude-opus-4-8`. Fable ו-Mythos, שדורשים שמירה של 30 יום, לא ברשימה בכוונה. | `ALLOWED_MODELS` ב-`dv-egress` וב-`dv-ai` (בדיקה ב-`dv-core` מוודאת שהם זהים) |
| בלי | Files API, Message Batches, code execution, כלי שרת אחרים, תמונות, קבצים, `metadata.user_id`, מידע מהתיק בתוך סכמות JSON. | – |

## לפני השליחה (אור ועינת, כ-15 דקות)
1. ב-Console של Anthropic צריך ארגון **על שם עינת או הקליניקה**, ועינת היא הבעלים (Owner). ZDR מופעל לכל ארגון בנפרד. אור יכול להיות חבר בארגון (Developer), לא בעלים.
2. להעתיק את **Organization ID** (Console ← Settings ← Organization) ולהכניס אותו במקום המתאים במייל.
3. להגדיר **תקרת הוצאה חודשית** (Spend limit) נמוכה ב-Console. זו שכבת הגנה נוספת מעבר לתקרה שבתוכנה.
4. לא ליצור עדיין מפתח API לשימוש אמיתי. אם צריך מפתח לבדיקה, משתמשים בו רק עם תיקים בדויים.
5. **אור שולח**, מהמייל שלו, דרך הטופס **Contact sales** באתר של Anthropic (או לאיש הקשר אם כבר יש), **עם עינת בהעתק (CC)**. Anthropic עשויה לבקש שבעלת הארגון תאשר בעצמה; אז עינת עונה "I confirm" מהמייל שלה. מבקשים תשובה **בכתב**.

## מה לא לכתוב
- שום פרט על מטופל, גם לא לדוגמה.
- לא את מפתח ה-API.
- לא לצרף קבצים מהתוכנה. התיאור הטכני במייל מספיק.

## אחרי שמגיעה תשובה
- לשמור את המייל מחוץ לריפו (בתיבת הדואר של עינת, ועותק PDF בתיקייה מוצפנת).
- לרשום ב-`DECISIONS.md`: "ZDR אושר בכתב, תאריך, אילו דגמים ואילו יכולות". עינת מסמנת בהגדרות ← "מוכנה לתיקים אמיתיים?" את הפריט הראשון.
- אם דגם מהרשימה **לא** מכוסה: להוריד אותו מ-`ALLOWED_MODELS` (בשני המקומות, `dv-ai` ו-`dv-egress`).
- אם `claude-opus-5-5` או `claude-sonnet-5-5` מכוסים: אפשר להוסיף אותם לרשימה, ב-PR נפרד שמפנה לתשובה.
- אם `count_tokens` או מצב המחקר לא מכוסים: לא מממשים אותם.
- לעדכן בטופס ההסכמה את סעיף 5 ("מה קורה למידע אצל החברה") לפי הנוסח המדויק.
- אם ZDR לא מאושר: לא חוזרים בשקט לשמירה הרגילה (30 יום). מתייעצים (ראו שאלה 8 במייל), ורושמים החלטה (D-004).

---

## המייל (באנגלית)

**To:** Anthropic Sales (Contact sales form, or the account contact)
**Cc:** [Einat's email], organization owner
**Subject:** Zero Data Retention request – small clinical practice (Israel), Messages API only

Hello,

I am the developer of a desktop application used by [Einat's full name], a licensed developmental psychologist in private practice in Israel, and I am writing on her behalf. She is copied on this email as the owner of the organization below, and can confirm the request directly. We would like Zero Data Retention enabled for the organization before any clinical use of the API.

- Organization name: [organization name as shown in the Console]
- Organization ID: [from Console → Settings → Organization]
- Organization owner: [Einat's full name], [clinic name]
- Technical contact: [Or's full name]

**The use case**

The application runs only on the psychologist's own computer and helps her word sections of children's developmental assessment reports. She makes every clinical finding and judgement. The model only helps with wording, and she reviews and approves every paragraph before it goes into a report. Scores are interpreted by fixed tables in the application, not by the model. There is no server of ours in between: requests go directly from her computer to the API.

**What is sent**

- De-identified text only. Before any request, the application replaces names with roles (such as "the child" or "the mother") and removes ID numbers, phone numbers, email addresses, addresses, places, schools and kindergartens, and all dates (converted to relative time such as "about two weeks ago"). A separate gate re-checks the exact bytes of every request and refuses to send if anything that looks like an identifier remains.
- What remains is the child's age in years and months, grammatical gender (for correct Hebrew), test scores, and clinical observations, without identifiers.
- No images, no files, no `metadata.user_id`, and no case information inside JSON schemas. The schemas are fixed in the code.

**Technical scope (enforced by allow-lists in the application code)**

- Endpoint: `POST https://api.anthropic.com/v1/messages` only. We would also like `POST /v1/messages/count_tokens` to be covered, so it can be added later; it is not used today. The Files API, Message Batches, code execution and other server tools are not implemented.
- Headers: `x-api-key`, `anthropic-version: 2023-06-01`, `content-type`. No beta headers.
- Request fields used today: `model`, `max_tokens`, `system`, `messages`, `output_config` (`effort`, and structured outputs with a fixed schema in `format`), `stream: true`, and `inference_geo: "us"` on every request. Prompt caching (`cache_control: {"type": "ephemeral"}`) is used on the fixed instructions and on the de-identified case material, so it is reused across turns.
- Allowed by the application but not used today: `thinking` (adaptive thinking), and one tool, the web search tool `web_search_20250305` with `allowed_domains` limited to professional sources, for a separate research mode that would send only general professional questions, never case content.
- Models: `claude-opus-5`, `claude-sonnet-5`, `claude-opus-4-8`. We would also like to use `claude-opus-5-5` and `claude-sonnet-5-5` once they are confirmed as ZDR-eligible. Models that require 30-day retention are deliberately excluded.
- Volume is low: a few dozen reports per year, roughly [estimate] requests per month.

**Please confirm in writing**

1. That ZDR is enabled for the organization above, from which date, and that it covers every API key in the organization, including keys created later.
2. Which of the models listed above are covered by ZDR, including `claude-opus-5-5` and `claude-sonnet-5-5`. If any is not, which models are.
3. Whether these are covered: `/v1/messages/count_tokens`, streaming, structured outputs (`output_config.format`), `output_config.effort`, prompt caching (`cache_control`), adaptive thinking, and the web search tool `web_search_20250305` with `allowed_domains`.
4. How long cached prompt content is kept under ZDR, and whether it is kept only in memory.
5. Which exceptions still apply under ZDR (for example content flagged by Trust & Safety, or legal requirements), and the retention period for each.
6. Whether `inference_geo: "us"` means the content is processed only in the United States, including any content retained under those exceptions.
7. Which Data Processing Addendum applies, and which transfer mechanism it relies on. The practice is subject to Israel's Privacy Protection (Transfer of Data to Databases Abroad) Regulations, 2001, so we need to know what commitments you make as the recipient.
8. If ZDR cannot be enabled for an organization of this size, what alternative arrangement with the least retention you can offer.
9. Which sub-processors could have access to request content.
10. How you will notify the organization owner if the ZDR terms, or the eligible models and features, change.

Thank you,

[Or's full name]
Developer, on behalf of [Einat's full name], licensed psychologist, [clinic name]
