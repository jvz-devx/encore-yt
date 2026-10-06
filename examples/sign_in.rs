//! Shows what ytfast can read from each sign-in source, then checks the
//! session it would use with YouTube Music. Prints counts and sources only,
//! never a cookie value, password or account name.
//!
//! `cargo run --example sign_in --no-default-features [-- <profile id>]`

use ytfast::model::Account;

fn main() -> anyhow::Result<()> {
    let preferred = std::env::args().nth(1);
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
            return Ok(());
        }
    };
    let source = session.source.clone();
    let client = ytfast::innertube::Client::new();
    client.set_session(Some(session));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    match runtime.block_on(client.verify(&source)) {
        Account::SignedIn { source, .. } => println!("Signed in (from {source})"),
        Account::SignedOut { reason } => println!("Signed out: {reason}"),
        Account::Unverified { reason } => println!("Not checked: {reason}"),
        Account::Checking => {}
    }
    let _ = std::fs::remove_dir(&scratch);
    Ok(())
}

/// A private directory for the cookie database copies.
fn tempdir() -> anyhow::Result<std::path::PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    let dir = std::env::temp_dir().join(format!("ytfast-sign-in-{}", std::process::id()));
    std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    Ok(dir)
}
