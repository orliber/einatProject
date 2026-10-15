//! Narrowed trust for the AI service (D-077, L2 in the master plan).
//!
//! Every connection already trusts only the Mozilla roots built into the program (never the
//! computer's own store), so a proxy or an antivirus with a root of its own cannot read the
//! traffic. For `api.anthropic.com` the trust is narrower still: its certificate must chain
//! to one of a few roots, named here by the SHA-256 of their public key (SPKI). These are the
//! roots of the authorities that have issued its certificates (Google Trust Services, Let's
//! Encrypt, ZeroSSL through USERTrust), and DigiCert as a spare. A certificate from any
//! other authority, even one the Mozilla list trusts, is refused, and nothing is sent.
//!
//! The list changes only with a new version of the program, through the signed update
//! channel (D-033). When the service moves to an authority not on the list, she reads that an
//! update is needed; the program never falls back to the wider trust.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, RootCertStore, SignatureScheme};

/// Where the narrowed trust applies.
pub(crate) const PINNED_HOSTS: &[&str] = &["api.anthropic.com"];

/// The roots `api.anthropic.com` may chain to: SHA-256 of each root's SubjectPublicKeyInfo
/// (the standard SPKI pin), with its name.
pub(crate) const ANTHROPIC_ROOTS: &[(&str, &str)] = &[
    (
        "GTS Root R1",
        "871a9194f4eed5b312ff40c84c1d524aed2f778bbff25f138cf81f680a7adc67",
    ),
    (
        "GTS Root R3",
        "4179edd981ef747477b49626408af43daa2ca7ab7f9e082c1060f84096774348",
    ),
    (
        "GTS Root R4",
        "9847e5653e5e9e847516e5cb818606aa7544a19be67fd7366d506988e8d84347",
    ),
    (
        "ISRG Root X1",
        "0b9fa5a59eed715c26c1020c711b4f6ec42d58b0015e14337a39dad301c5afc3",
    ),
    (
        "ISRG Root X2",
        "762195c225586ee6c0237456e2107dc54f1efc21f61a792ebd515913cce68332",
    ),
    (
        "USERTrust RSA Certification Authority",
        "c784333d20bcd742b9fdc3236f4e509b8937070e73067e254dd3bf9c45bf4dde",
    ),
    (
        "USERTrust ECC Certification Authority",
        "2021917e98263945c859c43f1d73cb4139053c414fa03ca3bc7ee88614298f3b",
    ),
    // The spare: a different authority, so one change of issuer does not stop her work.
    (
        "DigiCert Global Root G2",
        "8bb593a93be1d0e8a822bb887c547890c3e706aad2dab76254f97fb36b82fc26",
    ),
    (
        "DigiCert Global Root G3",
        "b94c198300cec5c057ad0727b70bbe91816992256439a7b32f4598119dda9c97",
    ),
];

/// In the error text of a connection refused by the narrowed trust alone.
pub(crate) const PIN_MARK: &str = "dv-pinned-root-mismatch";

/// Set when a certificate the Mozilla roots accept was refused by the narrowed trust (the
/// service changed authority): the program says an update is needed.
static PIN_REFUSED: AtomicBool = AtomicBool::new(false);

/// Was the last refusal the narrowed trust's alone? Reads and clears.
pub(crate) fn take_pin_refused() -> bool {
    PIN_REFUSED.swap(false, Ordering::SeqCst)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The standard SPKI pin of a root as the Mozilla list holds it (the key info without its
/// outer SEQUENCE): SHA-256 of the whole SubjectPublicKeyInfo.
pub(crate) fn spki_pin(inner: &[u8]) -> String {
    let n = inner.len();
    let mut der = vec![0x30];
    match u8::try_from(n) {
        Ok(b) if b < 0x80 => der.push(b),
        Ok(b) => der.extend([0x81, b]),
        Err(_) => {
            let [hi, lo] = u16::try_from(n).unwrap_or(u16::MAX).to_be_bytes();
            der.extend([0x82, hi, lo]);
        }
    }
    der.extend_from_slice(inner);
    hex(aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, &der).as_ref())
}

/// The Mozilla roots whose pin is on `pins`.
pub(crate) fn pinned_roots(pins: &[(&str, &str)]) -> RootCertStore {
    RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS
            .iter()
            .filter(|a| {
                let pin = spki_pin(a.subject_public_key_info.as_ref());
                pins.iter().any(|(_, p)| *p == pin)
            })
            .cloned()
            .collect(),
    }
}

fn is_pinned(name: &ServerName<'_>) -> bool {
    match name {
        ServerName::DnsName(d) => PINNED_HOSTS
            .iter()
            .any(|h| d.as_ref().eq_ignore_ascii_case(h)),
        _ => false,
    }
}

/// The Mozilla roots for every host, and the narrowed set for the pinned ones.
#[derive(Debug)]
pub(crate) struct PinnedVerifier {
    all: Arc<WebPkiServerVerifier>,
    pinned: Arc<WebPkiServerVerifier>,
}

impl PinnedVerifier {
    pub(crate) fn new(provider: &Arc<CryptoProvider>) -> Option<Self> {
        let all = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let all = WebPkiServerVerifier::builder_with_provider(Arc::new(all), Arc::clone(provider))
            .build()
            .ok()?;
        let pinned = WebPkiServerVerifier::builder_with_provider(
            Arc::new(pinned_roots(ANTHROPIC_ROOTS)),
            Arc::clone(provider),
        )
        .build()
        .ok()?;
        Some(Self { all, pinned })
    }
}

impl ServerCertVerifier for PinnedVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if !is_pinned(server_name) {
            return self.all.verify_server_cert(
                end_entity,
                intermediates,
                server_name,
                ocsp_response,
                now,
            );
        }
        match self.pinned.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        ) {
            Ok(v) => Ok(v),
            Err(e) => {
                let wider = self.all.verify_server_cert(
                    end_entity,
                    intermediates,
                    server_name,
                    ocsp_response,
                    now,
                );
                if wider.is_ok() {
                    PIN_REFUSED.store(true, Ordering::SeqCst);
                    return Err(rustls::Error::General(PIN_MARK.to_owned()));
                }
                Err(e)
            }
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.all.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.all.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.all.supported_verify_schemes()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn every_pin_names_a_root_the_program_carries_and_nothing_else_is_trusted() {
        let roots = pinned_roots(ANTHROPIC_ROOTS);
        assert_eq!(
            roots.roots.len(),
            ANTHROPIC_ROOTS.len(),
            "a pin with a typo, or a root gone from the Mozilla list"
        );
        assert!(roots.roots.len() < webpki_roots::TLS_SERVER_ROOTS.len() / 10);
        for a in &roots.roots {
            let subject = String::from_utf8_lossy(a.subject.as_ref()).into_owned();
            let pin = spki_pin(a.subject_public_key_info.as_ref());
            let (name, _) = ANTHROPIC_ROOTS.iter().find(|(_, p)| *p == pin).unwrap();
            assert!(subject.contains(name), "{name} is not {subject}");
        }
    }

    #[test]
    fn a_published_pin_is_computed_the_same_way() {
        // ISRG Root X1's SPKI pin, as Let's Encrypt publishes it (base64
        // C5+lpZ7tcVwmwQIMcRtPbsQtWLABXhQzejna0wHFr8M=).
        let x1 = webpki_roots::TLS_SERVER_ROOTS
            .iter()
            .find(|a| String::from_utf8_lossy(a.subject.as_ref()).contains("ISRG Root X1"))
            .unwrap();
        assert_eq!(
            spki_pin(x1.subject_public_key_info.as_ref()),
            "0b9fa5a59eed715c26c1020c711b4f6ec42d58b0015e14337a39dad301c5afc3"
        );
    }

    #[test]
    fn only_the_ai_service_is_held_to_the_narrowed_trust() {
        let name = |s: &str| ServerName::try_from(s.to_owned()).unwrap();
        assert!(is_pinned(&name("api.anthropic.com")));
        assert!(is_pinned(&name("API.Anthropic.com")));
        assert!(!is_pinned(&name("anthropic.com.evil.example")));
        assert!(!is_pinned(&name("github.com")));
        assert!(!is_pinned(&name("huggingface.co")));
    }

    #[test]
    fn a_certificate_no_root_vouches_for_is_refused_as_before_and_not_as_an_update() {
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let v = PinnedVerifier::new(&provider).unwrap();
        let junk = CertificateDer::from(vec![0x30, 0x03, 0x02, 0x01, 0x00]);
        for host in ["api.anthropic.com", "github.com"] {
            let name = ServerName::try_from(host.to_owned()).unwrap();
            assert!(v
                .verify_server_cert(&junk, &[], &name, &[], UnixTime::now())
                .is_err());
        }
        assert!(
            !take_pin_refused(),
            "only a certificate the wider trust takes is an update"
        );
    }
}
