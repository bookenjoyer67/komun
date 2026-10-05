//! Transactional mail for verification and password reset (SPEC Part 1.5).
//!
//! Composition and delivery are separated on purpose. [`verification_message`] and
//! [`password_reset_message`] are pure functions from data to a [`Message`], so the contents of
//! the mail — the link, the recipient, the subject, the absence of the password anywhere in it —
//! can be asserted in a unit test with no network. [`Mailer::send`] is the only part that touches
//! SMTP, and it is the only part the tests cannot exercise in this environment.

use lettre::{
    message::{header::ContentType, Mailbox, Message},
    transport::smtp::authentication::Credentials,
    AsyncSmtpTransport, AsyncTransport, Tokio1Executor,
};

use crate::config::Config;

/// A configured SMTP sender. Absent when `[email]` is unset, which is legal only while
/// `require_email_verification = false` (enforced at startup by `Config::validate_registration`).
pub struct Mailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
    public_url: String,
}

impl std::fmt::Debug for Mailer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The transport holds the SMTP password; deriving Debug would put it one `{:?}` away
        // from a log line.
        f.debug_struct("Mailer")
            .field("from", &self.from)
            .field("public_url", &self.public_url)
            .finish_non_exhaustive()
    }
}

impl Mailer {
    /// Build a mailer, or `None` when `[email]` is not configured.
    ///
    /// Errors are configuration errors (an unparseable from-address, an SMTP host that cannot be
    /// turned into a relay) and are returned rather than swallowed, so a typo surfaces at startup
    /// instead of at the first signup.
    pub fn from_config(config: &Config) -> anyhow::Result<Option<Self>> {
        if !config.email.is_configured() {
            return Ok(None);
        }

        let host = config.email.smtp_host.clone().unwrap_or_default();
        let from_raw = config.email.from.clone().unwrap_or_default();
        let from: Mailbox = from_raw.parse().map_err(|e| {
            anyhow::anyhow!("[email] from is not a valid address ({from_raw:?}): {e}")
        })?;

        // STARTTLS on the submission port, implicit TLS otherwise. Both verify certificates
        // against the webpki roots; there is no plaintext path and no "accept any certificate"
        // escape hatch, because either one would make the credentials interceptable.
        let builder = if config.email.starttls {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host)
        } else {
            AsyncSmtpTransport::<Tokio1Executor>::relay(&host)
        }
        .map_err(|e| anyhow::anyhow!("[email] smtp_host {host:?} is not usable as a relay: {e}"))?
        .port(config.email.port());

        let builder = match (&config.email.username, &config.email.password) {
            (Some(u), Some(p)) if !u.is_empty() => {
                builder.credentials(Credentials::new(u.clone(), p.clone()))
            }
            _ => builder,
        };

        let public_url = config
            .email
            .public_url
            .clone()
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| config.public_url());

        Ok(Some(Self {
            transport: builder.build(),
            from,
            public_url: public_url.trim_end_matches('/').to_string(),
        }))
    }

    pub fn from_address(&self) -> &Mailbox {
        &self.from
    }

    pub fn public_url(&self) -> &str {
        &self.public_url
    }

    pub fn verification_message(
        &self,
        to: &str,
        display_name: &str,
        token: &str,
    ) -> anyhow::Result<Message> {
        verification_message(&self.from, &self.public_url, to, display_name, token)
    }

    pub fn password_reset_message(
        &self,
        to: &str,
        display_name: &str,
        token: &str,
    ) -> anyhow::Result<Message> {
        password_reset_message(&self.from, &self.public_url, to, display_name, token)
    }

    /// Deliver a composed message. Failures are reported to the caller, which decides whether
    /// they are fatal — a signup, for instance, still succeeds when the mail bounces, because the
    /// account exists and the user can ask for another link.
    pub async fn send(&self, message: Message) -> anyhow::Result<()> {
        self.transport
            .send(message)
            .await
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("SMTP delivery failed: {e}"))
    }
}

pub fn verification_link(public_url: &str, token: &str) -> String {
    format!(
        "{}/verify-email?token={}",
        public_url.trim_end_matches('/'),
        urlencode(token)
    )
}

pub fn password_reset_link(public_url: &str, token: &str) -> String {
    format!(
        "{}/reset-password?token={}",
        public_url.trim_end_matches('/'),
        urlencode(token)
    )
}

/// Session tokens are URL-safe base64 already, but a value going into a query string should still
/// be encoded.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn verification_message(
    from: &Mailbox,
    public_url: &str,
    to: &str,
    display_name: &str,
    token: &str,
) -> anyhow::Result<Message> {
    let to: Mailbox = to
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid recipient address: {e}"))?;
    let link = verification_link(public_url, token);

    let body = format!(
        "Hello {display_name},\n\n\
         Confirm this address to finish setting up your Komun account:\n\n\
         {link}\n\n\
         The link works once and expires in 24 hours.\n\n\
         If you did not create an account, ignore this message — nothing was set up and the \
         address will not be contacted again.\n"
    );

    Message::builder()
        .from(from.clone())
        .to(to)
        .subject("Confirm your Komun account")
        .header(ContentType::TEXT_PLAIN)
        .body(body)
        .map_err(|e| anyhow::anyhow!("failed to compose verification email: {e}"))
}

pub fn password_reset_message(
    from: &Mailbox,
    public_url: &str,
    to: &str,
    display_name: &str,
    token: &str,
) -> anyhow::Result<Message> {
    let to: Mailbox = to
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid recipient address: {e}"))?;
    let link = password_reset_link(public_url, token);

    let body = format!(
        "Hello {display_name},\n\n\
         Someone asked to reset the password for your Komun account. If that was you, follow \
         this link within 30 minutes:\n\n\
         {link}\n\n\
         The link works once. Resetting your password signs out every other device.\n\n\
         If it was not you, ignore this message. Your password has not changed.\n\n\
         Note: resetting your password cannot recover messages encrypted to your old key. Use \
         your 12-word recovery code for that.\n"
    );

    Message::builder()
        .from(from.clone())
        .to(to)
        .subject("Reset your Komun password")
        .header(ContentType::TEXT_PLAIN)
        .body(body)
        .map_err(|e| anyhow::anyhow!("failed to compose password reset email: {e}"))
}

/// Used by the tests to assert on content without a network, and by `--dry-run`-style tooling.
pub fn render(message: &Message) -> String {
    String::from_utf8_lossy(&message.formatted()).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn from() -> Mailbox {
        "Komun <noreply@example.org>".parse().expect("valid from")
    }

    /// Undo the quoted-printable transfer encoding lettre applies to the body.
    ///
    /// The wire bytes really do contain `=3D` for `=` and `=\r\n` soft line breaks, so asserting
    /// on a raw link substring would be asserting on the wrong thing: what has to be intact is
    /// what the recipient's mail client shows after decoding. Headers are still checked against
    /// the undecoded wire form, where they actually live.
    fn decoded_body(wire: &str) -> String {
        let body = wire.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or(wire);
        let mut out = String::with_capacity(body.len());
        let mut chars = body.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '=' {
                out.push(c);
                continue;
            }
            // `=\r\n` (or `=\n`) is a soft break inserted purely to keep lines under 76 columns.
            if chars.peek() == Some(&'\r') {
                chars.next();
            }
            if chars.peek() == Some(&'\n') {
                chars.next();
                continue;
            }
            let hi = chars.next();
            let lo = chars.next();
            match (hi, lo) {
                (Some(hi), Some(lo)) => {
                    let byte = u8::from_str_radix(&format!("{hi}{lo}"), 16)
                        .unwrap_or_else(|_| panic!("malformed quoted-printable escape =${hi}{lo}"));
                    out.push(byte as char);
                }
                _ => panic!("truncated quoted-printable escape at end of body"),
            }
        }
        // Multi-byte UTF-8 arrives as one escape per byte, so reassemble before comparing text.
        let bytes: Vec<u8> = out.chars().map(|c| c as u32 as u8).collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    #[test]
    fn verification_mail_carries_a_single_use_link_and_no_secrets() {
        let token = "Zm9vYmFyLXRva2VuLTEyMw";
        let msg = verification_message(
            &from(),
            "https://komun.example.org",
            "member@example.com",
            "Ada",
            token,
        )
        .expect("compose");
        let wire = render(&msg);
        let body = decoded_body(&wire);

        assert!(wire.contains("To: member@example.com"), "{wire}");
        assert!(
            wire.contains("Subject: Confirm your Komun account"),
            "{wire}"
        );
        assert!(
            body.contains("https://komun.example.org/verify-email?token=Zm9vYmFyLXRva2VuLTEyMw"),
            "the link must be complete and clickable:\n{body}"
        );
        assert!(body.contains("expires in 24 hours"), "{body}");
        assert!(body.contains("Ada"), "{body}");
    }

    #[test]
    fn reset_mail_states_the_short_window_and_the_session_consequence() {
        let msg = password_reset_message(
            &from(),
            "https://komun.example.org/",
            "member@example.com",
            "Ada",
            "reset-token",
        )
        .expect("compose");
        let wire = render(&msg);
        let body = decoded_body(&wire);

        assert!(body.contains("/reset-password?token=reset-token"), "{body}");
        assert!(body.contains("30 minutes"), "{body}");
        assert!(
            body.contains("signs out every other device"),
            "the mail must say what the reset does:\n{body}"
        );
        assert!(
            !body.contains("verify-email"),
            "reset mail must not carry a verification link"
        );
    }

    #[test]
    fn trailing_slash_in_public_url_does_not_double_up() {
        assert_eq!(
            verification_link("https://example.org/", "t"),
            "https://example.org/verify-email?token=t"
        );
        assert_eq!(
            verification_link("https://example.org", "t"),
            "https://example.org/verify-email?token=t"
        );
    }

    #[test]
    fn token_is_percent_encoded_into_the_query_string() {
        // A token is URL-safe base64 in practice, but the encoder must not be the weak point if
        // that ever changes.
        let link = verification_link("https://example.org", "a b&c=d#e");
        assert_eq!(
            link,
            "https://example.org/verify-email?token=a%20b%26c%3Dd%23e"
        );
    }

    #[test]
    fn a_bad_recipient_is_an_error_not_a_panic() {
        let err = verification_message(&from(), "https://example.org", "not-an-address", "A", "t");
        assert!(err.is_err(), "malformed recipient must be rejected");
    }

    #[test]
    fn mailer_is_none_without_smtp_and_some_with_it() {
        let unconfigured: Config =
            toml::from_str("[registration]\nrequire_email_verification = false\n").expect("parse");
        assert!(Mailer::from_config(&unconfigured)
            .expect("no error")
            .is_none());

        let configured: Config = toml::from_str(
            "[email]\nsmtp_host = \"smtp.example.org\"\nfrom = \"Komun <noreply@example.org>\"\nstarttls = true\n",
        )
        .expect("parse");
        let mailer = Mailer::from_config(&configured)
            .expect("no error")
            .expect("configured SMTP yields a mailer");
        assert_eq!(
            mailer.from_address().email.to_string(),
            "noreply@example.org"
        );
    }

    #[test]
    fn a_malformed_from_address_fails_at_construction() {
        let bad: Config = toml::from_str(
            "[email]\nsmtp_host = \"smtp.example.org\"\nfrom = \"this is not an address\"\n",
        )
        .expect("parse");
        let err = Mailer::from_config(&bad)
            .expect_err("must not build")
            .to_string();
        assert!(
            err.contains("from is not a valid address"),
            "unhelpful: {err}"
        );
    }
}
