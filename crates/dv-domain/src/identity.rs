//! People and places that may be mentioned in a case, and the stable tags that
//! replace them in anything that leaves the computer.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Tag used for the psychologist and the clinic in every case.
pub const PRACTITIONER_TAG: &str = "[מאבחנת]";

/// Who an identity is, relative to the child. Drives the tag and the UI label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Role {
    Child,
    Mother,
    Father,
    Brother,
    Sister,
    Teacher,
    Assistant,
    Doctor,
    Slp,
    Psychologist,
    Therapist,
    OtherChild,
    Kindergarten,
    School,
    Town,
    Institution,
    Other,
    /// A grandparent, aunt, cousin… ("סבתא שמחה").
    Relative,
    /// A school teacher ("המורה X"); a kindergarten teacher is `Teacher`.
    SchoolTeacher,
    /// A counsellor, social worker, principal… named in a document.
    Professional,
    /// A family name on its own ("משפחת X").
    Family,
}

impl Role {
    pub const ALL: [Role; 21] = [
        Role::Child,
        Role::Mother,
        Role::Father,
        Role::Brother,
        Role::Sister,
        Role::Teacher,
        Role::Assistant,
        Role::Doctor,
        Role::Slp,
        Role::Psychologist,
        Role::Therapist,
        Role::OtherChild,
        Role::Kindergarten,
        Role::School,
        Role::Town,
        Role::Institution,
        Role::Other,
        Role::Relative,
        Role::SchoolTeacher,
        Role::Professional,
        Role::Family,
    ];

    /// The word inside the tag, e.g. `ילד` in `[ילד]`.
    #[must_use]
    pub fn tag_base(self) -> &'static str {
        match self {
            Role::Child => "ילד",
            Role::Mother => "אם",
            Role::Father => "אב",
            Role::Brother => "אח",
            Role::Sister => "אחות",
            Role::Teacher => "גננת",
            Role::Assistant => "סייעת",
            Role::Doctor => "רופא",
            Role::Slp => "קלינאית",
            Role::Psychologist => "פסיכולוגית",
            Role::Therapist => "מטפלת",
            Role::OtherChild => "ילד_גן",
            Role::Kindergarten => "גן",
            Role::School => "בית_ספר",
            Role::Town => "יישוב",
            Role::Institution => "מוסד",
            Role::Other => "אדם",
            Role::Relative => "קרוב_משפחה",
            Role::SchoolTeacher => "מורה",
            Role::Professional => "איש_מקצוע",
            Role::Family => "משפחה",
        }
    }

    /// Hebrew label for the identity form.
    #[must_use]
    pub fn label_he(self) -> &'static str {
        match self {
            Role::Child => "הילד/ה",
            Role::Mother => "אם",
            Role::Father => "אב",
            Role::Brother => "אח",
            Role::Sister => "אחות",
            Role::Teacher => "גננת / מחנכת",
            Role::Assistant => "סייעת",
            Role::Doctor => "רופא/ה",
            Role::Slp => "קלינאית תקשורת",
            Role::Psychologist => "פסיכולוג/ית",
            Role::Therapist => "מטפל/ת",
            Role::OtherChild => "ילד/ה אחר/ת",
            Role::Kindergarten => "גן / מסגרת",
            Role::School => "בית ספר",
            Role::Town => "יישוב",
            Role::Institution => "מוסד",
            Role::Other => "אחר",
            Role::Relative => "קרוב/ת משפחה",
            Role::SchoolTeacher => "מורה",
            Role::Professional => "איש/אשת מקצוע",
            Role::Family => "משפחה",
        }
    }

    /// Roles that normally occur once per case get an unnumbered first tag (`[ילד]`).
    #[must_use]
    pub fn is_singleton(self) -> bool {
        matches!(
            self,
            Role::Child
                | Role::Mother
                | Role::Father
                | Role::Teacher
                | Role::Kindergarten
                | Role::Town
        )
    }
}

/// Where an identity came from. Every one is hidden and refused by the gate alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum IdentitySource {
    /// Entered or confirmed by the psychologist.
    #[default]
    Manual,
    /// Found by the filter in the case's text and hidden without asking.
    Auto,
    /// Taken from a document's properties or its header and footer.
    Metadata,
}

/// A declared identity. `value` and `aliases` never leave the computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Identity {
    pub id: String,
    pub case_id: String,
    pub role: Role,
    pub tag: String,
    pub value: String,
    pub aliases: Vec<String>,
    #[serde(default)]
    pub source: IdentitySource,
    /// Why the filter added it ("אחרי 'הגננת'"); empty for a manual identity.
    #[serde(default)]
    pub reason: String,
}

/// A name the filter hid without asking, kept from then on as an identity of the case: hidden
/// in every text of it and refused by the gate like a declared one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundName {
    pub value: String,
    pub role: Role,
    pub source: IdentitySource,
    pub reason: String,
}

/// What the identity form sends to the core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct IdentityInput {
    /// Present when editing an existing identity (keeps its tag stable).
    pub id: Option<String>,
    pub role: Role,
    pub value: String,
    pub aliases: Vec<String>,
}

/// The next free tag for `role`, given the tags already used in the case.
///
/// Singletons: `[ילד]`, then `[ילד_2]`. Others: `[רופא_1]`, `[רופא_2]`, …
#[must_use]
pub fn assign_tag(role: Role, used: &[String]) -> String {
    let base = role.tag_base();
    if role.is_singleton() {
        let first = format!("[{base}]");
        if !used.contains(&first) {
            return first;
        }
    }
    let start = if role.is_singleton() { 2 } else { 1 };
    (start..)
        .map(|n| format!("[{base}_{n}]"))
        .find(|tag| !used.contains(tag))
        .unwrap_or_else(|| format!("[{base}_x]"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn singleton_roles_get_plain_tag_first_then_numbered() {
        assert_eq!(assign_tag(Role::Child, &[]), "[ילד]");
        assert_eq!(assign_tag(Role::Teacher, &["[גננת]".into()]), "[גננת_2]");
    }

    #[test]
    fn numbered_roles_skip_used_numbers() {
        let used = vec!["[רופא_1]".to_owned(), "[רופא_2]".to_owned()];
        assert_eq!(assign_tag(Role::Doctor, &used), "[רופא_3]");
        assert_eq!(assign_tag(Role::Sister, &[]), "[אחות_1]");
    }

    #[test]
    fn every_role_has_a_distinct_tag_base() {
        let mut bases: Vec<&str> = Role::ALL.iter().map(|r| r.tag_base()).collect();
        bases.sort_unstable();
        bases.dedup();
        assert_eq!(bases.len(), Role::ALL.len());
    }
}
