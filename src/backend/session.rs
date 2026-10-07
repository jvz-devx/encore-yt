use super::*;

impl super::Worker {
    pub(super) fn connect(&mut self) {
        self.last_connect = Some(Instant::now());
        self.sink.send(Event::Account(Account::Checking));
        let client = self.client.clone();
        let resolver = self.resolver.clone();
        let paths = self.paths.clone();
        let tx = self.internal_tx.clone();
        let sink = self.sink.clone();
        tokio::spawn(async move {
            let scratch = paths.runtime.clone();
            let settings = crate::settings::Settings::load(&paths);
            let preferred = settings.browser_profile;
            let loaded = tokio::task::spawn_blocking(move || {
                let session = crate::auth::load(&scratch, preferred.as_deref());
                (session, crate::auth::profiles(&scratch))
            })
            .await
            .map(|(session, profiles)| {
                let current = session.as_ref().ok().map(|s| s.profile.clone());
                sink.send(Event::Profiles {
                    list: profiles,
                    current,
                });
                session
            });
            let session = match loaded {
                Ok(Ok(session)) => session,
                Ok(Err(error)) => {
                    client.set_session(None);
                    client.set_page_id(None);
                    resolver.set_cookie_file(None);
                    sink.send(Event::Channels(Vec::new()));
                    let _ = tx.send(Internal::Connected(Account::SignedOut {
                        reason: format!("{error:#}"),
                    }));
                    return;
                }
                Err(error) => {
                    let _ = tx.send(Internal::Connected(Account::SignedOut {
                        reason: error.to_string(),
                    }));
                    return;
                }
            };
            let source = session.source.clone();
            let cookie_file = paths.cookie_file();
            match session.write_netscape(&cookie_file) {
                Ok(()) => resolver.set_cookie_file(Some(cookie_file)),
                Err(error) => log::warn!("could not write the cookie file: {error:#}"),
            }
            client.set_session(Some(session));
            let channels = act_as_channel(&client, settings.channel.as_deref()).await;
            let account = client.verify(&source).await;
            if matches!(account, Account::SignedOut { .. }) {
                client.set_session(None);
                client.set_page_id(None);
                resolver.set_cookie_file(None);
                sink.send(Event::Channels(Vec::new()));
            } else {
                sink.send(Event::Channels(channels));
            }
            let _ = tx.send(Internal::Connected(account));
        });
    }
}

/// Lists the account's channels and makes requests act as the chosen one
/// (see [`crate::settings::Settings::channel`]): the `X-Goog-PageId` of a
/// brand account, none for the account's own channel. Without the list,
/// requests act as the account's own channel.
async fn act_as_channel(client: &Client, chosen: Option<&str>) -> Vec<crate::model::Channel> {
    client.set_page_id(None);
    let mut channels = match client.channels().await {
        Ok(value) => crate::parse::channels(&value),
        Err(error) => {
            log::warn!("couldn't list the account's channels: {error}");
            return Vec::new();
        }
    };
    let pick = chosen
        .and_then(|id| {
            channels
                .iter()
                .position(|c| c.page_id.as_deref().unwrap_or_default() == id)
        })
        .or_else(|| channels.iter().position(|c| c.current));
    for (i, channel) in channels.iter_mut().enumerate() {
        channel.current = Some(i) == pick;
    }
    client.set_page_id(pick.and_then(|i| channels[i].page_id.clone()));
    log::info!(
        "{} channels, acting as {}",
        channels.len(),
        match pick {
            Some(i) if channels[i].page_id.is_some() => "a brand account",
            _ => "the account's own channel",
        }
    );
    channels
}
