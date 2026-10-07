//! One ytfast at a time, and the command line that drives it. A later launch
//! (`ytfast`, `ytfast next`, `ytfast open <link>`…) sends one line to the
//! running instance over a private socket in the runtime directory, waits
//! until it has been taken in, and exits.
//!
//! The protocol is one line per connection, a word and, for `open`, the link:
//! `show`, `toggle`, `play`, `pause`, `next`, `previous`,
//! `like`, `quit`, `open <link>`. The instance closes the connection once the
//! message is handed on, so the sender knows it arrived.
//!
//! On Windows the socket is a named pipe named after the runtime directory.

#[cfg(unix)]
use std::io::{BufRead, BufReader};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// Bring the window back (a relaunch, `ytfast show`).
    Show,
    Toggle,
    Play,
    Pause,
    Next,
    Previous,
    /// Like or unlike the playing song.
    Like,
    /// Quit for real: playback stops.
    Quit,
    /// Open a YouTube Music or YouTube link (see `links`).
    Open(String),
}

impl Message {
    /// The message a command-line word asks for; `open` takes `argument`.
    pub fn parse(word: &str, argument: Option<&str>) -> Option<Self> {
        Some(match word {
            "show" => Message::Show,
            "toggle" => Message::Toggle,
            "play" => Message::Play,
            "pause" => Message::Pause,
            "next" => Message::Next,
            "previous" => Message::Previous,
            "like" => Message::Like,
            "quit" => Message::Quit,
            "open" => Message::Open(
                argument
                    .map(str::trim)
                    .filter(|link| !link.is_empty() && !link.contains(char::is_whitespace))?
                    .to_owned(),
            ),
            _ => return None,
        })
    }

    fn line(&self) -> String {
        let word = match self {
            Message::Show => "show",
            Message::Toggle => "toggle",
            Message::Play => "play",
            Message::Pause => "pause",
            Message::Next => "next",
            Message::Previous => "previous",
            Message::Like => "like",
            Message::Quit => "quit",
            Message::Open(link) => return format!("open {link}\n"),
        };
        format!("{word}\n")
    }
}

/// Sends `message` to a running instance and waits (briefly) until it has
/// been taken in. `false` when no instance answers.
#[cfg(unix)]
pub fn notify(runtime: &Path, message: &Message) -> bool {
    let Ok(mut stream) = UnixStream::connect(runtime.join("ytfast.sock")) else {
        return false;
    };
    if stream.write_all(message.line().as_bytes()).is_err() {
        return false;
    }
    let _ = stream.shutdown(std::net::Shutdown::Write);
    // The instance closes the connection after handing the message on.
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.read(&mut [0u8; 16]);
    true
}

/// Claims the socket and forwards messages from later launches.
#[cfg(unix)]
pub fn listen(
    runtime: &Path,
    on_message: impl Fn(Message) + Send + 'static,
) -> std::io::Result<()> {
    let path = runtime.join("ytfast.sock");
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    std::thread::Builder::new()
        .name("ytfast-instance".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut line = String::new();
                if BufReader::new(&stream)
                    .take(4096)
                    .read_line(&mut line)
                    .is_ok()
                {
                    deliver(&line, &on_message);
                }
                // Dropping the stream tells the sender its message arrived.
            }
        })?;
    Ok(())
}

/// The pipe's name: one per user, since the runtime directory is.
#[cfg(windows)]
fn pipe_name(runtime: &Path) -> String {
    format!(
        r"\\.\pipe\ytfast-{}",
        crate::paths::hash(&runtime.to_string_lossy())
    )
}

#[cfg(windows)]
pub fn notify(runtime: &Path, message: &Message) -> bool {
    let Ok(mut pipe) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(pipe_name(runtime))
    else {
        return false;
    };
    if pipe.write_all(message.line().as_bytes()).is_err() {
        return false;
    }
    // The instance disconnects after handing the message on.
    let _ = pipe.read(&mut [0u8; 16]);
    true
}

#[cfg(windows)]
pub fn listen(
    runtime: &Path,
    on_message: impl Fn(Message) + Send + 'static,
) -> std::io::Result<()> {
    use tokio::io::AsyncBufReadExt;
    use tokio::net::windows::named_pipe::ServerOptions;
    let name = pipe_name(runtime);
    let reactor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    // The first instance claims the name; a second one fails here.
    let mut server = {
        let _entered = reactor.enter();
        ServerOptions::new()
            .first_pipe_instance(true)
            .create(&name)?
    };
    std::thread::Builder::new()
        .name("ytfast-instance".into())
        .spawn(move || {
            reactor.block_on(async move {
                loop {
                    if server.connect().await.is_err() {
                        continue;
                    }
                    let connected = server;
                    server = match ServerOptions::new().create(&name) {
                        Ok(next) => next,
                        Err(error) => {
                            log::warn!("the instance pipe closed: {error}");
                            return;
                        }
                    };
                    let mut line = String::new();
                    let mut reader = tokio::io::BufReader::new(connected);
                    let read =
                        tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut line));
                    if matches!(read.await, Ok(Ok(_))) {
                        deliver(&line, &on_message);
                    }
                    // Dropping the pipe tells the sender its message arrived.
                }
            });
        })?;
    Ok(())
}

fn deliver(line: &str, on_message: &impl Fn(Message)) {
    let line = line.trim();
    let (word, argument) = match line.split_once(' ') {
        Some((word, argument)) => (word, Some(argument)),
        None => (line, None),
    };
    match Message::parse(word, argument) {
        Some(message) => on_message(message),
        None => log::warn!("ignored an unknown instance message"),
    }
}
