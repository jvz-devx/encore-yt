//! End-to-end tests against fake devices on 127.0.0.1: the relay alone, a
//! fake Cast receiver (TLS, framing, heartbeat, LAUNCH/LOAD/SEEK/STOP) and
//! a fake DLNA renderer (SSDP, description, SOAP). Each fake fetches the
//! media from the relay as a real device would.

mod fake_cast;
mod fake_dlna;
mod relay;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

/// A file of `len` patterned bytes in the temp directory, removed on drop.
pub struct TestFile {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

impl TestFile {
    pub fn new(len: usize) -> TestFile {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("encore-cast-test-{}-{n}.bin", std::process::id()));
        let bytes: Vec<u8> = (0..len).map(|i| (i * 7 % 251) as u8).collect();
        std::fs::write(&path, &bytes).expect("write test file");
        TestFile { path, bytes }
    }
}

impl Drop for TestFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// GET `url` with an optional Range; status and body.
pub async fn fetch(url: &str, range: Option<&str>) -> (u16, Vec<u8>) {
    let mut request = reqwest::Client::new().get(url);
    if let Some(range) = range {
        request = request.header("Range", range);
    }
    let response = request.send().await.expect("fetch");
    let status = response.status().as_u16();
    (status, response.bytes().await.expect("body").to_vec())
}

/// The relay's log once it has `n` entries: a request is logged after its
/// body is sent, which can be just after the client has read it all.
pub async fn accesses(relay: &crate::relay::Relay, n: usize) -> Vec<crate::relay::Access> {
    for _ in 0..200 {
        let log = relay.accesses();
        if log.len() >= n {
            return log;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!(
        "the relay logged {:?}, wanted {n} entries",
        relay.accesses()
    );
}
