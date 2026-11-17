# ניהול קליניקה (בטא) – מפרט

> החלטות: D-170 עד D-179, D-194 (ייצוא לרואה החשבון) ו-D-196 עד D-200 (תכנון ותמונת מצב), ב-`DECISIONS.md`. המסך מסומן "בטא" בכל מקום שבו הוא מופיע.

## מה זה ולמי
עינת מנהלת קליניקה של אבחונים התפתחותיים. היום היומן, סיכומי הפגישות, התשלומים והמעקבים נמצאים מחוץ לתוכנה. המודול מביא אותם פנימה, לתוך הכספת המוצפנת, ומחבר אותם לתיק האבחון ולדוח.

נקודת ההשראה היא "קליניקה חכמה" (SmartClinic), שעינת ביקשה ללמוד ממנה. מה שיש שם, ומה עשינו עם כל דבר:

| אצלם | אצלנו |
|---|---|
| יומן פגישות, פגישות חוזרות, שינוי גורף של סדרה | יש: יום, שבוע וחודש; סדרה חוזרת; שינוי של "רק הפגישה הזו" או "זו וכל הבאות" |
| סיכום פגישה צמוד לפגישה | יש, עם הפרדה בין **רשומה** לבין **תרשומת אישית** (חוק זכויות החולה, סעיף 17; קוד האתיקה, 3.14). רק הרשומה יכולה לעבור לתיק האבחון |
| תשלומים, חוב מטופל, סיכומי הכנסות | יש: חיוב לפי פגישה, חיובים נוספים (חבילת אבחון), תקבולים, יתרה, סיכום חודשי ושנתי |
| "יומן רופא" עם היסטוריית שינויים | יש: יומן הכנסות יומי, וכל שינוי בפגישה, בסיכום פגישה, בחיוב או בתקבול נשמר ואפשר לראות אותו. תקבול וחיוב לא נמחקים, רק מבוטלים עם סיבה. **התוכנה אינה רשומה ברשות המסים**, והמסך אומר זאת |
| חשבוניות דרך "חשבונית ירוקה" | אין: זה שירות בתשלום וברשת. רושמים את מספר הקבלה שהופקה בתוכנת החשבוניות |
| סנכרון עם יומן גוגל | אין: שמות המטופלים והמועדים היו יוצאים מהמחשב (כלל הברזל של היציאה לרשת) |
| הודעת תשלום ב-WhatsApp עם נוסח שאפשר לערוך | יש: נוסח תזכורת ונוסח תשלום שעינת עורכת; התוכנה ממלאת ומעתיקה ללוח, ועינת מדביקה בעצמה. שום דבר לא נשלח מהתוכנה |
| אפליקציה לנייד, ענן | אין: הכל במחשב, בכספת |

ומה שהוספנו מעבר: ייצוא של התקבולים לרואה החשבון, בלי שמות (D-194); רשימת המתנה; מעקבים ומשימות; "מה דורש אותי" שהתוכנה מזהה לבד (פגישה שלא סומנה, פגישה בלי סיכום, חוב ישן, דוח שמתעכב, משפחה שממתינה הרבה זמן, מטופל פעיל בלי פגישה הבאה, שיחת משוב קרובה כשהדוח לא גמור); מסלול האבחון של כל ילד, מפגישת ההיכרות ועד מסירת הדוח; חיבור לתיק האבחון (פתיחת תיק מכרטיס המטופל, התקדמות הדוח בכרטיס, העברת סיכום פגישה לחומרי התיק).

## כללים שלא זזים
1. **הכל בתוך הכספת.** כרטיסי המטופלים (שמות, טלפונים) ב-`identity.db`; הפגישות, הסיכומים, הכספים והמשימות ב-`main.db`. כל רשומה חתומה ב-AES-256-GCM עם AAD, כמו שאר הכספת, ונכנסת לגיבוי כמו שהיא.
2. **שום דבר מהקליניקה לא נשלח ל-AI ולא יוצא לרשת.** אין במודול קריאה ל-`dv-egress`, ל-`dv-ai` או ל-`dv-privacy::gate`. הדרך היחידה שבה טקסט מהקליניקה מגיע לתיק היא "להעביר לחומרי התיק", שמעתיקה את הרשומה (לא את התרשומת האישית) כחומר בתיק; משם חלים הסינון והשער הרגילים.
3. **טקסט בלבד.** כל מה שמוצג הוא טקסט, אף פעם לא HTML.
4. **יומן האירועים** (audit) מקבל מטא-דאטה בלבד: שנוסף מטופל, שנמחק מטופל, שסיכום הועבר לתיק, שיומן ההכנסות יוצא לקובץ ולאיזו תקופה. בלי שמות ובלי תוכן.
5. **אין נתוני אמת בריפו.** כל הדוגמאות, הבדיקות וההדמיה בדויות.
6. **קובץ שיוצא מהכספת לא נושא שם.** הקובץ היחיד שהקליניקה כותבת החוצה הוא הייצוא לרואה החשבון, ואין בו שם, טלפון, מזהה של כרטיס או טקסט שעינת כתבה, מלבד הספרות של מספר הקבלה (D-194).

## מה נשמר, ומה אפשר למחוק
הקליניקה היא גם רשומה רפואית וגם רישום של הכנסות, ולכן רוב מה שנכתב בה נשאר. כך זה עובד (D-172, D-174, D-176 עד D-179):

- **תקבול וחיוב לא נמחקים אף פעם.** אם נרשמה טעות, מבטלים את השורה וכותבים למה. השורה נשארת ביומן, מסומנת כמבוטלת, ולא נספרת בסיכומים. הכספת עצמה מסרבת למחוק תקבול, חיוב או את רשומת ההגדרות, גם אם מסך כלשהו היה מבקש זאת בטעות.
- **פגישה** אפשר למחוק רק כל עוד היא "מתוכננת" ואף פעם לא הייתה משהו אחר. ביומן נשארת ממנה שורה שאומרת שנמחקה. פגישה שסומנה פעם (התקיימה, בוטלה או לא הגיעו) נשארת תמיד, גם אם הוחזרה אחר כך ל"מתוכננת": ההיסטוריה שלה זוכרת.
- **סימון שנעשה בטעות אפשר לתקן.** פגישה שסומנה חוזרת ל"מתוכננת" רק כשאין לה סיכום, וכשמה שהמשפחה כבר שילמה לא גדול ממה שנשאר לחיוב בלי הפגישה הזו. אחרת התוכנה מסרבת ואומרת מה לעשות: לסמן כמבוטלת ולכתוב מה קרה, או לבטל קודם את התקבול. כל שינוי אחר בסימון (התקיימה, בוטלה, לא הגיעו) אפשרי תמיד, ונשמר בהיסטוריה.
- **רישום לא עובר מכרטיס לכרטיס.** תקבול, חיוב, סיכום ופגישה שייכים לכרטיס שבו נרשמו. אם נרשמו בכרטיס הלא נכון: תקבול וחיוב מבטלים ורושמים מחדש בכרטיס הנכון, ופגישה מוחקים (או מסמנים כמבוטלת) וקובעים מחדש. כך מחיקה או שינוי של כרטיס אחד לא נוגעים בכסף או בהיסטוריה של כרטיס אחר.
- **"זו וכל הבאות" לא מזיז פגישה שיש לה סיכום** או שכבר לא "מתוכננת". התוכנה אומרת כמה פגישות נשארו כמו שהן.
- **סיכום פגישה הוא חלק מהרשומה.** סיכום שכתוב בו משהו לא נמחק: עורכים אותו, וכל גרסה שנשמרה נשארת ב"היסטוריית שינויים" שלו. רק סיכום שנשאר ריק לגמרי אפשר להסיר.
- **משימה** אפשר למחוק תמיד. היא תזכורת, לא רשומה.
- **כרטיס מטופל שלא קרה בו כלום** (אין פגישה שסומנה אי פעם, אין סיכום, אין חיוב ואין תקבול, גם לא מבוטלים) נמחק מיד, אחרי הקלדת השם. זה המקרה של טעות הקלדה או של כרטיס כפול. כרטיס כזה לא משאיר אחריו כלום, וגם לא נספר יותר כ"מטופל חדש".
- **כרטיס שקרה בו משהו נסגר, לא נמחק.** מחיקה אפשרית רק אחרי תקופת השמירה, כמו בתיקי האבחון: 7 שנים מהפעילות האחרונה בכרטיס, ולא לפני שהילד בן 25 (כשתאריך הלידה ידוע), המאוחר מבין השניים. "הפעילות האחרונה" היא התאריך המאוחר ביותר שכתוב בכרטיס (פתיחת הכרטיס, פגישה, סיכום, חיוב, תקבול או משימה), וגם היום האחרון שבו נשמר שינוי באחת הרשומות שלו: תיקון שנעשה שנים אחר כך נחשב פעילות. עד אז התוכנה מסרבת, מסבירה, ואומרת מאיזה יום המחיקה תהיה אפשרית.
- אחרי תקופת השמירה המחיקה מוחקת את הכרטיס, את כל הרשומות שלו ואת כל ההיסטוריה שלהן. תיק האבחון לא נמחק יחד עם הכרטיס.
- **הסיכומים של שנים שעברו לא משתנים אחרי מחיקה.** מה שהכרטיס נספר בו (בכל חודש: כמה פגישות התקיימו ומאיזה סוג, כמה בוטלו, לכמה לא הגיעו, כמה חויב, כמה התקבל ובאיזה אמצעי, כמה תקבולים בלי מספר קבלה, ושנפתח כרטיס חדש) נשאר כמספרים בלבד, בלי שם, בלי מזהה ובלי טקסט, חתום בכספת. הסיכום השנתי ממשיך להראות את אותם מספרים, וביומן ההכנסות של החודש מופיעה שורה אחת: "רשומות של כרטיסים שנמחקו", עם הסכומים שלה. מה שלא נשאר: מי, מתי בדיוק, ועל מה.
- **שורה שהתוכנה לא מצליחה לקרוא** (קובץ שנפגע, או שורה שנכתבה בגרסה חדשה יותר) לא מפילה את המסכים. היא נשארת בכספת כמו שהיא, לא מוצגת ולא נספרת, ובעמוד הראשי של הקליניקה כתוב כמה שורות כאלה יש. ערך שהגרסה לא מכירה (למשל אמצעי תשלום חדש) מוצג כ"לא מוכר", והתוכנה לא שומרת רישום כזה עד שבוחרים ערך מהרשימה, כדי שלא יהפוך בשקט למשהו אחר.

## ייצוא לרואה החשבון (D-194)
בלשונית "כספים", ליד היומן וליד הסיכום השנתי, יש כפתור שקט: "ייצוא לרואה החשבון". חלון קטן שואל אם לייצא חודש או שנה שלמה, ומאיזה חודש או שנה (ברירת המחדל: מה שעל המסך). בחודש שיש בו תקבולים בלי מספר קבלה, החלון אומר כמה, כדי שאפשר יהיה להשלים אותם קודם. אחרי השמירה מופיעה הודעה שאומרת איפה הקובץ נשמר.

**מה יש בקובץ.** לכל תקבול בתקופה: תאריך, מספר קבלה (רק הספרות והמפרידים שלו), סכום בשקלים עם שתי ספרות אחרי הנקודה, אמצעי התשלום בעברית, ומצב: "תקין" או "בוטל". אחר כך, לכל חודש: מה התקבל לפי אמצעי תשלום, עם עמודה נפרדת ל"רשומות של כרטיסים שנמחקו" (סכומים בלבד, כמו השורה שלהן ביומן) ושורת "סך החודש"; בשנה גם "סך השנה". בסוף: כמה תקבולים בלי מספר קבלה (ברשימה, ובכרטיסים שנמחקו), וכמה בוטלו. בראש הקובץ כתוב שהתוכנה אינה רשומה ברשות המסים.

**מה אין בקובץ.** שם של ילד או של הורה, טלפון, מייל, מזהה של כרטיס או של רישום, מי שילם, הערה, סיבת ביטול (היא יכולה לכלול שם), ופגישות או חיובים. תקבול שבוטל מופיע עם הסכום שלו ולא נספר בסיכומים. מספר קבלה שכתובות בו אותיות נשאר רק עם הספרות; בלי ספרות בכלל הוא נספר כ"בלי מספר קבלה".

**הפורמט.** CSV לפי RFC 4180, ב-UTF-8 עם BOM ושורות CRLF, כך שהעברית נפתחת נכון ב-Excel. כל שורה באותו רוחב, ושדה עם פסיק, מירכאות או ירידת שורה מוקף במירכאות. תא שמתחיל ב-`=`, `+`, `-`, `@`, טאב או CR מקבל גרש בהתחלה, כדי שגיליון לא יריץ אותו כנוסחה. שם הקובץ הוא התקופה בלבד: `הכנסות 2026-11.csv`, `הכנסות 2026.csv`.

**איפה נשמר.** כמו הדוח: בתיקיית ההורדות, ואם היא מסונכרנת לענן או לא קיימת, במסמכים או בתיקיית הבית; כשאף אחת מהן לא מתאימה, עינת בוחרת תיקייה, ותיקייה מסונכרנת לענן נדחית. התוכנה לא שולחת את הקובץ: עינת שולחת אותו בעצמה.

**שלם, או לא נוצר.** אם תקבול של התקופה או הספירה של הכרטיסים שנמחקו לא נקראים מהכספת, הקובץ לא נוצר, והמסך אומר למה. ביומן האירועים נרשם שהקובץ נוצר ולאיזו תקופה (`clinic_income_exported`, `{"period":"2026-11"}`), ולא שום דבר אחר.

## The technical contract (English, for whoever writes the code)

Types and the request/reply enums: `crates/dv-domain/src/clinic/mod.rs` (the source of truth; the
TypeScript in `apps/desktop/src/ipc/generated/` is generated from it by `cargo test -p dv-domain`).
Read that file first. Conventions: ISO dates, minutes from midnight, money in agorot (`i32`), an
empty `id` means "new". "Today" always comes from the window (`Today`), never from the core's clock.

### Layers
| Layer | File | Owns |
|---|---|---|
| rules | `crates/dv-domain/src/clinic/logic.rs` (+ `tests.rs`) | validation, recurrence dates, overlaps, charges and balances, alerts, the assessment process, the diary, income, message filling, Hebrew labels, history lines |
| the file for the accountant | `crates/dv-domain/src/clinic/income_export.rs` (its tests inside) | the period, the CSV (D-194) |
| storage | `crates/dv-vault/src/store/clinic.rs` | sealed opaque JSON rows, indexes, history rows, erasing a patient |
| service | `crates/dv-core/src/clinic.rs` | `Core::clinic(ClinicRequest) -> Result<ClinicReply, CoreError>`: loads, applies the rules, saves; the link to cases |
| shell | `apps/desktop/src-tauri/src/commands/clinic.rs`; `commands/report.rs` | one command, `clinic`; and `export_income`, which writes the file for the accountant beside the reports (D-194) |
| window | `apps/desktop/src/screens/clinic/`, `apps/desktop/src/ipc/clinic.ts`, `apps/desktop/src/web/fakeClinic.ts` | the screens, the typed client, the browser-preview stand-in |

### Storage
`main.db` migration v13:
```sql
CREATE TABLE clinic_records (
   id TEXT PRIMARY KEY,
   kind TEXT NOT NULL,          -- visit | note | payment | charge | task | settings | tally
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
   data_enc BLOB NOT NULL,
   erase_counted INTEGER NOT NULL DEFAULT 0  -- 1: an erase already counted it (D-179, retry)
);
CREATE TABLE clinic_erased (     -- one row: erased cards by the month they were opened in (D-179)
   id INTEGER PRIMARY KEY CHECK (id = 1),
   updated_at INTEGER NOT NULL,
   data_enc BLOB NOT NULL
);
```
Everything is sealed with the settings key (as `style_sources` is: it does not belong to a case).
The clear columns that say what a row is are bound into its seal (D-177): a record's AAD is
`aad("clinic_records", "data", id, "<kind>|<patient_id or empty>")`, a history row's is
`aad("clinic_history", "data", id, "<record_id>|<patient_id or empty>|<action>")`; a card and the
`clinic_erased` row are bound to their id. A row whose `kind`, `patient_id`, `record_id` or
`action` column was changed in the file does not open. `day`, `updated_at` and `saved_at` are not
bound (they only index and order; the date that counts is inside the sealed JSON).
Migrations only add; each one's hash is pinned in `db.rs`.

Reading (D-178): the lists (`clinic_kind`, `clinic_range`, `clinic_patients`) leave out a row that
does not open, and `clinic_unreadable()` counts such rows (records and cards). The core leaves out
a row whose JSON it cannot read and counts it too. `Overview` puts one `Unreadable` alert first
with the total, and writes one `clinic_unreadable` audit event (`{"rows": n}`, a number only) when
the count changes in this run. Nothing rewrites or removes such a row. Reading one record, one
patient's records (`clinic_of_patient`) or a history is strict: an erase, the payment message and
the "ever marked" check fail instead of deciding with a row missing. Unreadable settings show as
the defaults (and count as one row).

Every stored enum (`PatientStatus`, `ContactRole`, `VisitKind`, `VisitStatus`, `PayMethod`,
`TaskKind`) has an `Unknown` variant (`#[serde(other)]`, `"unknown"` in TypeScript) that any value
it does not know reads as. It has a label ("… לא מוכר"), is never among the choices
(`VISIT_KINDS`, `PAY_METHODS`), charges nothing and counts as no status. Every `validate_*` refuses
it, and so does every write in the core (a void included): a record that carries it is not written
until she picks a value, so the stored value is never turned into `"unknown"` or anything else.
History is written for `visit`, `note`, `payment` and `charge` on every save, in the same
transaction as the record (a session note is the clinical record: its changes are the ones most
worth seeing, D-172). Tasks and the settings record have no history.

Deleting one record at a time: the vault itself (`clinic_delete_many`) removes only a `visit`, a
`note` or a `task`, and refuses a `payment`, a `charge` and the `settings` record whoever asks:
a payment and a charge are voided, never removed, and the settings record is only replaced.
Those rows go only together with their patient. The `tally` record is written only by an erase
and is never removed or put by hand. On top of that the core allows a visit to be deleted only
while it is `Planned` and was never anything else by its saved history (D-176), and a note only
while both its texts are empty (a note that says something is edited, with its history). A visit or a note that is deleted leaves a
`deleted` history row holding its last state.

Erasing a patient (D-174) is allowed only for a card where nothing happened (no visit that ever
left `Planned` by its history, no note, no charge, no payment, voided ones included;
`card_happened`), or after the retention period
of a case: `KEEP_YEARS_AFTER_LAST_CHANGE` since the last activity in the card, and not before
the patient is `KEEP_UNTIL_AGE` when the birth date is known, whichever is later. The rule is a
pure function in `logic.rs`, `can_erase(records, marked, today, last_saved, keep_years,
keep_until_age)` (`marked`: ids of visits that ever left `Planned`),
and is enforced in the core. The last activity is the latest date anything in the card carries
(the day it was opened, every visit, note, charge and payment, a task's due and done dates) and
`last_saved`: the day the card or one of its records was last written (the newest `updated_at`
of the card and of its rows, by the core's clock), so a card or a record corrected years later
counts as activity then. `today`
here is the core's own clock, not the window's. The refusal says the card can be closed instead,
and from which day it can be erased. An erase then removes the card, every record of theirs and
the history of those records (payments and charges included), checkpoints the write-ahead logs
of both databases, and writes one audit event (`{"records": n}`). Only history that belongs to
the records being erased goes: rows of this patient's records, and rows this patient's removed
records left; a row of a record that belongs to another card is never touched (D-177).

What an erased card counted for stays (D-179). Before the erase the core folds the card into two
anonymous tallies (`fold_erased`, `fold_erased_card`): `ErasedTally` (by month: held visits by
kind, cancelled, no-show, charged, received by method, payments with no receipt), kept as the
`tally` record of `main.db`, and `ErasedCards` (by month: cards opened), kept in `clinic_erased`
of `identity.db`. Neither type has a field for an id, a name, a day or a text. Each is written in
the same transaction as the rows it stands for: the tally with the removal of the records, the
card count with the removal of the card. (The two databases cannot share one transaction. If the
second fails, the card is left with no records and still counts for itself; asking again finishes
the erase and counts nothing twice. A card is marked `erase_counted` before its records go, when
the erase counts it; asking again for a card so marked counts it then, though nothing is left in
it, so it is counted exactly once.) `Income` and `Diary` add them back (`income_with_erased`,
`diary_with_erased`), so a year reads the same before and after; `DiaryMonth.erased` is the one
anonymous line of a month (its sums are in the month's totals and in no day), and
`Overview.month_received` includes it. Voided amounts are in no summary, so they are not kept. A
card where nothing happened leaves nothing: it was a mistake, and stops counting as a new patient.
An erase is refused when a row of the card, or a tally, cannot be read.

Every stored record reads with `#[serde(default)]` (D-175), `KindDefault` and `NoteTemplate`
included.

### Rules (the single place: `logic.rs`)
Limits: a name 1–120 chars; short fields (place, title, receipt, payer, phone, email, framework,
referred_by, task title, charge `what`, reasons) ≤ 200; `reason`, `notes`, a task note, a visit
note ≤ 4,000; `record` and `personal` ≤ 20,000; message texts ≤ 1,000; ≤ 6 contacts; ≤ 12 places;
≤ 20 note templates of ≤ 5,000 chars; `start + minutes ≤ 1440`, `minutes` 5–720; a fee
0–10,000,000 agorot; a payment or charge amount 1–10,000,000; `Repeat.every_weeks` 1–8, `count`
2–52; dates are real ISO dates of the years 2000–2200 (`dates::parse_iso`), except the birth
date, which has its own check (`parse_birth_date`, `validate_birth_date`): 1900 onward (an older
sibling, a parent consulting about an adult child) and, where the caller knows today, not after
it. `SavePatient` carries no `Today`, so the core checks with its own clock
(`validate_patient_on`): a birth date later than the day after the core's (UTC) today is refused
(one day of slack, because her local day can be ahead of UTC). `age_on` answers `None` when the age does not fit the `Age` type. A refusal is a Hebrew
sentence she can act on.

Visits: a `Block` has no patient and no fee; every other kind needs an existing patient. A new
visit is `Planned`. A repeat creates `count` visits `7 × every_weeks` days apart with one
`series_id`. `Scope::Following` on a series applies kind, title, start, minutes, place, fee and
note to this visit and every later `Planned` one of the series that has no session note, and
moves each by the same number of days this one moved; status changes apply to this one only. A
later visit that has a note, is not `Planned` or holds a value of a later version (D-178) is never
moved or altered, and `SavedVisits.left_alone` says how many there were (D-176). "This and the
following" (a change or a removal) reaches only visits of the same patient (or reserved times,
from a reserved time), and a reserved time that is given a patient leaves its series. Only a visit that is `Planned` and
never left `Planned` by its saved history can be deleted (`validate_delete(visit, marked)`;
"this and the following" skips the marked ones). A status goes back to `Planned` only when the
visit has no note and the patient's payments (not voided) are not more than the charges left
without it (`validate_unmark`); every other status change is allowed. A visit, a payment, a
charge and a note never change their patient (a block may be given one), D-177. A visit's charge is its fee when `Held`, or when `Cancelled`/`NoShow` with
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
- `Unreadable`: rows that could not be read (D-178). Only in `Overview`, always first, no patient.

Process (the card): `intake`, `sessions`, `observation` (only when one exists), `report`,
`feedback`, `payment`; each `Done` / `Now` / `Todo` with one line of detail.

Diary month: every patient visit of the month, every charge and every payment (voided ones
shown as voided and not counted), by date, then visits by time, then charges, then payments.
Income year: twelve months of held / cancelled / no-show counts, charged, received; new patients
(by `created_on`); held visits by kind. Both include what erased cards counted for (D-179).

Planning (D-196..D-199, `clinic/holidays.rs`, `clinic/slots.rs`): `holidays_between(from, to)` and
`closed_day(date)` compute Israel's holidays from the Hebrew calendar (one day per festival;
`closed` for the festival days and Yom Ha'atzmaut). `free_slots(visits, settings, today, from,
minutes, count, skip_visit)`: up to `SLOT_SEARCH_DAYS` (120) ahead from the later of `from` and
today (today, not before her minute), on `work_days` not closed (when `settings.holidays`),
within `day_start..day_end`, starts on the quarter hour, clear of every visit that is not
cancelled by `buffer_minutes` (0–120) on each side, at most `SLOTS_PER_DAY` (3) non-overlapping
a day, `count` 1–12. `propose_pathway`: the first `settings.pathway` step at the first free time
from `from` (its gap is unused; `normalize_settings` keeps it 0), each later step (≤ 12, not a
block, gap 0–180) on or after the previous step's date + its gap (at least the next day), the
kind's minutes, the patient's fee (or the kind's), the first place; never saved by itself. The
core reads the visits for Σ(max(gap, 1) + 120) days, as far as the last step can reach.
`SavePathway` takes only new planned visits of that patient, not before today (≤ 12), and with
`package` (needs `package_fee` > 0) sets each fee to 0 and adds one charge (`package_name`,
dated the first visit) in the same transaction (`Vault::clinic_put_groups`, `created` history for
each); every check, the waiting card's activation included, runs before that transaction. `starter_tasks(settings, patient_id, open, today)`: one task per
`settings.starter_tasks` (≤ 20, title ≤ 200, due 0–365 days), skipping a title (case and spaces
ignored) already open for the patient; refused when none is left, or when the kit is empty. Alert `MaterialsMissing`: a
planned Feedback within `FEEDBACK_SOON_DAYS` while Questionnaire/Document tasks of the patient
are open; the text gives their number only.

Insights (D-200, `clinic/insights.rs`): `insights(records, settings, from_month, to_month,
today, tally, cards)` over whole months (≤ 24). Totals (held, cancelled, no-show, by kind,
charged, received, new patients, by method, finished assessments) come from `income_year` +
`income_with_erased`, so they equal the yearly summary; weekday, occupancy (working days as
`free_slots` counts them, holidays closed only when `settings.holidays`), cycle times, referrals
and debt ageing are of existing cards only. No field carries a name, an id or her text.

Messages: placeholders `{הורה} {ילד} {יום} {תאריך} {שעה} {מקום} {סכום} {שמי}`; a missing parent
name becomes "רב" ("שלום רב"); the date is `15.11.2026`, the time `09:30`, the day `ראשון`.

### The income export (D-194)
The one file the clinic writes out of the vault. `export_income(period)` (shell, beside
`export_security_pdf`): `period` is `YYYY-MM` or `YYYY` of 2000–2200 (`parse_income_period`,
anything else refused); the shell takes the folder as the report does (`export_dir`: Downloads,
Documents, home, never one synced to the cloud, else she picks one) and writes under a free name
(`save_export`). `Core::clinic_income_export(period)` reads only the payments of the period
(`Vault::clinic_range_strict`: every row of the stretch or an error) and the `tally` record
(strictly); a row that does not open or does not read refuses the export (a file for the
accountant is never made with a row missing). It never reads a card. It writes one audit event,
`clinic_income_exported`, meta `{"period": "<period>"}`. The window never receives the file.

`income_csv(period, payments, tally)` (pure): UTF-8 with a BOM, CRLF after every record, every
record 5 fields wide, RFC 4180 quoting, and a leading `'` on a cell that starts with `=` `+` `-`
`@` TAB or CR. Sections: a title (`יומן הכנסות לרואה החשבון`, the period), three fixed lines
(no names in the file; not a registered program; a voided payment is listed and not counted), a
fourth when erased cards count in the period; `תקבולים`: date, receipt number (`receipt_number`:
ASCII digits and `-` `/` `.` space only, trimmed; no digit → empty), amount (`shekels`: `450.00`),
the method's label, `תקין` / `בוטל`, by date then receipt number; `סיכום לפי חודש ואמצעי תשלום`:
for each month of the period, a line per method with anything (listed, erased cards, total) and
`סך החודש` always; for a year, the year by method and `סך השנה`; then the counts of payments
with no receipt number (listed, and of erased cards) and of voided ones. Payee, note, void
reason, the card's id and every name are never read into a cell: the function is not given a card.

### The link to cases
- `CreateCase`: a new case through `Core::create_case` (so every rule of a case holds): code
  `תיק-YYYY-MMDD` of `today` (with `-2`, `-3`… when taken), age from `birth_date` on `today`,
  gender, no consent, identities: the child (`Role::Child`) and the mother and father contacts.
  The patient keeps `case_id`.
- `LinkCase` / `CaseChoices`: a case is linked to at most one patient; a case in the recycle bin is not offered.
- The names of a card are names its case's filter knows (D-177). `clinic_sync_identities` runs
  when a card is linked (`LinkCase`), on every `SavePatient` of a card that has a case, and in
  `NoteToCase` before the record goes; if it fails, the link, the save or the send fails. It only
  adds: the child's name joins the case's `Role::Child` as an alias when the case has a child (a
  new `Role::Child` otherwise), and each named contact the case does not know (by value or alias)
  is added with their role. A name that left the card, and everything she set in the case, stays.
- `NoteToCase`: needs a linked case that is not in the bin and a non-empty `record`. Adds a
  material, or, when `case_input_id` still exists in the case, replaces it through the core's own
  `update_input`. The first send writes the material and the note that points to it in one
  transaction (`Vault::clinic_note_into_case`, with an id the core made first), then learns the
  material's names as `add_input` does: a failure leaves neither, and asking again never adds the
  record twice (D-177). The material: kind `Intake` for an intake visit, `Observation` for an observation, otherwise
  `SessionNote`; title `סיכום <kind> · <dd.mm.yyyy>`, and for a note with no visit (a phone
  call, or a note whose visit was removed) `סיכום שיחה · <dd.mm.yyyy>`; content = `record` only.
- `SavePatient` never changes `case_id`. Erasing a patient never touches the case.
- A case that was purged leaves a `case_id` that points nowhere: every read treats it as "no case"
  (no error), and the card offers to link or open another.
- `CreateCase` registers every named contact as an identity of the case (not only the mother and
  the father), so a session record sent to the case later has their names masked by the filter.

### The window
`Route` gets `{ name: "clinic"; tab?: ClinicTab; patientId?: string }`; the module is a lazy chunk.
Since D-191 the clinic is one of the program's two parts: the top bar's switch has "ניהול הקליניקה"
with a "בטא" badge, and the clinic's tabs are the row under the bar (`components/TopBar.tsx`,
still a tab list with arrow keys): היום · יומן · מטופלים · המתנה · מעקבים · כספים · הגדרות הקליניקה.
Discreet mode is the program's one switch in the top bar (`src/discreet.ts`, D-192); the clinic
reads it through `useDiscreet`/`useShownName` in `parts.tsx` as before. From a linked case, the
case's "עוד פעולות" menu opens the card (D-195).
Data through `@tanstack/react-query` (`["clinic", …]` keys, invalidated
after a write). Loading shows `Skeleton`; search is debounced; quick actions (marking a visit, a
task done) update the screen first and roll back on failure; long lists render in pages.
RTL, keyboard reachable, labelled, both colour themes (only tokens from `styles/tokens.css`).
In כספים, "ייצוא לרואה החשבון" (beside the month and beside the year) opens a dialog that picks
a month or a whole year and calls `ipc.exportIncome(period)` (not through `clinic`: the file goes
from the core to the disk, and the window gets only its path, said in a toast). The browser
preview answers with a path and the core's file name, and writes and downloads nothing.

## תכנון ותמונת מצב (D-196 עד D-200)
- **חגי ישראל ביומן.** מחושבים במחשב מהלוח העברי, בלי רשת. ימי חג מסומנים ומוצללים, ולא מוצעות בהם שעות; ערבי חג, חול המועד, חנוכה, פורים וימי הזיכרון מופיעים בשמם. אפשר לכבות בהגדרות.
- **שעות פנויות.** בחלון פגישה, "להציע שעות פנויות" מראה את השעות הראשונות שמתאימות לאורך הפגישה: בימי העבודה, בשעות היומן, בלי ימי חג, ובמרחק שעינת קבעה מכל פגישה אחרת.
- **מסלול אבחון.** בכרטיס וברשימת ההמתנה, "לקבוע את כל המסלול" מציע את כל פגישות האבחון (לפי השלבים שבהגדרות) בשעות הפנויות הראשונות: השלב הראשון מהיום שבוחרים, וכל שלב אחריו לפחות כמה ימים אחרי השלב שלפניו. עינת רואה ומשנה, ורק אז שומרת, הכל יחד. עם מחיר חבילה בהגדרות אפשר לחייב את האבחון כחבילה: חיוב אחד, והפגישות בלי מחיר.
- **ערכת פתיחה.** בלחיצה אחת בכרטיס נפתחים השאלונים והמסמכים שמבקשים מכל משפחה, כמעקבים עם תאריך יעד. שבוע לפני שיחת המשוב, מה שעוד פתוח מופיע ב"מה דורש אותי" (כמה, לא מה).
- **תמונת מצב.** לשונית עם המגמות של טווח חודשים: פגישות, ביטולים ואי-הגעות, תפוסה, משך האבחון, הפניות, כסף וחובות לפי ותק. מספרים בלבד, בלי שמות.

## מה לא נכנס לבטא (ולמה)
- תזכורות אוטומטיות ב-SMS או ב-WhatsApp: שירות בתשלום ויציאה לרשת.
- סנכרון עם יומן גוגל, אפליקציה לנייד, חשבוניות: ראו הטבלה למעלה.
