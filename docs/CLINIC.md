# ניהול קליניקה (בטא) – מפרט

> החלטות: D-170 עד D-179 ב-`DECISIONS.md`. המסך מסומן "בטא" בכל מקום שבו הוא מופיע.

## מה זה ולמי
עינת מנהלת קליניקה של אבחונים התפתחותיים. היום היומן, סיכומי הפגישות, התשלומים והמעקבים נמצאים מחוץ לתוכנה. המודול מביא אותם פנימה, לתוך הכספת המוצפנת, ומחבר אותם לתיק האבחון ולדוח.

נקודת ההשראה היא "קליניקה חכמה" (SmartClinic), שעינת ביקשה ללמוד ממנה. מה שיש שם, ומה עשינו עם כל דבר:

| אצלם | אצלנו |
|---|---|
| יומן פגישות, פגישות חוזרות, שינוי גורף של סדרה | יש: יום, שבוע וחודש; סדרה חוזרת; שינוי של "רק הפגישה הזו" או "זו וכל הבאות" |
| סיכום פגישה צמוד לפגישה | יש, עם הפרדה בין **רשומה** לבין **תרשומת אישית** (חוק זכויות החולה, סעיף 17; קוד האתיקה, 3.14). רק הרשומה יכולה לעבור לתיק האבחון |
| תשלומים, חוב מטופל, סיכומי הכנסות | יש: חיוב לפי פגישה, חיובים נוספים (חבילת אבחון), תקבולים, יתרה, סיכום חודשי ושנתי |
| "יומן רופא" עם היסטוריית שינויים | יש: יומן הכנסות יומי, וכל שינוי בפגישה, בחיוב או בתקבול נשמר ואפשר לראות אותו. תקבול לא נמחק, רק מבוטל עם סיבה. **התוכנה אינה רשומה ברשות המסים**, והמסך אומר זאת |
| חשבוניות דרך "חשבונית ירוקה" | אין: זה שירות בתשלום וברשת. רושמים את מספר הקבלה שהופקה בתוכנת החשבוניות |
| סנכרון עם יומן גוגל | אין: שמות המטופלים והמועדים היו יוצאים מהמחשב (כלל הברזל של היציאה לרשת) |
| הודעת תשלום ב-WhatsApp עם נוסח שאפשר לערוך | יש: נוסח תזכורת ונוסח תשלום שעינת עורכת; התוכנה ממלאת ומעתיקה ללוח, ועינת מדביקה בעצמה. שום דבר לא נשלח מהתוכנה |
| אפליקציה לנייד, ענן | אין: הכל במחשב, בכספת |

ומה שהוספנו מעבר: רשימת המתנה; מעקבים ומשימות; "מה דורש אותי" שהתוכנה מזהה לבד (פגישה שלא סומנה, פגישה בלי סיכום, חוב ישן, דוח שמתעכב, משפחה שממתינה הרבה זמן, מטופל פעיל בלי פגישה הבאה, שיחת משוב קרובה כשהדוח לא גמור); מסלול האבחון של כל ילד, מפגישת ההיכרות ועד מסירת הדוח; חיבור לתיק האבחון (פתיחת תיק מכרטיס המטופל, התקדמות הדוח בכרטיס, העברת סיכום פגישה לחומרי התיק).

## כללים שלא זזים
1. **הכל בתוך הכספת.** כרטיסי המטופלים (שמות, טלפונים) ב-`identity.db`; הפגישות, הסיכומים, הכספים והמשימות ב-`main.db`. כל רשומה חתומה ב-AES-256-GCM עם AAD, כמו שאר הכספת, ונכנסת לגיבוי כמו שהיא.
2. **שום דבר מהקליניקה לא נשלח ל-AI ולא יוצא לרשת.** אין במודול קריאה ל-`dv-egress`, ל-`dv-ai` או ל-`dv-privacy::gate`. הדרך היחידה שבה טקסט מהקליניקה מגיע לתיק היא "להעביר לחומרי התיק", שמעתיקה את הרשומה (לא את התרשומת האישית) כחומר בתיק; משם חלים הסינון והשער הרגילים.
3. **טקסט בלבד.** כל מה שמוצג הוא טקסט, אף פעם לא HTML.
4. **יומן האירועים** (audit) מקבל מטא-דאטה בלבד: שנוסף מטופל, שנמחק מטופל, שסיכום הועבר לתיק. בלי שמות ובלי תוכן.
5. **אין נתוני אמת בריפו.** כל הדוגמאות, הבדיקות וההדמיה בדויות.

## The technical contract (English, for whoever writes the code)

Types and the request/reply enums: `crates/dv-domain/src/clinic/mod.rs` (the source of truth; the
TypeScript in `apps/desktop/src/ipc/generated/` is generated from it by `cargo test -p dv-domain`).
Read that file first. Conventions: ISO dates, minutes from midnight, money in agorot (`i32`), an
empty `id` means "new". "Today" always comes from the window (`Today`), never from the core's clock.

### Layers
| Layer | File | Owns |
|---|---|---|
| rules | `crates/dv-domain/src/clinic/logic.rs` (+ `tests.rs`) | validation, recurrence dates, overlaps, charges and balances, alerts, the assessment process, the diary, income, message filling, Hebrew labels, history lines |
| storage | `crates/dv-vault/src/store/clinic.rs` | sealed opaque JSON rows, indexes, history rows, erasing a patient |
| service | `crates/dv-core/src/clinic.rs` | `Core::clinic(ClinicRequest) -> Result<ClinicReply, CoreError>`: loads, applies the rules, saves; the link to cases |
| shell | `apps/desktop/src-tauri/src/commands/clinic.rs` | one command, `clinic` |
| window | `apps/desktop/src/screens/clinic/`, `apps/desktop/src/ipc/clinic.ts`, `apps/desktop/src/web/fakeClinic.ts` | the screens, the typed client, the browser-preview stand-in |

### Storage
`main.db` migration v13:
```sql
CREATE TABLE clinic_records (
   id TEXT PRIMARY KEY,
   kind TEXT NOT NULL,          -- visit | note | payment | charge | task | settings
   patient_id TEXT,
   day INTEGER NOT NULL,        -- days since 1970-01-01 of the record's date; 0 when it has none
   updated_at INTEGER NOT NULL,
   data_enc BLOB NOT NULL
);
CREATE INDEX clinic_records_kind_day ON clinic_records(kind, day);
CREATE INDEX clinic_records_patient ON clinic_records(patient_id, kind);
CREATE TABLE clinic_history (
   id TEXT PRIMARY KEY,
   record_id TEXT NOT NULL,
   patient_id TEXT,
   saved_at INTEGER NOT NULL,
   action TEXT NOT NULL,        -- created | changed | voided | deleted
   data_enc BLOB NOT NULL
);
CREATE INDEX clinic_history_record ON clinic_history(record_id, saved_at);
```
`identity.db` migration v3:
```sql
CREATE TABLE clinic_patients (
   id TEXT PRIMARY KEY,
   created_at INTEGER NOT NULL,
   updated_at INTEGER NOT NULL,
   data_enc BLOB NOT NULL
);
```
Everything is sealed with the settings key (as `style_sources` is: it does not belong to a case),
AAD `aad(table, "data", id, "")`. Migrations only add; each one's hash is pinned in `db.rs`.
History is written for `visit`, `note`, `payment` and `charge` on every save, in the same
transaction as the record. A visit that is deleted leaves a `deleted` history row. Erasing a
patient (D-174) is allowed only for a card where nothing happened (no visit held or charged, no
note, no charge, no payment, voided ones included), or after the retention period of a case
(`KEEP_YEARS_AFTER_LAST_CHANGE` since the last activity in the card and `KEEP_UNTIL_AGE`); the
rule is a pure function in `logic.rs` and is enforced in the core. It then removes the card, every
record and every history row of theirs, checkpoints the write-ahead logs of both databases, and
writes one audit event. Every stored record reads with `#[serde(default)]` (D-175).

### Rules (the single place: `logic.rs`)
Limits: a name 1–120 chars; short fields (place, title, receipt, payer, phone, email, framework,
referred_by, task title, charge `what`, reasons) ≤ 200; `reason`, `notes`, a task note, a visit
note ≤ 4,000; `record` and `personal` ≤ 20,000; message texts ≤ 1,000; ≤ 6 contacts; ≤ 12 places;
≤ 20 note templates of ≤ 5,000 chars; `start + minutes ≤ 1440`, `minutes` 5–720; a fee
0–10,000,000 agorot; a payment or charge amount 1–10,000,000; `Repeat.every_weeks` 1–8, `count`
2–52; dates are real ISO dates (`dates::parse_iso`). A refusal is a Hebrew sentence she can act on.

Visits: a `Block` has no patient and no fee; every other kind needs an existing patient. A new
visit is `Planned`. A repeat creates `count` visits `7 × every_weeks` days apart with one
`series_id`. `Scope::Following` on a series applies kind, title, start, minutes, place, fee and
note to this visit and every later `Planned` one of the series, and moves each by the same number
of days this one moved; status changes apply to this one only. Only a `Planned` visit can be
deleted. A visit's charge is its fee when `Held`, or when `Cancelled`/`NoShow` with
`charge_missed`; otherwise 0. Two visits overlap when both are not cancelled, on the same date,
and their times intersect. Saving a visit for a `Waiting` patient makes them `Active`.

Balance: charged = visit charges + charges not voided; paid = payments not voided; due = charged − paid.

Alerts (computed, never stored; sorted by `since`, oldest first):
- `Unmarked`: a `Planned` visit (not a block) whose end has passed.
- `NoNote`: a `Held` visit of kind Intake/Assessment/Observation/Feedback/Therapy/Guidance from the last 60 days with no note.
- `Debt`: due > 0 and, paying the oldest charges first, the oldest charge still uncovered is at least `debt_days` old.
- `Waiting`: on the waiting list for at least `waiting_weeks` weeks.
- `ReportDue`: active; at least one Assessment held; none planned ahead; no Feedback held; the last held Assessment + `report_days` is before today; the report is not finished (no case, or approved < total).
- `FeedbackSoon`: a Feedback planned within the next 7 days while the report is not finished.
- `NoNextVisit`: active; has a past visit; nothing planned from today on; no open task; last visit more than 14 days ago; not (Feedback held and nothing owed).
- `TaskOverdue`: an open task whose date passed.

Process (the card): `intake`, `sessions`, `observation` (only when one exists), `report`,
`feedback`, `payment`; each `Done` / `Now` / `Todo` with one line of detail.

Diary month: every patient visit of the month, every charge and every payment (voided ones
shown as voided and not counted), by date, then visits by time, then charges, then payments.
Income year: twelve months of held / cancelled / no-show counts, charged, received; new patients
(by `created_on`); held visits by kind.

Messages: placeholders `{הורה} {ילד} {יום} {תאריך} {שעה} {מקום} {סכום} {שמי}`; a missing parent
name becomes "רב" ("שלום רב"); the date is `15.11.2026`, the time `09:30`, the day `ראשון`.

### The link to cases
- `CreateCase`: a new case through `Core::create_case` (so every rule of a case holds): code
  `תיק-YYYY-MMDD` of `today` (with `-2`, `-3`… when taken), age from `birth_date` on `today`,
  gender, no consent, identities: the child (`Role::Child`) and the mother and father contacts.
  The patient keeps `case_id`.
- `LinkCase` / `CaseChoices`: a case is linked to at most one patient; a case in the recycle bin is not offered.
- `NoteToCase`: needs a linked case that is not in the bin and a non-empty `record`. Adds (or, when
  `case_input_id` still exists, replaces) a material through the core's own `add_input` /
  `update_input`: kind `Intake` for an intake visit, `Observation` for an observation, otherwise
  `SessionNote`; title `סיכום <kind> · <dd.mm.yyyy>`; content = `record` only.
- `SavePatient` never changes `case_id`. Erasing a patient never touches the case.
- A case that was purged leaves a `case_id` that points nowhere: every read treats it as "no case"
  (no error), and the card offers to link or open another.
- `CreateCase` registers every named contact as an identity of the case (not only the mother and
  the father), so a session record sent to the case later has their names masked by the filter.

### The window
`Route` gets `{ name: "clinic"; tab?: ClinicTab; patientId?: string }`; the top bar gets
"קליניקה" with a "בטא" badge; the module is a lazy chunk. Tabs: היום · יומן · מטופלים · המתנה ·
מעקבים · כספים · הגדרות. Data through `@tanstack/react-query` (`["clinic", …]` keys, invalidated
after a write). Loading shows `Skeleton`; search is debounced; quick actions (marking a visit, a
task done) update the screen first and roll back on failure; long lists render in pages.
RTL, keyboard reachable, labelled, both colour themes (only tokens from `styles/tokens.css`).

## מה לא נכנס לבטא (ולמה)
- ייצוא יומן ההכנסות לקובץ (לרואה החשבון): קובץ עם שמות מחוץ לכספת. ממתין להחלטה של עינת.
- תזכורות אוטומטיות ב-SMS או ב-WhatsApp: שירות בתשלום ויציאה לרשת.
- סנכרון עם יומן גוגל, אפליקציה לנייד, חשבוניות: ראו הטבלה למעלה.
- קישור מתוך מסך התיק אל כרטיס המטופל: ייכנס אחרי ששני השרשורים האחרים שנוגעים במסכי התיק יתמזגו.
