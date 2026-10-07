//! SIGTERM, SIGINT and SIGHUP quit the way Ctrl+Q does, so the session is
//! saved (the backend's Drop doesn't run on a plain kill). A
//! second signal ends the process at once. Unix only; on Windows closing the
//! window or the tray's Quit is the way out.

#[cfg(unix)]
use tokio::signal::unix::{Signal, SignalKind, signal};
use ytfast::desktop::{Remote, Request};

#[cfg(not(unix))]
pub fn watch(_runtime: &tokio::runtime::Handle, _remote: Remote) {
    let _ = Request::Quit;
}

#[cfg(unix)]
pub fn watch(runtime: &tokio::runtime::Handle, remote: Remote) {
    runtime.spawn(async move {
        let (Ok(mut term), Ok(mut int), Ok(mut hup)) = (
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
            signal(SignalKind::hangup()),
        ) else {
            log::warn!("no signal handlers: a kill doesn't save the session");
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

#[cfg(unix)]
async fn next(term: &mut Signal, int: &mut Signal, hup: &mut Signal) -> &'static str {
    tokio::select! {
        _ = term.recv() => "SIGTERM",
        _ = int.recv() => "SIGINT",
        _ = hup.recv() => "SIGHUP",
    }
}
