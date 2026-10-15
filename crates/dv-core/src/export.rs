//! Letters, the locked PDF and the Word report.

use super::*;

impl Core {
    /// Whether a PDF is exactly one this vault exported (its fingerprint is in the audit log):
    /// when, or `None` for a file that was changed or not made here.
    pub fn check_original(&mut self, bytes: &[u8]) -> Result<Option<i64>, CoreError> {
        let sha256 = dv_vault::crypto::sha256_hex(bytes);
        let needle = format!("\"sha256\":\"{sha256}\"");
        Ok(self
            .vault_ref()?
            .audit_find("export", &needle)?
            .map(|e| e.ts))
    }

    /// A short letter to the parents or the school from the approved report (EX-4): an
    /// ordinary section request (same filter, review screen and gate), whose answer is kept as
    /// the letter's draft in this case. `note` is anything she wants to add.
    pub fn prepare_letter(
        &mut self,
        case_id: &str,
        audience: &str,
        note: &str,
    ) -> Result<Prepared, CoreError> {
        let letter = Letter::from_audience(audience)?;
        let instruction = if note.trim().is_empty() {
            letter.instruction().to_owned()
        } else {
            format!("{}\nבנוסף: {}", letter.instruction(), note.trim())
        };
        self.prepare_section(case_id, letter.key(), &instruction)
    }

    /// The letter's paragraphs, names restored, for her to read, edit and approve.
    pub fn letter(
        &mut self,
        case_id: &str,
        audience: &str,
    ) -> Result<Vec<ParagraphView>, CoreError> {
        let letter = Letter::from_audience(audience)?;
        let v = self.vault_ref()?;
        let identities = v.identities(case_id)?;
        let masks = v.masks(case_id)?;
        let practitioner = v.practitioner()?.names.first().cloned();
        let with_versions = v.drafts_with_versions(case_id)?;
        Ok(v.drafts(case_id, letter.key())?
            .into_iter()
            .filter(|d| matches!(d.status, DraftStatus::Proposed | DraftStatus::Approved))
            .map(|d| ParagraphView {
                has_versions: with_versions.contains(&d.id),
                // A letter is short and written from approved text: no style note (D-043).
                style_note: None,
                style: None,
                id: d.id,
                text: restore(&d.text_tagged, &identities, practitioner.as_deref(), &masks),
                status: d.status,
                by_ai: d.author == Author::Ai,
                sources: Vec::new(),
                warnings: Vec::new(),
                replaces: d.replaces,
            })
            .collect())
    }

    /// The letter as a Word file (in her template when she has one): approved paragraphs
    /// only, with the child's name and the date at the top and her signature at the end.
    pub fn export_letter(
        &mut self,
        case_id: &str,
        audience: &str,
        password: Option<&str>,
    ) -> Result<Vec<u8>, CoreError> {
        let letter = Letter::from_audience(audience)?;
        let settings = self.report_settings()?;
        let (report, _) = self.build_report(case_id)?;
        let paragraphs: Vec<String> = self
            .letter(case_id, audience)?
            .into_iter()
            .filter(|p| p.status == DraftStatus::Approved)
            .map(|p| p.text)
            .collect();
        if paragraphs.is_empty() {
            return Err(CoreError::Refused(
                "עוד אין במכתב פסקה מאושרת. מאשרים את הפסקאות, ואז מפיקים את הקובץ.".to_owned(),
            ));
        }
        let doc = dv_export::Report {
            title: letter.file_title().to_owned(),
            info: report.info,
            parts: vec![dv_export::ReportPart {
                title: String::new(),
                sections: vec![dv_export::ReportSection {
                    title: String::new(),
                    paragraphs,
                }],
            }],
            tables: Vec::new(),
            signature: report.signature,
            confidentiality: settings.confidentiality,
            font: settings.font,
        };
        let identities = self.vault_ref()?.identities(case_id)?;
        let known: HashSet<String> = identities.iter().map(|i| i.tag.clone()).collect();
        for p in doc
            .parts
            .iter()
            .flat_map(|part| &part.sections)
            .flat_map(|s| &s.paragraphs)
        {
            if dv_privacy::restore::remaining_tags(p)
                .iter()
                .any(|t| known.contains(t))
            {
                return Err(CoreError::Refused(
                    "נשארה במכתב תגית במקום שם. פותחים את הפסקה ומתקנים.".to_owned(),
                ));
            }
        }
        if let Some(l) = dv_export::leftover_placeholders(&doc).into_iter().next() {
            return Err(CoreError::Refused(format!(
                "נשאר במכתב סימון בסוגריים מרובעים: {l}"
            )));
        }
        let docx = self.render_docx(&doc)?;
        let out = match password {
            Some(pw) => dv_export::encrypt(&docx, pw).map_err(|e| match e {
                dv_export::ExportError::WeakPassword => CoreError::Refused(format!(
                    "סיסמה לקובץ צריכה להיות באורך {} תווים לפחות.",
                    dv_export::MIN_PASSWORD_CHARS
                )),
                other => CoreError::Internal(other.to_string()),
            })?,
            None => docx,
        };
        self.vault_mut()?.record(
            AuditEvent::Export,
            Some(case_id),
            &serde_json::json!({ "protected": password.is_some(), "letter": letter.key() }),
        )?;
        Ok(out)
    }

    pub fn report_settings(&mut self) -> Result<ReportSettings, CoreError> {
        let v = self.vault_ref()?;
        if let Some(json) = v.setting(REPORT_KEY)? {
            if let Ok(s) = serde_json::from_str(&json) {
                return Ok(s);
            }
        }
        Ok(ReportSettings {
            title: "דוח אבחון פסיכולוגי".to_owned(),
            font: "David".to_owned(),
            confidentiality: "חסוי – מידע רפואי. מיועד להורים ולמי שההורים אישרו בלבד.".to_owned(),
            signature: v
                .practitioner()?
                .names
                .first()
                .cloned()
                .into_iter()
                .collect(),
        })
    }

    pub fn set_report_settings(&mut self, settings: &ReportSettings) -> Result<(), CoreError> {
        let json =
            serde_json::to_string(settings).map_err(|e| CoreError::Internal(e.to_string()))?;
        self.vault_mut()?.set_setting(REPORT_KEY, &json)?;
        Ok(())
    }

    /// Her Word template, checked: refused with a sentence she can act on, or kept and
    /// described. Every report and letter after this is written into it.
    pub fn set_report_template(&mut self, bytes: &[u8]) -> Result<TemplateView, CoreError> {
        let view = template_view(bytes)?;
        self.vault_mut()?
            .set_setting(TEMPLATE_KEY, &dv_vault::crypto::hex(bytes))?;
        Ok(view)
    }

    /// Back to the plain report.
    pub fn clear_report_template(&mut self) -> Result<(), CoreError> {
        self.vault_mut()?.set_setting(TEMPLATE_KEY, "")?;
        Ok(())
    }

    pub fn report_template(&mut self) -> Result<Option<TemplateView>, CoreError> {
        self.template_bytes()?
            .map(|b| template_view(&b))
            .transpose()
    }

    pub(crate) fn template_bytes(&mut self) -> Result<Option<Vec<u8>>, CoreError> {
        match self.vault_ref()?.setting(TEMPLATE_KEY)? {
            Some(h) if !h.is_empty() => Ok(Some(
                dv_vault::crypto::unhex(&h).map_err(|e| CoreError::Internal(e.to_string()))?,
            )),
            _ => Ok(None),
        }
    }

    /// The Word file: in her template when she has one.
    pub(crate) fn render_docx(&mut self, report: &dv_export::Report) -> Result<Vec<u8>, CoreError> {
        let out = match self.template_bytes()? {
            Some(t) => dv_export::render_with_template(report, &t),
            None => dv_export::render(report),
        };
        out.map_err(|e| match e {
            dv_export::ExportError::Template(m) => CoreError::Refused(m),
            other => CoreError::Internal(other.to_string()),
        })
    }

    /// The report with real names, from approved paragraphs only, and what stops the export.
    pub(crate) fn build_report(
        &mut self,
        case_id: &str,
    ) -> Result<(dv_export::Report, ExportCheck), CoreError> {
        // A tag left by a name she brought back reads as its word, not as a block. A failure
        // leaves it, and the check below still blocks.
        let _ = self.settle_stale_tags(case_id, &HashMap::new());
        let settings = self.report_settings()?;
        let structure = structure_for(self.vault_ref()?, case_id)?;
        let v = self.vault_ref()?;
        let meta = v.case_meta(case_id)?;
        let identities = v.identities(case_id)?;
        let masks = v.masks(case_id)?;
        let practitioner = v.practitioner()?.names.first().cloned();
        let show = |t: &str| restore(t, &identities, practitioner.as_deref(), &masks);

        let mut blocking = Vec::new();
        let mut to_complete = Vec::new();
        let known_tags: HashSet<String> = v
            .all_identities()?
            .into_iter()
            .map(|i| i.tag)
            .chain(dv_privacy::GENERIC_TAGS.iter().map(|t| (*t).to_owned()))
            .collect();
        let mut empty_sections = Vec::new();
        let mut included = 0u32;
        let mut parts = Vec::new();
        let mut signature = settings.signature.clone();
        for part in &structure.parts {
            let mut sections = Vec::new();
            for s in &part.sections {
                let mut paragraphs: Vec<String> = v
                    .drafts(case_id, &s.key)?
                    .into_iter()
                    .filter(|d| d.status == DraftStatus::Approved)
                    .map(|d| show(&d.text_tagged))
                    .collect();
                for p in &mut paragraphs {
                    for tag in dv_privacy::restore::remaining_tags(p) {
                        if known_tags.contains(&tag) || dv_privacy::is_mask_tag(&tag) {
                            // A real tag the words could not be put back into (a person removed
                            // from the case, a draft from before the mask book): never a tag in
                            // a file, and no stop either. It becomes a marker to complete, and
                            // the finish screen lists it with the others (round 26).
                            let marker = format!("[חסר: {}]", dv_privacy::tag_word(&tag));
                            *p = p.replace(&tag, &marker);
                        } else {
                            // Words the model put in square brackets ("[מחנכת]"): plain text.
                            let inner = tag
                                .strip_prefix('[')
                                .and_then(|t| t.strip_suffix(']'))
                                .unwrap_or(&tag);
                            let plain = format!("({inner})");
                            *p = p.replace(&tag, &plain);
                        }
                    }
                    for (at, _) in p.match_indices("[חסר") {
                        // The marker up to its closing bracket, or to the end of the paragraph.
                        let rest = p.get(at..).unwrap_or_default();
                        let marker = rest.split_inclusive(']').next().unwrap_or(rest);
                        to_complete.push(format!("{}: {marker}", s.title));
                    }
                }
                if s.key == "signature" {
                    if !paragraphs.is_empty() {
                        signature = paragraphs
                            .iter()
                            .flat_map(|p| p.lines().map(str::to_owned))
                            .collect();
                    }
                    continue;
                }
                if paragraphs.is_empty() {
                    empty_sections.push(s.title.clone());
                } else {
                    included += 1;
                    sections.push(dv_export::ReportSection {
                        title: s.title.clone(),
                        paragraphs,
                    });
                }
            }
            if !sections.is_empty() {
                parts.push(dv_export::ReportPart {
                    title: part.title.clone(),
                    sections,
                });
            }
        }

        let child = identities
            .iter()
            .find(|i| i.role == Role::Child)
            .map(|i| i.value.clone());
        let child_label = match meta.child_gender {
            Some(dv_domain::GrammaticalGender::Female) => "שם הילדה",
            Some(dv_domain::GrammaticalGender::Male) => "שם הילד",
            None => "שם הילד/ה",
        };
        let (y, m, d) = today();
        let mut info = Vec::new();
        if let Some(name) = child {
            info.push(dv_export::InfoLine {
                label: child_label.to_owned(),
                value: name,
            });
        }
        if let Some(age) = meta.age {
            info.push(dv_export::InfoLine {
                label: "גיל בעת האבחון".to_owned(),
                value: age.display(),
            });
        }
        info.push(dv_export::InfoLine {
            label: "תאריך הדוח".to_owned(),
            value: format!("{d}.{m}.{y}"),
        });

        // Score tables entered in the table (not imported text), in the order they were added.
        let mut tables = Vec::new();
        for input in v
            .inputs(case_id)?
            .into_iter()
            .filter(|i| i.kind == InputKind::TestScores)
        {
            let Some(data) = v.input_data(case_id, &input.id)? else {
                continue;
            };
            let sheet: dv_domain::ScoreSheet =
                serde_json::from_str(&data).map_err(|e| CoreError::Internal(e.to_string()))?;
            let (title, rows, note) = dv_domain::sheet_table(&sheet).map_err(CoreError::Refused)?;
            let charts = dv_domain::sheet_profiles(&sheet)
                .map_err(CoreError::Refused)?
                .into_iter()
                .map(|p| dv_export::ScoreChart {
                    title: p.title,
                    min: p.min,
                    max: p.max,
                    step: p.step,
                    mean: p.mean,
                    sd: p.sd,
                    bars: p
                        .bars
                        .into_iter()
                        .map(|(label, value)| dv_export::ChartBar { label, value })
                        .collect(),
                    note: p.note,
                })
                .collect();
            tables.push(dv_export::ScoreTable {
                title,
                columns: ["מדד", "ציון", "אחוזון", "טווח"]
                    .map(str::to_owned)
                    .to_vec(),
                rows: rows
                    .into_iter()
                    .map(|r| vec![r.measure, r.score, r.percentile, r.range])
                    .collect(),
                note,
                charts,
            });
        }
        let score_tables = u32::try_from(tables.len()).unwrap_or(u32::MAX);

        let report = dv_export::Report {
            title: settings.title,
            info,
            parts,
            tables,
            signature,
            confidentiality: settings.confidentiality,
            font: settings.font,
        };
        for leftover in dv_export::leftover_placeholders(&report) {
            if leftover.starts_with("[חסר") {
                continue; // Pointed out in `to_complete`; hers to fill in.
            }
            let msg = format!("נשאר בדוח סימון בסוגריים מרובעים: {leftover}");
            if !blocking.iter().any(|b: &String| b.contains(&leftover)) {
                blocking.push(msg);
            }
        }
        if included == 0 {
            blocking.push(
                "אין עדיין פסקאות מאושרות. מאשרים פסקאות בטיוטה, והן נכנסות לדוח.".to_owned(),
            );
        }
        let code: String = meta
            .code
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect();
        let file_name = report_file_name(&code, &identities, dates::today());
        Ok((
            report,
            ExportCheck {
                blocking,
                to_complete,
                empty_sections,
                included_sections: included,
                score_tables,
                file_name,
            },
        ))
    }

    pub fn check_export(&mut self, case_id: &str) -> Result<ExportCheck, CoreError> {
        Ok(self.build_report(case_id)?.1)
    }
}
