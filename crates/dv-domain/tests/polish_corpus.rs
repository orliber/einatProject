//! The proofreading (D-167) on the fabricated past reports of `tests/fixtures/score_reports`:
//! ordinary report prose, written as reports are, makes no long-sentence note, and almost no
//! note at all. No real report is in the repo; these are invented.

use std::path::Path;

#[test]
fn ordinary_report_prose_makes_no_long_sentence_note() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/score_reports");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    files.sort();
    assert!(files.len() >= 10, "{dir:?}");
    let mut paragraphs = 0;
    let mut noted = Vec::new();
    for f in files {
        // The corpus marks every score "⟦7⟧" or "⟦7:block_design⟧": the plain text has "7".
        let raw = std::fs::read_to_string(&f).unwrap();
        let mut text = String::new();
        let (mut inside, mut name) = (false, false);
        for c in raw.chars() {
            match c {
                '⟦' => (inside, name) = (true, false),
                '⟧' => inside = false,
                ':' if inside => name = true,
                _ if inside && name => {}
                _ => text.push(c),
            }
        }
        // A paragraph is a block between blank lines. For the length of a sentence its wrapped
        // lines are one text, so a sentence that runs over two lines is counted whole; for the
        // other notes it is read as written (a list stays a list). The first block is the
        // report's title and date, not prose.
        let blocks: Vec<&str> = text
            .split("\n\n")
            .filter(|b| !b.trim().is_empty())
            .collect();
        for (i, block) in blocks.iter().enumerate() {
            paragraphs += 1;
            let joined = block.lines().map(str::trim).collect::<Vec<_>>().join(" ");
            let notes = dv_domain::polish_notes(&joined);
            assert!(
                !notes.iter().any(|n| n.starts_with("משפט ארוך")),
                "{f:?}: {joined}: {notes:?}"
            );
            let notes = dv_domain::polish_notes(block.trim());
            if i > 0 && !notes.is_empty() {
                noted.push(format!("{block}: {notes:?}"));
            }
        }
    }
    assert!(paragraphs > 30, "{paragraphs}");
    assert!(noted.is_empty(), "{noted:#?}");
}
