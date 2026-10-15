//! How her template looks (V4): the body and heading fonts, the text size, the page and its
//! margins, read from the template's own styles and section properties. The report page in
//! the app uses them, so what she sees is close to what the Word file will be. Only names and
//! numbers come out; a font name that is not plainly a name is dropped.

use quick_xml::events::Event;
use quick_xml::Reader;
use serde::{Deserialize, Serialize};

use crate::template::{attr, tag_attr, text, unpack, Parts};
use crate::ExportError;

/// What the page in the app takes from her template. Every field is optional: what the
/// template does not say stays as the app draws it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateLook {
    pub font: Option<String>,
    /// Body text size, in half points (Word's unit: 24 = 12pt).
    pub size_half_points: Option<u32>,
    pub heading_font: Option<String>,
    /// Page width and height, in twentieths of a point.
    pub page: Option<(u32, u32)>,
    /// Top, right, bottom, left margins, in twentieths of a point.
    pub margins: Option<[u32; 4]>,
}

/// A font name she could have typed in Word: letters, digits, spaces and a few marks, at
/// most 64 characters. Anything else is not used.
fn font_name(raw: &str) -> Option<String> {
    let name = raw.trim();
    let ok = !name.is_empty()
        && name.chars().count() <= 64
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'));
    ok.then(|| name.to_owned())
}

/// Fonts named by the theme: (major, minor), Hebrew script first.
fn theme_fonts(xml: &str) -> (Option<String>, Option<String>) {
    let mut reader = Reader::from_str(xml);
    let (mut major, mut minor) = (None, None);
    let mut inside: Option<bool> = None; // Some(true) = major
    let mut latin: Option<String> = None;
    let mut hebrew: Option<String> = None;
    while let Ok(ev) = reader.read_event() {
        match ev {
            Event::Start(e) if matches!(e.local_name().as_ref(), "majorFont" | "minorFont") => {
                inside = Some(e.local_name().as_ref() == "majorFont");
                latin = None;
                hebrew = None;
            }
            Event::Empty(e) if inside.is_some() => {
                let tf = attr(&e, "typeface").filter(|t| !t.is_empty());
                match e.local_name().as_ref() {
                    "latin" => latin = tf,
                    "font" if attr(&e, "script").as_deref() == Some("Hebr") => hebrew = tf,
                    _ => {}
                }
            }
            Event::End(e) if matches!(e.local_name().as_ref(), "majorFont" | "minorFont") => {
                let chosen = hebrew.take().or(latin.take());
                if inside == Some(true) {
                    major = chosen;
                } else {
                    minor = chosen;
                }
                inside = None;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    (major, minor)
}

/// The font of a `w:rFonts` element: the complex-script one (Hebrew) first.
fn rfonts(
    e: &quick_xml::events::BytesStart<'_>,
    theme: &(Option<String>, Option<String>),
) -> Option<String> {
    for name in ["w:cs", "w:ascii", "w:hAnsi"] {
        if let Some(f) = attr(e, name).and_then(|f| font_name(&f)) {
            return Some(f);
        }
    }
    for name in ["w:cstheme", "w:asciiTheme", "w:hAnsiTheme"] {
        if let Some(t) = attr(e, name) {
            let f = if t.starts_with("major") {
                &theme.0
            } else {
                &theme.1
            };
            if let Some(f) = f.as_deref().and_then(font_name) {
                return Some(f);
            }
        }
    }
    None
}

#[derive(Default)]
struct Run {
    font: Option<String>,
    size: Option<u32>,
}

fn styles(xml: &str, theme: &(Option<String>, Option<String>)) -> (Run, Run) {
    let mut reader = Reader::from_str(xml);
    let mut defaults = Run::default();
    let mut normal = Run::default();
    let mut heading = Run::default();
    // Where the reader is: in the document defaults, or in a style (is default, is heading 1).
    let mut in_defaults = false;
    let mut style: Option<(bool, bool)> = None;
    let mut normal_seen = false;
    while let Ok(ev) = reader.read_event() {
        match ev {
            Event::Start(e) if e.local_name().as_ref() == "rPrDefault" => in_defaults = true,
            Event::End(e) if e.local_name().as_ref() == "rPrDefault" => in_defaults = false,
            Event::Start(e) if e.local_name().as_ref() == "style" => {
                let para = attr(&e, "w:type").is_some_and(|t| t == "paragraph");
                let default = para
                    && attr(&e, "w:default").is_some_and(|d| d == "1" || d == "true")
                    && !normal_seen;
                normal_seen |= default;
                style = para.then_some((default, false));
            }
            Event::End(e) if e.local_name().as_ref() == "style" => style = None,
            Event::Empty(e) => {
                let name = e.local_name();
                if name.as_ref() == "name" {
                    if let Some((_, heading1)) = style.as_mut() {
                        *heading1 =
                            attr(&e, "w:val").is_some_and(|v| v.eq_ignore_ascii_case("heading 1"));
                    }
                    continue;
                }
                let target = if in_defaults {
                    Some(&mut defaults)
                } else {
                    match style {
                        Some((true, _)) => Some(&mut normal),
                        Some((_, true)) => Some(&mut heading),
                        _ => None,
                    }
                };
                let Some(run) = target else { continue };
                match name.as_ref() {
                    "rFonts" => {
                        if let Some(f) = rfonts(&e, theme) {
                            run.font = Some(f);
                        }
                    }
                    "szCs" | "sz" => {
                        let v = attr(&e, "w:val").and_then(|v| v.parse::<u32>().ok());
                        // The complex-script size wins over the Latin one.
                        if let Some(v) = v.filter(|v| (12..=96).contains(v)) {
                            if name.as_ref() == "szCs" || run.size.is_none() {
                                run.size = Some(v);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let body = Run {
        font: normal.font.or(defaults.font),
        size: normal.size.or(defaults.size),
    };
    (body, heading)
}

/// `w:<el> w:<name>="…"` in the last section properties of the document.
fn section_number(doc: &str, el: &str, names: &[&str]) -> Option<u32> {
    let start = doc.rfind(&format!("<w:{el} "))?;
    let (tag, _) = doc.get(start..)?.split_once('>')?;
    names
        .iter()
        .find_map(|name| tag_attr(tag, name)?.parse().ok())
}

fn get<'a>(parts: &'a Parts, name: &str) -> Option<&'a [u8]> {
    parts
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, d)| d.as_slice())
}

/// The look of her template. A template the export accepts always gives an answer; what it
/// does not say is `None`.
pub fn template_look(template: &[u8]) -> Result<TemplateLook, ExportError> {
    let parts = unpack(template)?;
    let theme = get(&parts, "word/theme/theme1.xml")
        .and_then(|t| text(t).ok())
        .map(theme_fonts)
        .unwrap_or_default();
    let (body, heading) = get(&parts, "word/styles.xml")
        .and_then(|s| text(s).ok())
        .map(|s| styles(s, &theme))
        .unwrap_or_default();
    let doc = get(&parts, "word/document.xml")
        .and_then(|d| text(d).ok())
        .unwrap_or_default();
    let page = section_number(doc, "pgSz", &["w"])
        .zip(section_number(doc, "pgSz", &["h"]))
        .filter(|(w, h)| (5000..=40000).contains(w) && (5000..=40000).contains(h));
    let margin = |names: &[&str]| section_number(doc, "pgMar", names).filter(|m| *m <= 8000);
    let margins = match (
        margin(&["top"]),
        margin(&["right", "end"]),
        margin(&["bottom"]),
        margin(&["left", "start"]),
    ) {
        (Some(t), Some(r), Some(b), Some(l)) => Some([t, r, b, l]),
        _ => None,
    };
    Ok(TemplateLook {
        font: body.font,
        size_half_points: body.size,
        heading_font: heading.font,
        page,
        margins,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn font_names_are_names_and_nothing_else() {
        assert_eq!(font_name(" David "), Some("David".to_owned()));
        assert_eq!(
            font_name("Times New Roman"),
            Some("Times New Roman".to_owned())
        );
        assert_eq!(font_name("פרנק-רוהל"), Some("פרנק-רוהל".to_owned()));
        for bad in ["", "x\";background:url(a)", "a}b", &"א".repeat(65)] {
            assert_eq!(font_name(bad), None, "{bad}");
        }
    }

    /// The last section's numbers, with Hebrew text before and between the tags; a value that
    /// is not a number is not an answer.
    #[test]
    fn section_numbers_are_read_next_to_hebrew() {
        let doc = "<w:p><w:t>מכתב להורים</w:t></w:p><w:sectPr><w:pgMar w:left=\"999\"/></w:sectPr>\
                   <w:t>סוף</w:t><w:sectPr>שוליים<w:pgMar w:top=\"1800\" w:start=\"1300\"/></w:sectPr>";
        assert_eq!(section_number(doc, "pgMar", &["top"]), Some(1800));
        assert_eq!(section_number(doc, "pgMar", &["left", "start"]), Some(1300));
        assert_eq!(section_number(doc, "pgMar", &["bottom"]), None);
        assert_eq!(
            section_number("<w:pgSz w:w=\"רחב\" w:h=\"16838\"/>", "pgSz", &["w"]),
            None
        );
        assert_eq!(
            section_number("<w:pgSz w:w=\"11906\" ילד", "pgSz", &["w"]),
            None
        );
    }

    #[test]
    fn the_theme_gives_the_hebrew_font_first() {
        let xml = r#"<a:theme><a:themeElements><a:fontScheme>
            <a:majorFont><a:latin typeface="Calibri Light"/><a:font script="Hebr" typeface="Times New Roman"/></a:majorFont>
            <a:minorFont><a:latin typeface="Calibri"/><a:font script="Jpan" typeface="x"/></a:minorFont>
            </a:fontScheme></a:themeElements></a:theme>"#;
        assert_eq!(
            theme_fonts(xml),
            (
                Some("Times New Roman".to_owned()),
                Some("Calibri".to_owned())
            )
        );
    }
}
