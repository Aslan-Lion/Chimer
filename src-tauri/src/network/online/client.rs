//! The host's outbound leg: a blocking TLS WebSocket client to the relay.
//!
//! One connection, one thread. `rustls::StreamOwned<ClientConnection, TcpStream>`
//! implements `Read + Write`, so the *reader* half of the M6 codec in
//! [`super::super::transport`] is reused verbatim for frames arriving from the
//! relay. Two things the M6 codec does not provide are implemented here, because
//! the host is a WebSocket **client** in this leg:
//!
//! * the client half of the opening handshake (write `GET …`, read the `101`),
//!   where the M6 module only has the server half; and
//! * RFC 6455 **client masking** on every outbound frame. The M6 writer is
//!   server-only and writes unmasked frames, which a conforming server must
//!   refuse; there is no way to reuse it for a client and no way to change it
//!   (`m7-online-networking.md` §14 keeps `transport.rs` untouched).
//!
//! Neither is a second frame *codec*: parsing, fragmentation and the
//! before-allocation size limit all remain in `transport::read_frame`.
//!
//! # No insecure path exists here
//!
//! `wss://` is verified against the **operating system trust store** through
//! `rustls-platform-verifier`. There is no "accept invalid certificate" switch,
//! no configurable verifier and no fallback from `wss://` to plaintext: a `ws://`
//! URL is rejected unless the host is loopback, which exists so a developer can
//! run the relay on the same machine.

use std::io::{self, Read, Write};
use std::net::{IpAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use rustls::{ClientConnection, StreamOwned};
use rustls_platform_verifier::ConfigVerifierExt;

use super::super::transport;

/// How long a single blocking read waits before the supervisor gets a chance to
/// drain the outbound queue. Also the granularity at which a stop request is
/// noticed, so it is short on purpose.
pub const IDLE_POLL: Duration = Duration::from_millis(250);

/// How long the TCP connect, the TLS handshake and the WebSocket upgrade may
/// each take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Largest handshake head the client will accept from the relay.
const MAX_HEAD_BYTES: usize = 16 * 1024;

/// Largest frame the client will accept from the relay.
///
/// A memory guard only. The relay's own cap is smaller by default
/// (`relay::config::DEFAULT_MAX_FRAME_BYTES`), and the application limit is
/// applied after unsealing.
pub const MAX_RELAY_FRAME_BYTES: usize = 96 * 1024;

/// True for an I/O error that just means "nothing yet".
pub fn is_timeout(error: &io::Error) -> bool {
    matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut)
}

// ---------------------------------------------------------------------------
// Relay URL
// ---------------------------------------------------------------------------

/// A validated relay origin, e.g. `wss://relay.example.com` or
/// `wss://relay.example.com/base`.
///
/// Parsed strictly rather than leniently: a URL that cannot be understood is
/// refused with a reason, never coerced into something that might connect
/// somewhere else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayUrl {
    secure: bool,
    host: String,
    port: u16,
    /// Empty, or a path beginning with `/` and carrying no trailing slash.
    base_path: String,
}

impl RelayUrl {
    /// Parses and validates a relay URL.
    ///
    /// `ws://` is accepted **only** when the host is loopback; every other
    /// insecure URL is refused, so the plaintext path cannot be reached by
    /// configuration.
    pub fn parse(raw: &str) -> Result<RelayUrl, String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err("A relay URL is required.".to_string());
        }

        let (scheme, rest) = trimmed
            .split_once("://")
            .ok_or_else(|| "The relay URL must include a scheme: wss:// (or ws:// for loopback).".to_string())?;
        let secure = match scheme.to_ascii_lowercase().as_str() {
            "wss" => true,
            "ws" => false,
            other => {
                return Err(format!(
                    "The relay URL scheme {other:?} is not supported; use wss:// (ws:// is loopback-only)."
                ));
            }
        };

        let (authority, path) = match rest.find('/') {
            Some(index) => (&rest[..index], &rest[index..]),
            None => (rest, ""),
        };

        if authority.is_empty() {
            return Err("The relay URL has no host.".to_string());
        }
        if authority.contains('@') {
            return Err("The relay URL must not carry credentials.".to_string());
        }
        if authority.contains('?') || authority.contains('#') || path.contains('?') || path.contains('#') {
            return Err("The relay URL must not carry a query string or fragment.".to_string());
        }

        let (host, port) = split_host_port(authority, secure)?;
        let base_path = normalise_path(path)?;

        if !secure && !is_loopback(&host) {
            return Err(format!(
                "An insecure ws:// relay URL is only allowed for a loopback host; {host:?} is not loopback."
            ));
        }

        Ok(RelayUrl {
            secure,
            host,
            port,
            base_path,
        })
    }

    /// True when this URL is `wss://`.
    pub fn is_secure(&self) -> bool {
        self.secure
    }

    /// The host component, without brackets for an IPv6 literal.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// `host:port`, with the port omitted when it is the scheme default.
    fn authority(&self) -> String {
        let default = if self.secure { 443 } else { 80 };
        if self.port == default {
            host_header(&self.host)
        } else {
            format!("{}:{}", host_header(&self.host), self.port)
        }
    }

    /// The request path for the host leg: the base path plus `/ws/host`.
    pub fn host_endpoint(&self) -> String {
        format!("{}/ws/host", self.base_path)
    }

    /// The canonical origin, for display and for the QR fragment.
    pub fn origin(&self) -> String {
        let scheme = if self.secure { "wss" } else { "ws" };
        format!("{scheme}://{}", self.authority())
    }
}

impl std::fmt::Display for RelayUrl {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.origin())
    }
}

fn split_host_port(authority: &str, secure: bool) -> Result<(String, u16), String> {
    let default_port = if secure { 443 } else { 80 };

    if let Some(rest) = authority.strip_prefix('[') {
        // An IPv6 literal: [::1] or [::1]:8443.
        let (inside, tail) = rest
            .split_once(']')
            .ok_or_else(|| "The relay URL has an unterminated IPv6 literal.".to_string())?;
        if inside.parse::<IpAddr>().is_err() {
            return Err(format!("The relay URL host {inside:?} is not a valid address."));
        }
        let port = match tail {
            "" => default_port,
            _ => {
                let digits = tail
                    .strip_prefix(':')
                    .ok_or_else(|| "The relay URL host is malformed.".to_string())?;
                parse_port(digits)?
            }
        };
        return Ok((inside.to_string(), port));
    }

    match authority.rsplit_once(':') {
        Some((host, digits)) if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) => {
            if host.is_empty() {
                return Err("The relay URL has no host.".to_string());
            }
            Ok((host.to_string(), parse_port(digits)?))
        }
        _ => Ok((authority.to_string(), default_port)),
    }
}

fn parse_port(digits: &str) -> Result<u16, String> {
    match digits.parse::<u16>() {
        Ok(0) | Err(_) => Err(format!("The relay URL port {digits:?} is not valid.")),
        Ok(port) => Ok(port),
    }
}

fn normalise_path(path: &str) -> Result<String, String> {
    if path.is_empty() || path == "/" {
        return Ok(String::new());
    }
    if !path.starts_with('/') {
        return Err("The relay URL path is malformed.".to_string());
    }
    let trimmed = path.trim_end_matches('/');
    for segment in trimmed.split('/') {
        if segment == "." || segment == ".." {
            return Err("The relay URL path must not contain relative segments.".to_string());
        }
    }
    Ok(trimmed.to_string())
}

fn is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .map(|address| address.is_loopback())
            .unwrap_or(false)
}

fn host_header(host: &str) -> String {
    if host.parse::<IpAddr>().is_ok_and(|address| address.is_ipv6()) {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

// ---------------------------------------------------------------------------
// Connection
// ---------------------------------------------------------------------------

/// The byte stream under the frames: TLS to the relay, or a plain socket for a
/// loopback `ws://` development relay.
enum Wire {
    Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
    Plain(TcpStream),
}

impl Read for Wire {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            Wire::Tls(stream) => stream.read(buffer),
            Wire::Plain(stream) => stream.read(buffer),
        }
    }
}

impl Write for Wire {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        match self {
            Wire::Tls(stream) => stream.write(buffer),
            Wire::Plain(stream) => stream.write(buffer),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Wire::Tls(stream) => stream.flush(),
            Wire::Plain(stream) => stream.flush(),
        }
    }
}

/// An open, upgraded WebSocket to the relay.
pub struct RelayConnection {
    wire: Wire,
    /// The relay origin, for diagnostics only. Never carries a secret.
    origin: String,
}

impl RelayConnection {
    /// Opens a connection and completes the WebSocket upgrade.
    ///
    /// Every failure — a refused TCP connection, a certificate the operating
    /// system does not trust, a non-`101` response, a missing or wrong
    /// `Sec-WebSocket-Accept` — is returned as an error. There is no retry with
    /// weaker settings.
    pub fn open(url: &RelayUrl) -> Result<RelayConnection, String> {
        let address = format!("{}:{}", host_header(url.host()), url.port());
        let socket = connect_tcp(&address, CONNECT_TIMEOUT)?;

        let _ = socket.set_nodelay(true);
        socket
            .set_read_timeout(Some(IDLE_POLL))
            .map_err(|error| format!("The relay socket could not be configured: {error}"))?;
        socket
            .set_write_timeout(Some(CONNECT_TIMEOUT))
            .map_err(|error| format!("The relay socket could not be configured: {error}"))?;

        let mut wire = if url.is_secure() {
            Wire::Tls(Box::new(tls_connect(url, socket)?))
        } else {
            Wire::Plain(socket)
        };

        upgrade(&mut wire, url)?;

        Ok(RelayConnection {
            wire,
            origin: url.origin(),
        })
    }

    /// The origin this connection was opened against.
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// Writes one masked text frame. Client frames are always masked
    /// (RFC 6455 §5.3).
    pub fn send_text(&mut self, text: &str) -> io::Result<()> {
        frame::text(&mut self.wire, text)
    }

    /// Writes one masked `pong`.
    pub fn send_pong(&mut self, payload: &[u8]) -> io::Result<()> {
        frame::pong(&mut self.wire, payload)
    }

    /// Writes one masked `close`.
    pub fn send_close(&mut self, code: u16, reason: &str) -> io::Result<()> {
        frame::close(&mut self.wire, code, reason)
    }

    /// Reads one message, reusing the M6 frame codec unchanged.
    pub fn read_frame(&mut self) -> io::Result<transport::Frame> {
        transport::read_frame(&mut self.wire, MAX_RELAY_FRAME_BYTES)
    }

    /// Reads one message, running `drain` whenever the read would otherwise
    /// block.
    ///
    /// One blocking socket cannot watch for inbound bytes *and* flush an
    /// outbound queue at the same time. The socket carries a short read timeout,
    /// so on every idle timeout `drain` is called and the *same*
    /// `read_frame` call is resumed. The frame parser is therefore never
    /// restarted and no partially-read frame is discarded — and no second parser
    /// exists.
    ///
    /// `drain` is given the writer half only; returning an error from it aborts
    /// the read, which the supervisor uses to stop promptly.
    pub fn read_frame_with<F>(&mut self, mut drain: F) -> io::Result<transport::Frame>
    where
        F: FnMut(&mut dyn Write) -> io::Result<()>,
    {
        let mut reader = DrainReader {
            wire: &mut self.wire,
            drain: &mut drain,
        };
        transport::read_frame(&mut reader, MAX_RELAY_FRAME_BYTES)
    }
}

/// Wraps the wire so that each idle timeout runs the drain callback before the
/// read is retried. Only `Read` is implemented here, so the frame parser can
/// never write to the socket.
struct DrainReader<'a, F> {
    wire: &'a mut Wire,
    drain: &'a mut F,
}

impl<F> Read for DrainReader<'_, F>
where
    F: FnMut(&mut dyn Write) -> io::Result<()>,
{
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        loop {
            match self.wire.read(buffer) {
                Ok(count) => return Ok(count),
                Err(error) if is_timeout(&error) => {
                    let drain = &mut *self.drain;
                    let wire: &mut dyn Write = &mut *self.wire;
                    drain(wire)?;
                }
                Err(error) => return Err(error),
            }
        }
    }
}

fn connect_tcp(address: &str, timeout: Duration) -> Result<TcpStream, String> {
    let addresses = address
        .to_socket_addrs()
        .map_err(|error| format!("The relay host {address:?} could not be resolved: {error}"))?;

    let mut last: Option<String> = None;
    for candidate in addresses {
        match TcpStream::connect_timeout(&candidate, timeout) {
            Ok(socket) => return Ok(socket),
            Err(error) => last = Some(error.to_string()),
        }
    }
    Err(match last {
        Some(error) => format!("The relay at {address:?} could not be reached: {error}"),
        None => format!("The relay host {address:?} resolved to no address."),
    })
}

fn tls_connect(url: &RelayUrl, socket: TcpStream) -> Result<StreamOwned<ClientConnection, TcpStream>, String> {
    let config = rustls::ClientConfig::with_platform_verifier()
        .map_err(|error| format!("The TLS verifier could not be initialised: {error}"))?;

    let server_name = rustls::pki_types::ServerName::try_from(url.host().to_string())
        .map_err(|_| format!("The relay host {:?} is not a valid TLS server name.", url.host()))?;

    let connection = ClientConnection::new(Arc::new(config), server_name)
        .map_err(|error| format!("The TLS session could not be created: {error}"))?;

    let mut stream = StreamOwned::new(connection, socket);

    // Complete the handshake explicitly so a certificate refusal is reported
    // here, before a single application byte is written.
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    while stream.conn.is_handshaking() {
        match stream.conn.complete_io(&mut stream.sock) {
            Ok(_) => {}
            Err(error) if is_timeout(&error) => {
                if Instant::now() >= deadline {
                    return Err(format!("The TLS handshake with {} timed out.", url.host()));
                }
            }
            Err(error) => return Err(format!("The TLS handshake with {} failed: {error}", url.host())),
        }
    }

    Ok(stream)
}

fn upgrade(wire: &mut Wire, url: &RelayUrl) -> Result<(), String> {
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).map_err(|error| format!("The system random number generator failed: {error}"))?;
    let key = STANDARD.encode(nonce);

    let request = format!(
        "GET {endpoint} HTTP/1.1\r\n\
         Host: {authority}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: {key}\r\n\
         Sec-WebSocket-Version: 13\r\n\r\n",
        endpoint = url.host_endpoint(),
        authority = url.authority(),
    );
    wire.write_all(request.as_bytes())
        .and_then(|_| wire.flush())
        .map_err(|error| format!("The relay did not accept the WebSocket request: {error}"))?;

    let head = read_head(wire)?;
    let mut lines = head.split("\r\n");
    let status = lines.next().unwrap_or("");
    let code = status
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| "The relay returned a malformed handshake response.".to_string())?;
    if code != 101 {
        return Err(format!("The relay refused the WebSocket upgrade with status {code}."));
    }

    let accept = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("sec-websocket-accept"))
        .map(|(_, value)| value.trim().to_string());
    if accept.as_deref() != Some(transport::websocket_accept(&key).as_str()) {
        return Err("The relay's WebSocket handshake carried no valid Sec-WebSocket-Accept.".to_string());
    }

    Ok(())
}

fn read_head(wire: &mut Wire) -> Result<String, String> {
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    let mut raw: Vec<u8> = Vec::with_capacity(256);
    let mut byte = [0u8; 1];

    loop {
        match wire.read(&mut byte) {
            Ok(0) => return Err("The relay closed the connection during the WebSocket handshake.".to_string()),
            Ok(_) => {
                raw.push(byte[0]);
                if raw.len() > MAX_HEAD_BYTES {
                    return Err("The relay's handshake response head is too large.".to_string());
                }
                if raw.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            Err(error) if is_timeout(&error) => {
                if Instant::now() >= deadline {
                    return Err("The relay did not answer the WebSocket handshake in time.".to_string());
                }
            }
            Err(error) => return Err(format!("The relay handshake could not be read: {error}")),
        }
    }

    String::from_utf8(raw).map_err(|_| "The relay's handshake response was not valid UTF-8.".to_string())
}

// ---------------------------------------------------------------------------
// Client-side frame writing
// ---------------------------------------------------------------------------

/// Writes client frames. Every frame is masked, as RFC 6455 §5.3 requires of a
/// client; the mask is drawn from the OS CSPRNG per frame.
pub mod frame {
    use std::io::{self, Write};

    const OP_TEXT: u8 = 0x1;
    const OP_CLOSE: u8 = 0x8;
    const OP_PONG: u8 = 0xA;

    /// One masked text frame.
    pub fn text(writer: &mut dyn Write, text: &str) -> io::Result<()> {
        masked(writer, OP_TEXT, text.as_bytes())
    }

    /// One masked `pong`, answering the relay's `ping`.
    pub fn pong(writer: &mut dyn Write, payload: &[u8]) -> io::Result<()> {
        masked(writer, OP_PONG, payload)
    }

    /// One masked `close`, carrying a status code and a short reason.
    pub fn close(writer: &mut dyn Write, code: u16, reason: &str) -> io::Result<()> {
        let mut payload = Vec::with_capacity(2 + reason.len());
        payload.extend_from_slice(&code.to_be_bytes());
        let reason = reason.as_bytes();
        payload.extend_from_slice(&reason[..reason.len().min(123)]);
        masked(writer, OP_CLOSE, &payload)
    }

    fn masked(writer: &mut dyn Write, opcode: u8, payload: &[u8]) -> io::Result<()> {
        let mut mask = [0u8; 4];
        getrandom::fill(&mut mask).map_err(|error| io::Error::other(error.to_string()))?;

        let mut header: Vec<u8> = Vec::with_capacity(14);
        header.push(0x80 | opcode);

        let length = payload.len();
        if length < 126 {
            header.push(0x80 | length as u8);
        } else if length <= u16::MAX as usize {
            header.push(0x80 | 126);
            header.extend_from_slice(&(length as u16).to_be_bytes());
        } else {
            header.push(0x80 | 127);
            header.extend_from_slice(&(length as u64).to_be_bytes());
        }
        header.extend_from_slice(&mask);

        let mut masked = Vec::with_capacity(payload.len());
        for (index, byte) in payload.iter().enumerate() {
            masked.push(byte ^ mask[index % 4]);
        }

        writer.write_all(&header)?;
        writer.write_all(&masked)?;
        writer.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn parse(raw: &str) -> Result<RelayUrl, String> {
        RelayUrl::parse(raw)
    }

    #[test]
    fn a_secure_relay_url_parses_to_its_defaults() {
        let url = parse("wss://relay.example.com").unwrap();
        assert!(url.is_secure());
        assert_eq!(url.host(), "relay.example.com");
        assert_eq!(url.port(), 443);
        assert_eq!(url.host_endpoint(), "/ws/host");
        assert_eq!(url.origin(), "wss://relay.example.com");
    }

    #[test]
    fn an_explicit_port_and_base_path_are_kept() {
        let url = parse("wss://relay.example.com:8443/base/").unwrap();
        assert_eq!(url.port(), 8443);
        assert_eq!(url.host_endpoint(), "/base/ws/host");
        assert_eq!(url.origin(), "wss://relay.example.com:8443");
    }

    #[test]
    fn ipv6_literals_are_understood() {
        let url = parse("wss://[2001:db8::1]:8443").unwrap();
        assert_eq!(url.host(), "2001:db8::1");
        assert_eq!(url.port(), 8443);
        assert_eq!(url.origin(), "wss://[2001:db8::1]:8443");
    }

    #[test]
    fn insecure_ws_is_allowed_only_for_loopback() {
        assert!(parse("ws://127.0.0.1:8723").is_ok());
        assert!(parse("ws://localhost:8723").is_ok());
        assert!(parse("ws://[::1]:8723").is_ok());

        let refused = parse("ws://relay.example.com").unwrap_err();
        assert!(refused.contains("loopback"), "{refused}");
        assert!(parse("ws://203.0.113.9:8723").is_err());
    }

    #[test]
    fn a_malformed_or_unsafe_url_is_refused() {
        for bad in [
            "",
            "relay.example.com",
            "https://relay.example.com",
            "wss://",
            "wss://user:pass@relay.example.com",
            "wss://relay.example.com:0",
            "wss://relay.example.com:99999",
            "wss://relay.example.com/path?x=1",
            "wss://relay.example.com#frag",
            "wss://relay.example.com/a/../b",
            "wss://[::1",
            "wss://[not-an-address]",
        ] {
            assert!(parse(bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn a_tls_connection_never_falls_back_to_plaintext() {
        // A plain TCP listener that never speaks TLS. A `wss://` client must
        // fail rather than silently degrade to an unencrypted socket.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();

        std::thread::spawn(move || {
            if let Ok((mut socket, _)) = listener.accept() {
                // Send something that is not a TLS record and close.
                let _ = socket.write_all(b"not a tls record");
                let _ = socket.shutdown(std::net::Shutdown::Both);
            }
        });

        let url = RelayUrl::parse(&format!("wss://127.0.0.1:{}", address.port())).unwrap();
        let result = RelayConnection::open(&url);
        assert!(result.is_err(), "a wss:// connection must not succeed against a non-TLS peer");
    }

    #[test]
    fn a_certificate_the_operating_system_does_not_trust_is_refused() {
        use rustls::pki_types::{CertificateDer, PrivateKeyDer};

        // A throwaway self-signed certificate, valid for `localhost` and not
        // installed in the operating system trust store. The client config must
        // refuse it: the design pins verification to the OS store with no
        // "accept invalid" path.
        let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("a self-signed certificate is generated");
        let certificate: CertificateDer<'static> = certified.cert.der().clone();
        let key = PrivateKeyDer::try_from(certified.signing_key.serialize_der())
            .expect("the generated key is a valid PKCS#8 key");

        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("the default protocol versions are available")
        .with_no_client_auth()
        .with_single_cert(vec![certificate], key)
        .expect("the server config accepts the test certificate");

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = std::thread::spawn(move || {
            if let Ok((socket, _)) = listener.accept() {
                let connection = rustls::ServerConnection::new(Arc::new(config)).expect("a server session");
                let mut stream = rustls::StreamOwned::new(connection, socket);
                // The client is expected to abort the handshake with an alert.
                while stream.conn.is_handshaking() {
                    if stream.conn.complete_io(&mut stream.sock).is_err() {
                        return;
                    }
                }
            }
        });

        let url = RelayUrl::parse(&format!("wss://localhost:{port}")).unwrap();
        let error = RelayConnection::open(&url)
            .err()
            .expect("an untrusted certificate must be refused");
        assert!(
            error.contains("TLS handshake") && error.contains("certificate"),
            "the refusal must be reported as a certificate failure, got: {error}"
        );

        server.join().ok();
    }

    #[test]
    fn a_client_frame_is_masked_as_rfc_6455_requires() {
        let mut output: Vec<u8> = Vec::new();
        frame::text(&mut output, "Hi").unwrap();

        assert_eq!(output[0], 0x81, "FIN bit set, opcode text");
        assert_eq!(output[1] & 0x80, 0x80, "the mask bit is set");
        assert_eq!(output[1] & 0x7F, 2, "the payload length is 2");
        assert_eq!(output.len(), 2 + 4 + 2, "header, mask and payload");

        let mask = [output[2], output[3], output[4], output[5]];
        let unmasked: Vec<u8> = output[6..]
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ mask[index % 4])
            .collect();
        assert_eq!(unmasked, b"Hi");
    }

    #[test]
    fn a_medium_client_frame_uses_the_16_bit_length() {
        let payload = vec![b'x'; 300];
        let mut output: Vec<u8> = Vec::new();
        frame::pong(&mut output, &payload).unwrap();

        assert_eq!(output[0], 0x80 | 0x0A);
        assert_eq!(output[1], 0x80 | 126);
        assert_eq!(u16::from_be_bytes([output[2], output[3]]), 300);
        assert_eq!(output.len(), 4 + 4 + 300);
    }
}
