//! A second client on mpv's JSON IPC socket (mpv takes many), to ask for
//! the stream URL of the song it plays. Unix only for now: elsewhere
//! `connect` fails and the waveform stays off.

use std::io::{BufRead as _, BufReader, Write as _};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(not(unix))]
type UnixStream = std::fs::File;
use std::path::Path;
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
use anyhow::Context as _;
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

pub struct Ipc {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next: u64,
}

impl Ipc {
    #[cfg(not(unix))]
    pub fn connect(socket: &Path) -> Result<Self> {
        bail!("no mpv socket on this system ({})", socket.display())
    }

    #[cfg(unix)]
    pub fn connect(socket: &Path) -> Result<Self> {
        let stream = UnixStream::connect(socket)
            .with_context(|| format!("connecting to {}", socket.display()))?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        Ok(Self {
            reader: BufReader::new(stream.try_clone()?),
            writer: stream,
            next: 1,
        })
    }

    pub fn get(&mut self, property: &str) -> Result<Value> {
        self.command(json!(["get_property", property]))
    }

    fn command(&mut self, command: Value) -> Result<Value> {
        let id = self.next;
        self.next += 1;
        let line = json!({ "command": command, "request_id": id }).to_string();
        self.writer.write_all(format!("{line}\n").as_bytes())?;
        let mut reply = String::new();
        loop {
            reply.clear();
            if self.reader.read_line(&mut reply)? == 0 {
                bail!("mpv closed the socket");
            }
            let message: Value = serde_json::from_str(&reply)?;
            if message["request_id"] != id {
                continue;
            }
            if message["error"] != "success" {
                return Err(anyhow!("mpv: {}", message["error"]));
            }
            return Ok(message["data"].clone());
        }
    }
}
