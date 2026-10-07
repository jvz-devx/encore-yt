//! A tiny static file server with `Range` support and an optional rate
//! limit, which logs every request (to show the player's range requests).
//!
//! cargo run -p ytfast-audio --example serve -- <dir> [port] [bytes per second]

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().unwrap_or_else(|| ".".into()));
    let port: u16 = args.next().and_then(|p| p.parse().ok()).unwrap_or(8765);
    let rate: u64 = args.next().and_then(|r| r.parse().ok()).unwrap_or(0);
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    eprintln!(
        "serving {} on http://127.0.0.1:{port}/ (rate {rate} B/s, 0 = unlimited)",
        dir.display()
    );
    let start = Instant::now();
    for (n, stream) in listener.incoming().enumerate() {
        let (stream, dir) = (stream?, dir.clone());
        thread::spawn(move || {
            if let Err(e) = serve(stream, &dir, rate, n, start) {
                eprintln!("[{:8.3}] #{n} {e}", start.elapsed().as_secs_f64());
            }
        });
    }
    Ok(())
}

fn serve(
    stream: TcpStream,
    dir: &Path,
    rate: u64,
    n: usize,
    start: Instant,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut out = stream;
    // Keep-alive: one connection may carry several requests.
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
        let mut range = None;
        loop {
            let mut header = String::new();
            reader.read_line(&mut header)?;
            let header = header.trim_end();
            if header.is_empty() {
                break;
            }
            if let Some(v) = header.to_ascii_lowercase().strip_prefix("range: bytes=") {
                range = Some(v.to_owned());
            }
        }
        let name = path.trim_start_matches('/').split('?').next().unwrap_or("");
        let file = dir.join(name);
        let Ok(mut f) = File::open(&file) else {
            write!(out, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")?;
            continue;
        };
        let len = f.metadata()?.len();
        let (from, to) = parse_range(range.as_deref(), len);
        let t = start.elapsed().as_secs_f64();
        eprintln!(
            "[{t:8.3}] #{n} GET {name} Range: {} -> {from}-{to}/{len}",
            range.as_deref().unwrap_or("-")
        );
        let body = to + 1 - from;
        let status = if range.is_some() {
            "206 Partial Content"
        } else {
            "200 OK"
        };
        write!(
            out,
            "HTTP/1.1 {status}\r\nContent-Type: {}\r\nAccept-Ranges: bytes\r\nContent-Range: bytes {from}-{to}/{len}\r\nContent-Length: {body}\r\n\r\n",
            mime(name)
        )?;
        f.seek(SeekFrom::Start(from))?;
        let mut left = body;
        let mut buf = vec![0u8; 16 << 10];
        let began = Instant::now();
        let mut sent = 0u64;
        while left > 0 {
            let want = left.min(buf.len() as u64) as usize;
            let k = f.read(&mut buf[..want])?;
            if k == 0 {
                break;
            }
            if let Err(e) = out.write_all(&buf[..k]) {
                let t = start.elapsed().as_secs_f64();
                eprintln!("[{t:8.3}] #{n} client dropped the response after {sent} bytes ({e})");
                return Ok(());
            }
            sent += k as u64;
            left -= k as u64;
            if rate > 0 {
                let due = Duration::from_secs_f64(sent as f64 / rate as f64);
                if let Some(wait) = due.checked_sub(began.elapsed()) {
                    thread::sleep(wait);
                }
            }
        }
    }
}

fn parse_range(range: Option<&str>, len: u64) -> (u64, u64) {
    let Some((a, b)) = range.and_then(|r| r.split_once('-')) else {
        return (0, len.saturating_sub(1));
    };
    let from = a.parse().unwrap_or(0).min(len.saturating_sub(1));
    let to = b.parse().unwrap_or(len - 1).min(len - 1);
    (from, to.max(from))
}

fn mime(name: &str) -> &'static str {
    match name.rsplit('.').next() {
        Some("webm") => "audio/webm",
        Some("m4a") | Some("mp4") => "audio/mp4",
        _ => "application/octet-stream",
    }
}
