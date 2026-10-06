//! Writes a fabricated sample report (plain and password-protected) for checking the
//! rendering in Word or LibreOffice: `cargo run -p dv-export --example sample -- <dir> <password>
//! [template.docx]`. With a template, a third file is written into it.

use dv_export::{encrypt, render, InfoLine, Report, ReportPart, ReportSection};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let dir = std::path::PathBuf::from(args.next().unwrap_or_else(|| ".".into()));
    let password = args.next().unwrap_or_else(|| "דוגמה-בלבד-1234".into());
    let para = |t: &str| t.to_owned();
    let report = Report {
        tables: vec![dv_export::ScoreTable {
            title: "WPPSI-IV".into(),
            columns: ["מדד", "ציון", "אחוזון", "טווח"].map(str::to_owned).to_vec(),
            rows: [
                ["מנת משכל כללית (FSIQ)", "102", "55", "ממוצע"],
                ["הבנה מילולית (VCI)", "112", "79", "ממוצע גבוה"],
                ["מהירות עיבוד (PSI)", "84", "14", "ממוצע נמוך"],
            ]
            .map(|r| r.map(str::to_owned).to_vec())
            .to_vec(),
            note: "ציוני המדדים הם ציוני תקן (ממוצע 100, סטיית תקן 15).".into(),
            charts: vec![dv_export::ScoreChart {
                title: "פרופיל המדדים".into(),
                min: 40.0,
                max: 160.0,
                step: 5.0,
                mean: 100.0,
                sd: 15.0,
                bars: [("מנת משכל כללית (FSIQ)", 102.0), ("הבנה מילולית (VCI)", 112.0), ("מהירות עיבוד (PSI)", 84.0)]
                    .map(|(l, v)| dv_export::ChartBar { label: l.into(), value: v })
                    .to_vec(),
                note: "הרקע הבהיר: טווח של סטיית תקן אחת סביב הממוצע (85–115).".into(),
            }],
        }],
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
                        para("הציון הכולל נמצא בטווח הממוצע (FSIQ = 102). נמצא פער בין ההבנה המילולית (112) לבין מהירות העיבוד (84), פער שמשמעותו הקלינית נדונה בסיכום."),
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
    if let Some(template) = args.next() {
        let template = std::fs::read(template)?;
        std::fs::write(
            dir.join("sample-report-template.docx"),
            dv_export::render_with_template(&report, &template)?,
        )?;
    }
    Ok(())
}
