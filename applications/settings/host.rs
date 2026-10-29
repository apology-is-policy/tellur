// Mechanisms only: validation, defaults and failure policy live in settings.tellur.
use crate::rust_api;
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);

pub fn text_trim(text: &str) -> rust_api::text_trim_outcome {
    rust_api::text_trim_outcome::trimmed(text.trim().to_owned())
}

// A sequential HTTP/1.1 profile: one bounded request per connection. These
// mechanisms do not know the application's routes, keys or response policy.
pub use std::net::TcpListener as Listener;
pub struct Connection {
    stream: std::net::TcpStream,
    head: std::cell::Cell<bool>,
}
const HEADER_LIMIT: usize = 16384;
const BODY_LIMIT: usize = 1048576;
const IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

pub fn http_listen(port: u32) -> rust_api::http_listen_outcome {
    use rust_api::http_listen_outcome::*;
    let result = (|| -> std::io::Result<_> {
        let port = u16::try_from(port).map_err(|_| std::io::Error::other("invalid port"))?;
        let listener = Listener::bind((std::net::Ipv4Addr::LOCALHOST, port))?;
        listener.set_nonblocking(true)?;
        let authority = listener.local_addr()?.to_string();
        Ok((listener, authority))
    })();
    match result {
        Ok((socket, address)) => listening(socket, address),
        Err(e) => failed(e.to_string()),
    }
}
pub fn http_accept(listener: &Listener, idle_ms: u32) -> rust_api::http_accept_outcome {
    use rust_api::http_accept_outcome::*;
    if idle_ms == 0 || idle_ms > 60000 {
        return failed("idle timeout outside 1..60000 ms".into());
    }
    let until = std::time::Instant::now() + std::time::Duration::from_millis(idle_ms.into());
    loop {
        if std::time::Instant::now() >= until {
            return idle;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if let Err(e) = stream.set_nonblocking(false) {
                    return failed(e.to_string());
                }
                return accepted(Connection {
                    stream,
                    head: std::cell::Cell::new(false),
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(
                    std::time::Duration::from_millis(5)
                        .min(until.saturating_duration_since(std::time::Instant::now())),
                );
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return failed(e.to_string()),
        }
    }
}
fn io_failure(e: std::io::Error) -> rust_api::http_receive_outcome {
    use rust_api::http_receive_outcome::*;
    match e.kind() {
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => timed_out,
        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted => disconnected,
        _ => failed(e.to_string()),
    }
}
fn read_before(
    stream: &std::net::TcpStream,
    bytes: &mut [u8],
    until: std::time::Instant,
) -> std::io::Result<usize> {
    let mut stream = stream;
    loop {
        let remaining = until
            .checked_duration_since(std::time::Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::TimedOut))?;
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|e| std::io::Error::new(e.kind(), format!("set read timeout: {e}")))?;
        match stream.read(bytes) {
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
}
pub fn http_receive(connection: &Connection) -> rust_api::http_receive_outcome {
    use rust_api::http_receive_outcome::*;
    let until = std::time::Instant::now() + IO_TIMEOUT;
    let mut input = Vec::new();
    let (offset, method, target, headers) = loop {
        let mut slots = [httparse::EMPTY_HEADER; 64];
        let mut parsed = httparse::Request::new(&mut slots);
        let result = parsed.parse(&input);
        connection.head.set(parsed.method == Some("HEAD"));
        match result {
            Ok(httparse::Status::Complete(offset)) => {
                if offset > HEADER_LIMIT {
                    return headers_too_large;
                }
                // httparse tolerates lone LF; this bounded profile requires CRLF.
                if input[..offset].iter().enumerate().any(|(i, b)| {
                    (*b == b'\n' && (i == 0 || input[i - 1] != b'\r'))
                        || (*b == b'\r' && input.get(i + 1) != Some(&b'\n'))
                }) {
                    return malformed("headers require CRLF".into());
                }
                if parsed.version != Some(1) {
                    return malformed("HTTP/1.1 required".into());
                }
                let mut headers = BTreeMap::new();
                for header in parsed.headers.iter() {
                    let value = match std::str::from_utf8(header.value) {
                        Ok(v) => v.trim_matches([' ', '\t']),
                        Err(_) => return malformed("header is not UTF-8".into()),
                    };
                    if headers
                        .insert(header.name.to_ascii_lowercase(), value.to_owned())
                        .is_some()
                    {
                        return malformed("duplicate headers are unsupported".into());
                    }
                }
                if headers.get("host").is_none_or(|v| v.is_empty()) {
                    return malformed("one Host header is required".into());
                }
                break (
                    offset,
                    parsed.method.unwrap().to_owned(),
                    parsed.path.unwrap().to_owned(),
                    headers,
                );
            }
            Ok(httparse::Status::Partial) => {
                if input.len() >= HEADER_LIMIT {
                    return headers_too_large;
                }
            }
            Err(httparse::Error::TooManyHeaders) => return headers_too_large,
            Err(e) => return malformed(e.to_string()),
        }
        let mut chunk = [0; 2048];
        let limit = chunk.len().min(HEADER_LIMIT - input.len());
        match read_before(&connection.stream, &mut chunk[..limit], until) {
            Ok(0) if input.is_empty() => return disconnected,
            Ok(0) => return malformed("incomplete headers".into()),
            Ok(n) => input.extend_from_slice(&chunk[..n]),
            Err(e) => return io_failure(e),
        }
    };
    if !target.starts_with('/') || target.starts_with("//") || target.contains('#') {
        return malformed("origin-form request target required".into());
    }
    if headers.contains_key("transfer-encoding") && headers.contains_key("content-length") {
        return malformed("conflicting message framing".into());
    }
    if headers.contains_key("transfer-encoding") {
        return unsupported_transfer;
    }
    if headers.contains_key("expect") {
        return unsupported_expectation;
    }
    let length = match headers.get("content-length") {
        None => 0,
        Some(s) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => match s.parse::<u64>()
        {
            Ok(n) if n <= BODY_LIMIT as u64 => n as usize,
            _ => return body_too_large,
        },
        Some(_) => return malformed("invalid Content-Length".into()),
    };
    let mut body = input[offset..]
        .iter()
        .copied()
        .take(length)
        .collect::<Vec<_>>();
    while body.len() < length {
        let mut chunk = [0; 4096];
        let limit = chunk.len().min(length - body.len());
        match read_before(&connection.stream, &mut chunk[..limit], until) {
            Ok(0) => return malformed("incomplete body".into()),
            Ok(n) => body.extend_from_slice(&chunk[..n]),
            Err(e) => return io_failure(e),
        }
    }
    match String::from_utf8(body) {
        Ok(body) => request(method, target, headers, body),
        Err(_) => malformed("body is not UTF-8".into()),
    }
}
pub fn http_send(
    connection: &Connection,
    status: u32,
    body: &str,
    media: &str,
    allow: &str,
) -> rust_api::http_send_outcome {
    use rust_api::http_send_outcome::*;
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        417 => "Expectation Failed",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        _ => return failed("unsupported response status".into()),
    };
    let valid_allow = allow.is_empty()
        || (allow.len() <= 256
            && allow.split(',').all(|method| {
                let method = method.trim_matches([' ', '\t']);
                !method.is_empty()
                    && method
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
            }));
    if !["application/json", "text/plain; charset=utf-8"].contains(&media)
        || !valid_allow
        || body.len() > 2 * BODY_LIMIT
    {
        return failed("unsupported response metadata or response exceeds 2 MiB".into());
    }
    let allow = if allow.is_empty() {
        String::new()
    } else {
        format!("Allow: {allow}\r\n")
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nContent-Type: {media}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n{allow}\r\n",
        body.len()
    );
    let until = std::time::Instant::now() + IO_TIMEOUT;
    let result = (|| -> std::io::Result<()> {
        let mut stream = &connection.stream;
        for mut remaining in [
            header.as_bytes(),
            if connection.head.get() {
                b""
            } else {
                body.as_bytes()
            },
        ] {
            while !remaining.is_empty() {
                let timeout = until
                    .checked_duration_since(std::time::Instant::now())
                    .filter(|d| !d.is_zero())
                    .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::TimedOut))?;
                stream.set_write_timeout(Some(timeout))?;
                match stream.write(remaining) {
                    Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
                    Ok(n) => remaining = &remaining[n..],
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => sent,
        Err(e) => failed(e.to_string()),
    }
}
pub fn http_connection_close(connection: Connection) -> rust_api::http_connection_close_outcome {
    // Half-close first so unread rejected input cannot reset a queued response.
    // Drain only a bounded amount/time; never process a second request.
    let result = (|| -> std::io::Result<()> {
        connection.stream.set_nonblocking(true)?;
        match connection.stream.shutdown(std::net::Shutdown::Write) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotConnected => return Ok(()),
            Err(e) => {
                return Err(std::io::Error::new(
                    e.kind(),
                    format!("write-half shutdown: {e}"),
                ));
            }
        }
        let until = std::time::Instant::now() + std::time::Duration::from_millis(100);
        let mut remaining = BODY_LIMIT + HEADER_LIMIT;
        let mut chunk = [0; 4096];
        let mut stream = &connection.stream;
        while remaining > 0 && std::time::Instant::now() < until {
            let limit = remaining.min(chunk.len());
            match stream.read(&mut chunk[..limit]) {
                Ok(0) => break,
                Ok(n) => remaining -= n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(
                        std::time::Duration::from_millis(5)
                            .min(until.saturating_duration_since(std::time::Instant::now())),
                    );
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::ConnectionAborted
                    ) =>
                {
                    break;
                }
                Err(e) => {
                    return Err(std::io::Error::new(
                        e.kind(),
                        format!("draining connection: {e}"),
                    ));
                }
            }
        }
        Ok(())
    })();
    drop(connection);
    match result {
        Ok(()) => rust_api::http_connection_close_outcome::closed,
        Err(e) if e.kind() == std::io::ErrorKind::NotConnected => {
            rust_api::http_connection_close_outcome::closed
        }
        Err(e) => rust_api::http_connection_close_outcome::failed(e.to_string()),
    }
}
pub fn http_listener_release(listener: Listener) -> rust_api::http_listener_release_outcome {
    drop(listener);
    rust_api::http_listener_release_outcome::released
}
pub fn file_read(path: &str, limit: u32) -> rust_api::file_read_outcome {
    use rust_api::file_read_outcome::*;
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return missing,
        Err(e) => return read_failed(e.to_string()),
    };
    let mut bytes = Vec::new();
    if let Err(e) = file.take(u64::from(limit) + 1).read_to_end(&mut bytes) {
        return read_failed(e.to_string());
    }
    if bytes.len() > limit as usize {
        return too_large;
    }
    match String::from_utf8(bytes) {
        Ok(text) => read(text),
        Err(e) => read_failed(e.to_string()),
    }
}
// Reject duplicate keys as an encoding error, instead of silently keeping one value.
struct Object(BTreeMap<String, String>);
impl<'de> serde::Deserialize<'de> for Object {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Object;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an object with unique keys and string values")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Object, A::Error> {
                let mut values = BTreeMap::new();
                while let Some((key, value)) = map.next_entry::<String, String>()? {
                    if values.insert(key.clone(), value).is_some() {
                        return Err(serde::de::Error::custom(format!("duplicate key: {key}")));
                    }
                }
                Ok(Object(values))
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}
pub fn json_decode(text: String) -> rust_api::json_decode_outcome {
    match serde_json::from_str::<Object>(&text) {
        Ok(Object(values)) => rust_api::json_decode_outcome::decoded(values),
        Err(e) => rust_api::json_decode_outcome::invalid(e.to_string()),
    }
}
pub fn json_encode(values: &BTreeMap<String, String>) -> rust_api::json_encode_outcome {
    match serde_json::to_string_pretty(values) {
        Ok(text) => rust_api::json_encode_outcome::encoded(text),
        Err(e) => rust_api::json_encode_outcome::encode_failed(e.to_string()),
    }
}
pub fn file_replace(path: &str, text: &str, limit: u32) -> rust_api::file_replace_outcome {
    use rust_api::file_replace_outcome::*;
    if text.len() > limit as usize {
        return too_large;
    }
    let destination = Path::new(path);
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut pending = None;
    for _ in 0..32 {
        let temporary = parent.join(format!(
            ".tellur-settings-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => {
                pending = Some((temporary, file));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return write_failed(e.to_string()),
        }
    }
    let Some((temporary, mut file)) = pending else {
        return write_failed("temporary-file names are occupied".into());
    };
    let cleanup = |mut message: String| {
        if let Err(e) = fs::remove_file(&temporary) {
            message.push_str(&format!("; cleanup of {} failed: {e}", temporary.display()));
        }
        message
    };
    let written = file
        .write_all(text.as_bytes())
        .and_then(|_| file.sync_all());
    drop(file);
    if let Err(e) = written {
        return write_failed(cleanup(e.to_string()));
    }
    // Once replacement has been attempted, an error is not a promise of rollback.
    match fs::rename(&temporary, destination) {
        Ok(()) => replaced,
        Err(e) => commit_uncertain(cleanup(e.to_string())),
    }
}
