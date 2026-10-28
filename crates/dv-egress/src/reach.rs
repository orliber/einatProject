//! "בדיקת מערכת" (D-142): can this computer reach the AI service she chose?
//!
//! A TLS handshake and nothing more. The name of the service's host is looked up, a TCP
//! connection is opened to it, and the program's own TLS setup (TLS 1.3, the Mozilla roots
//! built into the program, the pins of the AI service, no proxy) shakes hands with it. Then
//! the connection is closed with TLS's own close alert. No HTTP request is written, so no
//! path, no header, no API key and nothing from the vault ever leaves: what goes on the wire
//! is the ClientHello (the public host name, which every request names anyway, and fresh
//! random key shares), the client's Finished message, and the close alert. The host comes
//! from the model she chose, through the same allow-lists as a real request, never from the
//! window. While the emergency stop is on, nothing is opened at all.
//!
//! The release workflow already does exactly this before every release (D-077); here she can
//! do it on her own computer, where an antivirus that reads encrypted traffic, a proxy, or a
//! blocked network is told apart from "no internet".

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustls::pki_types::ServerName;
use rustls::{CertificateError, ClientConnection};

use crate::pinning;
use crate::providers::{Provider, GEMINI_HOST, MISTRAL_HOST, OPENAI_HOST};

/// Each address gets this long to accept the connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
/// One read or write on the socket.
const IO_TIMEOUT: Duration = Duration::from_secs(10);
/// The whole handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
/// At most this many of the addresses the name has are tried.
const MAX_ADDRESSES: usize = 2;
/// The details for "copy for Or" are short.
const MAX_DETAIL: usize = 160;

/// What the handshake came to. The `String`s are technical details for the copy she sends
/// Or: ASCII, short, the kind of error and the system's number, never a path or a host's
/// answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// The service answered and proved who it is with a certificate the program trusts.
    Reached,
    /// The host name could not be looked up (DNS): no internet, or a network that blocks it.
    NoName(String),
    /// No connection could be opened: no network, or a firewall that refuses it.
    NoNetwork(String),
    /// The connection or the handshake ran out of time.
    TimedOut(String),
    /// The certificate did not chain to a root the program trusts. Typical of an antivirus
    /// that scans encrypted traffic, or a proxy, standing in the middle.
    Intercepted(String),
    /// A valid certificate, but not from an authority this version is pinned to (D-077): a
    /// new version is needed.
    Pinned,
    /// The secure connection failed another way (something answered that does not speak TLS
    /// 1.3, or hung up in the middle).
    Tls(String),
    /// The emergency stop is on: nothing was opened.
    Stopped,
    /// The model runs on this computer: there is no service on the internet to reach.
    Local,
    /// The model is on no allow-list: nothing was opened (fail closed).
    NotAllowed,
}

/// Shake hands with the AI service of `model` and hang up. Blocking; up to about half a
/// minute when the network does not answer. Never sends a request, a key or any data.
#[must_use]
pub fn reach_ai_service(model: &str) -> Reach {
    reach_with(&SystemNet, model, crate::is_stopped())
}

/// The host a model's requests go to, from the same allow-lists the requests use. `None` for
/// the local model and for a model on no list.
fn host_of(provider: Provider) -> Option<&'static str> {
    match provider {
        Provider::Anthropic => Some(crate::API_HOST),
        Provider::OpenAi => Some(OPENAI_HOST),
        Provider::Gemini => Some(GEMINI_HOST),
        Provider::Mistral => Some(MISTRAL_HOST),
        Provider::Local => None,
    }
}

/// A socket the handshake runs over: the real one, or a stand-in in the tests.
pub(crate) trait Wire: Read + Write {}
impl<T: Read + Write> Wire for T {}

/// Name lookup and connection: the system's, or a stand-in in the tests (never the network).
pub(crate) trait Net {
    fn resolve(&self, host: &str, port: u16) -> std::io::Result<Vec<SocketAddr>>;
    fn connect(&self, addr: &SocketAddr) -> std::io::Result<Box<dyn Wire>>;
}

/// The system's own lookup and TCP, with time limits. No proxy is ever used (the program
/// never reads the system's proxy settings).
struct SystemNet;

impl Net for SystemNet {
    fn resolve(&self, host: &str, port: u16) -> std::io::Result<Vec<SocketAddr>> {
        (host, port).to_socket_addrs().map(Iterator::collect)
    }

    fn connect(&self, addr: &SocketAddr) -> std::io::Result<Box<dyn Wire>> {
        let sock = TcpStream::connect_timeout(addr, CONNECT_TIMEOUT)?;
        sock.set_read_timeout(Some(IO_TIMEOUT))?;
        sock.set_write_timeout(Some(IO_TIMEOUT))?;
        let _ = sock.set_nodelay(true);
        Ok(Box::new(sock))
    }
}

pub(crate) fn reach_with(net: &dyn Net, model: &str, stopped: bool) -> Reach {
    let Some(provider) = Provider::of_model(model) else {
        return Reach::NotAllowed;
    };
    let Some(host) = host_of(provider) else {
        return Reach::Local;
    };
    if stopped {
        return Reach::Stopped;
    }
    let addrs = match net.resolve(host, 443) {
        Ok(a) if !a.is_empty() => a,
        Ok(_) => return Reach::NoName("dns: no address".to_owned()),
        Err(e) => return Reach::NoName(format!("dns: {}", io_detail(&e))),
    };
    let mut last: Option<std::io::Error> = None;
    let mut wire = None;
    for addr in addrs.iter().take(MAX_ADDRESSES) {
        match net.connect(addr) {
            Ok(w) => {
                wire = Some(w);
                break;
            }
            Err(e) => last = Some(e),
        }
    }
    let Some(mut wire) = wire else {
        return last.map_or_else(
            || Reach::NoNetwork("connect: no address tried".to_owned()),
            |e| connect_failure(&e),
        );
    };
    handshake(host, &mut wire)
}

/// A connection that could not be opened.
fn connect_failure(e: &std::io::Error) -> Reach {
    use std::io::ErrorKind as K;
    let detail = format!("connect: {}", io_detail(e));
    match e.kind() {
        K::TimedOut | K::WouldBlock => Reach::TimedOut(detail),
        _ => Reach::NoNetwork(detail),
    }
}

/// The handshake over `wire`, then the close alert. Only TLS records are ever written.
fn handshake(host: &'static str, wire: &mut Box<dyn Wire>) -> Reach {
    let Ok(name) = ServerName::try_from(host) else {
        return Reach::NotAllowed;
    };
    let config = Arc::new(crate::tls_config());
    let mut conn = match ClientConnection::new(config, name) {
        Ok(c) => c,
        Err(e) => return Reach::Tls(clip(&format!("setup: {e}"))),
    };
    let started = Instant::now();
    while conn.is_handshaking() {
        if started.elapsed() > HANDSHAKE_TIMEOUT {
            return Reach::TimedOut("handshake: over the time limit".to_owned());
        }
        match conn.complete_io(wire) {
            Ok((0, 0)) if conn.is_handshaking() => {
                return Reach::Tls("handshake: no progress".to_owned());
            }
            Ok(_) => {}
            Err(e) => return handshake_failure(&e),
        }
    }
    // Hang up politely: TLS's own close alert, and nothing else.
    conn.send_close_notify();
    let _ = conn.complete_io(wire);
    let _ = wire.flush();
    Reach::Reached
}

/// A handshake that failed: the certificate, the time, or anything else on the way.
fn handshake_failure(e: &std::io::Error) -> Reach {
    use std::io::ErrorKind as K;
    if let Some(tls) = e
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>())
    {
        return tls_failure(tls);
    }
    let detail = format!("handshake: {}", io_detail(e));
    match e.kind() {
        K::TimedOut | K::WouldBlock => Reach::TimedOut(detail),
        _ => Reach::Tls(detail),
    }
}

/// What a TLS error says about the way to the service.
pub(crate) fn tls_failure(e: &rustls::Error) -> Reach {
    match e {
        rustls::Error::General(m) if m == pinning::PIN_MARK => {
            // The verifier noted it for the next send's message: this was only a check.
            let _ = pinning::take_pin_refused();
            Reach::Pinned
        }
        rustls::Error::InvalidCertificate(c) => {
            Reach::Intercepted(clip(&format!("certificate: {}", certificate_kind(c))))
        }
        other => Reach::Tls(clip(&format!("tls: {other}"))),
    }
}

/// The certificate error by its name only (some variants carry the names in the certificate).
fn certificate_kind(c: &CertificateError) -> &'static str {
    match c {
        CertificateError::UnknownIssuer => "unknown issuer",
        CertificateError::BadSignature => "bad signature",
        CertificateError::BadEncoding => "bad encoding",
        CertificateError::Expired | CertificateError::ExpiredContext { .. } => "expired",
        CertificateError::NotValidYet | CertificateError::NotValidYetContext { .. } => {
            "not valid yet"
        }
        CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. } => {
            "not valid for the name"
        }
        CertificateError::Revoked => "revoked",
        _ => "other",
    }
}

/// An io error by its kind and the system's number: nothing it says.
fn io_detail(e: &std::io::Error) -> String {
    match e.raw_os_error() {
        Some(n) => format!("{:?} (os error {n})", e.kind()),
        None => format!("{:?}", e.kind()),
    }
}

/// ASCII, no control characters, short.
fn clip(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii() && !c.is_ascii_control())
        .take(MAX_DETAIL)
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::io::ErrorKind;
    use std::rc::Rc;

    /// The first model of the Claude allow-list.
    const CLAUDE: &str = crate::ALLOWED_MODELS[0];

    /// A network that must never be reached.
    struct Untouchable;
    impl Net for Untouchable {
        fn resolve(&self, _: &str, _: u16) -> std::io::Result<Vec<SocketAddr>> {
            panic!("looked up a name")
        }
        fn connect(&self, _: &SocketAddr) -> std::io::Result<Box<dyn Wire>> {
            panic!("opened a connection")
        }
    }

    /// What the fake server does.
    #[derive(Clone)]
    enum Peer {
        /// Says nothing until the time runs out.
        Silent,
        /// Answers like a web proxy, not TLS.
        Http,
        /// Hangs up at once.
        Closes,
    }

    /// A wire to a fake server that records every byte the program writes.
    struct FakeWire {
        peer: Peer,
        sent: Rc<RefCell<Vec<u8>>>,
        answered: bool,
    }

    impl Read for FakeWire {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            match self.peer {
                Peer::Silent => Err(std::io::Error::from(ErrorKind::TimedOut)),
                Peer::Closes => Ok(0),
                Peer::Http => {
                    if self.answered {
                        return Ok(0);
                    }
                    self.answered = true;
                    let page = b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n";
                    let n = page.len().min(buf.len());
                    buf.get_mut(..n)
                        .unwrap()
                        .copy_from_slice(page.get(..n).unwrap());
                    Ok(n)
                }
            }
        }
    }

    impl Write for FakeWire {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.sent.borrow_mut().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct FakeNet {
        resolve: Result<Vec<SocketAddr>, ErrorKind>,
        connect: Result<Peer, ErrorKind>,
        hosts: RefCell<Vec<String>>,
        sent: Rc<RefCell<Vec<u8>>>,
    }

    impl FakeNet {
        fn new(
            resolve: Result<Vec<SocketAddr>, ErrorKind>,
            connect: Result<Peer, ErrorKind>,
        ) -> Self {
            Self {
                resolve,
                connect,
                hosts: RefCell::default(),
                sent: Rc::default(),
            }
        }
        fn up(peer: Peer) -> Self {
            Self::new(Ok(vec![SocketAddr::from(([192, 0, 2, 1], 443))]), Ok(peer))
        }
    }

    impl Net for FakeNet {
        fn resolve(&self, host: &str, port: u16) -> std::io::Result<Vec<SocketAddr>> {
            assert_eq!(port, 443);
            self.hosts.borrow_mut().push(host.to_owned());
            self.resolve.clone().map_err(std::io::Error::from)
        }
        fn connect(&self, _: &SocketAddr) -> std::io::Result<Box<dyn Wire>> {
            let peer = self.connect.clone().map_err(std::io::Error::from)?;
            Ok(Box::new(FakeWire {
                peer,
                sent: Rc::clone(&self.sent),
                answered: false,
            }))
        }
    }

    #[test]
    fn the_host_comes_from_the_model_and_the_local_model_opens_nothing() {
        let lists: [(&[&str], &str); 4] = [
            (crate::ALLOWED_MODELS, "api.anthropic.com"),
            (crate::OPENAI_MODELS, "api.openai.com"),
            (crate::GEMINI_MODELS, "generativelanguage.googleapis.com"),
            (crate::MISTRAL_MODELS, "api.mistral.ai"),
        ];
        for (models, host) in lists {
            for model in models {
                let net = FakeNet::new(Err(ErrorKind::NotFound), Ok(Peer::Closes));
                let _ = reach_with(&net, model, false);
                assert_eq!(*net.hosts.borrow(), [host], "{model}");
            }
        }
        for local in crate::LOCAL_MODELS {
            assert_eq!(reach_with(&Untouchable, local, false), Reach::Local);
        }
        assert_eq!(
            reach_with(&Untouchable, "some-other-model", false),
            Reach::NotAllowed
        );
        assert_eq!(
            reach_with(&Untouchable, "https://evil.example/", false),
            Reach::NotAllowed
        );
    }

    #[test]
    fn the_emergency_stop_opens_nothing() {
        assert_eq!(reach_with(&Untouchable, CLAUDE, true), Reach::Stopped);
    }

    #[test]
    fn a_name_that_cannot_be_looked_up_is_dns() {
        let net = FakeNet::new(Err(ErrorKind::Other), Ok(Peer::Closes));
        assert!(
            matches!(reach_with(&net, CLAUDE, false), Reach::NoName(d) if d.starts_with("dns:"))
        );
        let none = FakeNet::new(Ok(Vec::new()), Ok(Peer::Closes));
        assert_eq!(
            reach_with(&none, CLAUDE, false),
            Reach::NoName("dns: no address".to_owned())
        );
        assert!(none.sent.borrow().is_empty());
    }

    #[test]
    fn a_connection_refused_or_unreachable_is_no_network_and_a_slow_one_timed_out() {
        for kind in [
            ErrorKind::ConnectionRefused,
            ErrorKind::NetworkUnreachable,
            ErrorKind::HostUnreachable,
            ErrorKind::PermissionDenied,
        ] {
            let net = FakeNet::new(Ok(vec![SocketAddr::from(([192, 0, 2, 1], 443))]), Err(kind));
            assert!(
                matches!(reach_with(&net, CLAUDE, false), Reach::NoNetwork(d) if d.starts_with("connect:")),
                "{kind:?}"
            );
        }
        let slow = FakeNet::new(
            Ok(vec![SocketAddr::from(([192, 0, 2, 1], 443))]),
            Err(ErrorKind::TimedOut),
        );
        assert!(matches!(
            reach_with(&slow, CLAUDE, false),
            Reach::TimedOut(_)
        ));
    }

    #[test]
    fn a_server_that_says_nothing_timed_out_and_one_that_hangs_up_or_speaks_http_is_tls() {
        assert!(matches!(
            reach_with(&FakeNet::up(Peer::Silent), CLAUDE, false),
            Reach::TimedOut(d) if d.starts_with("handshake:")
        ));
        assert!(matches!(
            reach_with(&FakeNet::up(Peer::Closes), CLAUDE, false),
            Reach::Tls(_)
        ));
        assert!(matches!(
            reach_with(&FakeNet::up(Peer::Http), CLAUDE, false),
            Reach::Tls(d) if d.starts_with("tls:")
        ));
    }

    #[test]
    fn only_a_tls_client_hello_is_written_no_request_no_key() {
        let net = FakeNet::up(Peer::Silent);
        let _ = reach_with(&net, CLAUDE, false);
        let sent = net.sent.borrow().clone();
        // A TLS handshake record (content type 22), and the host name in it (SNI).
        assert_eq!(sent.first(), Some(&0x16), "not a TLS record");
        assert_eq!(sent.get(1), Some(&0x03));
        let text = String::from_utf8_lossy(&sent).to_lowercase();
        assert!(text.contains("api.anthropic.com"));
        for never in [
            "post ",
            "get ",
            "http/1",
            "x-api-key",
            "authorization",
            "anthropic-version",
            "/v1/",
            "sk-ant",
        ] {
            assert!(!text.contains(never), "{never} on the wire");
        }
        // One record: the ClientHello, and nothing after it.
        let len = usize::from(u16::from_be_bytes([sent[3], sent[4]]));
        assert_eq!(sent.len(), 5 + len, "something after the ClientHello");
        assert_eq!(sent.get(5), Some(&0x01), "not a ClientHello");
    }

    #[test]
    fn certificate_errors_are_told_apart() {
        assert_eq!(
            tls_failure(&rustls::Error::InvalidCertificate(
                CertificateError::UnknownIssuer
            )),
            Reach::Intercepted("certificate: unknown issuer".to_owned())
        );
        assert!(matches!(
            tls_failure(&rustls::Error::InvalidCertificate(
                CertificateError::BadSignature
            )),
            Reach::Intercepted(_)
        ));
        assert_eq!(
            tls_failure(&rustls::Error::General(pinning::PIN_MARK.to_owned())),
            Reach::Pinned
        );
        assert!(matches!(
            tls_failure(&rustls::Error::AlertReceived(
                rustls::AlertDescription::HandshakeFailure
            )),
            Reach::Tls(_)
        ));
        // Wrapped the way the connection hands it back.
        let wrapped = std::io::Error::new(
            ErrorKind::InvalidData,
            rustls::Error::InvalidCertificate(CertificateError::UnknownIssuer),
        );
        assert!(matches!(handshake_failure(&wrapped), Reach::Intercepted(_)));
    }

    #[test]
    fn details_are_short_ascii_and_name_no_path() {
        let e = std::io::Error::other("C:\\Users\\רותם\\secret.txt");
        let d = io_detail(&e);
        assert_eq!(d, "Other");
        assert_eq!(clip(&"א".repeat(10)), "");
        assert_eq!(clip(&"x".repeat(500)).len(), MAX_DETAIL);
    }
}
