# ZDR מספקים שאינם Claude (ChatGPT, Gemini, Mistral)

**למה:** התוכנה מאפשרת לבחור ספק AI (D-040). בתנאים הרגילים שלהם, OpenAI ו-Mistral שומרים את התוכן עד 30 יום לזיהוי שימוש לרעה, ו-Google שומרת לזמן מוגבל. ההמלצה ב-`docs/compliance/DPIA.md`: **ספק בלי ZDR בכתב לא משמש לתיק אמיתי**, ולא מופיע בטבלה של טופס ההסכמה שמוצג להורים.
**האם בכלל לשלוח:** רק אם עינת באמת מתכוונת להשתמש בספק הזה לתיקים אמיתיים. אם Claude מספיק, אין צורך, והספקים האחרים נשארים לתיקים בדויים.
**לפני השליחה:** כמו ב-`ZDR_REQUEST.md`: חשבון ארגוני על שם עינת או הקליניקה, בתשלום, עם תקרת הוצאה. לא ליצור מפתח לשימוש אמיתי לפני תשובה. שולחים מהמייל של עינת, דרך טופס יצירת הקשר עם המכירות של כל חברה, ומבקשים תשובה **בכתב**.

## מה לא לכתוב
אותם כללים כמו ב-`ZDR_REQUEST.md`: שום פרט על מטופל, לא מפתח API, לא קבצים מהתוכנה.

## אחרי שמגיעה תשובה
- לשמור את המייל מחוץ לריפו.
- לרשום ב-`DECISIONS.md`: "ZDR אושר בכתב מ-[ספק], תאריך, אילו דגמים".
- לעדכן את השורה של הספק בטבלה בסעיף 4 של `CONSENT_DRAFT.md` ובסעיף 5 שלו, ואת `DATABASE_DEFINITION.md` §6.
- אם ZDR לא מאושר: הספק לא משמש לתיק אמיתי, ונמחק מהטבלה בטופס שמוצג להורים.

---

## המייל (באנגלית, אותו נוסח לכל ספק)

נשלח בנפרד לכל חברה. מחליפים את מה שבסוגריים המרובעים:

| | [Provider] | [API] | [Models] | [Setting] |
|---|---|---|---|---|
| OpenAI | OpenAI | the OpenAI API (Responses / Chat Completions) | `gpt-5.1`, `gpt-5-mini` | requests are sent with `store: false` |
| Google | Google | the Gemini API, paid tier | `gemini-2.5-pro`, `gemini-2.5-flash` | the project is on the paid tier |
| Mistral | Mistral AI | La Plateforme API | the numbered versions behind `mistral-large` and `mistral-medium` | none |

**Subject:** Zero Data Retention request – small clinical practice (Israel), [API] only

Hello,

I am a licensed developmental psychologist in private practice in Israel. I would like to enable Zero Data Retention for my organization on [API] before any clinical use.

- Organization / project: [name and ID as shown in the console]
- Contact: [full name], [clinic name]

**The use case**

A desktop application that runs only on my own computer and helps me word sections of children's developmental assessment reports. I make every clinical finding and judgement, and I review and approve every paragraph. Scores are interpreted by fixed tables in the application, not by the model.

**What is sent**

- De-identified text only. Before any request, the application removes names (replaced by roles such as "the child"), ID numbers, phone numbers, email addresses, addresses, places, schools and kindergartens, and all dates (converted to relative time).
- What remains is the child's age in years and months, test scores, and clinical observations, without identifiers.
- No images, no files, no user identifiers, and no case information inside JSON schemas. Text generation only, no tools. [Setting].
- Models: [Models]. Volume is low: a few dozen reports per year.

**Please confirm in writing**

1. That zero data retention is enabled for the organization above, from which date, and that it covers every API key in it, including keys created later.
2. Which of the models listed above are covered.
3. Which exceptions still apply (for example abuse monitoring or legal requirements), and the retention period for each.
4. In which countries request content is processed, including any content retained under those exceptions.
5. That request content is not used to train or improve your models.
6. Which Data Processing Addendum applies, and which transfer mechanism it relies on. I am subject to Israel's Privacy Protection (Transfer of Data to Databases Abroad) Regulations, 2001, so I need to know what commitments you make as the recipient.
7. Which sub-processors could have access to request content.
8. How you will notify me if these terms change.

Thank you,

[full name]
Licensed psychologist, [clinic name]
