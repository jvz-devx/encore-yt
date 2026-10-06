//! SIGTERM, SIGINT and SIGHUP quit the way Ctrl+Q does, so the session is
//! saved and mpv stops (the backend's Drop doesn't run on a plain kill). A
//! second signal ends the process at once.

use tokio::signal::unix::{Signal, SignalKind, signal};
use ytfast::desktop::{Remote, Request};

pub fn watch(runtime: &tokio::runtime::Handle, remote: Remote) {
    runtime.spawn(async move {
        let (Ok(mut term), Ok(mut int), Ok(mut hup)) = (
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
            signal(SignalKind::hangup()),
        ) else {
            log::warn!("no signal handlers: a kill leaves mpv playing");
            return;
        };
        let name = next(&mut term, &mut int, &mut hup).await;
        log::info!("{name}: quitting");
        remote.request(Request::Quit);
        let name = next(&mut term, &mut int, &mut hup).await;
        log::warn!("{name} again: exiting at once");
        std::process::exit(1);
    });
}

async fn next(term: &mut Signal, int: &mut Signal, hup: &mut Signal) -> &'static str {
    tokio::select! {
        _ = term.recv() => "SIGTERM",
        _ = int.recv() => "SIGINT",
        _ = hup.recv() => "SIGHUP",
    }
}
