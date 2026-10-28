//! Three fabricated cases, the documents a developmental psychologist really receives about a
//! child: a referral, the parents' questionnaire, the kindergarten or school report, earlier
//! evaluations, a score sheet, a doctor's summary and her own session notes.
//!
//! Every child, parent, teacher, clinic, school, town and number here is INVENTED. The ID
//! numbers, phones and e-mails are assembled at run time ([`secrets`]) so that the repository
//! scan never sees one whole.

use dv_domain::{Age, GrammaticalGender, Role};

/// One piece of a document, in reading order.
#[derive(Debug, Clone)]
pub(super) enum Block {
    /// A heading (Word "Heading 1").
    H(String),
    /// A paragraph.
    P(String),
    /// A bulleted or numbered item.
    Li(String),
    /// A table, row by row.
    Table(Vec<Vec<String>>),
    /// A footnote attached to the paragraph before it.
    Note(String),
    /// A hard page break.
    Break,
}

/// How the document reaches her.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Format {
    /// Word 365: styles, numbering, footnotes, header/footer, comments, tracked changes,
    /// a text box, runs split mid-word, `word/document.xml`.
    Docx,
    /// The same, with the main part at `word/document2.xml` (Word Online, D-135).
    Docx2,
    Odt,
    /// Word's RTF, Hebrew in code page 1255 (`\'e0`) and `\u` escapes.
    Rtf,
    /// Plain text, UTF-8 with a BOM and CRLF (Notepad).
    TextUtf8,
    /// Plain text in Windows-1255, CRLF (an old Notepad or a clinic system export).
    Text1255,
    /// A PDF Word or LibreOffice saved: text layer with Type0/Identity-H Hebrew fonts.
    Pdf,
    /// An office scanner: a PDF of JPEG page images, no text layer.
    ScanPdf,
    /// A phone photo of a page: a 12 MP JPEG, slightly turned, uneven background.
    Photo,
}

impl Format {
    pub(super) fn ext(self) -> &'static str {
        match self {
            Self::Docx | Self::Docx2 => "docx",
            Self::Odt => "odt",
            Self::Rtf => "rtf",
            Self::TextUtf8 | Self::Text1255 => "txt",
            Self::Pdf | Self::ScanPdf => "pdf",
            Self::Photo => "jpg",
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Docx => "docx (Word 365)",
            Self::Docx2 => "docx (document2.xml)",
            Self::Odt => "odt",
            Self::Rtf => "rtf (cp1255)",
            Self::TextUtf8 => "txt utf-8",
            Self::Text1255 => "txt windows-1255",
            Self::Pdf => "pdf (text layer)",
            Self::ScanPdf => "pdf (scan, OCR)",
            Self::Photo => "jpg photo (OCR)",
        }
    }

    pub(super) fn ocr(self) -> bool {
        matches!(self, Self::ScanPdf | Self::Photo)
    }
}

#[derive(Debug, Clone)]
pub(super) struct Doc {
    pub file: String,
    pub format: Format,
    pub blocks: Vec<Block>,
    /// Header and footer: never imported, names in them are kept with the case.
    pub header: Option<String>,
    pub footer: Option<String>,
    /// The file's author (docProps / meta / \info).
    pub creator: Option<String>,
    pub company: Option<String>,
    /// A reviewer's comment: never imported.
    pub comment: Option<String>,
    /// A tracked deletion: never imported.
    pub deleted: Option<String>,
    /// A floating text box (Word): imported once, not twice.
    pub textbox: Option<String>,
}

impl Doc {
    fn new(file: &str, format: Format, blocks: Vec<Block>) -> Self {
        Self {
            file: format!("{file}.{}", format.ext()),
            format,
            blocks,
            header: None,
            footer: None,
            creator: None,
            company: None,
            comment: None,
            deleted: None,
            textbox: None,
        }
    }

    /// The text a reader of the printed page sees, block by block (the ground truth).
    pub(super) fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for b in &self.blocks {
            match b {
                Block::H(t) | Block::P(t) | Block::Li(t) | Block::Note(t) => out.push(t.clone()),
                Block::Table(rows) => {
                    for r in rows {
                        out.push(r.join("\t"));
                    }
                }
                Block::Break => {}
            }
        }
        if let Some(t) = &self.textbox {
            out.push(t.clone());
        }
        out
    }
}

pub(super) struct Person {
    pub role: Role,
    pub value: &'static str,
    pub aliases: &'static [&'static str],
}

const fn p(role: Role, value: &'static str) -> Person {
    Person {
        role,
        value,
        aliases: &[],
    }
}

pub(super) struct Case {
    pub code: &'static str,
    pub age: Age,
    pub gender: GrammaticalGender,
    /// What she types in the case form.
    pub people: Vec<Person>,
    pub docs: Vec<Doc>,
    /// Must never reach the transport, in any request.
    pub secrets: Vec<String>,
    /// Ordinary words and clinical facts that must reach the transport as written (nothing
    /// hidden that should not be). Checked in the outgoing text of the materials they are in.
    pub keep: Vec<&'static str>,
    /// The score sheet she types into the scores screen.
    pub sheet: dv_domain::ScoreSheet,
    /// (measure, value, band, percentile) as the manuals give them, computed by hand.
    pub expected_bands: Vec<(&'static str, f64, &'static str, &'static str)>,
}

/// A valid Israeli ID from eight digits: the check digit is computed here, so the full number
/// is never written in the repository.
pub(super) fn valid_id(first8: &str) -> String {
    let digits: Vec<u32> = first8.chars().filter_map(|c| c.to_digit(10)).collect();
    assert_eq!(digits.len(), 8);
    let sum: u32 = digits
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let x = d * if i % 2 == 0 { 1 } else { 2 };
            if x > 9 {
                x - 9
            } else {
                x
            }
        })
        .sum();
    format!("{first8}{}", (10 - sum % 10) % 10)
}

/// The secrets of the corpus, made at run time.
pub(super) struct Secrets {
    pub id_a: String,
    pub id_b: String,
    pub id_c: String,
    pub phone_a: String,
    pub phone_b: String,
    pub phone_c: String,
    pub mail_a: String,
    pub mail_b: String,
}

pub(super) fn secrets() -> Secrets {
    Secrets {
        id_a: valid_id("32581479"),
        id_b: valid_id("21478036"),
        id_c: valid_id("34120985"),
        phone_a: ["052", "731", "4462"].join("-"),
        phone_b: ["054", "6629", "180"].concat(),
        phone_c: ["08", "9412", "776"].join("-"),
        mail_a: ["or.almagori", "example.net"].join("@"),
        mail_b: ["shira.d", "example.org"].join("@"),
    }
}

fn s(t: &str) -> String {
    t.to_owned()
}
fn h(t: &str) -> Block {
    Block::H(s(t))
}
fn para(t: &str) -> Block {
    Block::P(s(t))
}
fn li(t: &str) -> Block {
    Block::Li(s(t))
}
fn note(t: &str) -> Block {
    Block::Note(s(t))
}
fn table(rows: &[&[&str]]) -> Block {
    Block::Table(
        rows.iter()
            .map(|r| r.iter().map(|c| s(c)).collect())
            .collect(),
    )
}

fn sheet(instrument: &str, entries: &[(&str, f64)]) -> dv_domain::ScoreSheet {
    dv_domain::ScoreSheet {
        instrument: instrument.into(),
        module: String::new(),
        cutoff: None,
        entries: entries
            .iter()
            .map(|(m, v)| dv_domain::ScoreEntry {
                measure: (*m).into(),
                value: *v,
                note: String::new(),
            })
            .collect(),
        notes: String::new(),
    }
}

/// Case A: Shaked, 5;2, kindergarten, WPPSI-IV. The parents' names are also words ("אור",
/// "גיל"), the brother's name is two letters ("ים"), and the kindergarten friends are not
/// listed in the case.
pub(super) fn case_a(x: &Secrets) -> Case {
    let id = &x.id_a;
    let phone = &x.phone_a;
    let mail = &x.mail_a;
    let mut referral = Doc::new(
        "מכתב הפניה - ד\"ר קורן",
        Format::Pdf,
        vec![
            para("לכבוד: מכון לאבחון והתפתחות הילד"),
            para(&format!(
                "הנדון: הפניה לאבחון פסיכולוגי התפתחותי – שקד אלמגורי, ת\"ז {id}"
            )),
            para("תאריך: 14/07/2026"),
            para("שקד, בת 5 שנים וחודשיים, מוכרת לי מגיל שנה. נבדקה במרפאה ביום 2.7.2026 בעקבות פניית ההורים, אור וגיל, בשל קושי בשפה ובמעברים בגן."),
            para("ההריון והלידה היו תקינים, לידה בשבוע 39, משקל לידה 3,250 גרם. אבני הדרך המוטוריות הושגו בזמן: ישיבה בגיל 7 חודשים, הליכה בגיל 13 חודשים."),
            para("לדברי ההורים המילים הראשונות הופיעו בגיל שנתיים, ומשפטים של שתי מילים רק לקראת גיל 3. בבדיקת שמיעה (BERA) בתאריך 12.3.2025 לא נמצאה ירידה בשמיעה."),
            para("בבדיקה הגופנית: היקף ראש באחוזון 40, ללא סימנים דיסמורפיים, בדיקה נוירולוגית תקינה. לאור זאת, ובשל הדיווח מהגן על קושי במשחק משותף, אבקש אבחון פסיכולוגי התפתחותי מקיף, כולל הערכה קוגניטיבית ורגשית."),
            para(&format!("לתיאום: אור (אם), טלפון {phone}, דוא\"ל {mail}.")),
            para("בברכה, ד\"ר יעל קורן, רופאת ילדים"),
        ],
    );
    referral.header = Some(s("מרפאת ילדים שדה ענבר · רחוב הגפן 3"));
    referral.footer = Some(s("ד\"ר יעל קורן · מ.ר. 4471"));

    let mut intake = Doc::new(
        "שאלון הורים - שקד",
        Format::Docx,
        vec![
            h("שאלון קליטה להורים – לפני אבחון התפתחותי"),
            table(&[
                &["שם הילד/ה", "שקד אלמגורי"],
                &["תאריך לידה", "3.9.2021"],
                &["מספר זהות", id],
                &["כתובת", "רחוב הדקל 14, מושב שדה ענבר"],
                &["שם האם / טלפון", &format!("אור / {phone}")],
                &["שם האב", "גיל אלמגורי, מהנדס תוכנה"],
                &["מסגרת", "גן הרימונים, הגננת ורד"],
            ]),
            h("מה מדאיג אתכם?"),
            para("שקד ילדה חמה ומצחיקה, אבל בגן היא מתקשה להסביר את עצמה, ולפעמים מתפרצת בבכי כשלא מבינים אותה. בבית היא מדברת יותר, בעיקר עם אחיה הקטן ים."),
            para("בגיל 4 התחלנו טיפול אצל קלינאית התקשורת רוני שגיא, פעם בשבוע, וראינו שיפור באוצר המילים."),
            note("הטיפול מומן חלקית על ידי קופת החולים, 12 מפגשים בשנה."),
            h("התפתחות מוקדמת"),
            li("הריון: תקין, לידה רגילה בשבוע 39."),
            li("שינה: נרדמת בקושי, מתעוררת פעם בלילה, בדרך כלל אחרי חלום."),
            li("אכילה: בררנית, אוהבת שקדים ופסטה, מסרבת לירקות."),
            li("שליטה בסוגרים: ביום מגיל 3, בלילה מגיל 4."),
            h("חברים ומשחק"),
            para("בגן היא משחקת בעיקר עם עלמה, ולפעמים עם נועה מהשכונה. בימים ארוכים היא מעדיפה לשחק לבד בפינת הבובות. סבתא פנינה מספרת שאצלה שקד משחקת במשחקי דמיון ארוכים."),
            para("לאור ההמלצה של ד\"ר קורן פנינו לאבחון. ביקשנו גם את חוות הדעת של הנוירולוג, ד\"ר עמנואל ברקוביץ'."),
        ],
    );
    intake.header = Some(s("שאלון הורים · משפחת אלמגורי"));
    intake.footer = Some(s(&format!("נמסר על ידי: אור אלמגורי · {mail}")));
    intake.creator = Some(s("אור אלמגורי"));
    intake.company = Some(s("מכון בדוי לאבחון"));
    intake.comment = Some(s("לא לשתף את הגננת בפרטים על הסבתא"));
    intake.deleted = Some(s("אנחנו חושבים שאולי יש לה אוטיזם"));
    intake.textbox = Some(s("חשוב לנו: האבחון יתקיים בימי שלישי בלבד"));

    let mut kindergarten = Doc::new(
        "דיווח גננת - גן הרימונים",
        Format::Odt,
        vec![
            h("דיווח הגננת לקראת אבחון"),
            para("שקד נמצאת בגן הרימונים זו השנה השנייה. בבוקר היא נפרדת מאמא בקושי, אבל נרגעת אחרי כעשר דקות כשהסייעת חן מציעה לה פעילות קבועה."),
            para("במפגש הבוקר היא יושבת בשקט, מקשיבה לסיפור, אך כמעט שאינה משתתפת בשיחה. כשהיא נשאלת שאלה ישירה היא עונה במילה אחת או מצביעה."),
            note("התצפית נערכה בשבועות האחרונים של חודש יוני."),
            para("בחצר היא נמשכת למתקנים ולמשחקי תנועה, ומשחקת לרוב עם עלמה. כשמצטרפים ילדים נוספים היא נוטה לעזוב את המשחק."),
            table(&[
                &["תחום", "חוזקות", "קשיים"],
                &["שפה", "מבינה הוראות פשוטות", "משפטים קצרים, קושי לספר על אירוע"],
                &["חברתי", "חמה עם מבוגרים", "מעט יוזמה מול קבוצה"],
                &["מוטורי", "מטפסת ורצה היטב", "אחיזת עיפרון לא בשלה"],
            ]),
            para("ההמלצה שלי, ורד, היא להמשיך את הטיפול בתקשורת ולשקול שילוב של ריפוי בעיסוק בשנה הבאה."),
        ],
    );
    kindergarten.header = Some(s("גן הרימונים · מושב שדה ענבר"));
    kindergarten.creator = Some(s("ורד שמעוני"));
    kindergarten.comment = Some(s("לבדוק עם המפקחת לפני שליחה"));

    let mut slp = Doc::new(
        "סיכום אבחון תקשורת",
        Format::Rtf,
        vec![
            h("סיכום הערכה בתקשורת – רוני שגיא, קלינאית תקשורת"),
            para("שקד הופנתה להערכה בגיל 4 שנים ו-3 חודשים (דצמבר 2025) בשל איחור בהתפתחות השפה."),
            para("שפה קולטת: הבנת הוראות דו-שלביות תקינה, קושי בהבנת שאלות 'למה' ו'איך'. שפה מבעית: משפטים של 3–4 מילים, השמטות של מילות יחס, אוצר מילים מצומצם לגילה."),
            table(&[
                &["תחום", "ציון", "פירוש"],
                &["הבנת הנשמע", "85", "בגבול התחתון של התקין"],
                &["הבעה", "72", "מתחת לטווח התקין"],
                &["היגוי", "תקין", "שיבושים התפתחותיים בלבד"],
            ]),
            para("המלצות: המשך טיפול שבועי, הדרכת הורים, ושיתוף פעולה עם הצוות בגן."),
        ],
    );
    slp.creator = Some(s("רוני שגיא"));

    let mut scores = Doc::new(
        "ציוני WPPSI-IV",
        Format::Docx2,
        vec![
            h("WPPSI-IV – טבלת ציונים"),
            para("תאריך העברה: 2026-08-25. גיל בעת המבחן: 4:11."),
            table(&[
                &["מדד", "ציון תקן", "אחוזון", "רווח סמך 95%"],
                &["הבנה מילולית (VCI)", "88", "21", "81–96"],
                &["חזותי-מרחבי (VSI)", "103", "58", "94–111"],
                &["חשיבה פלואידית (FRI)", "97", "42", "89–105"],
                &["זיכרון עבודה (WMI)", "85", "16", "78–94"],
                &["מהירות עיבוד (PSI)", "106", "66", "96–114"],
                &["מנת משכל כללית (FSIQ)", "94", "34", "88–100"],
            ]),
            para("תת-מבחנים: מידע 8, שיתופיות 7, קוביות 11, מטריצות 9, זיכרון תמונות 7, חיפוש חרקים 12."),
        ],
    );
    scores.creator = Some(s("מכון בדוי לאבחון"));

    let neuro = Doc::new(
        "סיכום ביקור נוירולוג",
        Format::Text1255,
        vec![
            para("סיכום ביקור במרפאה הנוירולוגית לילדים"),
            para(&format!("שם: שקד אלמגורי   ת.ז.: {id}   תאריך לידה: 03/09/2021")),
            para("תאריך הביקור: 21 ביולי 2026"),
            para("רקע: ילדה בת 4 שנים ו-10 חודשים, הופנתה על ידי ד\"ר קורן בשל עיכוב שפתי."),
            para("בדיקה: ערה, משתפת פעולה חלקית, קשר עין תקין. טונוס תקין, החזרים סימטריים, הליכה תקינה."),
            para("EEG בשינה מתאריך 15.6.2026 – תקין."),
            para("סיכום: אין עדות למחלה נוירולוגית. ממליץ על אבחון פסיכולוגי התפתחותי והמשך טיפול בתקשורת."),
            para("ד\"ר עמנואל ברקוביץ', נוירולוג ילדים"),
        ],
    );

    let notes = Doc::new(
        "רשמי מפגשים - שקד",
        Format::TextUtf8,
        vec![
            para("מפגש 1 (2.9.26): שקד הגיעה עם אור. נפרדה בקלות יחסית, נכנסה לחדר אחרי שהראיתי לה את הבובות."),
            para("שיתפה פעולה במטלות החזותיות, בקוביות עבדה בריכוז ובהתמדה. במטלות המילוליות ענתה בקצרה, ולעיתים אמרה 'לא יודעת' לפני שניסתה."),
            para("מפגש 2 (9.9.26): הגיעה עם גיל. בתחילת המפגש הייתה מכונסת, אחר כך בנתה סיפור קצר בבובות על ילדה שהולכת לים עם אחיה."),
            para("במשחק הסמלי נראו רצף ויוזמה. כשהמשחק לא הלך לפי רצונה הגיבה בתסכול, אך נרגעה עם תיווך מילולי."),
            para("מפגש 3 (16.9.26): שיחה עם ההורים. לדבריהם השינה השתפרה בחודשיים האחרונים."),
        ],
    );

    Case {
        code: "BENCH-A",
        age: Age {
            years: 5,
            months: 0,
        },
        gender: GrammaticalGender::Female,
        people: vec![
            Person {
                role: Role::Child,
                value: "שקד",
                aliases: &["אלמגורי"],
            },
            p(Role::Mother, "אור"),
            p(Role::Father, "גיל"),
            p(Role::Brother, "ים"),
            p(Role::Teacher, "ורד"),
            p(Role::Assistant, "חן"),
            p(Role::Kindergarten, "גן הרימונים"),
            p(Role::Town, "שדה ענבר"),
            p(Role::Slp, "רוני שגיא"),
            p(Role::Doctor, "ד\"ר יעל קורן"),
            p(Role::Doctor, "ד\"ר עמנואל ברקוביץ'"),
        ],
        docs: vec![referral, intake, kindergarten, slp, scores, neuro, notes],
        secrets: vec![
            "שקד".into(),
            "אלמגורי".into(),
            "עלמה".into(),
            "נועה".into(),
            "פנינה".into(),
            "ורד".into(),
            "רימונים".into(),
            "שדה ענבר".into(),
            "רוני".into(),
            "שגיא".into(),
            "קורן".into(),
            "ברקוביץ".into(),
            "עמנואל".into(),
            "הדקל 14".into(),
            "הגפן 3".into(),
            "3.9.2021".into(),
            "03/09/2021".into(),
            id.clone(),
            phone.clone(),
            mail.clone(),
            "לא לשתף".into(),
            "אוטיזם".into(),
            "שמעוני".into(),
            "4471".into(),
        ],
        keep: vec![
            "לאור זאת",
            "אבני הדרך",
            "קושי בשפה",
            "שפה קולטת",
            "WPPSI-IV",
            "בקוביות עבדה בריכוז",
            "קשר עין תקין",
            "EEG",
        ],
        sheet: sheet(
            "wppsi_iv",
            &[
                ("vci", 88.0),
                ("vsi", 103.0),
                ("fri", 97.0),
                ("wmi", 85.0),
                ("psi", 106.0),
                ("fsiq", 94.0),
                ("information", 8.0),
                ("similarities", 7.0),
                ("block_design", 11.0),
                ("matrix", 9.0),
                ("picture_memory", 7.0),
                ("bug_search", 12.0),
            ],
        ),
        expected_bands: vec![
            ("vci", 88.0, "ממוצע נמוך", "21"),
            ("vsi", 103.0, "ממוצע", "58"),
            ("wmi", 85.0, "ממוצע נמוך", "16"),
            ("psi", 106.0, "ממוצע", "66"),
            ("fsiq", 94.0, "ממוצע", "34"),
            ("similarities", 7.0, "ממוצע נמוך", "16"),
            ("bug_search", 12.0, "ממוצע", "75"),
        ],
    }
}

/// One page of the earlier psycho-didactic report (case B), as a long report really reads.
fn long_report_page(n: usize) -> Vec<Block> {
    let topics = [
        ("רקע ההפניה", "אביב הופנה לאבחון פסיכו-דידקטי על ידי היועצת אסנת בשל קושי ברכישת הקריאה, איטיות בכתיבה ותלונות על כאבי בטן בבקרים לפני בית הספר."),
        ("רקע התפתחותי", "לדברי ההורים ההתפתחות המוטורית והשפתית הייתה תקינה. בגן חובה עלה קושי בזיהוי אותיות, ובכיתה א' קיבל תגבור בקריאה במסגרת בית הספר."),
        ("התרשמות כללית", "אביב הגיע בזמן, נפרד מהאם בקלות ונכנס לחדר בסקרנות. לאורך המפגשים שיתף פעולה, אך בתום כשעה של עבודה ניכרה עייפות ותנועתיות מוגברת."),
        ("קשב וריכוז", "במטלות קצרות ומובנות הקשב היה טוב. במטלות ארוכות ושגרתיות נדרשו הכוונה חוזרת והפסקות, ונצפו טעויות של חוסר תשומת לב."),
        ("קריאה", "קריאת מילים בודדות איטית ומפוענחת, עם שגיאות בתנועות ובאותיות דומות. קריאת טקסט מנוקד בקצב של 38 מילים לדקה, מתחת לנורמה לשכבת הגיל."),
        ("הבנת הנקרא", "כאשר הטקסט הוקרא לו, ההבנה הייתה טובה ואף מעמיקה. בקריאה עצמאית ההבנה נפגעה בשל המאמץ המושקע בפענוח."),
        ("כתיבה", "כתב היד לא מאורגן, עם היפוכי אותיות ושגיאות כתיב פונטיות. בהכתבה של עשר מילים כתב נכון ארבע."),
        ("חשבון", "שליטה טובה בעובדות החיבור והחיסור, ותפיסה כמותית תקינה. בבעיות מילוליות הקושי נבע מהקריאה ולא מההבנה המתמטית."),
        ("רגשי", "בשיחה תיאר את בית הספר כ'משעמם וקשה', וסיפר שהוא מתבייש לקרוא בקול מול הכיתה. בציורים ובסיפורים עלו נושאים של כישלון והשוואה לאחותו נגה."),
        ("חברתי", "לדבריו יש לו שני חברים קרובים, ובהפסקות הוא משחק כדורגל. המחנכת מירב מתארת ילד אהוב בכיתה, אך נוטה להתבודד בשיעורים."),
    ];
    let (title, text) = topics[n % topics.len()];
    let mut out = vec![h(&format!("{}. {title}", n + 1)), para(text)];
    out.push(para(&format!(
        "בהמשך לממצאי עמוד {} נבחנה גם עקביות הביצוע: בשני המפגשים, בהפרש של שבועיים, נמצאה תמונה דומה, מה שמחזק את מהימנות הממצאים בתחום זה.",
        n + 1
    )));
    out.push(para("התמונה העולה מתחום זה משתלבת בממצאי שאר התחומים: יכולת חשיבה תקינה לצד קושי ממוקד ברכישת הקריאה והכתיבה, המלווה בעייפות ובתסכול."));
    out.push(para("לצורך השוואה נבחנו גם מטלות קצרות בעל-פה, ובהן הביצוע היה מהיר ומדויק יותר, בעיקר כאשר ההוראות ניתנו בשלבים והודגמו לפני הביצוע."));
    if n == 6 {
        out.push(note("הנתונים נאספו בשני מפגשים, ב-3.1.2025 וב-17.1.2025."));
    }
    if n == 9 {
        out.push(table(&[
            &["תחום", "ממצא", "רמה"],
            &["קריאה", "מילים לדקה: 38", "נמוך"],
            &["הכתבה", "4 מתוך 10", "נמוך"],
            &["חשבון", "עובדות יסוד", "תקין"],
        ]));
    }
    out
}

/// Case B: Aviv, 8;7, third grade, WISC-V. Names that are also words ("אביב", "שירה", "דור",
/// "נגה"), an Arabic name, a 15-page earlier report, a phone photo of the referral.
pub(super) fn case_b(x: &Secrets) -> Case {
    let id = &x.id_b;
    let phone = &x.phone_b;
    let mail = &x.mail_b;
    let referral = Doc::new(
        "IMG_20260914_101532",
        Format::Photo,
        vec![
            para("בית הספר נופים – שירות ייעוץ"),
            para("לכבוד: הפסיכולוגית המאבחנת"),
            para("הנדון: הפניה לאבחון – אביב שחרורי, כיתה ג'"),
            para("אביב לומד בכיתה ג' אצל המחנכת מירב. מתחילת השנה הוא מתקשה בקריאה ובכתיבה, ובשיעורים ארוכים הוא מתנתק ומשוטט בכיתה."),
            para("בשיחה עם ההורים, שירה ודור, הם סיפרו על קושי דומה בבית בהכנת שיעורי הבית. אבקש להעריך קשב, קריאה ומצב רגשי."),
            para("בברכה, אסנת, יועצת בית הספר"),
        ],
    );

    let mut intake = Doc::new(
        "שאלון הורים - אביב",
        Format::Docx,
        vec![
            h("שאלון להורים – אבחון ילד בגיל בית הספר"),
            table(&[
                &["שם הילד", "אביב שחרורי"],
                &["תאריך לידה", "12 בפברואר 2018"],
                &["ת.ז.", id],
                &["טלפון ההורים", phone],
                &["דואר אלקטרוני", mail],
                &["בית ספר וכיתה", "בית הספר נופים, ג'2"],
            ]),
            h("תיאור הקושי"),
            para("אביב ילד סקרן ויצירתי, בונה שעות בלגו ומספר סיפורים מצחיקים. בבית הספר הוא מרגיש 'טיפש', ובערב לפני מבחן הוא בוכה וכואבת לו הבטן."),
            para("באביב שעבר הוא קיבל עזרה בקריאה מהמורה לחינוך מיוחד, וראינו התקדמות קטנה. ד\"ר פאדי חורי, הנוירולוג, המליץ לנסות טיפול תרופתי לקשב (Ritalin 10mg), ועדיין לא החלטנו."),
            note("שיעורי העזר ניתנו פעמיים בשבוע, 45 דקות כל פעם."),
            h("משפחה"),
            para("שירה (אמא) עובדת כמורה בבית ספר יסודי, ודור (אבא) עובד כטכנאי מזגנים. לאביב יש אחות גדולה, נגה, בת 12, שגם לה היה קושי בקריאה בכיתות הנמוכות."),
            para("לדור לדור עוברת אצלנו אהבה לספרים, גם אם הקריאה קשה. אביב אוהב שמקריאים לו, בעיקר ספרי הרפתקאות."),
            li("שינה: נרדם בקלות, ישן כעשר שעות."),
            li("מסכים: כשעתיים ביום, בעיקר משחקי מחשב."),
            li("חברים: מתרועע עם איתי מהשכונה, משחק כדורגל בהפסקות."),
        ],
    );
    intake.header = Some(s("משפחת שחרורי · רמת הנרקיסים"));
    intake.footer = Some(s("עמוד 1 מתוך 2"));
    intake.creator = Some(s("שירה שחרורי"));
    intake.comment = Some(s("להזכיר להם להביא את האבחון מכיתה ב"));
    intake.deleted = Some(s("המורה הקודמת אמרה שהוא עצלן"));
    intake.textbox = Some(s("הערה: ההורים גרושים? לא – נשואים, לברר"));

    let mut teacher = Doc::new(
        "שאלון מחנכת",
        Format::Docx2,
        vec![
            h("שאלון למחנכת – בית הספר נופים"),
            para("אביב לומד אצלי מתחילת כיתה ג'. הוא ילד חברותי ואהוב, משתתף בשיעורים בעל-פה ומעלה רעיונות מקוריים."),
            para("בקריאה הוא איטי מאוד ביחס לכיתה, נמנע מלקרוא בקול, ובמבחנים כתובים הוא משאיר חלק מהשאלות ריקות. בהקראה של המבחן הציונים שלו עולים בכ-20 נקודות."),
            table(&[
                &["מקצוע", "הערכה", "הערה"],
                &["חשבון", "טוב", "מבין מהר, טועה בהעתקה"],
                &["שפה", "חלש", "קריאה איטית, כתיבה לא מאורגנת"],
                &["אנגלית", "בינוני", "בעל-פה טוב יותר מבכתב"],
            ]),
            para("בהפסקות הוא משחק כדורגל עם איתי ועם ילדים מכיתות אחרות, ולא נצפו קשיים חברתיים."),
            para("המחנכת: מירב, ספטמבר 2026"),
        ],
    );
    teacher.header = Some(s("בית הספר נופים · רמת הנרקיסים"));
    teacher.creator = Some(s("מירב אלון-טל"));

    let ot = Doc::new(
        "הערכת ריפוי בעיסוק - סריקה",
        Format::ScanPdf,
        vec![
            para("דוח הערכה בריפוי בעיסוק"),
            para("שם: אביב שחרורי     גיל: 7 שנים ו-4 חודשים"),
            para("מעריכה: מיכל זוהר, מרפאה בעיסוק"),
            para("סיבת ההפניה: כתב יד לא קריא, איטיות בהעתקה מהלוח ועייפות בכתיבה."),
            para("ממצאים: אחיזת עיפרון לא יעילה, לחץ חזק על הדף, קושי בתכנון המרחב בשורה."),
            Block::Break,
            para("בתפיסה החזותית (מבחן VMI) הביצוע תקין, בקואורדינציה מוטורית עדינה נמצא קושי קל."),
            para("המלצות: טיפול בריפוי בעיסוק פעם בשבוע במשך חצי שנה, ושימוש במחזיק עיפרון."),
            para("מיכל זוהר, מרפאה בעיסוק, אפריל 2025"),
        ],
    );

    let scores = Doc::new(
        "ציוני WISC-V",
        Format::Pdf,
        vec![
            h("WISC-V – סיכום ציונים"),
            para("שם הנבדק: אביב שחרורי. תאריך העברה: 22.9.2026."),
            table(&[
                &["מדד", "ציון", "אחוזון", "טווח"],
                &["VCI", "108", "70", "ממוצע"],
                &["VSI", "100", "50", "ממוצע"],
                &["FRI", "112", "79", "ממוצע גבוה"],
                &["WMI", "82", "12", "ממוצע נמוך"],
                &["PSI", "78", "7", "גבולי"],
                &["FSIQ", "96", "39", "ממוצע"],
            ]),
            para("ציוני תת-מבחנים: שיתופיות 12, אוצר מילים 11, קוביות 10, פאזלים חזותיים 10, מטריצות 13, משקלות 12, זכירת ספרות 7, זכירת תמונות 6, צפנים 5, חיפוש סמלים 7."),
        ],
    );

    let neuro = Doc::new(
        "סיכום נוירולוג - ד\"ר חורי",
        Format::Rtf,
        vec![
            h("סיכום ביקור – מרפאה להפרעות קשב"),
            para(&format!("שם המטופל: אביב שחרורי, ת.ז. {id}, גיל 8 שנים ו-5 חודשים.")),
            para("תלונה עיקרית: קשיי קשב וריכוז, אי-שקט בשיעורים, קושי בקריאה."),
            para("בשאלון Conners להורים ולמורה עלו ציונים גבוהים בתחום הקשב (T=68 ו-T=71)."),
            para("בדיקה נוירולוגית תקינה. אין רקע של פרכוסים. מומלץ לשקול Methylphenidate במינון נמוך, לאחר אבחון פסיכו-דידקטי עדכני."),
            para("ד\"ר פאדי חורי, נוירולוג ילדים"),
        ],
    );

    let mut long_blocks = vec![
        h("דוח אבחון פסיכו-דידקטי"),
        para("שם: אביב שחרורי · כיתה ב' · מועד האבחון: ינואר 2025"),
        para("מאבחנת: ד\"ר ענבל קדמי, פסיכולוגית חינוכית מומחית"),
    ];
    for n in 0..15 {
        if n > 0 {
            long_blocks.push(Block::Break);
        }
        long_blocks.extend(long_report_page(n));
    }
    long_blocks.push(h("סיכום והמלצות"));
    long_blocks.push(para("הממצאים מתאימים ללקות למידה ספציפית בקריאה ובכתיבה (דיסלקציה ודיסגרפיה), על רקע יכולת חשיבה תקינה. מומלץ על התאמות בדרכי היבחנות: הקראת שאלונים, הארכת זמן ב-25% והתעלמות משגיאות כתיב."));
    let mut long = Doc::new("אבחון פסיכודידקטי 2025", Format::Docx, long_blocks);
    long.header = Some(s("מכון 'מבט' לאבחון · ד\"ר ענבל קדמי"));
    long.footer = Some(s("חסוי · נכתב עבור ההורים בלבד"));
    long.creator = Some(s("ענבל קדמי"));
    long.company = Some(s("מכון מבט"));
    // The same report as the PDF the institute sent.
    let mut long_pdf = long.clone();
    long_pdf.file = "אבחון פסיכודידקטי 2025.pdf".into();
    long_pdf.format = Format::Pdf;
    long_pdf.comment = None;
    long_pdf.deleted = None;
    long_pdf.textbox = None;

    let notes = Doc::new(
        "רשמי מפגשים - אביב",
        Format::TextUtf8,
        vec![
            para("מפגש 1, 22/09/2026: אביב הגיע עם דור. חייכן, מדבר הרבה, שואל על כל חפץ בחדר."),
            para("ב-WISC עבד ברצינות. במטריצות ובמשקלות הפגין חשיבה גמישה; בצפנים עבד לאט, מחק הרבה ונראה מתוסכל."),
            para("מפגש 2, 29/09/2026: קריאת טקסט – 41 מילים לדקה, עם שגיאות בתנועות. סיפר שהוא 'שונא לקרוא בקול'."),
            para("בציור משפחה צייר את נגה גבוהה מאוד ואת עצמו בצד. אמר שנגה 'תמיד יודעת הכל'."),
        ],
    );

    Case {
        code: "BENCH-B",
        age: Age {
            years: 8,
            months: 7,
        },
        gender: GrammaticalGender::Male,
        people: vec![
            // Typed whole in the name field, as she may: every tag of the child is shown
            // back as the full name.
            Person {
                role: Role::Child,
                value: "אביב שחרורי",
                aliases: &[],
            },
            p(Role::Mother, "שירה"),
            p(Role::Father, "דור"),
            p(Role::Sister, "נגה"),
            p(Role::SchoolTeacher, "מירב"),
            p(Role::School, "בית הספר נופים"),
            p(Role::Town, "רמת הנרקיסים"),
            p(Role::Professional, "אסנת"),
            p(Role::Doctor, "ד\"ר פאדי חורי"),
            p(Role::Therapist, "מיכל זוהר"),
            p(Role::Psychologist, "ד\"ר ענבל קדמי"),
        ],
        docs: vec![
            referral, intake, teacher, ot, scores, neuro, long, long_pdf, notes,
        ],
        secrets: vec![
            "שחרורי".into(),
            "אסנת".into(),
            "מירב".into(),
            "נופים".into(),
            "הנרקיסים".into(),
            "פאדי".into(),
            "חורי".into(),
            "זוהר".into(),
            "קדמי".into(),
            "ענבל".into(),
            "איתי".into(),
            "12 בפברואר 2018".into(),
            id.clone(),
            phone.clone(),
            mail.clone(),
            "עצלן".into(),
            "להזכיר להם".into(),
        ],
        keep: vec![
            "WISC",
            "קושי בקריאה",
            "Ritalin",
            "Methylphenidate",
            "Conners",
            "כדורגל",
            "מטריצות",
        ],
        sheet: sheet(
            "wisc_v",
            &[
                ("vci", 108.0),
                ("vsi", 100.0),
                ("fri", 112.0),
                ("wmi", 82.0),
                ("psi", 78.0),
                ("fsiq", 96.0),
                ("similarities", 12.0),
                ("vocabulary", 11.0),
                ("block_design", 10.0),
                ("visual_puzzles", 10.0),
                ("matrix", 13.0),
                ("figure_weights", 12.0),
                ("digit_span", 7.0),
                ("picture_span", 6.0),
                ("coding", 5.0),
                ("symbol_search", 7.0),
            ],
        ),
        expected_bands: vec![
            ("vci", 108.0, "ממוצע", "70"),
            ("fri", 112.0, "ממוצע גבוה", "79"),
            ("wmi", 82.0, "ממוצע נמוך", "12"),
            ("psi", 78.0, "גבולי", "7"),
            ("fsiq", 96.0, "ממוצע", "39"),
            ("coding", 5.0, "נמוך", "5"),
            ("matrix", 13.0, "ממוצע גבוה", "84"),
        ],
    }
}

/// One page of the ten-page scanned report of the child development institute (case C).
fn scan_page(n: usize) -> Vec<Block> {
    let pages = [
        "הדר הגיעה להערכה התפתחותית בגיל שנתיים ושלושה חודשים, בליווי אמה אביגיל, בעקבות דאגה לגבי התפתחות הדיבור.",
        "ההריון היה תקין. הלידה בשבוע 37 בניתוח קיסרי מתוכנן. בתקופת הינקות נרשמו דלקות אוזניים חוזרות, וטופלו באנטיביוטיקה.",
        "התפתחות מוטורית: התהפכה בגיל 5 חודשים, ישבה בגיל 8 חודשים, הלכה בגיל 15 חודשים. כיום רצה ומטפסת היטב.",
        "התפתחות שפתית: מבינה הוראות פשוטות בהקשר. אוצר מילים של כ-20 מילים, ללא צירופים. מצביעה לבקשה ומושכת את המבוגר ביד.",
        "קשר ותקשורת: קשר עין מזדמן, חיוך חברתי קיים. תגובה לשם עקבית בחלק מהניסיונות בלבד. משחק בעיקר בסידור חפצים בשורה.",
        "במשחק עם הפסיכולוגית הדר שיתפה פעולה במשחקי סיבה ותוצאה, ונהנתה מבועות סבון. משחק סמלי לא נצפה.",
        "הערכת ריפוי בעיסוק: רגישות למגע בידיים ובפה, הימנעות ממרקמים רכים. מוטוריקה עדינה תואמת גיל.",
        "הערכת קלינאית תקשורת: שפה קולטת ומבעית מתחת לטווח הגיל. מומלץ טיפול פעמיים בשבוע.",
        "בדיקת שמיעה התנהגותית בתאריך 4.4.2025: שמיעה תקינה בשתי האוזניים. בדיקה גנטית (CMA) – ללא ממצא.",
        "סיכום: עיכוב שפתי ותקשורתי. מומלץ על מעקב התפתחותי בעוד חצי שנה, טיפול בתקשורת, והערכה פסיכולוגית בגיל שלוש.",
    ];
    vec![
        para(&format!("עמוד {} מתוך 10", n + 1)),
        para(pages[n % pages.len()]),
        para("הממצאים נדונו בישיבת הצוות הרב-מקצועי, והוסברו להורים בשיחת סיכום."),
    ]
}

/// Case C: Hadar, 2;10, day care, Bayley-4. A ten-page scan, a one-page scan from the nurse,
/// the parents' intake in ODT and the day care's report in an old code page.
pub(super) fn case_c(x: &Secrets) -> Case {
    let id = &x.id_c;
    let phone = &x.phone_c;
    let mut blocks = vec![
        para("המכון להתפתחות הילד – סיכום הערכה רב-מקצועית"),
        para("שם הילדה: הדר נחמיאס"),
    ];
    for n in 0..10 {
        if n > 0 {
            blocks.push(Block::Break);
        }
        blocks.extend(scan_page(n));
    }
    blocks.push(para("ד\"ר נעמה פלג, רופאה התפתחותית"));
    let institute = Doc::new("סריקה - המכון להתפתחות הילד", Format::ScanPdf, blocks);

    let nurse = Doc::new(
        "הפניה מטיפת חלב",
        Format::ScanPdf,
        vec![
            para("טיפת חלב קריית הדמיון"),
            para("הפניה להערכה פסיכולוגית התפתחותית"),
            para(&format!("שם: הדר נחמיאס   ת.ז. {id}")),
            para("הדר בת שנתיים ועשרה חודשים. במעקב התפתחותי שגרתי עלה עיכוב בדיבור ובתקשורת."),
            para(&format!("האחות ליאורה, טלפון {phone}")),
        ],
    );

    let mut intake = Doc::new(
        "אינטייק הורים - הדר",
        Format::Odt,
        vec![
            h("שיחת אינטייק עם ההורים"),
            para("השתתפו: אביגיל ועמית, הורי הדר. השיחה התקיימה ב-1 בספטמבר 2026."),
            para("ההורים מתארים ילדה שמחה ושקטה, שאוהבת לסדר את הצעצועים בשורות ולהסתכל על גלגלים מסתובבים."),
            para("עמית מספר שהדר כמעט אינה פונה אליהם בבקשה, אלא לוקחת את היד שלהם אל מה שהיא רוצה. אביגיל מתארת קושי ברחצה ובתספורת."),
            para("סבתא רחל שומרת עליה פעמיים בשבוע, ולדבריה הדר שרה איתה שירים מוכרים בהברות."),
            table(&[
                &["אבן דרך", "גיל"],
                &["הליכה", "15 חודשים"],
                &["מילה ראשונה", "20 חודשים"],
                &["צירוף שתי מילים", "טרם"],
            ]),
            para("שינה: נרדמת רק עם אחד ההורים, מתעוררת פעמיים בלילה. אכילה: בררנית מאוד, אוכלת בעיקר מזון יבש."),
        ],
    );
    intake.header = Some(s("רשמי אינטייק · משפחת נחמיאס"));
    intake.creator = Some(s("ד\"ר רותם אלמוג"));

    let daycare = Doc::new(
        "דיווח מעון הפרפר",
        Format::Text1255,
        vec![
            para("דיווח מהמעון – מעון הפרפר, קבוצת הפעוטות"),
            para("הדר נמצאת אצלנו מגיל שנה וחצי. המטפלת זהבה מתארת ילדה רגועה שמשחקת בעיקר לבד."),
            para("במפגש מעגל היא יושבת בקצה ומסתכלת, ולפעמים יוצאת למשחק בגלגלי המכונית. בארוחה היא אוכלת רק לחם ופריכיות."),
            para("כשהיא רוצה משהו היא מושכת את המטפלת ביד. היא לא מצביעה ולא מביאה חפצים להראות."),
            para("בחודשים האחרונים התחילה לחקות תנועות בשירים, ונהנית מאוד מנדנדה."),
        ],
    );

    Case {
        code: "BENCH-C",
        age: Age {
            years: 2,
            months: 10,
        },
        gender: GrammaticalGender::Female,
        people: vec![
            Person {
                role: Role::Child,
                value: "הדר",
                aliases: &["נחמיאס"],
            },
            p(Role::Mother, "אביגיל"),
            p(Role::Father, "עמית"),
            p(Role::Relative, "רחל"),
            p(Role::Kindergarten, "מעון הפרפר"),
            p(Role::Assistant, "זהבה"),
            p(Role::Town, "קריית הדמיון"),
            p(Role::Professional, "ליאורה"),
            p(Role::Doctor, "ד\"ר נעמה פלג"),
        ],
        docs: vec![institute, nurse, intake, daycare],
        secrets: vec![
            "נחמיאס".into(),
            "אביגיל".into(),
            "זהבה".into(),
            "ליאורה".into(),
            "פלג".into(),
            "נעמה".into(),
            "רחל".into(),
            "הדמיון".into(),
            "הפרפר".into(),
            id.clone(),
            phone.clone(),
        ],
        keep: vec![
            "בגלגלי המכונית",
            "קשר עין",
            "בועות סבון",
            "CMA",
            "שמיעה תקינה",
        ],
        sheet: sheet(
            "bayley_4",
            &[
                ("cognitive", 85.0),
                ("language", 68.0),
                ("motor", 97.0),
                ("receptive", 4.0),
                ("expressive", 3.0),
                ("fine_motor", 9.0),
                ("gross_motor", 10.0),
            ],
        ),
        expected_bands: vec![
            ("cognitive", 85.0, "ממוצע נמוך", "16"),
            ("language", 68.0, "נמוך מאוד", "2"),
            ("motor", 97.0, "ממוצע", "42"),
            ("expressive", 3.0, "נמוך מאוד", "1"),
        ],
    }
}
