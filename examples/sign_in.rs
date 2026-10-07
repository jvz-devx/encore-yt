//! Shows what ytfast can read from each sign-in source, then checks the
//! session it would use with YouTube Music. Prints counts and sources only,
//! never a cookie value, password or account name.
//!
//! `cargo run --example sign_in --no-default-features [-- <profile id> [<video id>]]`
//!
//! Signed in, it also browses Home and, given a video id, resolves that song
//! with the session and prints the format it got.

use ytfast::model::Account;

fn main() -> anyhow::Result<()> {
    let preferred = std::env::args().nth(1).filter(|p| !p.is_empty());
    let video_id = std::env::args().nth(2);
    let scratch = tempdir()?;
    println!("Sources:");
    for found in ytfast::auth::inspect(&scratch) {
        match &found.error {
            Some(error) => println!("  {} [{}]: can't read: {error}", found.label, found.id),
            None => println!(
                "  {} [{}]: {} cookies, {} undecryptable, {} portal-encrypted, password from {}, {}",
                found.label,
                found.id,
                found.cookies,
                found.undecryptable,
                found.portal,
                found.password.unwrap_or("-"),
                if found.signed_in {
                    "signed in"
                } else {
                    "not signed in"
                },
            ),
        }
    }
    let listed = ytfast::auth::profiles(&scratch);
    println!(
        "Profiles offered in Settings: {}",
        if listed.is_empty() {
            "none".to_owned()
        } else {
            listed
                .iter()
                .map(|p| p.label.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    let session = match ytfast::auth::load(&scratch, preferred.as_deref()) {
        Ok(session) => session,
        Err(error) => {
            println!("Signed out: {error:#}");
            let _ = std::fs::remove_dir_all(&scratch);
            return Ok(());
        }
    };
    let source = session.source.clone();
    let client = std::sync::Arc::new(ytfast::innertube::Client::new());
    client.set_session(Some(session));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    runtime.block_on(async {
        match client.verify(&source).await {
            Account::SignedIn { source, .. } => println!("Signed in (from {source})"),
            Account::SignedOut { reason } => return println!("Signed out: {reason}"),
            Account::Unverified { reason } => return println!("Not checked: {reason}"),
            Account::Checking => return,
        }
        match client.browse("FEmusic_home", None).await {
            Ok(home) => println!(
                "Home: logged_in={:?}, {} shelves",
                ytfast::parse::logged_in(&home),
                ytfast::parse::page(&home).shelves.len()
            ),
            Err(error) => println!("Home: {error}"),
        }
        if let Some(video_id) = &video_id {
            let resolver = std::sync::Arc::new(ytfast::resolver::Resolver::new(scratch.clone()));
            match ytfast::paths::Paths::new() {
                Ok(paths) => resolver.use_innertube(client.clone(), &paths),
                Err(error) => return println!("No directories: {error:#}"),
            }
            match resolver.resolve(video_id).await {
                Ok(stream) => println!(
                    "Resolved {video_id}: itag {} ({})",
                    stream.itag,
                    ytfast::resolver::describe(stream.itag)
                ),
                Err(error) => println!("Resolving {video_id} failed: {error:#}"),
            }
        }
    });
    let _ = std::fs::remove_dir_all(&scratch);
    Ok(())
}

/// A private directory for the cookie database copies.
fn tempdir() -> anyhow::Result<std::path::PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    let dir = std::env::temp_dir().join(format!("ytfast-sign-in-{}", std::process::id()));
    std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    Ok(dir)
}
