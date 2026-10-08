use super::{TestFile, accesses, fetch};
use crate::relay::{Relay, Source};

#[tokio::test]
async fn a_file_is_served_whole_in_ranges_and_by_head() {
    let file = TestFile::new(100_000);
    let relay = Relay::start("127.0.0.1".parse().unwrap()).await.unwrap();
    let url = relay
        .publish(Source::File(file.path.clone()), "audio/webm")
        .unwrap();
    assert!(url.ends_with(".webm"));

    let (status, body) = fetch(&url, None).await;
    assert_eq!((status, body.len()), (200, 100_000));
    assert_eq!(body, file.bytes);

    let (status, body) = fetch(&url, Some("bytes=50000-50999")).await;
    assert_eq!(status, 206);
    assert_eq!(body, &file.bytes[50_000..51_000]);

    let (status, _) = fetch(&url, Some("bytes=200000-")).await;
    assert_eq!(status, 416);

    let head = reqwest::Client::new().head(&url).send().await.unwrap();
    assert_eq!(head.headers()["content-length"], "100000");
    assert_eq!(head.headers()["content-type"], "audio/webm");
    assert_eq!(head.headers()["transfermode.dlna.org"], "Streaming");

    // Unknown paths and guessed tokens get nothing.
    let base = format!("http://{}/s/", relay.addr());
    assert_eq!(fetch(&format!("{base}0000.webm"), None).await.0, 404);
    assert_eq!(
        fetch(&format!("http://{}/", relay.addr()), None).await.0,
        404
    );

    let log = accesses(&relay, 6).await;
    let ranged = log
        .iter()
        .find(|a| a.range.as_deref() == Some("bytes=50000-50999"))
        .unwrap();
    assert_eq!((ranged.status, ranged.bytes), (206, 1000));
}

/// A remote source (googlevideo in the app) is fetched as the device asks,
/// with its Range passed upstream. Here the upstream is a second relay.
#[tokio::test]
async fn a_remote_source_passes_ranges_upstream() {
    let file = TestFile::new(300_000);
    let upstream = Relay::start("127.0.0.1".parse().unwrap()).await.unwrap();
    let origin = upstream
        .publish(Source::File(file.path.clone()), "audio/webm")
        .unwrap();
    let relay = Relay::start("127.0.0.1".parse().unwrap()).await.unwrap();
    let url = relay
        .publish(Source::Remote(origin), "audio/webm; codecs=opus")
        .unwrap();

    let (status, body) = fetch(&url, Some("bytes=123456-")).await;
    assert_eq!(status, 206);
    assert_eq!(body, &file.bytes[123_456..]);
    let (status, body) = fetch(&url, None).await;
    assert_eq!((status, body.len()), (200, 300_000));

    let seen = accesses(&upstream, 2).await;
    let mut ranges: Vec<_> = seen.iter().map(|a| a.range.clone()).collect();
    ranges.sort();
    assert_eq!(ranges, [None, Some("bytes=123456-".to_owned())]);
}
