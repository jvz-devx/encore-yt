//! The command line: `encore-yt [command]`. A launch while Encore runs
//! hands its message to the running instance (`single_instance`) and
//! exits.

use encore_core::paths::Paths;
use encore_core::single_instance::{self, Message};

const USAGE: &str = "usage: encore-yt [command]

Without a command, opens Encore (or brings back the running one).

  show               bring back the window
  toggle             play or pause
  play | pause
  next | previous
  like               like or unlike the playing song
  open <link>        open a YouTube Music or YouTube link (starts Encore if needed)
  quit               quit Encore, stopping playback
  --version          print the version";

/// What this process starts with, once no other instance took the message.
pub struct Launch {
    /// A link to open once the app runs (`open <link>` with none running).
    pub link: Option<String>,
}

/// Reads the arguments. `None` when the process is done: the message went
/// to the running instance, or help was printed. Bad arguments exit.
/// `args` are the arguments after the program's name.
pub fn command_line(paths: &Paths, args: Vec<String>) -> Option<Launch> {
    let Some(word) = args.first().map(String::as_str) else {
        if single_instance::notify(&paths.runtime, &Message::Show) {
            return None;
        }
        return Some(Launch { link: None });
    };
    // The update helper asks a downloaded version this (M16).
    if word == "--version" {
        println!("encore-yt {}", crate::update::VERSION);
        return None;
    }
    if matches!(word, "-h" | "--help" | "help") {
        println!("{USAGE}");
        return None;
    }
    let Some(message) = Message::parse(word, args.get(1).map(String::as_str)) else {
        eprintln!("encore-yt: unknown command {:?}\n{USAGE}", args.join(" "));
        std::process::exit(2);
    };
    if let Message::Open(target) = &message
        && encore_core::links::target_from_link(target).is_none()
    {
        eprintln!("encore-yt: not a YouTube Music or YouTube link: {target}");
        std::process::exit(2);
    }
    if single_instance::notify(&paths.runtime, &message) {
        return None;
    }
    match message {
        // Nothing running: start, and open the link or just show.
        Message::Open(link) => Some(Launch { link: Some(link) }),
        Message::Show => Some(Launch { link: None }),
        _ => {
            eprintln!("encore-yt: Encore isn't running");
            std::process::exit(1);
        }
    }
}
