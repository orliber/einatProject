//! Writes a fabricated sample report (plain and password-protected) for checking the
//! rendering in Word or LibreOffice: `cargo run -p dv-export --example sample -- <dir> <password>`.

use dv_export::{encrypt, render, InfoLine, Report, ReportPart, ReportSection};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let dir = std::path::PathBuf::from(args.next().unwrap_or_else(|| ".".into()));
    let password = args.next().unwrap_or_else(|| "דוגמה-בלבד-1234".into());
    let para = |t: &str| t.to_owned();
    let report = Report {
        title: "דוח אבחון פסיכולוגי התפתחותי".into(),
        info: vec![
            InfoLine { label: "שם הילד".into(), value: "אלון בדוי".into() },
            InfoLine { label: "גיל בעת האבחון".into(), value: "5:4".into() },
            InfoLine { label: "מסגרת".into(), value: "גן חובה".into() },
        ],
        parts: vec![
            ReportPart {
                title: "רקע".into(),
                sections: vec![
                    ReportSection { title: "סיבת הפניה".into(), paragraphs: vec![para("ההורים פנו לאבחון בשל קושי במעברים בין פעילויות ותגובות רגשיות עוצמתיות בגן, לצד סקרנות ויכולת ריכוז טובה במשחק בנייה.")] },
                    ReportSection { title: "רקע התפתחותי".into(), paragraphs: vec![para("ההריון והלידה עברו ללא סיבוכים. אבני הדרך המוטוריות הושגו בזמן. לפי ההורים, המילים הראשונות הופיעו סביב גיל שנה, ומשפטים בני שתי מילים בגיל שנתיים.")] },
                ],
            },
            ReportPart {
                title: "ממצאי האבחון".into(),
                sections: vec![
                    ReportSection { title: "כלי האבחון".into(), paragraphs: vec![para("WPPSI-IV (גרסה עברית), Vineland-3 (שאלון להורים), תצפית בגן ומשחק חופשי.")] },
                    ReportSection { title: "פרופיל קוגניטיבי".into(), paragraphs: vec![
                        para("הציון הכולל נמצא בטווח הממוצע (FSIQ = 102). נמצא פער בין ההבנה המילולית (112) לבין מהירות העיבוד (88), פער שמשמעותו הקלינית נדונה בסיכום."),
                        para("במשימות חזותיות-מרחביות אלון עבד בשיטתיות, בדק את עצמו ונהנה מהאתגר."),
                    ] },
                ],
            },
        ],
        signature: vec!["בברכה,".into(), "פסיכולוגית התפתחותית מומחית (בדוי)".into(), "מס' רישיון: לדוגמה בלבד".into()],
        confidentiality: "חסוי – מידע רפואי. מיועד להורים ולגורמים שההורים אישרו בלבד.".into(),
        font: "David".into(),
    };
    let docx = render(&report)?;
    std::fs::write(dir.join("sample-report.docx"), &docx)?;
    std::fs::write(
        dir.join("sample-report-protected.docx"),
        encrypt(&docx, &password)?,
    )?;
    Ok(())
}
