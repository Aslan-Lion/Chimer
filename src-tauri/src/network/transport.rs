//! The socket layer: a minimal HTTP/1.1 reader, the WebSocket handshake, and
//! the RFC 6455 frame codec.
//!
//! Hand-rolled rather than pulled from a crate, for one reason: the whole
//! server only ever has to speak two things — serve the remote client's own
//! files over `GET`, and carry small JSON frames over one WebSocket per device.
//! A full HTTP stack would be a large amount of unused surface, and every byte
//! of parsing here is exercised by the tests below.
//!
//! Two invariants hold throughout:
//!
//! * a frame larger than [`super::model::MAX_MESSAGE_BYTES`] is refused *while
//!   reading the header*, before the payload is ever buffered.
//! * nothing in this module interprets a message. It moves bytes; the engine
//!   decides what they mean.

use std::collections::HashMap;
use std::io::{self, Read, Write};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha1::{Digest, Sha1};

/// The constant RFC 6455 appends to the client key before hashing it.
const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Largest request head the server will read. A handshake is a few hundred
/// bytes; anything approaching this is an attempt to make the server buffer.
const MAX_REQUEST_BYTES: usize = 16 * 1024;

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

/// A parsed request head. The body is never read: every request this server
/// accepts is a `GET`.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: String,
    /// Path without the query string, always starting with `/`.
    pub path: String,
    pub query: Option<String>,
    headers: HashMap<String, String>,
}

impl HttpRequest {
    /// Case-insensitive header lookup, as HTTP requires.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_ascii_lowercase()).map(String::as_str)
    }

    /// True when the request asks to upgrade this connection to a WebSocket.
    pub fn is_websocket_upgrade(&self) -> bool {
        let upgrade = self
            .header("upgrade")
            .is_some_and(|value| value.to_ascii_lowercase().contains("websocket"));
        let connection = self
            .header("connection")
            .is_some_and(|value| value.to_ascii_lowercase().contains("upgrade"));
        upgrade && connection && self.header("sec-websocket-key").is_some()
    }

    /// The client's WebSocket nonce.
    pub fn websocket_key(&self) -> Option<&str> {
        self.header("sec-websocket-key")
    }

    /// The browser's user agent, used to describe the device to the operator.
    pub fn user_agent(&self) -> String {
        self.header("user-agent").unwrap_or("").to_string()
    }
}

/// Reads and parses a request head.
///
/// Read one byte at a time on purpose: a buffered read could swallow the first
/// WebSocket frame that follows the handshake, and there is nowhere to put it.
pub fn read_request<R: Read>(reader: &mut R) -> Result<HttpRequest, String> {
    let mut raw: Vec<u8> = Vec::with_capacity(512);
    let mut byte = [0u8; 1];

    loop {
        match reader.read(&mut byte) {
            Ok(0) => return Err("The connection closed before a request arrived.".to_string()),
            Ok(_) => raw.push(byte[0]),
            Err(error) => return Err(format!("Could not read the request: {error}")),
        }
        if raw.len() > MAX_REQUEST_BYTES {
            return Err("The request head is too large.".to_string());
        }
        if raw.ends_with(b"\r\n\r\n") {
            break;
        }
    }

    let text = String::from_utf8_lossy(&raw);
    let mut lines = text.split("\r\n");

    let request_line = lines
        .next()
        .ok_or_else(|| "The request had no request line.".to_string())?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| "The request had no method.".to_string())?
        .to_ascii_uppercase();
    let target = parts
        .next()
        .ok_or_else(|| "The request had no target.".to_string())?;

    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path.to_string(), Some(query.to_string())),
        None => (target.to_string(), None),
    };
    let path = if path.starts_with('/') {
        path
    } else {
        format!("/{path}")
    };

    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }

    Ok(HttpRequest {
        method,
        path,
        query,
        headers,
    })
}

/// Computes the `Sec-WebSocket-Accept` value for a client nonce.
pub fn websocket_accept(key: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(WEBSOCKET_GUID.as_bytes());
    STANDARD.encode(hasher.finalize())
}

/// Writes the `101 Switching Protocols` response.
pub fn write_websocket_handshake<W: Write>(writer: &mut W, key: &str) -> io::Result<()> {
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\r\n",
        websocket_accept(key)
    );
    writer.write_all(response.as_bytes())?;
    writer.flush()
}

/// Writes a complete HTTP response.
///
/// `cache` is the `Cache-Control` value; assets are cached for a short while and
/// the shell is not cached at all, so a refreshed remote page always picks up
/// the current host.
pub fn write_http_response<W: Write>(
    writer: &mut W,
    status: &str,
    content_type: &str,
    cache: &str,
    body: &[u8],
) -> io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: {cache}\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    writer.write_all(head.as_bytes())?;
    writer.write_all(body)?;
    writer.flush()
}

// ---------------------------------------------------------------------------
// WebSocket frames
// ---------------------------------------------------------------------------

/// One fully assembled message from a client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// A complete text message.
    Text(String),
    /// A binary message. The protocol is JSON-only, so the engine refuses it —
    /// but it is surfaced rather than silently dropped, so the refusal is
    /// explicit.
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close,
}

const OP_CONTINUATION: u8 = 0x0;
const OP_TEXT: u8 = 0x1;
const OP_BINARY: u8 = 0x2;
const OP_CLOSE: u8 = 0x8;
const OP_PING: u8 = 0x9;
const OP_PONG: u8 = 0xA;

/// Reads one message, reassembling fragments.
///
/// `max_payload` is enforced on both a single frame and the running total of a
/// fragmented message, so a client cannot slip past the limit by splitting.
pub fn read_frame<R: Read>(reader: &mut R, max_payload: usize) -> io::Result<Frame> {
    let mut assembled: Vec<u8> = Vec::new();
    let mut assembling = false;

    loop {
        let mut header = [0u8; 2];
        reader.read_exact(&mut header)?;

        let fin = header[0] & 0x80 != 0;
        let opcode = header[0] & 0x0F;
        let masked = header[1] & 0x80 != 0;
        let mut length = u64::from(header[1] & 0x7F);

        if length == 126 {
            let mut extended = [0u8; 2];
            reader.read_exact(&mut extended)?;
            length = u64::from(u16::from_be_bytes(extended));
        } else if length == 127 {
            let mut extended = [0u8; 8];
            reader.read_exact(&mut extended)?;
            length = u64::from_be_bytes(extended);
        }

        // The limit is checked here, before the payload is allocated.
        if length > max_payload as u64 || assembled.len() as u64 + length > max_payload as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "frame exceeds the maximum message size",
            ));
        }

        let mut mask = [0u8; 4];
        if masked {
            reader.read_exact(&mut mask)?;
        }

        let mut payload = vec![0u8; length as usize];
        reader.read_exact(&mut payload)?;
        if masked {
            for (index, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[index % 4];
            }
        }

        match opcode {
            OP_CONTINUATION => {
                if !assembling {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "a continuation frame arrived with nothing to continue",
                    ));
                }
                assembled.extend_from_slice(&payload);
                if fin {
                    return Ok(Frame::Text(decode_text(assembled)?));
                }
            }
            OP_TEXT | OP_BINARY => {
                if assembling {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "a new data frame arrived before the previous message finished",
                    ));
                }
                if fin {
                    return if opcode == OP_TEXT {
                        Ok(Frame::Text(decode_text(payload)?))
                    } else {
                        Ok(Frame::Binary(payload))
                    };
                }
                assembling = true;
                assembled = payload;
            }
            OP_CLOSE => return Ok(Frame::Close),
            OP_PING => return Ok(Frame::Ping(payload)),
            OP_PONG => return Ok(Frame::Pong(payload)),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unknown WebSocket opcode",
                ));
            }
        }
    }
}

fn decode_text(payload: Vec<u8>) -> io::Result<String> {
    String::from_utf8(payload)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "a text frame was not valid UTF-8"))
}

/// Writes an unmasked frame. Server frames are never masked, per RFC 6455.
fn write_frame<W: Write>(writer: &mut W, opcode: u8, payload: &[u8]) -> io::Result<()> {
    let mut header: Vec<u8> = Vec::with_capacity(10);
    header.push(0x80 | opcode);

    let length = payload.len();
    if length < 126 {
        header.push(length as u8);
    } else if length <= u16::MAX as usize {
        header.push(126);
        header.extend_from_slice(&(length as u16).to_be_bytes());
    } else {
        header.push(127);
        header.extend_from_slice(&(length as u64).to_be_bytes());
    }

    writer.write_all(&header)?;
    writer.write_all(payload)?;
    writer.flush()
}

/// Writes a text frame.
pub fn write_text<W: Write>(writer: &mut W, text: &str) -> io::Result<()> {
    write_frame(writer, OP_TEXT, text.as_bytes())
}

/// Writes a `ping` frame.
pub fn write_ping<W: Write>(writer: &mut W, payload: &[u8]) -> io::Result<()> {
    write_frame(writer, OP_PING, payload)
}

/// Writes a `pong` frame.
pub fn write_pong<W: Write>(writer: &mut W, payload: &[u8]) -> io::Result<()> {
    write_frame(writer, OP_PONG, payload)
}

/// Writes a `close` frame carrying a status code and a short reason.
pub fn write_close<W: Write>(writer: &mut W, code: u16, reason: &str) -> io::Result<()> {
    let mut payload = Vec::with_capacity(2 + reason.len());
    payload.extend_from_slice(&code.to_be_bytes());
    // The reason is capped at 123 bytes so the control frame never exceeds the
    // 125-byte limit RFC 6455 places on control frames.
    let reason = reason.as_bytes();
    payload.extend_from_slice(&reason[..reason.len().min(123)]);
    write_frame(writer, OP_CLOSE, &payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn the_accept_key_matches_the_rfc_example() {
        // RFC 6455 §1.3.
        assert_eq!(
            websocket_accept("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn a_request_head_is_parsed() {
        let raw = "GET /remote/controller?x=1 HTTP/1.1\r\nHost: 10.0.0.5\r\nUpgrade: WebSocket\r\nConnection: keep-alive, Upgrade\r\nSec-WebSocket-Key: abc\r\nUser-Agent: Mozilla/5.0 (iPhone)\r\n\r\n";
        let mut cursor = Cursor::new(raw.as_bytes());
        let request = read_request(&mut cursor).expect("the head parses");

        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/remote/controller");
        assert_eq!(request.query.as_deref(), Some("x=1"));
        assert!(request.is_websocket_upgrade());
        assert_eq!(request.websocket_key(), Some("abc"));
        assert_eq!(request.user_agent(), "Mozilla/5.0 (iPhone)");
    }

    #[test]
    fn a_plain_get_is_not_mistaken_for_an_upgrade() {
        let raw = "GET / HTTP/1.1\r\nHost: 10.0.0.5\r\n\r\n";
        let mut cursor = Cursor::new(raw.as_bytes());
        let request = read_request(&mut cursor).expect("the head parses");
        assert!(!request.is_websocket_upgrade());
    }

    #[test]
    fn an_oversized_request_head_is_refused() {
        let raw = format!("GET / HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(MAX_REQUEST_BYTES));
        let mut cursor = Cursor::new(raw.as_bytes());
        assert!(read_request(&mut cursor).is_err());
    }

    #[test]
    fn frames_round_trip_in_both_directions() {
        for payload in ["{}", &"x".repeat(200), &"y".repeat(70_000)] {
            let mut buffer: Vec<u8> = Vec::new();
            write_text(&mut buffer, payload).expect("the frame is written");

            let mut cursor = Cursor::new(buffer);
            match read_frame(&mut cursor, 100_000).expect("the frame is read") {
                Frame::Text(text) => assert_eq!(text, payload),
                other => panic!("expected a text frame, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_masked_client_frame_is_unmasked() {
        // A client text frame "Hi" masked with the key 0x01020304.
        let frame = vec![
            0x81, 0x82, 0x01, 0x02, 0x03, 0x04, b'H' ^ 0x01, b'i' ^ 0x02,
        ];
        let mut cursor = Cursor::new(frame);
        match read_frame(&mut cursor, 1024).expect("the frame is read") {
            Frame::Text(text) => assert_eq!(text, "Hi"),
            other => panic!("expected a text frame, got {other:?}"),
        }
    }

    #[test]
    fn a_frame_over_the_limit_is_refused_before_its_payload_is_read() {
        // The header claims 8 bytes; the limit is 4. Only the header is present,
        // so a reader that buffered first would block instead of refusing.
        let frame = vec![0x81, 0x08, b'a', b'b', b'c', b'd'];
        let mut cursor = Cursor::new(frame);
        let error = read_frame(&mut cursor, 4).expect_err("the limit is enforced");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn a_fragmented_message_is_reassembled() {
        let mut frame: Vec<u8> = Vec::new();
        // "Hel" without FIN, then "lo" with FIN.
        frame.extend_from_slice(&[0x01, 0x03]);
        frame.extend_from_slice(b"Hel");
        frame.extend_from_slice(&[0x80, 0x02]);
        frame.extend_from_slice(b"lo");

        let mut cursor = Cursor::new(frame);
        match read_frame(&mut cursor, 1024).expect("the message is assembled") {
            Frame::Text(text) => assert_eq!(text, "Hello"),
            other => panic!("expected a text frame, got {other:?}"),
        }
    }

    #[test]
    fn a_continuation_without_a_start_is_refused() {
        let frame = vec![0x80, 0x02, b'h', b'i'];
        let mut cursor = Cursor::new(frame);
        assert!(read_frame(&mut cursor, 1024).is_err());
    }

    #[test]
    fn control_frames_are_surfaced() {
        let mut frame: Vec<u8> = Vec::new();
        frame.extend_from_slice(&[0x89, 0x02]);
        frame.extend_from_slice(b"hi");
        let mut cursor = Cursor::new(frame);
        assert_eq!(read_frame(&mut cursor, 1024).unwrap(), Frame::Ping(b"hi".to_vec()));

        let mut frame: Vec<u8> = vec![0x88, 0x00];
        let mut cursor = Cursor::new(std::mem::take(&mut frame));
        assert_eq!(read_frame(&mut cursor, 1024).unwrap(), Frame::Close);
    }

    #[test]
    fn invalid_utf8_in_a_text_frame_is_refused() {
        let frame = vec![0x81, 0x02, 0xff, 0xfe];
        let mut cursor = Cursor::new(frame);
        assert!(read_frame(&mut cursor, 1024).is_err());
    }

    #[test]
    fn an_http_response_carries_its_length() {
        let mut buffer: Vec<u8> = Vec::new();
        write_http_response(&mut buffer, "200 OK", "text/html", "no-store", b"hello")
            .expect("the response is written");
        let text = String::from_utf8(buffer).expect("the response is text");
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("Content-Length: 5\r\n"));
        assert!(text.ends_with("hello"));
    }
}
