#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "synthetic offline fixtures"
)]

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

fn track(index: usize) -> Track {
    Track {
        video_id: format!("synthetic-{index}"),
        title: format!("Song {index}"),
        artists: Vec::new(),
        album: None,
        thumbnail: None,
        duration: Some(120),
        like: None,
        set_video_id: None,
    }
}

fn part(range: std::ops::Range<usize>, next: Option<&str>) -> Part {
    Part {
        counts: PlaylistCounts {
            rows: range.len(),
            ..Default::default()
        },
        tracks: range.map(track).collect(),
        next: next.map(str::to_owned),
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static SERIAL: AtomicUsize = AtomicUsize::new(0);
        Self(std::env::temp_dir().join(format!(
            "encore-playlist-test-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        )))
    }

    fn file(&self, account: &str) -> PathBuf {
        self.0.join(account).join(format!(
            "{}.json",
            paths::hash(&Target::browse("VLLM").key())
        ))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn collects_all_pages_beyond_five_hundred_before_returning() {
    let mut page = 0;
    let songs = collect(
        &mut |token| {
            assert_eq!(token, (page > 0).then(|| format!("page-{page}")));
            let start = page * 100;
            page += 1;
            std::future::ready(Ok(Part {
                counts: PlaylistCounts {
                    rows: 100,
                    ..Default::default()
                },
                tracks: (start..start + 100).map(track).collect(),
                next: (page < 12).then(|| format!("page-{page}")),
            }))
        },
        Duration::ZERO,
    )
    .await
    .unwrap();
    assert_eq!(page, 12);
    assert_eq!(songs, (0..1200).map(track).collect::<Vec<_>>());
}

#[tokio::test]
async fn retries_the_same_page_and_stops_after_three_attempts() {
    let mut answers = VecDeque::from([
        Err(ApiError::Offline("synthetic disconnect".into())),
        Err(ApiError::Http(503)),
        Ok(part(0..100, None)),
    ]);
    let mut attempts = 0;
    let got = request(
        &mut |token| {
            assert_eq!(token.as_deref(), Some("same-token"));
            attempts += 1;
            std::future::ready(answers.pop_front().unwrap())
        },
        Some("same-token".into()),
        Duration::ZERO,
    )
    .await
    .unwrap();
    assert_eq!(attempts, 3);
    assert_eq!(got.tracks.len(), 100);

    attempts = 0;
    let error = request(
        &mut |_| {
            attempts += 1;
            std::future::ready(Err(ApiError::Http(429)))
        },
        None,
        Duration::ZERO,
    )
    .await
    .err()
    .unwrap();
    assert!(matches!(error, ApiError::Http(429)));
    assert_eq!(attempts, 3);
}

#[tokio::test]
async fn authentication_and_permanent_errors_are_not_retried() {
    for code in [None, Some(400), Some(404)] {
        let mut calls = 0;
        let result = request(
            &mut |_| {
                calls += 1;
                std::future::ready(Err(code.map_or(ApiError::Auth, ApiError::Http)))
            },
            None,
            Duration::ZERO,
        )
        .await;
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }
}

#[test]
fn retry_backoff_is_exponential_bounded_and_slower_for_rate_limits() {
    assert_eq!(
        retry_delay(&ApiError::Http(503), 0),
        Some(Duration::from_secs(1))
    );
    assert_eq!(
        retry_delay(&ApiError::Http(503), 1),
        Some(Duration::from_secs(2))
    );
    assert_eq!(
        retry_delay(&ApiError::Http(429), 0),
        Some(Duration::from_secs(5))
    );
    assert_eq!(
        retry_delay(&ApiError::Http(429), 1),
        Some(Duration::from_secs(10))
    );
    assert_eq!(retry_delay(&ApiError::Http(429), 2), None);
    assert_eq!(retry_delay(&ApiError::Auth, 0), None);
}

#[tokio::test]
async fn continuation_failure_never_returns_a_partial_playlist() {
    let mut calls = 0;
    let failure = collect(
        &mut |token| {
            calls += 1;
            std::future::ready(if token.is_none() {
                Ok(part(0..100, Some("next")))
            } else {
                Err(ApiError::Http(503))
            })
        },
        Duration::ZERO,
    )
    .await
    .unwrap_err();
    assert_eq!(calls, 4);
    assert_eq!(failure.loaded, 100);
    assert!(failure.message().contains("queue is unchanged"));
}

#[tokio::test]
async fn continuation_cycles_fail_instead_of_looping_or_claiming_completion() {
    let failure = collect(
        &mut |_| std::future::ready(Ok(part(0..100, Some("loop")))),
        Duration::ZERO,
    )
    .await
    .unwrap_err();
    assert_eq!(failure.loaded, 200);
    assert!(matches!(failure.error, ApiError::Invalid(_)));
}

#[tokio::test]
async fn a_new_process_reuses_a_complete_cache_without_requests() {
    let scratch = Scratch::new();
    let library = Library::default();
    let loaded = library
        .load(&scratch.0, "account-a", &Target::browse("VLLM"), |_| {
            std::future::ready(Ok(part(0..1200, None)))
        })
        .await
        .unwrap();
    assert_eq!(loaded.tracks.len(), 1200);
    let restarted = Library::default();
    let cached = restarted
        .load(
            &scratch.0,
            "account-a",
            &Target::browse("VLLM"),
            |_| async { panic!("a cache hit must not fetch") },
        )
        .await
        .unwrap();
    assert_eq!(cached.tracks, loaded.tracks);
    assert!(cached.warning.is_none());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(scratch.file("account-a"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(scratch.0.join("account-a"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}

#[tokio::test]
async fn stale_complete_cache_is_explicit_fallback_but_never_on_auth_failure() {
    let scratch = Scratch::new();
    let old = Snapshot {
        version: 1,
        saved: now() - FRESH_FOR - 1,
        tracks: (0..1000).map(track).collect(),
        counts: None,
    };
    save(&scratch.file("account-a"), &old).await.unwrap();
    let library = Library::default();
    let loaded = library
        .load(&scratch.0, "account-a", &Target::browse("VLLM"), |_| {
            std::future::ready(Err(ApiError::Invalid("synthetic incomplete page".into())))
        })
        .await
        .unwrap();
    assert_eq!(loaded.tracks, old.tracks);
    assert!(loaded.warning.unwrap().contains("saved copy of 1000 songs"));
    assert_eq!(
        read(&scratch.file("account-a")).await.unwrap().saved,
        old.saved
    );
    let failure = library
        .load(&scratch.0, "account-a", &Target::browse("VLLM"), |_| {
            std::future::ready(Err(ApiError::Auth))
        })
        .await
        .unwrap_err();
    assert!(matches!(failure.error, ApiError::Auth));
}

#[tokio::test]
async fn partial_scan_does_not_create_a_cache_and_malformed_cache_is_rebuilt() {
    let scratch = Scratch::new();
    let library = Library::default();
    let result = library
        .load(&scratch.0, "account-a", &Target::browse("VLLM"), |token| {
            std::future::ready(if token.is_none() {
                Ok(part(0..100, Some("next")))
            } else {
                Err(ApiError::Invalid("synthetic malformed page".into()))
            })
        })
        .await;
    assert!(result.is_err());
    assert!(!scratch.file("account-a").exists());
    paths::private_dir(&scratch.0.join("account-a")).unwrap();
    std::fs::write(scratch.file("account-a"), "broken").unwrap();
    library
        .load(&scratch.0, "account-a", &Target::browse("VLLM"), |_| {
            std::future::ready(Ok(part(0..1200, None)))
        })
        .await
        .unwrap();
    assert_eq!(
        read(&scratch.file("account-a")).await.unwrap().tracks.len(),
        1200
    );
}

#[tokio::test]
async fn invalidation_and_account_boundaries_do_not_reuse_another_library() {
    let scratch = Scratch::new();
    let library = Library::default();
    for account in ["account-a", "account-b"] {
        library
            .load(&scratch.0, account, &Target::browse("VLLM"), |_| {
                std::future::ready(Ok(part(0..100, None)))
            })
            .await
            .unwrap();
    }
    library.invalidate(&scratch.0, "account-a").await.unwrap();
    assert!(!scratch.file("account-a").exists());
    assert!(scratch.file("account-b").exists());
    let mut calls = 0;
    let result = library
        .load(&scratch.0, "account-a", &Target::browse("VLLM"), |_| {
            calls += 1;
            std::future::ready(Ok(part(100..1200, None)))
        })
        .await
        .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(result.tracks.len(), 1100);
}

#[tokio::test]
async fn rate_limit_cooldown_blocks_new_scans_without_hitting_the_api() {
    let scratch = Scratch::new();
    let library = Library::default();
    *library.gate.lock().await = Some(Instant::now() + Duration::from_secs(60));
    let failure = library
        .load(
            &scratch.0,
            "account-a",
            &Target::browse("VLLM"),
            |_| async { panic!("cooldown must not fetch") },
        )
        .await
        .unwrap_err();
    assert!(matches!(failure.error, ApiError::Http(429)));
}

#[tokio::test]
async fn cancelling_a_scan_releases_the_gate_and_does_not_save_a_partial_cache() {
    let scratch = Scratch::new();
    let library = std::sync::Arc::new(Library::default());
    let (entered, wait) = tokio::sync::oneshot::channel();
    let directory = scratch.0.clone();
    let scanning = library.clone();
    let task = tokio::spawn(async move {
        let mut entered = Some(entered);
        scanning
            .load(&directory, "account-a", &Target::browse("VLLM"), |_| {
                if let Some(entered) = entered.take() {
                    let _ = entered.send(());
                }
                std::future::pending::<Result<Part, ApiError>>()
            })
            .await
    });
    wait.await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(!scratch.file("account-a").exists());
    library
        .load(&scratch.0, "account-a", &Target::browse("VLLM"), |_| {
            std::future::ready(Ok(part(0..100, None)))
        })
        .await
        .unwrap();
}

#[test]
fn malformed_responses_are_not_mistaken_for_the_end_of_a_playlist() {
    assert!(more(&serde_json::json!({"error": {"message": "not a playlist"}})).is_err());
    let end = more(&serde_json::json!({"continuationContents": {"musicPlaylistShelfContinuation": {"contents": []}}})).unwrap();
    assert!(end.tracks.is_empty() && end.next.is_none());
}

fn worker(
    scratch: &Scratch,
) -> (
    super::super::Worker,
    std::sync::mpsc::Receiver<super::super::Event>,
) {
    let (tx, events) = std::sync::mpsc::channel();
    let (now, _) = tokio::sync::watch::channel(crate::desktop::Now::default());
    let sink = super::super::Sink {
        tx,
        wake: Arc::new(|| {}),
        now: Arc::new(now),
    };
    let client = Arc::new(Client::new().unwrap());
    let resolver = Arc::new(crate::resolver::Resolver::new(scratch.0.join("runtime")));
    let paths = paths::Paths {
        config: scratch.0.join("config"),
        cache: scratch.0.clone(),
        runtime: scratch.0.join("runtime"),
    };
    (
        super::super::Worker::new(client, resolver, paths, sink),
        events,
    )
}

#[tokio::test]
async fn repeated_play_clicks_share_a_scan_and_keep_the_latest_selection() {
    let scratch = Scratch::new();
    let (mut worker, _) = worker(&scratch);
    // Hold the scan gate so this test never reaches a real HTTP request.
    let library = worker.playlists.clone();
    let _gate = library.gate.lock().await;
    worker.play_playlist(Target::browse("VLLM"), None, true);
    let epoch = worker.epoch;
    let task = worker.playlist_task.as_ref().unwrap().id();
    worker.play_playlist(Target::browse("VLLM"), Some("synthetic-7".into()), false);
    assert_eq!(worker.epoch, epoch);
    assert_eq!(worker.playlist_task.as_ref().unwrap().id(), task);
    let request = worker.playlist_request.as_ref().unwrap();
    assert_eq!(request.start.as_deref(), Some("synthetic-7"));
    assert!(!request.shuffle);
    worker.cancel_playlist();
    assert!(worker.playlist_task.is_none() && worker.playlist_request.is_none());
    assert!(!worker.state.loading);
}

#[tokio::test]
async fn a_failed_complete_load_preserves_the_existing_queue_and_reports_the_count() {
    let scratch = Scratch::new();
    let (mut worker, events) = worker(&scratch);
    worker.queue.replace((1000..1010).map(track).collect());
    worker.pos = Some(4);
    worker.state.index = Some(4);
    worker.state.loading = true;
    worker.playlist_request = Some(Pending {
        target: Target::browse("VLLM"),
        start: None,
        shuffle: true,
        optimistic_entry: None,
        advance_when_ready: false,
    });
    worker
        .internal(Internal::Playlist {
            epoch: worker.epoch,
            scope: scope(&worker.client),
            result: Err(Failure {
                loaded: 100,
                error: ApiError::Http(503),
            }),
        })
        .await;
    assert_eq!(
        worker.queue.tracks(),
        (1000..1010).map(track).collect::<Vec<_>>()
    );
    assert_eq!(worker.pos, Some(4));
    assert!(
        worker.state.loading,
        "playlist failure must not change unrelated audio buffering"
    );
    assert!(events.try_iter().any(|event| matches!(event, super::super::Event::Error(message) if message.contains("100 songs") && message.contains("queue is unchanged"))));
}

#[test]
fn first_page_reads_the_playlist_only_and_keeps_its_continuation() {
    let value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/innertube/playlist.json"
    ))
    .unwrap();
    let page = crate::parse::page(&value);
    let expected = crate::account::entries(&page)
        .unwrap()
        .items
        .iter()
        .filter_map(|i| i.track.clone())
        .collect::<Vec<_>>();
    let got = first(&value).unwrap();
    assert_eq!(got.tracks, expected);
}

#[test]
fn cache_scope_ignores_rotating_visitor_cookies_but_separates_accounts_and_channels() {
    let key = |account: &str, visitor: &str, channel: Option<String>| {
        identity_scope(
            |name| match name {
                "SAPISID" => Some(account.into()),
                "VISITOR_INFO1_LIVE" | "__Secure-1PSIDTS" => Some(visitor.into()),
                _ => None,
            },
            channel,
        )
    };
    assert_eq!(
        key("synthetic-a", "old", None),
        key("synthetic-a", "renewed", None)
    );
    assert_ne!(
        key("synthetic-a", "old", None),
        key("synthetic-b", "old", None)
    );
    assert_ne!(
        key("synthetic-a", "old", None),
        key("synthetic-a", "old", Some("other-channel".into()))
    );
    assert_ne!(
        identity_scope(|_| None, None),
        key("synthetic-a", "old", None)
    );
}

#[tokio::test]
async fn account_changes_do_not_wait_for_a_scan_or_allow_it_to_save_old_data() {
    let scratch = Scratch::new();
    let library = Arc::new(Library::default());
    let scanning = library.clone();
    let directory = scratch.0.clone();
    let (entered, wait) = tokio::sync::oneshot::channel();
    let (resume, resumed) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let mut entered = Some(entered);
        let mut resumed = Some(resumed);
        scanning
            .load(&directory, "account-a", &Target::browse("VLLM"), |_| {
                let entered = entered.take().unwrap();
                let resumed = resumed.take().unwrap();
                async move {
                    entered.send(()).unwrap();
                    resumed.await.unwrap();
                    Ok(part(0..1200, None))
                }
            })
            .await
    });
    wait.await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(1),
        library.invalidate(&scratch.0, "account-a"),
    )
    .await
    .unwrap()
    .unwrap();
    resume.send(()).unwrap();
    let failure = task.await.unwrap().unwrap_err();
    assert!(matches!(failure.error, ApiError::Invalid(_)));
    assert!(!scratch.file("account-a").exists());
}

#[tokio::test]
async fn scan_progress_counts_page_overlap_without_collapsing_distinct_playlist_entries() {
    let mut a = track(1);
    a.set_video_id = Some("entry-a".into());
    let mut b = a.clone();
    b.set_video_id = Some("entry-b".into());
    let mut pages = VecDeque::from([
        Part {
            tracks: vec![a.clone()],
            next: Some("next".into()),
            counts: PlaylistCounts {
                rows: 1,
                expected: Some(2),
                ..Default::default()
            },
        },
        Part {
            tracks: vec![a, b],
            next: None,
            counts: PlaylistCounts {
                rows: 2,
                ..Default::default()
            },
        },
    ]);
    let mut updates = Vec::new();
    let (tracks, counts) = collect_with_progress(
        &mut |_| std::future::ready(Ok(pages.pop_front().unwrap())),
        Duration::ZERO,
        "synthetic",
        &mut |progress| updates.push(progress),
    )
    .await
    .unwrap();
    assert_eq!(tracks.len(), 2);
    assert_eq!(counts.rows, 3);
    assert_eq!(counts.overlapping_entries, 1);
    assert_eq!(updates.last().unwrap().unique_tracks, 1);
    assert_eq!(updates.last().unwrap().entries, 2);
    assert_eq!(
        updates.last().unwrap().counts.as_ref().unwrap().expected,
        Some(2)
    );
}

#[tokio::test]
async fn provisional_queue_waits_instead_of_repeating_or_fetching_radio() {
    let scratch = Scratch::new();
    let (mut worker, _) = worker(&scratch);
    worker.queue.replace(vec![track(1)]);
    worker.pos = Some(0);
    worker.state.autoplay = true;
    worker.playlist_request = Some(Pending {
        target: Target::browse("VLLM"),
        start: Some("synthetic-1".into()),
        shuffle: false,
        optimistic_entry: worker.queue.id(0),
        advance_when_ready: false,
    });
    worker.maybe_extend();
    assert!(
        !worker.extending,
        "metadata scanning must not trigger radio requests"
    );
    assert!(worker.wait_for_playlist(0));
    assert!(worker.playlist_request.as_ref().unwrap().advance_when_ready);
    worker.keep_playlist_current_song();
    assert!(
        !worker.playlist_request.as_ref().unwrap().advance_when_ready,
        "explicit transport input takes precedence over pending auto-advance"
    );
    worker.cancel_optimistic_playlist();
    assert!(!worker.wait_for_playlist(0));
    assert_eq!(worker.queue.tracks(), vec![track(1)]);
}

#[tokio::test]
async fn metadata_progress_and_failures_do_not_override_audio_buffering_or_selected_song() {
    let scratch = Scratch::new();
    let (mut worker, events) = worker(&scratch);
    worker.queue.replace(vec![track(1)]);
    worker.pos = Some(0);
    worker.state.loading = false;
    worker.state.playing = true;
    worker.playlist_request = Some(Pending {
        target: Target::browse("VLLM"),
        start: Some("synthetic-1".into()),
        shuffle: false,
        optimistic_entry: worker.queue.id(0),
        advance_when_ready: false,
    });
    let epoch = worker.epoch;
    let account = scope(&worker.client);
    let progress = PlaylistLoad {
        key: Target::browse("VLLM").key(),
        entries: 100,
        ..Default::default()
    };
    worker.playlist_progress(epoch + 1, &account, progress.clone());
    assert!(worker.state.playlist_load.is_none());
    worker.playlist_progress(epoch, &account, progress);
    assert_eq!(worker.state.playlist_load.as_ref().unwrap().entries, 100);
    assert!(!worker.state.loading && worker.state.playing);
    worker
        .playlist_result(
            epoch,
            account,
            Err(Failure {
                loaded: 100,
                error: ApiError::Http(503),
            }),
        )
        .await;
    assert_eq!(worker.queue.tracks(), vec![track(1)]);
    assert!(!worker.state.loading && worker.state.playing);
    assert!(worker.state.playlist_load.is_none());
    assert!(events.try_iter().any(|event| matches!(event, super::super::Event::Error(message) if message.contains("selected song is kept") && !message.contains("queue is unchanged"))));
}

#[tokio::test]
async fn unparsed_rows_are_reported_but_never_saved_as_a_complete_playlist() {
    let mut updates = Vec::new();
    let failure = collect_with_progress(
        &mut |_| {
            std::future::ready(Ok(Part {
                tracks: vec![track(1)],
                next: None,
                counts: PlaylistCounts {
                    rows: 2,
                    unparsed: 1,
                    ..Default::default()
                },
            }))
        },
        Duration::ZERO,
        "synthetic",
        &mut |progress| updates.push(progress),
    )
    .await
    .unwrap_err();
    assert_eq!(failure.loaded, 1);
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].counts.as_ref().unwrap().unparsed, 1);
}
