# בקשת ZDR מ-Anthropic

**מה זה:** Zero Data Retention. אחרי שהתשובה חוזרת, Anthropic לא שומרת את מה שנשלח. בלי אישור בכתב, **אסור** לשלוח מידע אמיתי (`STANDARDS.md` §4, §6).

**עודכן 3.10.2026:** נוספו לשאלה 2 הדגמים החדשים (`claude-opus-5-5`, `claude-sonnet-5-5`, AI-2 בסקירת השדרוגים), כדי שהאישור יכסה אותם לפני שמוסיפים אותם לרשימה הלבנה. לספקים האחרים שהתוכנה מאפשרת (ChatGPT, Gemini, Mistral): `ZDR_OTHER_PROVIDERS.md`.

## לפני השליחה (אור ועינת, כ-15 דקות)
1. ב-Console של Anthropic צריך ארגון **על שם עינת או הקליניקה**, ועינת היא הבעלים (Owner). ZDR מופעל לכל ארגון בנפרד.
2. להעתיק את **Organization ID** (Console ← Settings ← Organization) ולהכניס אותו במקום המתאים במייל.
3. להגדיר **תקרת הוצאה חודשית** (Spend limit) נמוכה ב-Console. זו שכבת הגנה נוספת מעבר לתקרה שבתוכנה.
4. לא ליצור עדיין מפתח API לשימוש אמיתי. אם צריך מפתח לבדיקה, משתמשים בו רק עם תיקים בדויים.
5. שולחים מהמייל של עינת, דרך הטופס **Contact sales** באתר של Anthropic, או לאיש הקשר אם כבר יש. מבקשים תשובה **בכתב**.

## מה לא לכתוב
- שום פרט על מטופל, גם לא לדוגמה.
- לא את מפתח ה-API.
- לא לצרף קבצים מהתוכנה. התיאור הטכני במייל מספיק.

## אחרי שמגיעה תשובה
- לשמור את המייל מחוץ לריפו (בתיבת הדואר של עינת, ועותק PDF בתיקייה מוצפנת).
- לרשום ב-`DECISIONS.md`: "ZDR אושר בכתב, תאריך, אילו דגמים ואילו יכולות".
- אם דגם מהרשימה **לא** מכוסה: להוריד אותו מ-`ALLOWED_MODELS` (בשני המקומות, `dv-ai` ו-`dv-egress`; בדיקה ב-`dv-core` מוודאת שהם זהים).
- אם כלי החיפוש (מצב מחקר) לא מכוסה: מצב המחקר נשאר כבוי. הוא ממילא לא שולח תוכן מהתיק, אבל עדיף להחמיר.
- לעדכן בטופס ההסכמה את סעיף 4 ("מה קורה למידע שם") לפי הנוסח המדויק.
- אם ZDR לא מאושר: לא חוזרים בשקט לשמירה הרגילה (30 יום). מתייעצים (ראו שאלה 8 במייל), ורושמים החלטה.

---

## המייל (באנגלית)

**Subject:** Zero Data Retention request – small clinical practice (Israel), Messages API only

Hello,

I am a licensed developmental psychologist in private practice in Israel. I would like to enable Zero Data Retention for my organization before any clinical use of the API.

- Organization name: [organization name as shown in the Console]
- Organization ID: [from Console → Settings → Organization]
- Contact: [full name], [clinic name]

**The use case**

A desktop application that runs only on my own computer and helps me word sections of children's developmental assessment reports. I make every clinical finding and judgement. The model only helps with wording, and I review and approve every paragraph before it goes into a report. Scores are interpreted by fixed tables in the application, not by the model.

**What is sent**

- De-identified text only. Before any request, the application removes names (replaced by roles such as "the child" or "the mother"), ID numbers, phone numbers, email addresses, addresses, places, schools and kindergartens, and all dates (converted to relative time such as "about two weeks ago").
- I see the exact text of every request on screen and approve it before it is sent.
- What remains is the child's age in years and months, test scores, and clinical observations, without identifiers.
- No images, no files, no `metadata.user_id`, and no case information inside JSON schemas.

**Technical scope (enforced by allow-lists in the application code)**

- Endpoint: `POST /v1/messages` only (`count_tokens` may be added later). The Files API, Message Batches, code execution and other server tools are not implemented.
- One exception: a separate "research" mode sends only general professional questions (never case content) with the web search tool `web_search_20250305`, restricted by `allowed_domains` to professional sources.
- Models: `claude-opus-5`, `claude-sonnet-5`, `claude-opus-4-8`. I would also like to use `claude-opus-5-5` and `claude-sonnet-5-5` once they are confirmed as ZDR-eligible.
- `inference_geo` is always `"us"`.
- Structured outputs with fixed schemas, and prompt caching of the fixed system prompt. Adaptive thinking may be added later.
- Volume is low: a few dozen reports per year, roughly [estimate] requests per month.

**Please confirm in writing**

1. That ZDR is enabled for the organization above, from which date, and that it covers every API key in the organization, including keys created later.
2. Which of the models listed above are covered by ZDR, including `claude-opus-5-5` and `claude-sonnet-5-5`. If any is not, which models are.
3. Whether these features are covered: structured outputs (`output_config`), prompt caching, adaptive thinking, `count_tokens`, and the web search tool `web_search_20250305`.
4. Which exceptions still apply under ZDR (for example content flagged by Trust & Safety, or legal requirements), and the retention period for each.
5. Whether `inference_geo: "us"` means the content is processed only in the United States, including any content retained under those exceptions.
6. Which Data Processing Addendum applies, and which transfer mechanism it relies on. I am subject to Israel's Privacy Protection (Transfer of Data to Databases Abroad) Regulations, 2001, so I need to know what commitments you make as the recipient.
7. Which sub-processors could have access to request content.
8. If ZDR cannot be enabled for an organization of this size, what alternative arrangement with the least retention you can offer.
9. How you will notify me if the ZDR terms, or the eligible models and features, change.

Thank you,

[full name]
Licensed psychologist, [clinic name]
