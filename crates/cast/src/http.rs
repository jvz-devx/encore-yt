//! Just enough HTTP/1.1 for the relay (and the fake devices in the tests):
//! one request per connection, a bounded head, `Range: bytes=` with one
//! range. Receivers ask for little else.

use anyhow::{Context, Result, bail};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};

const MAX_HEAD: usize = 16 * 1024;
const MAX_BODY: usize = 1024 * 1024;

#[derive(Debug, Default)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Reads one request, or None when the peer closed before sending one.
pub async fn read_request<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
) -> Result<Option<Request>> {
    let mut head = Vec::new();
    loop {
        let available = reader.fill_buf().await.context("read request head")?;
        if available.is_empty() {
            if head.is_empty() {
                return Ok(None);
            }
            bail!("connection closed inside the request head");
        }
        let read = available
            .iter()
            .position(|&byte| byte == b'\n')
            .map_or(available.len(), |at| at + 1);
        if read > MAX_HEAD - head.len() {
            bail!("request head over {MAX_HEAD} bytes");
        }
        head.extend_from_slice(&available[..read]);
        reader.consume(read);
        if head.ends_with(b"\r\n\r\n") || head.ends_with(b"\n\n") {
            break;
        }
    }
    let head = String::from_utf8(head).context("request head is not UTF-8")?;
    let mut lines = head.lines();
    let mut first = lines.next().unwrap_or_default().split_whitespace();
    let mut request = Request {
        method: first.next().unwrap_or_default().to_owned(),
        path: first.next().unwrap_or_default().to_owned(),
        ..Request::default()
    };
    for line in lines {
        if let Some((key, value)) = line.split_once(':') {
            request
                .headers
                .push((key.trim().to_owned(), value.trim().to_owned()));
        }
    }
    let length: usize = request
        .header("content-length")
        .map(str::parse)
        .transpose()
        .context("invalid Content-Length")?
        .unwrap_or(0);
    if length > MAX_BODY {
        bail!("request body over {MAX_BODY} bytes");
    }
    request.body.resize(length, 0);
    reader
        .read_exact(&mut request.body)
        .await
        .context("read request body")?;
    Ok(Some(request))
}

/// What a `Range` header asks of a body of `len` bytes.
#[derive(Debug, PartialEq, Eq)]
pub enum Range {
    /// No range (or one this server ignores, such as several): the whole body.
    Full,
    /// Bytes `start..=end`.
    Part { start: u64, end: u64 },
    /// A range past the end: 416.
    Unsatisfiable,
}

pub fn range(header: Option<&str>, len: u64) -> Range {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else {
        return Range::Full;
    };
    if spec.contains(',') {
        return Range::Full;
    }
    let Some((first, last)) = spec.split_once('-') else {
        return Range::Full;
    };
    let (first, last) = (first.trim(), last.trim());
    let part = match (first.parse::<u64>(), last.parse::<u64>()) {
        // "bytes=-500": the last 500 bytes.
        (Err(_), Ok(suffix)) if first.is_empty() => {
            if suffix == 0 || len == 0 {
                return Range::Unsatisfiable;
            }
            (len.saturating_sub(suffix), len - 1)
        }
        (Ok(start), Err(_)) if last.is_empty() => (start, len.saturating_sub(1)),
        (Ok(start), Ok(end)) if end >= start => (start, end.min(len.saturating_sub(1))),
        _ => return Range::Full,
    };
    if part.0 >= len {
        return Range::Unsatisfiable;
    }
    Range::Part {
        start: part.0,
        end: part.1,
    }
}

/// A response head with `Connection: close` (one request per connection).
pub fn head(status: u16, reason: &str, headers: &[(&str, String)]) -> String {
    let mut out = format!("HTTP/1.1 {status} {reason}\r\n");
    for (key, value) in headers {
        out.push_str(&format!("{key}: {value}\r\n"));
    }
    out.push_str("Connection: close\r\n\r\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(range(None, 100), Range::Full);
        assert_eq!(
            range(Some("bytes=0-"), 100),
            Range::Part { start: 0, end: 99 }
        );
        assert_eq!(
            range(Some("bytes=10-19"), 100),
            Range::Part { start: 10, end: 19 }
        );
        assert_eq!(
            range(Some("bytes=90-200"), 100),
            Range::Part { start: 90, end: 99 }
        );
        assert_eq!(
            range(Some("bytes=-10"), 100),
            Range::Part { start: 90, end: 99 }
        );
        assert_eq!(range(Some("bytes=100-"), 100), Range::Unsatisfiable);
        assert_eq!(range(Some("bytes=0-1,5-6"), 100), Range::Full);
        assert_eq!(range(Some("items=0-1"), 100), Range::Full);
        assert_eq!(range(Some("bytes=5-1"), 100), Range::Full);
    }

    #[tokio::test]
    async fn a_request_with_a_body() {
        let raw = b"POST /ctl HTTP/1.1\r\nHost: x\r\nSOAPACTION: \"a#b\"\r\nContent-Length: 4\r\n\r\nbodyEXTRA";
        let mut reader = BufReader::new(&raw[..]);
        let request = read_request(&mut reader).await.unwrap().unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/ctl");
        assert_eq!(request.header("soapaction"), Some("\"a#b\""));
        assert_eq!(request.body, b"body");
        let mut empty = BufReader::new(&b""[..]);
        assert!(read_request(&mut empty).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn oversized_heads_are_rejected_before_reading_an_unbounded_line() {
        for suffix in ["", "\r\n\r\n", "\n\n"] {
            let raw = format!("GET / HTTP/1.1\r\nX: {}{suffix}", "x".repeat(MAX_HEAD));
            let mut reader = BufReader::new(raw.as_bytes());
            let error = read_request(&mut reader).await.unwrap_err();
            assert!(error.to_string().contains("request head over"));
        }
    }

    #[tokio::test]
    async fn a_head_at_the_limit_is_accepted() {
        let prefix = "GET / HTTP/1.1\r\nX: ";
        let raw = format!(
            "{prefix}{}\r\n\r\n",
            "x".repeat(MAX_HEAD - prefix.len() - 4)
        );
        let mut reader = BufReader::new(raw.as_bytes());
        assert!(read_request(&mut reader).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn invalid_body_lengths_are_not_treated_as_empty_requests() {
        for length in ["abc", "-1", "18446744073709551616", "1048577"] {
            let raw = format!("POST / HTTP/1.1\r\nContent-Length: {length}\r\n\r\n");
            let mut reader = BufReader::new(raw.as_bytes());
            assert!(read_request(&mut reader).await.is_err());
        }
    }
}
