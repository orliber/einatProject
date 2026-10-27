//! Reading a document into a case and the privacy filter's questions about it.

use super::*;

impl Core {
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
                    // A greeting ("שלום,", "לכבוד ד\"ר …,") names a letter worse than its file.
                    && !l.ends_with(',')
            })
            .map(str::to_owned);
        let stem = first_line.unwrap_or_else(|| {
            let stem = Path::new(file_name)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(file_name)
                .trim();
            // "05 מייל מההורים" → "מייל מההורים": the number only orders her folder.
            let unnumbered = stem
                .trim_start_matches(|c: char| c.is_ascii_digit())
                .trim_start_matches([' ', '-', '_', '.']);
            if unnumbered.is_empty() || unnumbered.len() == stem.len() {
                stem.to_owned()
            } else {
                unnumbered.to_owned()
            }
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
        if !case_id.is_empty() {
            // A case that is not there has no names to hide with: refused, not filtered bare.
            self.vault_ref()?.case_meta(case_id)?;
            // Learning is not needed here; a failure leaves the tag, and the gate blocks. Each
            // pass reads every stored text of the case: it is skipped while nothing was
            // written since the last one (the deep review, 4–6 passes per saved material).
            let counts = self.vault_ref()?.change_counts();
            let fresh = self
                .settled
                .as_ref()
                .is_some_and(|(id, at)| id == case_id && *at == counts);
            if !fresh && self.settle_stale_tags(case_id, &HashMap::new()).is_ok() {
                self.settled = Some((case_id.to_owned(), self.vault_ref()?.change_counts()));
            }
        }
        let v = self.vault_ref()?;
        Ok(PrivacyData {
            case_id: case_id.to_owned(),
            identities: v.all_identities()?,
            practitioner: v.practitioner()?.names,
            allow: v.not_a_name_hmacs(case_id)?.into_iter().collect(),
            is_name: v.is_name_hmacs(case_id)?.into_iter().collect(),
            past: style::past_name_hmacs(v)?,
            masks: dv_privacy::MaskBook::new(if case_id.is_empty() {
                Vec::new()
            } else {
                v.masks(case_id)?
            }),
        })
    }

    /// A tag in the case's stored text that no longer belongs to the case (a name she brought
    /// back, whose row is gone) would block every later request. It reads as the word she
    /// brought back when it is known (`known`: tag to word), else as the tag's plain word
    /// ("[מוסד_1]" becomes "מוסד"). What it becomes still goes through the filter and the
    /// gate like any text.
    pub(crate) fn settle_stale_tags(
        &mut self,
        case_id: &str,
        known: &HashMap<String, String>,
    ) -> Result<usize, CoreError> {
        let v = self.vault_mut()?;
        let tags: HashSet<String> = v.identities(case_id)?.into_iter().map(|i| i.tag).collect();
        let change = |text: &str| {
            dv_privacy::replace_foreign_tags(text, &tags, |t| {
                known
                    .get(t)
                    .cloned()
                    .unwrap_or_else(|| dv_privacy::tag_word(t))
            })
        };
        Ok(v.rewrite_case_texts(case_id, &change)?)
    }

    /// The button on a request stopped for a tag that is not the case's: every such tag in
    /// the case's text, its materials included, becomes its plain word. Returns how many
    /// texts changed; what remains is in what she is writing now.
    pub fn settle_foreign_tags(&mut self, case_id: &str) -> Result<usize, CoreError> {
        let mut changed = self.settle_stale_tags(case_id, &HashMap::new())?;
        let v = self.vault_mut()?;
        let tags: HashSet<String> = v.identities(case_id)?.into_iter().map(|i| i.tag).collect();
        let settle = |t: &str| dv_privacy::replace_foreign_tags(t, &tags, dv_privacy::tag_word);
        for input in v.inputs(case_id)? {
            let title = settle(&input.title);
            let content = settle(&input.content);
            if title.is_some() || content.is_some() {
                v.update_input(
                    case_id,
                    &input.id,
                    title.as_deref().unwrap_or(&input.title),
                    content.as_deref().unwrap_or(&input.content),
                )?;
                changed += 1;
            }
        }
        Ok(changed)
    }

    /// Filter one text for the case (used by the upload preview and the review screen).
    pub fn preview_filter(
        &mut self,
        case_id: &str,
        text: &str,
    ) -> Result<FilterOutcome, CoreError> {
        self.filter_hinted(case_id, text, &[])
    }

    /// [`Self::preview_filter`] with the name model's hints (none when it is off).
    fn filter_hinted(
        &mut self,
        case_id: &str,
        text: &str,
        hints: &[NameHint],
    ) -> Result<FilterOutcome, CoreError> {
        let data = self.privacy_data(case_id)?;
        let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
        let lookups = data.lookups(v);
        let ctx = lookups.context();
        filter_with_hints(text, &ctx, hints).map_err(|e| CoreError::Internal(e.to_string()))
    }

    /// Filter `text` for the case and keep the new names it found, then filter it again so
    /// they carry their kept tags (the outcome lists them, for the card). When the name model
    /// is on (D-053), its hints join both passes, and what it found is kept with the case.
    pub(crate) fn learn(&mut self, case_id: &str, text: &str) -> Result<FilterOutcome, CoreError> {
        let hints = self.name_hints(case_id, text);
        let first = self.filter_hinted(case_id, text, &hints)?;
        let data = self.privacy_data(case_id)?;
        let found = found_names(&data, &first.auto_hidden);
        if found.is_empty() {
            return Ok(first);
        }
        self.vault_mut()?.add_found_names(case_id, &found)?;
        self.filter_hinted(case_id, text, &hints)
    }

    /// Her own words, to store as a paragraph of the case (D-126): the new names are kept as
    /// in [`Self::learn`], and what the outgoing filter would leave out or turn into relative
    /// time (a kindergarten's name, a date) is kept in the case's mask book, so the paragraph
    /// shows and exports exactly as she wrote it. Returns the text to store.
    pub(crate) fn learn_to_keep(&mut self, case_id: &str, text: &str) -> Result<String, CoreError> {
        self.learn(case_id, text)?;
        let hints = self.name_hints(case_id, text);
        let data = self.privacy_data(case_id)?;
        let (tagged, added) = {
            let v = self.vault.as_ref().ok_or(CoreError::Locked)?;
            let lookups = data.lookups(v);
            let ctx = lookups.masked_context();
            let out = dv_privacy::filter_to_keep(text, &ctx, &hints)
                .map_err(|e| CoreError::Internal(e.to_string()))?;
            (out.tagged, data.masks.added())
        };
        self.vault_mut()?.add_masks(case_id, &added)?;
        Ok(tagged)
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
            masks: None,
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
        let tok = normalize(token);
        // A word the program kept as a name in another case (OTHER_CASE_TAG on the card):
        // it leaves that case's names, and is "not a name" there from now on.
        if tag == OTHER_CASE_TAG {
            let mut removed = Vec::new();
            for i in v.all_identities()?.into_iter().filter(|i| {
                i.case_id != case_id
                    && i.source != IdentitySource::Manual
                    && normalize(&i.value) == tok
            }) {
                v.remove_identity(&i.case_id, &i.id)?;
                v.mark_not_a_name(Some(&i.case_id), &tok)?;
                removed.push(i);
            }
            for i in removed {
                self.settle_stale_tags(&i.case_id, &[(i.tag, i.value)].into())?;
            }
            return Ok(());
        }
        let ids = v.identities(case_id)?;
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
        let mut removed = HashMap::new();
        for i in ids.iter().filter(|i| {
            i.source != IdentitySource::Manual
                && (i.tag == tag || Some(normalize(&i.value)) == value)
        }) {
            v.remove_identity(case_id, &i.id)?;
            removed.insert(i.tag.clone(), i.value.clone());
        }
        if let Some(value) = &value {
            v.mark_not_a_name(Some(case_id), value)?;
        }
        v.mark_not_a_name(Some(case_id), &normalize(token))?;
        // The paragraphs and conversations already written with its tag read the word again.
        self.settle_stale_tags(case_id, &removed)?;
        Ok(())
    }

    /// The card's role menu: who a name the filter kept is ("סבתא", "המורה"). The name gets a
    /// tag for that role; "להחזיר" still works on it.
    /// "נכון, תמיד להסתיר": a name the filter hid without asking is confirmed for the whole
    /// case (U2). The kept name becomes hers, like one she typed, and a close spelling becomes
    /// one of its forms, so neither is listed on the card again. Nothing is hidden less.
    pub fn confirm_auto_hidden(
        &mut self,
        case_id: &str,
        token: &str,
        tag: &str,
    ) -> Result<(), CoreError> {
        let v = self.vault_mut()?;
        let ids = v.identities(case_id)?;
        let Some(person) = ids.iter().find(|i| i.tag == tag) else {
            return Err(CoreError::Refused(
                "אפשר לאשר כאן רק שם של אדם מהתיק".to_owned(),
            ));
        };
        let tok = normalize(token);
        let known = std::iter::once(&person.value)
            .chain(person.aliases.iter())
            .any(|x| normalize(x) == tok);
        if !known && !token.trim().is_empty() {
            let inputs: Vec<IdentityInput> = ids
                .iter()
                .map(|i| IdentityInput {
                    id: Some(i.id.clone()),
                    role: i.role,
                    value: i.value.clone(),
                    aliases: if i.tag == tag {
                        i.aliases
                            .iter()
                            .cloned()
                            .chain(std::iter::once(token.trim().to_owned()))
                            .collect()
                    } else {
                        i.aliases.clone()
                    },
                })
                .collect();
            v.set_identities(case_id, &inputs)?;
        }
        if person.source != IdentitySource::Manual {
            v.confirm_identity(case_id, &person.id)?;
        }
        Ok(())
    }

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
