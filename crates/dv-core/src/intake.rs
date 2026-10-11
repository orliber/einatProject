//! Reading a document into a case and the privacy filter's questions about it.

use super::*;

impl Core {
    /// Read a document: in the isolated worker when the app set one, then OCR for a scan.
    pub(crate) fn read_document(
        &self,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<dv_ingest::Extracted, CoreError> {
        dv_ingest::import(
            self.ingest_exe.as_deref(),
            self.ocr.as_ref(),
            file_name,
            bytes,
        )
        .map_err(|e| CoreError::Refused(e.message_he()))
    }

    /// Read a document and show what would be stored and hidden. Nothing is saved until the
    /// psychologist confirms (then the UI calls [`Core::add_input`] with the reviewed text).
    pub fn import_document(
        &mut self,
        case_id: &str,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<ImportPreview, CoreError> {
        self.vault_ref()?.case_meta(case_id)?;
        let extracted = self.read_document(file_name, bytes)?;
        self.preview_extracted(case_id, file_name, extracted)
    }

    /// The upload preview for a document already read (OCR'd text included).
    pub(crate) fn preview_extracted(
        &mut self,
        case_id: &str,
        file_name: &str,
        extracted: dv_ingest::Extracted,
    ) -> Result<ImportPreview, CoreError> {
        // Names in the margins and the file's properties are not imported, but they are the
        // names most likely to appear in the body too: kept with the case first, so the body
        // hides them under their tags (fail-closed: if the import is cancelled, they stay
        // hidden, which costs nothing).
        let mut found: Vec<FoundName> = Vec::new();
        let keep = |value: &str, role: Role, reason: String, found: &mut Vec<FoundName>| {
            let value = value.trim();
            if person_like(value)
                && !found
                    .iter()
                    .any(|f| normalize(&f.value) == normalize(value))
            {
                found.push(FoundName {
                    value: value.to_owned(),
                    role,
                    source: IdentitySource::Metadata,
                    reason,
                });
            }
        };
        for (field, value) in &extracted.metadata {
            let (label, role) = match field.as_str() {
                "creator" => ("יוצר המסמך", Role::Other),
                "lastModifiedBy" => ("שמר לאחרונה", Role::Other),
                "Manager" => ("מנהל", Role::Other),
                "Company" => ("ארגון", Role::Institution),
                _ => continue,
            };
            keep(value, role, format!("מאפייני הקובץ · {label}"), &mut found);
        }
        let mut texts = vec![(extracted.margins.clone(), "כותרת עליונה/תחתונה".to_owned())];
        for (field, value) in &extracted.metadata {
            if matches!(
                field.as_str(),
                "title" | "subject" | "keywords" | "description"
            ) {
                texts.push((value.clone(), "מאפייני הקובץ · כותרת".to_owned()));
            }
        }
        for (text, reason) in &texts {
            for a in self.preview_filter(case_id, text)?.auto_hidden {
                if matches!(a.kind, AutoKind::Name | AutoKind::OtherCase) {
                    keep(&a.token, a.role, reason.clone(), &mut found);
                }
            }
        }
        let practitioner: Vec<String> = self
            .vault_ref()?
            .practitioner()?
            .names
            .iter()
            .map(|n| normalize(n))
            .collect();
        found.retain(|f| !practitioner.contains(&normalize(&f.value)));
        let mut auto_hidden: Vec<AutoHidden> = self
            .vault_mut()?
            .add_found_names(case_id, &found)?
            .into_iter()
            .map(|i| AutoHidden {
                token: i.value,
                tag: i.tag,
                role: i.role,
                reason: i.reason,
                uncertain: false,
                kind: AutoKind::Name,
            })
            .collect();
        // A scan can misread a name by a letter ("אלוו" for "אלון"): kept as that name's
        // spelling first, so the body hides it under the name's tag.
        if extracted.ocr {
            auto_hidden.extend(self.learn_ocr_misreads(case_id, &extracted.body)?);
        }
        let body = self.learn(case_id, &extracted.body)?;
        for a in &body.auto_hidden {
            if !auto_hidden.iter().any(|x| x.tag == a.tag) {
                auto_hidden.push(a.clone());
            }
        }

        // The document's own first line ("דוח קלינאית תקשורת – אבחון שפתי") names it better
        // than a file name ("slp2").
        let first_line = extracted
            .body
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .filter(|l| {
                (3..=80).contains(&l.chars().count())
                    && l.chars().filter(char::is_ascii_digit).count() < 6
                    && !l.trim_end_matches('.').contains(". ")
            })
            .map(str::to_owned);
        let stem = first_line.unwrap_or_else(|| {
            Path::new(file_name)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(file_name)
                .trim()
                .to_owned()
        });
        Ok(ImportPreview {
            file_name: file_name.to_owned(),
            format: extracted.format.name().to_owned(),
            pages: extracted.pages,
            title: stem,
            suggested_kind: InputKind::guess(&extracted.body, file_name),
            preview: body.original_segments,
            suspects: Vec::new(),
            auto_hidden,
            hidden: body.hidden,
            body: extracted.body,
            left_out: extracted
                .margins
                .lines()
                .map(str::to_owned)
                .filter(|l| !l.trim().is_empty())
                .collect(),
            name_suggestions: Vec::new(),
            warnings: extracted.warnings,
        })
    }

    pub(crate) fn privacy_data(&mut self, case_id: &str) -> Result<PrivacyData, CoreError> {
        let v = self.vault_ref()?;
        Ok(PrivacyData {
            case_id: case_id.to_owned(),
            identities: v.all_identities()?,
            practitioner: v.practitioner()?.names,
            allow: v.not_a_name_hmacs(case_id)?.into_iter().collect(),
            is_name: v.is_name_hmacs(case_id)?.into_iter().collect(),
            past: style::past_name_hmacs(v)?,
        })
    }

    /// Filter one text for the case (used by the upload preview and the review screen).
    pub fn preview_filter(
        &mut self,
        case_id: &str,
        text: &str,
    ) -> Result<FilterOutcome, CoreError> {
        let data = self.privacy_data(case_id)?;
        let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
        let allow = |t: &str| data.allow.contains(&v.token_hmac(t));
        let is_name = |t: &str| data.is_name.contains(&v.token_hmac(t));
        let past = |t: &str| !data.past.is_empty() && data.past.contains(&v.token_hmac(t));
        let ctx = PrivacyContext {
            case_id: &data.case_id,
            identities: &data.identities,
            practitioner: &data.practitioner,
            allowlisted: &allow,
            confirmed_names: &is_name,
            past_names: &past,
            today: today(),
        };
        filter(text, &ctx).map_err(|e| CoreError::Internal(e.to_string()))
    }

    /// Filter `text` for the case and keep the new names it found, then filter it again so
    /// they carry their kept tags (the outcome lists them, for the card).
    pub(crate) fn learn(&mut self, case_id: &str, text: &str) -> Result<FilterOutcome, CoreError> {
        let first = self.preview_filter(case_id, text)?;
        let data = self.privacy_data(case_id)?;
        let found = found_names(&data, &first.auto_hidden);
        if found.is_empty() {
            return Ok(first);
        }
        self.vault_mut()?.add_found_names(case_id, &found)?;
        self.preview_filter(case_id, text)
    }

    /// Scanned text (OCR) can misread a name by one letter ("אלוו" for "אלון"). Call this for
    /// OCR-sourced text only, before the text is filtered: each misread is kept as one more
    /// spelling of that name, so every later text hides it and the gate refuses it (D-048).
    /// Returned for the card, where "להחזיר" takes the spelling off again.
    pub fn learn_ocr_misreads(
        &mut self,
        case_id: &str,
        text: &str,
    ) -> Result<Vec<AutoHidden>, CoreError> {
        let data = self.privacy_data(case_id)?;
        let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
        let allow = |t: &str| data.allow.contains(&v.token_hmac(t));
        let none = |_: &str| false;
        let ctx = PrivacyContext {
            case_id: &data.case_id,
            identities: &data.identities,
            practitioner: &data.practitioner,
            allowlisted: &allow,
            confirmed_names: &none,
            past_names: &none,
            today: today(),
        };
        let misreads = ocr_misreads(text, &ctx);
        if misreads.is_empty() {
            return Ok(Vec::new());
        }
        let v = self.vault_mut()?;
        let mut ids = v.identities(case_id)?;
        let mut out = Vec::new();
        for m in misreads {
            let Some(i) = ids.iter_mut().find(|i| i.tag == m.tag) else {
                continue;
            };
            i.aliases.push(m.written.clone());
            out.push(AutoHidden {
                token: m.written,
                tag: m.tag,
                role: i.role,
                reason: format!("בסריקה, אות אחת שונה מ-{}", i.value),
                uncertain: true,
                kind: AutoKind::SimilarSpelling,
            });
        }
        let ids: Vec<IdentityInput> = ids
            .into_iter()
            .map(|i| IdentityInput {
                role: i.role,
                id: Some(i.id),
                value: i.value,
                aliases: i.aliases,
            })
            .collect();
        v.set_identities(case_id, &ids)?;
        Ok(out)
    }

    /// "להחזיר" on the summary card: from now on this case keeps `token` as it is written.
    /// A name the filter kept with the case (under `tag`) is dropped from its names.
    pub fn restore_auto_hidden(
        &mut self,
        case_id: &str,
        token: &str,
        tag: &str,
    ) -> Result<(), CoreError> {
        let v = self.vault_mut()?;
        let ids = v.identities(case_id)?;
        let tok = normalize(token);
        // A spelling of the name rather than the name ("אלוו", kept from a scan, or "נואם"
        // next to "נועם"): only that spelling comes back, the name stays hidden.
        if !ids.iter().any(|i| {
            let value = normalize(&i.value);
            i.tag == tag && (value == tok || value.split(' ').any(|w| w == tok))
        }) && ids.iter().any(|i| i.tag == tag)
        {
            let ids: Vec<IdentityInput> = ids
                .into_iter()
                .map(|i| IdentityInput {
                    aliases: if i.tag == tag {
                        i.aliases
                            .into_iter()
                            .filter(|a| normalize(a) != tok)
                            .collect()
                    } else {
                        i.aliases
                    },
                    role: i.role,
                    id: Some(i.id),
                    value: i.value,
                })
                .collect();
            v.set_identities(case_id, &ids)?;
            v.mark_not_a_name(Some(case_id), &tok)?;
            return Ok(());
        }
        let value = ids
            .iter()
            .find(|i| i.tag == tag && i.source != IdentitySource::Manual)
            .map(|i| normalize(&i.value));
        // Every row the filter kept for that name (a role change keeps the earlier row, so
        // drafts with its tag can still be restored).
        for i in ids.iter().filter(|i| {
            i.source != IdentitySource::Manual
                && (i.tag == tag || Some(normalize(&i.value)) == value)
        }) {
            v.remove_identity(case_id, &i.id)?;
        }
        if let Some(value) = &value {
            v.mark_not_a_name(Some(case_id), value)?;
        }
        v.mark_not_a_name(Some(case_id), &normalize(token))?;
        Ok(())
    }

    /// The card's role menu: who a name the filter kept is ("סבתא", "המורה"). The name gets a
    /// tag for that role; "להחזיר" still works on it.
    pub fn change_role(
        &mut self,
        case_id: &str,
        tag: &str,
        role: Role,
    ) -> Result<Vec<dv_domain::Identity>, CoreError> {
        let v = self.vault_mut()?;
        let ids: Vec<IdentityInput> = v
            .identities(case_id)?
            .into_iter()
            .map(|i| IdentityInput {
                role: if i.tag == tag { role } else { i.role },
                id: Some(i.id),
                value: i.value,
                aliases: i.aliases,
            })
            .collect();
        Ok(v.set_identities(case_id, &ids)?)
    }

    pub fn decide_suspect(
        &mut self,
        case_id: &str,
        token: &str,
        decision: SuspectDecision,
    ) -> Result<(), CoreError> {
        let v = self.vault_mut()?;
        match decision {
            SuspectDecision::Hide { role } => {
                let mut ids: Vec<IdentityInput> = v
                    .identities(case_id)?
                    .into_iter()
                    .map(|i| IdentityInput {
                        id: Some(i.id),
                        role: i.role,
                        value: i.value,
                        aliases: i.aliases,
                    })
                    .collect();
                ids.push(IdentityInput {
                    id: None,
                    role,
                    value: token.trim().to_owned(),
                    aliases: Vec::new(),
                });
                v.set_identities(case_id, &ids)?;
            }
            SuspectDecision::IsName => v.mark_is_name(case_id, &normalize(token))?,
            // Per case: a word that is ordinary here may be a name in another case.
            SuspectDecision::NotAName => v.mark_not_a_name(Some(case_id), &normalize(token))?,
        }
        Ok(())
    }
}
