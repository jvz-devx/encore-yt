use super::*;

impl super::Worker {
    pub(super) fn connect(&mut self) {
        // Stand-in checks must never inspect browser credentials or sign in,
        // including Firefox profiles stored outside XDG_CONFIG_HOME.
        let check = encore_cast::discovery::Policy::from_env();
        if check.local_only || check.shield_only {
            self.client.set_session(None);
            self.client.set_page_id(None);
            self.sink.send(Event::Profiles {
                list: Vec::new(),
                current: None,
            });
            self.sink.send(Event::Account(Account::SignedOut {
                reason: "Local casting test".into(),
            }));
            self.prepare_restored();
            return;
        }
        self.last_connect = Some(Instant::now());
        self.sink.send(Event::Account(Account::Checking));
        let client = self.client.clone();
        let connection = client.begin_connection();
        let paths = self.paths.clone();
        let tx = self.internal_tx.clone();
        let sink = self.sink.clone();
        tokio::spawn(async move {
            let scratch = paths.runtime.clone();
            let loaded = tokio::task::spawn_blocking(move || {
                let settings = crate::settings::Settings::load(&paths);
                let session = crate::auth::load(&scratch, settings.browser_profile.as_deref());
                (session, crate::auth::profiles(&scratch), settings.channel)
            })
            .await
            .map(|(session, profiles, channel)| {
                let current = session.as_ref().ok().map(|s| s.profile.clone());
                if client.connection_is_current(connection) {
                    sink.send(Event::Profiles {
                        list: profiles,
                        current,
                    });
                }
                (session, channel)
            });
            let (session, channel) = match loaded {
                Ok((Ok(session), channel)) => (session, channel),
                Ok((Err(error), _)) => {
                    if !client.set_session_for_connection(connection, None) {
                        return;
                    }
                    client.set_page_id_for_connection(connection, None);
                    let _ = tx.send(Internal::Connected {
                        connection,
                        account: Account::SignedOut {
                            reason: format!("{error:#}"),
                        },
                        channels: Vec::new(),
                    });
                    return;
                }
                Err(error) => {
                    let _ = tx.send(Internal::Connected {
                        connection,
                        account: Account::SignedOut {
                            reason: error.to_string(),
                        },
                        channels: Vec::new(),
                    });
                    return;
                }
            };
            let source = session.source.clone();
            if !client.set_session_for_connection(connection, Some(session)) {
                return;
            }
            let mut channels = act_as_channel(&client, connection, channel.as_deref()).await;
            if !client.connection_is_current(connection) {
                return;
            }
            let account = client.verify(&source).await;
            if matches!(account, Account::SignedOut { .. }) {
                if !client.set_session_for_connection(connection, None) {
                    return;
                }
                client.set_page_id_for_connection(connection, None);
                channels.clear();
            }
            let _ = tx.send(Internal::Connected {
                connection,
                account,
                channels,
            });
        });
    }

    /// Uses a cookie file just saved from an import or a paste: it becomes
    /// the chosen profile and the backend connects with it.
    pub(super) async fn save_cookies(&mut self, saved: anyhow::Result<crate::auth::Profile>) {
        let saved = saved.map_err(|error| format!("{error:#}"));
        if let Ok(profile) = &saved {
            let id = profile.id.clone();
            if let Err(error) = self
                .change_settings(move |s| s.browser_profile = Some(id))
                .await
            {
                log::warn!("could not save the account choice: {error:#}");
            }
        }
        let connect = saved.is_ok();
        self.sink.send(Event::CookiesSaved(saved));
        if connect {
            self.connect();
        }
    }

    pub(super) fn scan_browsers(&self) {
        let scratch = self.paths.runtime.clone();
        let sink = self.sink.clone();
        tokio::spawn(async move {
            let scan =
                match tokio::task::spawn_blocking(move || crate::auth::scan_browsers(&scratch))
                    .await
                {
                    Ok(scan) => scan,
                    Err(error) => {
                        log::warn!("browser scan worker stopped: {error}");
                        sink.send(Event::Error(
                            "Couldn't check browser profiles. Try again.".into(),
                        ));
                        return;
                    }
                };
            sink.send(Event::BrowserScan(scan));
        });
    }
}

/// Lists the account's channels and makes requests act as the chosen one
/// (see [`crate::settings::Settings::channel`]): the `X-Goog-PageId` of a
/// brand account, none for the account's own channel. Without the list,
/// requests act as the account's own channel.
async fn act_as_channel(
    client: &Client,
    connection: crate::innertube::ConnectionToken,
    chosen: Option<&str>,
) -> Vec<crate::model::Channel> {
    if !client.set_page_id_for_connection(connection, None) {
        return Vec::new();
    }
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
    if !client
        .set_page_id_for_connection(connection, pick.and_then(|i| channels[i].page_id.clone()))
    {
        return Vec::new();
    }
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
