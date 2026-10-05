//! The only path by which this server fetches a URL that a caller or a peer chose.
//!
//! Every hop is checked before it connects: http or https, no userinfo, and every address the
//! host resolves to must be public. The connection is pinned to an address that was checked, so a
//! second DNS answer cannot swap in a private one. No proxy is used, because a proxy resolves the
//! name itself and would bypass the pin.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use reqwest::{header::LOCATION, redirect::Policy, Url};

const MAX_REDIRECTS: usize = 3;

/// One deadline for the whole chain, body included: per-hop timeouts would let a redirect chain
/// or a slow-drip body multiply it.
const DEADLINE: Duration = Duration::from_secs(5);

pub(crate) const JSON_BODY_CAP: usize = 64 * 1024;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    max_body: usize,
    /// Keep the first `max_body` bytes instead of failing: the meta tags a preview needs sit in
    /// `<head>`.
    truncate: bool,
    deadline: Duration,
}

pub(crate) const LINK_PREVIEW: Limits = Limits {
    max_body: 512 * 1024,
    truncate: true,
    deadline: DEADLINE,
};

pub(crate) const NODE_INFO: Limits = Limits {
    max_body: JSON_BODY_CAP,
    truncate: false,
    deadline: DEADLINE,
};

/// Its `Display` never carries the URL: a link-preview or geocode URL can hold a caller's query.
#[derive(Debug)]
pub(crate) enum OutboundError {
    NotAllowed,
    Unresolvable,
    Request(reqwest::Error),
    Status(u16),
    TooManyRedirects,
    TooLarge,
    Timeout,
}

impl OutboundError {
    pub(crate) fn is_refused(&self) -> bool {
        matches!(self, OutboundError::NotAllowed)
    }

    fn request(e: reqwest::Error) -> Self {
        OutboundError::Request(e.without_url())
    }
}

impl fmt::Display for OutboundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OutboundError::NotAllowed => f.write_str("target not allowed"),
            OutboundError::Unresolvable => f.write_str("host did not resolve"),
            OutboundError::Request(e) => write!(f, "request failed: {e}"),
            OutboundError::Status(status) => write!(f, "upstream returned status {status}"),
            OutboundError::TooManyRedirects => f.write_str("too many redirects"),
            OutboundError::TooLarge => f.write_str("response body over the cap"),
            OutboundError::Timeout => f.write_str("deadline exceeded"),
        }
    }
}

/// Fetches `raw` under the production address policy and returns the body of a 2xx response.
pub(crate) async fn fetch_public(raw: &str, limits: Limits) -> Result<Vec<u8>, OutboundError> {
    fetch_with_policy(raw, limits, is_public_ip).await
}

/// Private so that `allow` can be widened only by this module's tests, to reach a loopback
/// server; production code reaches it only through [`fetch_public`].
async fn fetch_with_policy(
    raw: &str,
    limits: Limits,
    allow: fn(IpAddr) -> bool,
) -> Result<Vec<u8>, OutboundError> {
    let url = check_url(raw)?;
    tokio::time::timeout(limits.deadline, follow(url, limits, allow))
        .await
        .unwrap_or(Err(OutboundError::Timeout))
}

async fn follow(
    mut url: Url,
    limits: Limits,
    allow: fn(IpAddr) -> bool,
) -> Result<Vec<u8>, OutboundError> {
    let mut redirects = 0;
    loop {
        let response = send_pinned(&url, allow).await?;
        let status = response.status();

        if !status.is_redirection() {
            if !status.is_success() {
                return Err(OutboundError::Status(status.as_u16()));
            }
            return read_capped(response, limits.max_body, limits.truncate).await;
        }

        if redirects == MAX_REDIRECTS {
            return Err(OutboundError::TooManyRedirects);
        }
        let next = response
            .headers()
            .get(LOCATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|location| url.join(location).ok())
            .ok_or_else(|| OutboundError::Status(status.as_u16()))?;
        url = check_url(next.as_str())?;
        redirects += 1;
    }
}

async fn send_pinned(
    url: &Url,
    allow: fn(IpAddr) -> bool,
) -> Result<reqwest::Response, OutboundError> {
    let host = url.host_str().ok_or(OutboundError::NotAllowed)?;
    let port = url
        .port_or_known_default()
        .ok_or(OutboundError::NotAllowed)?;

    let mut builder = reqwest::Client::builder()
        .redirect(Policy::none())
        .no_proxy();

    match literal_ip(host) {
        Some(ip) => {
            if !allow(ip) {
                return Err(OutboundError::NotAllowed);
            }
        }
        None => {
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
                .await
                .map_err(|_| OutboundError::Unresolvable)?
                .collect();
            let Some(&first) = addrs.first() else {
                return Err(OutboundError::Unresolvable);
            };
            // One private answer refuses the host: which answer a client would use is not ours
            // to predict.
            if !addrs.iter().all(|addr| allow(addr.ip())) {
                return Err(OutboundError::NotAllowed);
            }
            builder = builder.resolve(host, first);
        }
    }

    let client = builder.build().map_err(OutboundError::request)?;
    client
        .get(url.clone())
        .send()
        .await
        .map_err(OutboundError::request)
}

fn literal_ip(host: &str) -> Option<IpAddr> {
    host.strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
        .unwrap_or(host)
        .parse()
        .ok()
}

/// Reads at most `cap` bytes. Past the cap, `truncate` keeps what was read; otherwise it is an
/// error, and a declared `Content-Length` over the cap is refused before any body is read.
pub(crate) async fn read_capped(
    mut response: reqwest::Response,
    cap: usize,
    truncate: bool,
) -> Result<Vec<u8>, OutboundError> {
    if !truncate
        && response
            .content_length()
            .is_some_and(|declared| declared > cap as u64)
    {
        return Err(OutboundError::TooLarge);
    }

    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(OutboundError::request)? {
        let room = cap - body.len();
        if chunk.len() > room {
            if truncate {
                body.extend_from_slice(&chunk[..room]);
                return Ok(body);
            }
            return Err(OutboundError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

pub(crate) fn check_url(raw: &str) -> Result<Url, OutboundError> {
    let url = Url::parse(raw).map_err(|_| OutboundError::NotAllowed)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(OutboundError::NotAllowed);
    }
    match url.host_str() {
        Some(host) if !host.is_empty() => {}
        _ => return Err(OutboundError::NotAllowed),
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(OutboundError::NotAllowed);
    }
    Ok(url)
}

/// Bitmask checks rather than the `std` helpers, several of which are unstable or narrower than
/// this list.
pub(crate) fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => is_public_v6(v6),
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, _, _] = ip.octets();
    let private = a == 0
        || a == 10
        || a == 127
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 100 && (64..=127).contains(&b))
        // 224/4 multicast, 240/4 reserved, and the broadcast address.
        || a >= 224;
    !private
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    if let Some(inner) = embedded_v4(ip) {
        return is_public_v4(inner);
    }
    let first = ip.segments()[0];
    let private = ip.is_unspecified()
        || ip.is_loopback()
        || first & 0xfe00 == 0xfc00
        || first & 0xffc0 == 0xfe80
        || first & 0xff00 == 0xff00;
    !private
}

/// The IPv4 address an IPv6 address reaches: IPv4-mapped, NAT64, and the deprecated
/// IPv4-compatible form, each of which a dual-stack host or a gateway delivers over IPv4.
fn embedded_v4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let s = ip.segments();
    let o = ip.octets();
    let inner = Ipv4Addr::new(o[12], o[13], o[14], o[15]);

    let mapped = s[..5] == [0; 5] && s[5] == 0xffff;
    let nat64 = s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0; 4];
    // `::` and `::1` share the compatible prefix but are their own addresses.
    let compatible = s[..6] == [0; 6] && !(s[6] == 0 && s[7] <= 1);

    (mapped || nat64 || compatible).then_some(inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Instant;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    fn loopback_only(ip: IpAddr) -> bool {
        ip.is_loopback()
    }

    struct Reply {
        bytes: Vec<u8>,
        hold_open: bool,
    }

    fn reply(status_line: &str, headers: &[String], body: &[u8]) -> Reply {
        let mut bytes = format!("HTTP/1.1 {status_line}\r\nConnection: close\r\n").into_bytes();
        for header in headers {
            bytes.extend_from_slice(header.as_bytes());
            bytes.extend_from_slice(b"\r\n");
        }
        bytes.extend_from_slice(b"\r\n");
        bytes.extend_from_slice(body);
        Reply {
            bytes,
            hold_open: false,
        }
    }

    fn redirect(location: &str) -> Reply {
        reply(
            "302 Found",
            &[format!("Location: {location}"), "Content-Length: 0".into()],
            b"",
        )
    }

    async fn read_head(socket: &mut TcpStream) -> String {
        let mut head = Vec::new();
        let mut buf = [0u8; 1024];
        while !head.windows(4).any(|w| w == b"\r\n\r\n") {
            match socket.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => head.extend_from_slice(&buf[..n]),
            }
        }
        String::from_utf8_lossy(&head).into_owned()
    }

    /// A loopback HTTP/1.1 server answering every request with `respond(request_head)`; the
    /// counter is the number of connections accepted.
    async fn serve(respond: fn(&str) -> Reply) -> (SocketAddr, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                counter.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let head = read_head(&mut socket).await;
                    let reply = respond(&head);
                    let _ = socket.write_all(&reply.bytes).await;
                    if reply.hold_open {
                        tokio::time::sleep(Duration::from_secs(30)).await;
                    }
                    let _ = socket.shutdown().await;
                });
            }
        });
        (addr, hits)
    }

    /// T1
    #[test]
    fn classifier_refuses_private_ranges_and_their_mapped_forms() {
        for raw in [
            "127.0.0.1",
            "10.0.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "169.254.1.1",
            "100.64.0.1",
            "100.127.255.255",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "::",
            "::1",
            "fc00::1",
            "fd00:ec2::254",
            "fe80::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "::ffff:169.254.169.254",
            "::ffff:100.64.0.1",
            "::ffff:192.168.0.1",
            "64:ff9b::a9fe:a9fe",
        ] {
            let ip: IpAddr = raw.parse().expect("test address parses");
            assert!(!is_public_ip(ip), "{raw} must be refused");
        }

        for raw in [
            "8.8.8.8",
            "1.1.1.1",
            "100.63.255.255",
            "100.128.0.1",
            "172.32.0.1",
            "2606:4700:4700::1111",
        ] {
            let ip: IpAddr = raw.parse().expect("test address parses");
            assert!(is_public_ip(ip), "{raw} must be allowed");
        }
    }

    /// T2
    #[test]
    fn check_url_allows_only_http_and_https_with_a_host_and_no_userinfo() {
        for raw in [
            "javascript:alert(1)",
            "data:text/html,x",
            "file:///etc/passwd",
            "ftp://h/",
            "http://user:pw@h/",
            "http:///",
        ] {
            assert!(
                matches!(check_url(raw), Err(OutboundError::NotAllowed)),
                "{raw} must be refused"
            );
        }

        assert!(check_url("https://example.org").is_ok());
        let upper = check_url("HTTP://EXAMPLE.ORG").expect("scheme and host are case-insensitive");
        assert_eq!(upper.scheme(), "http");
    }

    /// T3
    #[tokio::test]
    async fn production_policy_refuses_a_name_that_resolves_to_loopback() {
        let result = fetch_public("http://localhost:1/", NODE_INFO).await;
        assert!(
            matches!(result, Err(OutboundError::NotAllowed)),
            "localhost must be refused, got {result:?}"
        );
    }

    /// T4
    #[tokio::test]
    async fn every_redirect_hop_is_rechecked() {
        let (addr, hits) = serve(|_| redirect("http://169.254.169.254/latest/meta-data")).await;

        let result =
            fetch_with_policy(&format!("http://{addr}/"), LINK_PREVIEW, loopback_only).await;

        assert!(
            matches!(result, Err(OutboundError::NotAllowed)),
            "a redirect to the metadata address must be refused, got {result:?}"
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    /// T5
    #[tokio::test]
    async fn redirects_are_capped_and_relative_locations_resolve() {
        let (addr, hits) = serve(|_| redirect("/again")).await;
        let result =
            fetch_with_policy(&format!("http://{addr}/start"), NODE_INFO, loopback_only).await;
        assert!(
            matches!(result, Err(OutboundError::TooManyRedirects)),
            "got {result:?}"
        );
        assert_eq!(
            hits.load(Ordering::SeqCst),
            MAX_REDIRECTS + 1,
            "the first request plus one per followed redirect"
        );

        let (addr, _) = serve(|head| {
            if head.starts_with("GET /ok ") {
                reply("200 OK", &["Content-Length: 4".into()], b"done")
            } else {
                redirect("ok")
            }
        })
        .await;
        let body = fetch_with_policy(&format!("http://{addr}/start"), NODE_INFO, loopback_only)
            .await
            .expect("a relative redirect to a 200 must succeed");
        assert_eq!(body, b"done");
    }

    const ONE_MIB: usize = 1024 * 1024;

    /// T6
    #[tokio::test]
    async fn bodies_are_capped() {
        let (addr, _) = serve(|_| {
            reply(
                "200 OK",
                &[format!("Content-Length: {ONE_MIB}")],
                &vec![b'a'; ONE_MIB],
            )
        })
        .await;
        let url = format!("http://{addr}/");

        let body = fetch_with_policy(&url, LINK_PREVIEW, loopback_only)
            .await
            .expect("a preview keeps the head of an oversize body");
        assert_eq!(body.len(), LINK_PREVIEW.max_body);

        let result = fetch_with_policy(&url, NODE_INFO, loopback_only).await;
        assert!(
            matches!(result, Err(OutboundError::TooLarge)),
            "got {result:?}"
        );

        // No Content-Length: the cap is enforced while reading.
        let (addr, _) = serve(|_| reply("200 OK", &[], &vec![b'a'; ONE_MIB])).await;
        let result = fetch_with_policy(&format!("http://{addr}/"), NODE_INFO, loopback_only).await;
        assert!(
            matches!(result, Err(OutboundError::TooLarge)),
            "got {result:?}"
        );

        // A declared length over the cap is refused before a body that never arrives.
        let (addr, _) = serve(|_| Reply {
            hold_open: true,
            ..reply("200 OK", &["Content-Length: 10485760".into()], b"")
        })
        .await;
        let patient = Limits {
            deadline: Duration::from_secs(3),
            ..NODE_INFO
        };
        let result = fetch_with_policy(&format!("http://{addr}/"), patient, loopback_only).await;
        assert!(
            matches!(result, Err(OutboundError::TooLarge)),
            "got {result:?}"
        );
    }

    /// T7
    #[tokio::test]
    async fn a_silent_server_hits_the_deadline() {
        let (addr, _) = serve(|_| Reply {
            bytes: Vec::new(),
            hold_open: true,
        })
        .await;
        let short = Limits {
            deadline: Duration::from_millis(300),
            ..NODE_INFO
        };

        let started = Instant::now();
        let result = fetch_with_policy(&format!("http://{addr}/"), short, loopback_only).await;

        assert!(
            matches!(result, Err(OutboundError::Timeout)),
            "got {result:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the deadline must bound the wait"
        );
    }

    #[tokio::test]
    async fn read_capped_refuses_an_oversize_json_body() {
        let response = axum::http::Response::builder()
            .status(200)
            .body(vec![b' '; JSON_BODY_CAP + 1])
            .expect("build response");
        let result = read_capped(reqwest::Response::from(response), JSON_BODY_CAP, false).await;
        assert!(
            matches!(result, Err(OutboundError::TooLarge)),
            "got {result:?}"
        );
    }
}
