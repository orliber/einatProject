//! A one-way copy of the clinic's calendar to her Google Calendar (D-202). Off by default.
//!
//! The app makes one calendar of its own in her account and keeps its events equal to her
//! planned and held visits. Scope `calendar.app.created`: the app sees and touches only the
//! calendars it made itself, none of her others.
//!
//! Rules, as for every other connection in this crate:
//! * What an event says is fixed here, not by the caller: one of [`TITLES`], a start and an
//!   end (`YYYY-MM-DDTHH:MM:00`), the time zone [`TIME_ZONE`], private, and an id of
//!   [`EVENT_ID_LEN`] characters the caller made with a keyed hash. No description, no
//!   place, no guests, no colour, no extended properties. The calendar's own name is
//!   [`CALENDAR_NAME`].
//! * Only [`crate::google::GOOGLE_HOSTS`], through [`crate::google::Http`] (TLS 1.3, no
//!   proxy, no redirects); only these calls: make the calendar, read it, list its events,
//!   add, replace or delete one event, delete the calendar. A PUT goes only to one event's
//!   address ([`event_path_allowed`]), checked again by `GoogleHttp` itself.
//! * A calendar id or an event id from Google's answer is used only after it passed a strict
//!   shape; an answer with a second page is an error, not "no events".

use serde_json::Value;
use zeroize::Zeroizing;

use crate::google::{json, GoogleError, Http, Method, Reply, Request, Session};

const API: &str = "https://www.googleapis.com/calendar/v3";
const CALENDARS_PATH: &str = "/calendar/v3/calendars/";
const GROUP_SUFFIX: &str = "@group.calendar.google.com";
/// The calendar's name in her account. Says nothing about her work or anyone in it.
pub const CALENDAR_NAME: &str = "פגישות";
/// The time zone of the calendar and of every event.
pub const TIME_ZONE: &str = "Asia/Jerusalem";
/// The only titles an event may have: a visit, and time she blocked for herself.
pub const TITLES: &[&str] = &["פגישה", "לא פנויה"];
/// The app's event ids: this prefix and 32 hex digits (a keyed hash of the visit id). Other
/// events in the calendar (one she added there by hand) are never touched.
pub const EVENT_PREFIX: &str = "dv";
pub const EVENT_ID_LEN: usize = 34;
/// More events than a calendar of half a year can hold; more is refused, not cut.
const MAX_EVENTS: &str = "2500";

/// One event as the app writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub id: String,
    /// Local wall time in [`TIME_ZONE`], `YYYY-MM-DDTHH:MM:00`.
    pub start: String,
    pub end: String,
    pub title: String,
}

/// One event as Google holds it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub id: String,
    /// `cancelled` once deleted (Google keeps the id; an event added again under it is a
    /// replace, not an insert).
    pub cancelled: bool,
    /// As the app would write it, or `None` when it says something else.
    pub event: Option<Event>,
}

/// What one round of the copy changed at Google.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Applied {
    pub added: u32,
    pub changed: u32,
    pub removed: u32,
}

/// The id of one of the app's events.
#[must_use]
pub fn event_id_ok(id: &str) -> bool {
    id.len() == EVENT_ID_LEN
        && id.strip_prefix(EVENT_PREFIX).is_some_and(|hex| {
            hex.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
}

/// The id of a calendar Google made: `<letters, digits, _>@group.calendar.google.com`.
#[must_use]
pub fn calendar_id_ok(id: &str) -> bool {
    id.strip_suffix(GROUP_SUFFIX).is_some_and(|head| {
        !head.is_empty()
            && head.len() <= 160
            && head
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    })
}

/// `YYYY-MM-DDTHH:MM:00`, digits where digits go.
fn wall_time_ok(t: &str) -> bool {
    let b = t.as_bytes();
    b.len() == 19
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            10 => *c == b'T',
            13 | 16 => *c == b':',
            17 | 18 => *c == b'0',
            _ => c.is_ascii_digit(),
        })
}

fn event_ok(e: &Event) -> bool {
    event_id_ok(&e.id)
        && wall_time_ok(&e.start)
        && wall_time_ok(&e.end)
        && e.start < e.end
        && TITLES.contains(&e.title.as_str())
}

/// A PUT may go here: one event of a calendar, nothing more in the address (D-202).
pub(crate) fn event_path_allowed(url: &reqwest::Url) -> bool {
    if url.query().is_some() || url.fragment().is_some() {
        return false;
    }
    let Some(rest) = url.path().strip_prefix(CALENDARS_PATH) else {
        return false;
    };
    let mut parts = rest.split('/');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(cal), Some("events"), Some(id), None) => {
            calendar_id_ok(&cal.replace("%40", "@")) && event_id_ok(id)
        }
        _ => false,
    }
}

fn protocol(text: &str) -> GoogleError {
    GoogleError::Protocol(text.to_owned())
}

fn send(
    http: &dyn Http,
    s: &Session,
    method: Method,
    url: String,
    body: Option<&Value>,
) -> Result<Reply, GoogleError> {
    http.send(Request {
        method,
        url,
        bearer: Some(s.token.clone()),
        content_type: body.map(|_| "application/json; charset=UTF-8".to_owned()),
        body: Zeroizing::new(body.map(|b| b.to_string().into_bytes()).unwrap_or_default()),
        timeout: None,
    })
}

fn calendar_url(calendar: &str) -> Result<String, GoogleError> {
    if !calendar_id_ok(calendar) {
        return Err(protocol("calendar id"));
    }
    Ok(format!("{API}/calendars/{calendar}"))
}

/// Make the app's calendar in her account. Returns its id.
pub fn create_calendar(http: &dyn Http, s: &Session) -> Result<String, GoogleError> {
    let body = serde_json::json!({ "summary": CALENDAR_NAME, "timeZone": TIME_ZONE });
    let Reply { status, body, .. } = send(
        http,
        s,
        Method::Post,
        format!("{API}/calendars"),
        Some(&body),
    )?;
    json(status, &body)?
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| calendar_id_ok(id))
        .map(str::to_owned)
        .ok_or_else(|| protocol("no calendar id"))
}

/// Whether the app's calendar is still there (she may have deleted it in Google Calendar).
pub fn calendar_exists(http: &dyn Http, s: &Session, calendar: &str) -> Result<bool, GoogleError> {
    let url = format!("{}?fields=id", calendar_url(calendar)?);
    let Reply { status, body, .. } = send(http, s, Method::Get, url, None)?;
    if status == 404 || status == 410 {
        return Ok(false);
    }
    let v = json(status, &body)?;
    Ok(v.get("id").and_then(Value::as_str) == Some(calendar))
}

/// Delete the app's calendar with every event in it. One already gone counts as deleted.
pub fn delete_calendar(http: &dyn Http, s: &Session, calendar: &str) -> Result<(), GoogleError> {
    let Reply { status, .. } = send(http, s, Method::Delete, calendar_url(calendar)?, None)?;
    if (200..300).contains(&status) || status == 404 || status == 410 {
        Ok(())
    } else {
        Err(protocol(&format!("status {status}")))
    }
}

/// The app's events in the calendar between `from` and `to` (dates, both included), deleted
/// ones too. Events whose id is not the app's are left out (and so never touched).
pub fn list_events(
    http: &dyn Http,
    s: &Session,
    calendar: &str,
    from: &str,
    to: &str,
) -> Result<Vec<Remote>, GoogleError> {
    let day_ok = |d: &str| wall_time_ok(&format!("{d}T00:00:00"));
    if !day_ok(from) || !day_ok(to) {
        return Err(protocol("dates"));
    }
    // A day of slack on each side: the bounds are in UTC, the events in Israel's time.
    let url = reqwest::Url::parse_with_params(
        &format!("{}/events", calendar_url(calendar)?),
        &[
            ("showDeleted", "true"),
            ("singleEvents", "true"),
            ("maxResults", MAX_EVENTS),
            ("timeZone", TIME_ZONE),
            ("timeMin", &format!("{from}T00:00:00Z")),
            ("timeMax", &format!("{to}T23:59:59Z")),
            ("fields", "items(id,status,summary,start,end),nextPageToken"),
        ],
    )
    .map_err(|e| GoogleError::Protocol(e.to_string()))?;
    let Reply { status, body, .. } = send(http, s, Method::Get, url.to_string(), None)?;
    let v = json(status, &body)?;
    if v.get("nextPageToken").is_some_and(|t| !t.is_null()) {
        return Err(protocol("more than one page of events"));
    }
    let items = v
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| protocol("no event list"))?;
    let local = |side: &str, item: &Value| -> Option<String> {
        // Google answers `2026-11-18T09:00:00+02:00` in the time zone asked for.
        let t = item.get(side)?.get("dateTime")?.as_str()?;
        let wall = t.get(..19)?;
        wall_time_ok(wall).then(|| wall.to_owned())
    };
    Ok(items
        .iter()
        .filter_map(|item| {
            let id = item.get("id").and_then(Value::as_str)?;
            if !event_id_ok(id) {
                return None;
            }
            let cancelled = item.get("status").and_then(Value::as_str) == Some("cancelled");
            let event = (|| {
                let e = Event {
                    id: id.to_owned(),
                    start: local("start", item)?,
                    end: local("end", item)?,
                    title: item.get("summary")?.as_str()?.to_owned(),
                };
                event_ok(&e).then_some(e)
            })();
            Some(Remote {
                id: id.to_owned(),
                cancelled,
                event,
            })
        })
        .collect())
}

fn event_body(e: &Event) -> Value {
    serde_json::json!({
        "id": e.id,
        "summary": e.title,
        "start": { "dateTime": e.start, "timeZone": TIME_ZONE },
        "end": { "dateTime": e.end, "timeZone": TIME_ZONE },
        "visibility": "private",
        "transparency": "opaque",
        "status": "confirmed",
    })
}

/// Add an event that was never in the calendar.
pub fn insert_event(
    http: &dyn Http,
    s: &Session,
    calendar: &str,
    e: &Event,
) -> Result<(), GoogleError> {
    if !event_ok(e) {
        return Err(protocol("event"));
    }
    let url = format!("{}/events", calendar_url(calendar)?);
    let Reply { status, body, .. } = send(http, s, Method::Post, url, Some(&event_body(e)))?;
    json(status, &body).map(|_| ())
}

/// Replace an event (also one that was deleted: it comes back).
pub fn put_event(
    http: &dyn Http,
    s: &Session,
    calendar: &str,
    e: &Event,
) -> Result<(), GoogleError> {
    if !event_ok(e) {
        return Err(protocol("event"));
    }
    let url = format!("{}/events/{}", calendar_url(calendar)?, e.id);
    let Reply { status, body, .. } = send(http, s, Method::Put, url, Some(&event_body(e)))?;
    json(status, &body).map(|_| ())
}

/// Delete one of the app's events. One already gone counts as deleted.
pub fn delete_event(
    http: &dyn Http,
    s: &Session,
    calendar: &str,
    id: &str,
) -> Result<(), GoogleError> {
    if !event_id_ok(id) {
        return Err(protocol("event id"));
    }
    let url = format!("{}/events/{id}", calendar_url(calendar)?);
    let Reply { status, .. } = send(http, s, Method::Delete, url, None)?;
    if (200..300).contains(&status) || status == 404 || status == 410 {
        Ok(())
    } else {
        Err(protocol(&format!("status {status}")))
    }
}

/// Make the calendar's events equal to `wanted` (all inside the listed stretch): add what is
/// missing, replace what differs, delete the app's events that are no longer wanted. Asks
/// `still_wanted` before every change; when it says no (the vault locked, the copy turned
/// off), it stops there with [`GoogleError::Cancelled`].
pub fn apply(
    http: &dyn Http,
    s: &Session,
    calendar: &str,
    remote: &[Remote],
    wanted: &[Event],
    still_wanted: &dyn Fn() -> bool,
) -> Result<Applied, GoogleError> {
    if wanted.iter().any(|e| !event_ok(e)) {
        return Err(protocol("event"));
    }
    let mut done = Applied::default();
    let step = || {
        if still_wanted() {
            Ok(())
        } else {
            Err(GoogleError::Cancelled)
        }
    };
    for e in wanted {
        match remote.iter().find(|r| r.id == e.id) {
            Some(r) if !r.cancelled && r.event.as_ref() == Some(e) => {}
            Some(_) => {
                step()?;
                put_event(http, s, calendar, e)?;
                done.changed += 1;
            }
            None => {
                step()?;
                insert_event(http, s, calendar, e)?;
                done.added += 1;
            }
        }
    }
    for r in remote {
        if !r.cancelled && !wanted.iter().any(|e| e.id == r.id) {
            step()?;
            delete_event(http, s, calendar, &r.id)?;
            done.removed += 1;
        }
    }
    Ok(done)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    const CAL: &str = "c_0a1b2c@group.calendar.google.com";

    /// A fake Google Calendar: one calendar, its events by id; refuses any host off the list.
    #[derive(Default)]
    struct FakeCalendar {
        /// id → (cancelled, body)
        events: Mutex<BTreeMap<String, (bool, Value)>>,
        seen: Mutex<Vec<(Method, String, String)>>,
        exists: Mutex<bool>,
        /// Answer the list with a second page.
        paged: bool,
    }

    impl Http for FakeCalendar {
        fn send(&self, req: Request) -> Result<Reply, GoogleError> {
            let url = reqwest::Url::parse(&req.url).unwrap();
            assert!(crate::google::host_allowed(&url), "{}", req.url);
            if req.method == Method::Put {
                assert!(crate::google::put_allowed(&url), "{}", req.url);
            }
            let body = String::from_utf8(req.body.to_vec()).unwrap();
            self.seen
                .lock()
                .unwrap()
                .push((req.method, req.url.clone(), body.clone()));
            let path = url.path().to_owned();
            let ok = |v: Value| Ok(Reply::new(200, v.to_string().into_bytes()));
            match (req.method, path.as_str()) {
                (Method::Post, "/calendar/v3/calendars") => {
                    *self.exists.lock().unwrap() = true;
                    ok(serde_json::json!({ "id": CAL }))
                }
                (Method::Get, p) if p == format!("/calendar/v3/calendars/{CAL}") => {
                    if *self.exists.lock().unwrap() {
                        ok(serde_json::json!({ "id": CAL }))
                    } else {
                        Ok(Reply::new(404, Vec::new()))
                    }
                }
                (Method::Delete, p) if p == format!("/calendar/v3/calendars/{CAL}") => {
                    *self.exists.lock().unwrap() = false;
                    self.events.lock().unwrap().clear();
                    Ok(Reply::new(204, Vec::new()))
                }
                (Method::Get, p) if p.ends_with("/events") => {
                    let items: Vec<Value> = self
                        .events
                        .lock()
                        .unwrap()
                        .iter()
                        .map(|(id, (cancelled, b))| {
                            let mut b = b.clone();
                            b["id"] = Value::from(id.clone());
                            b["status"] =
                                Value::from(if *cancelled { "cancelled" } else { "confirmed" });
                            for side in ["start", "end"] {
                                let t = b[side]["dateTime"].as_str().unwrap_or("").to_owned();
                                b[side]["dateTime"] = Value::from(format!("{t}+02:00"));
                            }
                            b
                        })
                        .collect();
                    let mut v = serde_json::json!({ "items": items });
                    if self.paged {
                        v["nextPageToken"] = Value::from("x");
                    }
                    ok(v)
                }
                (Method::Post, p) if p.ends_with("/events") => {
                    let b: Value = serde_json::from_str(&body).unwrap();
                    let id = b["id"].as_str().unwrap().to_owned();
                    let mut events = self.events.lock().unwrap();
                    if events.contains_key(&id) {
                        return Ok(Reply::new(409, Vec::new()));
                    }
                    events.insert(id, (false, b));
                    ok(serde_json::json!({}))
                }
                (Method::Put, p) => {
                    let id = p.rsplit('/').next().unwrap().to_owned();
                    let b: Value = serde_json::from_str(&body).unwrap();
                    self.events.lock().unwrap().insert(id, (false, b));
                    ok(serde_json::json!({}))
                }
                (Method::Delete, p) => {
                    let id = p.rsplit('/').next().unwrap().to_owned();
                    if let Some(e) = self.events.lock().unwrap().get_mut(&id) {
                        e.0 = true;
                    }
                    Ok(Reply::new(204, Vec::new()))
                }
                other => panic!("unexpected call {other:?}"),
            }
        }
    }

    fn ev(n: u8, start: &str, end: &str) -> Event {
        Event {
            id: format!("dv{:032x}", n),
            start: start.to_owned(),
            end: end.to_owned(),
            title: TITLES[0].to_owned(),
        }
    }

    fn sync(fake: &FakeCalendar, wanted: &[Event]) -> Applied {
        let s = Session::for_tests("t");
        let remote = list_events(fake, &s, CAL, "2026-11-01", "2027-05-01").unwrap();
        apply(fake, &s, CAL, &remote, wanted, &|| true).unwrap()
    }

    #[test]
    fn ids_and_times_have_one_shape() {
        assert!(event_id_ok(&format!("dv{:032x}", 7)));
        assert!(!event_id_ok("dv123"));
        assert!(!event_id_ok(&format!("xx{:032x}", 7)));
        assert!(!event_id_ok(&format!("dv{:031x}g", 7)));
        assert!(calendar_id_ok(CAL));
        assert!(!calendar_id_ok("primary"));
        assert!(!calendar_id_ok("a/b@group.calendar.google.com"));
        assert!(!calendar_id_ok("@group.calendar.google.com"));
        assert!(wall_time_ok("2026-11-18T09:30:00"));
        assert!(!wall_time_ok("2026-11-18T09:30:01"));
        assert!(!wall_time_ok("2026-11-18 09:30:00"));
    }

    #[test]
    fn a_put_goes_only_to_one_event() {
        let ok = |u: &str| crate::google::put_allowed(&reqwest::Url::parse(u).unwrap());
        let id = format!("dv{:032x}", 1);
        assert!(ok(&format!("{API}/calendars/{CAL}/events/{id}")));
        assert!(ok("https://www.googleapis.com/upload/drive/v3/files"));
        for bad in [
            format!("{API}/calendars/{CAL}/events/{id}?x=1"),
            format!("{API}/calendars/{CAL}/events"),
            format!("{API}/calendars/{CAL}"),
            format!("{API}/calendars/primary/events/{id}"),
            format!("{API}/calendars/{CAL}/events/{id}/move"),
            format!("{API}/calendars/{CAL}/events/abc"),
            format!("https://oauth2.googleapis.com/calendar/v3/calendars/{CAL}/events/{id}"),
            format!("https://www.googleapis.com:444/calendar/v3/calendars/{CAL}/events/{id}"),
        ] {
            assert!(!ok(&bad), "{bad}");
        }
    }

    #[test]
    fn the_calendar_follows_the_visits_and_says_nothing_more() {
        let fake = FakeCalendar::default();
        let s = Session::for_tests("t");
        assert!(!calendar_exists(&fake, &s, CAL).unwrap());
        assert_eq!(create_calendar(&fake, &s).unwrap(), CAL);
        assert!(calendar_exists(&fake, &s, CAL).unwrap());

        let a = ev(1, "2026-11-18T09:00:00", "2026-11-18T10:30:00");
        let b = ev(2, "2026-11-19T12:00:00", "2026-11-19T13:00:00");
        assert_eq!(
            sync(&fake, &[a.clone(), b.clone()]),
            Applied {
                added: 2,
                changed: 0,
                removed: 0
            }
        );
        // Nothing changed: nothing is written.
        assert_eq!(sync(&fake, &[a.clone(), b.clone()]), Applied::default());
        // Moved, then gone.
        let a2 = ev(1, "2026-11-18T11:00:00", "2026-11-18T12:30:00");
        assert_eq!(
            sync(&fake, std::slice::from_ref(&a2)),
            Applied {
                added: 0,
                changed: 1,
                removed: 1
            }
        );
        // Back again under the id Google still keeps as deleted: a replace, not an insert.
        assert_eq!(
            sync(&fake, &[a2, b]),
            Applied {
                added: 0,
                changed: 1,
                removed: 0
            }
        );

        for (method, url, body) in fake.seen.lock().unwrap().iter() {
            assert!(url.starts_with(API), "{url}");
            if *method == Method::Post || *method == Method::Put {
                let v: Value = serde_json::from_str(body).unwrap();
                let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
                let allowed = [
                    "id",
                    "summary",
                    "start",
                    "end",
                    "visibility",
                    "transparency",
                    "status",
                    "timeZone",
                ];
                assert!(keys.iter().all(|k| allowed.contains(k)), "{keys:?}");
                let summary = v["summary"].as_str().unwrap();
                assert!(
                    TITLES.contains(&summary) || summary == CALENDAR_NAME,
                    "{summary}"
                );
            }
        }
    }

    #[test]
    fn her_own_events_in_that_calendar_are_left_alone() {
        let fake = FakeCalendar::default();
        fake.events.lock().unwrap().insert(
            "abcdefghijklmnopqrstuv0123".to_owned(),
            (false, serde_json::json!({ "summary": "של עינת", "start": { "dateTime": "2026-11-20T10:00:00" }, "end": { "dateTime": "2026-11-20T11:00:00" } })),
        );
        assert_eq!(sync(&fake, &[]), Applied::default());
        assert!(!fake.events.lock().unwrap()["abcdefghijklmnopqrstuv0123"].0);
    }

    #[test]
    fn a_second_page_is_an_error_not_an_empty_calendar() {
        let fake = FakeCalendar {
            paged: true,
            ..FakeCalendar::default()
        };
        let s = Session::for_tests("t");
        assert!(list_events(&fake, &s, CAL, "2026-11-01", "2027-05-01").is_err());
    }

    #[test]
    fn nothing_is_written_once_it_is_no_longer_wanted() {
        let fake = FakeCalendar::default();
        let s = Session::for_tests("t");
        let a = ev(1, "2026-11-18T09:00:00", "2026-11-18T10:30:00");
        let got = apply(&fake, &s, CAL, &[], &[a], &|| false);
        assert!(matches!(got, Err(GoogleError::Cancelled)));
        assert!(fake.seen.lock().unwrap().is_empty());
    }

    #[test]
    fn an_event_off_the_rules_is_never_sent() {
        let fake = FakeCalendar::default();
        let s = Session::for_tests("t");
        let mut a = ev(1, "2026-11-18T09:00:00", "2026-11-18T10:30:00");
        a.title = "דנה בדויה".to_owned();
        assert!(apply(&fake, &s, CAL, &[], &[a], &|| true).is_err());
        let b = ev(2, "2026-11-18T10:00:00", "2026-11-18T09:00:00");
        assert!(insert_event(&fake, &s, CAL, &b).is_err());
        assert!(delete_event(
            &fake,
            &s,
            "primary",
            &ev(3, "2026-11-18T09:00:00", "2026-11-18T10:00:00").id
        )
        .is_err());
        assert!(fake.seen.lock().unwrap().is_empty());
    }
}
