//! Sending a message to the family from her own device (D-201): WhatsApp, SMS or email.
//!
//! Nothing here sends anything. A channel only builds the address that opens the right app
//! on her computer (WhatsApp Desktop, the phone link, her mail program) at the family's
//! chat, and the filled text goes to the protected clipboard; she pastes and presses send
//! herself. The address carries the number or the email address only, never the text: an
//! address handed to another program can be seen in the computer's list of running programs.
//!
//! The way is open to more later (D-201): a provider that sends by itself (an SMS gateway,
//! the WhatsApp Business API, mail) would be another [`Delivery`], whose connection lives in
//! `dv-egress` like every other one and is turned on by her, with its price, first.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{Contact, Patient};
use crate::DomainError;

/// How a message reaches the family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Channel {
    Whatsapp,
    Sms,
    Email,
    /// A value this version does not know: refused.
    #[serde(other)]
    Unknown,
}

/// Who presses "send". Today only she does, on her device; an automatic provider would be a
/// second variant, behind its own switch, cost approval and egress rule (D-201).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Delivery {
    /// The app opens the chat on her device; the text waits on the protected clipboard.
    #[default]
    OnHerDevice,
}

/// A message ready for her: the filled text, and the address that opens the chat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Composed {
    pub text: String,
    pub link: String,
    pub delivery: Delivery,
    /// The contact it goes to, as she wrote them (for the line under the button).
    pub to: String,
}

/// What the window hears after the chat was opened on her device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Handed {
    /// The contact the chat was opened with.
    pub to: String,
    /// Seconds the text stays on the protected clipboard.
    pub clears_in: u32,
}

fn refuse<T>(text: &str) -> Result<T, DomainError> {
    Err(DomainError::Invalid(text.to_owned()))
}

/// A phone number as WhatsApp and SMS take it: country code and digits, no `+`.
/// An Israeli number written locally (`05X-XXXXXXX`, `0X-XXXXXXX`) gets `972`; a number
/// with `+` or `00` keeps its own country code. `None` when it is not a phone number.
#[must_use]
pub fn international_phone(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let plus = trimmed.starts_with('+');
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | ' ' | '(' | ')' | '.'))
        || trimmed.matches('+').count() > usize::from(plus)
    {
        return None;
    }
    let digits: String = trimmed.chars().filter(char::is_ascii_digit).collect();
    let full = if plus {
        digits
    } else if let Some(rest) = digits.strip_prefix("00") {
        rest.to_owned()
    } else if let Some(rest) = digits.strip_prefix('0') {
        format!("972{rest}")
    } else if digits.starts_with("972") {
        digits
    } else {
        return None;
    };
    // E.164: at most 15 digits; an Israeli number is 11 or 12 with its code.
    let israeli_ok = !full.starts_with("972") || (11..=12).contains(&full.len());
    ((8..=15).contains(&full.len()) && !full.starts_with('0') && israeli_ok).then_some(full)
}

/// An email address plain enough to put in a `mailto:` address as it is: one `@`, a dot
/// in the domain, and only letters, digits and `.`, `_`, `-`, `+` (nothing a mail program
/// could read as a second field). `None` otherwise.
#[must_use]
pub fn plain_email(raw: &str) -> Option<String> {
    let email = raw.trim();
    let (local, domain) = email.split_once('@')?;
    let ok_part = |s: &str, extra: &[char]| {
        !s.is_empty()
            && !s.starts_with('.')
            && !s.ends_with('.')
            && !s.contains("..")
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || extra.contains(&c))
    };
    (email.len() <= 254
        && ok_part(local, &['_', '+'])
        && ok_part(domain, &[])
        && domain.contains('.'))
    .then(|| email.to_owned())
}

/// The address that opens the chat with this contact. The text never goes in it.
pub fn chat_link(channel: Channel, contact: &Contact) -> Result<String, DomainError> {
    match channel {
        Channel::Whatsapp => match international_phone(&contact.phone) {
            Some(phone) => Ok(format!("whatsapp://send?phone={phone}")),
            None => {
                refuse("למספר הטלפון של איש הקשר אי אפשר לשלוח בוואטסאפ. כדאי לבדוק אותו בכרטיס.")
            }
        },
        Channel::Sms => match international_phone(&contact.phone) {
            Some(phone) => Ok(format!("sms:+{phone}")),
            None => refuse("למספר הטלפון של איש הקשר אי אפשר לשלוח SMS. כדאי לבדוק אותו בכרטיס."),
        },
        Channel::Email => match plain_email(&contact.email) {
            Some(email) => Ok(format!("mailto:{email}")),
            None => refuse("כתובת המייל של איש הקשר לא תקינה. כדאי לבדוק אותה בכרטיס."),
        },
        Channel::Unknown => refuse("דרך השליחה לא מוכרת בגרסה הזו."),
    }
}

/// The address is exactly one that [`chat_link`] makes: the shell's last check before it
/// hands an address to another program.
#[must_use]
pub fn is_chat_link(link: &str) -> bool {
    if let Some(phone) = link.strip_prefix("whatsapp://send?phone=") {
        return international_phone(&format!("+{phone}")).as_deref() == Some(phone);
    }
    if let Some(phone) = link.strip_prefix("sms:+") {
        return international_phone(&format!("+{phone}")).as_deref() == Some(phone);
    }
    if let Some(email) = link.strip_prefix("mailto:") {
        return plain_email(email).as_deref() == Some(email);
    }
    false
}

/// The contact a message goes to: the one she picked (by its place on the card), or else
/// the first one with what the channel needs.
pub fn pick_contact(
    patient: &Patient,
    channel: Channel,
    picked: Option<u8>,
) -> Result<Contact, DomainError> {
    let usable = |c: &Contact| match channel {
        Channel::Whatsapp | Channel::Sms => !c.phone.trim().is_empty(),
        Channel::Email => !c.email.trim().is_empty(),
        Channel::Unknown => false,
    };
    let found = match picked {
        Some(i) => patient.contacts.get(usize::from(i)).filter(|c| usable(c)),
        None => patient.contacts.iter().find(|c| usable(c)),
    };
    match (found, channel) {
        (Some(c), _) => Ok(c.clone()),
        (None, Channel::Email) => {
            refuse("בכרטיס אין כתובת מייל של איש קשר. אפשר להוסיף אותה בכרטיס.")
        }
        (None, _) => refuse("בכרטיס אין מספר טלפון של איש קשר. אפשר להוסיף אותו בכרטיס."),
    }
}

/// The text a message is addressed with: the contact's own name when it has one.
#[must_use]
pub fn contact_label(contact: &Contact) -> String {
    let name = contact.name.trim();
    if name.is_empty() {
        "איש הקשר".to_owned()
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn contact(phone: &str, email: &str) -> Contact {
        Contact {
            name: "דנה בדויה".to_owned(),
            phone: phone.to_owned(),
            email: email.to_owned(),
            ..Contact::default()
        }
    }

    #[test]
    fn israeli_numbers_get_their_country_code() {
        assert_eq!(
            international_phone("060-1234567").as_deref(),
            Some("972601234567")
        );
        assert_eq!(
            international_phone("06 123 4567").as_deref(),
            Some("97261234567")
        );
        assert_eq!(
            international_phone("+972 60 123 4567").as_deref(),
            Some("972601234567")
        );
        assert_eq!(
            international_phone("00972601234567").as_deref(),
            Some("972601234567")
        );
        assert_eq!(
            international_phone("+1 (212) 555-0100").as_deref(),
            Some("12125550100")
        );
        for bad in [
            "",
            "abc",
            "05012",
            "+972 5",
            "1234567",
            "060-1234567&text=x",
            "+97+2501234567",
            "0601234567890123",
        ] {
            assert_eq!(international_phone(bad), None, "{bad}");
        }
    }

    #[test]
    fn email_must_be_plain() {
        assert_eq!(
            plain_email(" a.b+c@mail.example.com ").as_deref(),
            Some("a.b+c@mail.example.com")
        );
        for bad in [
            "",
            "a@b",
            "a@@example.com",
            "a b@example.com",
            "a@example.com?subject=x",
            "a@example.com&body=x",
            ".a@example.com",
            "a@b..example.com",
            "שם@example.com",
        ] {
            assert_eq!(plain_email(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_link_carries_the_address_only() {
        let c = contact("060-1234567", "parent@example.com");
        assert_eq!(
            chat_link(Channel::Whatsapp, &c).unwrap(),
            "whatsapp://send?phone=972601234567"
        );
        assert_eq!(chat_link(Channel::Sms, &c).unwrap(), "sms:+972601234567");
        assert_eq!(
            chat_link(Channel::Email, &c).unwrap(),
            "mailto:parent@example.com"
        );
        assert!(chat_link(Channel::Unknown, &c).is_err());
        assert!(chat_link(Channel::Email, &contact("060-1234567", "")).is_err());
    }

    #[test]
    fn only_our_own_links_pass_the_last_check() {
        let c = contact("060-1234567", "parent@example.com");
        for ch in [Channel::Whatsapp, Channel::Sms, Channel::Email] {
            assert!(is_chat_link(&chat_link(ch, &c).unwrap()));
        }
        for bad in [
            "https://wa.me/972601234567",
            "whatsapp://send?phone=972601234567&text=hi",
            "sms:+972601234567?body=hi",
            "mailto:parent@example.com?body=hi",
            "file:///c:/windows",
            "whatsapp://send?phone=",
            "mailto:",
        ] {
            assert!(!is_chat_link(bad), "{bad}");
        }
    }

    #[test]
    fn the_contact_is_the_picked_one_or_the_first_that_fits() {
        let p = Patient {
            contacts: vec![contact("", "a@example.com"), contact("060-1234567", "")],
            ..Patient::default()
        };
        assert_eq!(
            pick_contact(&p, Channel::Whatsapp, None).unwrap().phone,
            "060-1234567"
        );
        assert_eq!(
            pick_contact(&p, Channel::Email, None).unwrap().email,
            "a@example.com"
        );
        assert!(pick_contact(&p, Channel::Whatsapp, Some(0)).is_err());
        assert!(pick_contact(&p, Channel::Sms, Some(9)).is_err());
        assert!(pick_contact(&Patient::default(), Channel::Sms, None).is_err());
    }
}
