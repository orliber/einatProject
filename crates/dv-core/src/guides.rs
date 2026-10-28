//! How each section is written (round 29): its guide. The general guide is the institute's
//! text from the template until she edits it in settings; a case can add lines of its own.
//! The whole guide goes out with every drafting request of the section, filtered like her
//! request, and each paragraph says which line it answers, so a line nobody wrote is shown as
//! a missing paragraph. Until she writes her own, the general guide is the order she writes
//! the section in, when it was learned from her past reports (K3).

use super::*;

/// Settings key prefix of her own general guide for a section: `guide/cognitive`.
const GUIDE_PREFIX: &str = "guide/";
/// The longest guide, general or a case's addition (characters).
const GUIDE_MAX: usize = 4_000;

/// A section's guide as it applies to one case.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct SectionGuide {
    /// Hers from settings, or the template's.
    pub general: String,
    /// `general` is the template's text, not edited.
    pub is_default: bool,
    /// `general` is the order she writes the section in, learned from this many of her
    /// reports (K3), in place of the template's text. 0: not learned.
    pub learned_from: u32,
    /// This case's own lines.
    pub case: String,
}

impl SectionGuide {
    /// The whole guide: the general lines, then the case's.
    pub(crate) fn text(&self) -> String {
        [self.general.trim(), self.case.trim()]
            .into_iter()
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// The guide of `section` in a case with `meta`.
pub(crate) fn guide_for(
    v: &Vault,
    meta: &CaseMeta,
    section: &dv_domain::ReportSection,
) -> Result<SectionGuide, CoreError> {
    let stored = v
        .setting(&format!("{GUIDE_PREFIX}{}", section.key))?
        .filter(|t| !t.trim().is_empty());
    // Until she writes her own, the order she writes the section in, when it was learned
    // from her reports (K3, by the child's age band, K13); else the institute's text.
    let learned = if stored.is_none() {
        crate::style::style_skeleton(v, &section.key, meta.age)?
    } else {
        None
    };
    let learned_from = learned.as_ref().map_or(0, |k| k.reports);
    Ok(SectionGuide {
        is_default: stored.is_none(),
        learned_from,
        general: stored
            .or_else(|| learned.map(|k| k.guide()))
            .unwrap_or_else(|| section.guide.clone()),
        case: meta
            .guides
            .as_ref()
            .and_then(|g| g.get(&section.key))
            .cloned()
            .unwrap_or_default(),
    })
}

/// A guide as she typed it: one kind of line break, no trailing space, within the limit.
fn clean(text: &str) -> Result<String, CoreError> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let text = text.trim().to_owned();
    if text.chars().count() > GUIDE_MAX {
        return Err(CoreError::Refused(format!(
            "ההנחיה ארוכה מדי: עד {GUIDE_MAX} תווים."
        )));
    }
    Ok(text)
}

/// A section of either template (a case from before round 28 is in the old one).
pub(crate) fn known_section(key: &str) -> bool {
    [
        Some(dv_domain::DEFAULT_TEMPLATE),
        Some(dv_domain::INSTITUTE_TEMPLATE),
    ]
    .into_iter()
    .filter_map(|t| ReportStructure::for_template(t).ok())
    .any(|s| s.section(key).is_some())
}

impl Core {
    /// Her general guide for a section, for every case. Empty: back to the institute's text.
    pub fn set_section_guide(&mut self, section_key: &str, text: &str) -> Result<(), CoreError> {
        if !known_section(section_key) {
            return Err(CoreError::NotFound("הסעיף".to_owned()));
        }
        let text = clean(text)?;
        self.vault_mut()?
            .set_setting(&format!("{GUIDE_PREFIX}{section_key}"), &text)?;
        Ok(())
    }

    /// Lines added to a section's guide for one case only. Empty: none.
    pub fn set_case_guide(
        &mut self,
        case_id: &str,
        section_key: &str,
        text: &str,
    ) -> Result<(), CoreError> {
        let v = self.vault_ref()?;
        if structure_for(v, case_id)?.section(section_key).is_none() {
            return Err(CoreError::NotFound("הסעיף".to_owned()));
        }
        let text = clean(text)?;
        let mut meta = v.case_meta(case_id)?;
        let mut guides = meta.guides.take().unwrap_or_default();
        if text.is_empty() {
            guides.remove(section_key);
        } else {
            guides.insert(section_key.to_owned(), text);
        }
        meta.guides = (!guides.is_empty()).then_some(guides);
        Ok(self.vault_mut()?.update_case_meta(case_id, &meta)?)
    }
}
