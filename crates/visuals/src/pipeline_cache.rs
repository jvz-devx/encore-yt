//! Private pipeline-cache persistence. Only this application's versioned
//! envelope is accepted. Its checksum detects accidental changes; it is not
//! authentication against the local user who owns the cache directory.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::gpu::ms;

const MAGIC: &[u8; 8] = b"ENCPIP01";
const HEADER: usize = 32;
const MAX_BYTES: u64 = 64 * 1024 * 1024;

pub(crate) struct DiskCache {
    file: PathBuf,
    identity: u64,
    loaded: Option<Vec<u8>>,
    pub cache: wgpu::PipelineCache,
}

impl DiskCache {
    pub fn open(device: &wgpu::Device, info: &wgpu::AdapterInfo, dir: &Path) -> Option<Self> {
        let key = wgpu::util::pipeline_cache_key(info)?;
        let file = dir.join(format!(
            "{key}_{:016x}.bin",
            fnv1a(&[&info.driver, &info.driver_info])
        ));
        let identity = fnv1a(&[&key, &info.driver, &info.driver_info]);
        let loaded = match read(&file, identity) {
            Ok(data) => data,
            Err(error) => {
                log::warn!("visuals: pipeline cache discarded: {error}");
                None
            }
        };
        // SAFETY: save only persists this device's get_data output, in an
        // exclusive atomic file. read checks the private directory/file,
        // format version, adapter/driver identity, exact length and checksum
        // before supplying that payload. We trust the local cache owner not
        // to forge both payload and checksum; the checksum detects corruption,
        // not hostile edits by that user or privileged software. This is the
        // disk-origin trust assumption acknowledged by wgpu 29's safety docs.
        // Unwrapped/legacy or damaged bytes are never passed to the driver.
        let cache = unsafe {
            device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
                label: Some("encore visuals"),
                data: loaded.as_deref(),
                fallback: true,
            })
        };
        Some(Self {
            file,
            identity,
            loaded,
            cache,
        })
    }

    pub fn loaded(&self) -> usize {
        self.loaded.as_ref().map_or(0, Vec::len)
    }

    pub fn save(&self) {
        let Some(data) = self.cache.get_data() else {
            return;
        };
        if self
            .loaded
            .as_deref()
            .is_some_and(|loaded| same_cache(loaded, &data))
        {
            return;
        }
        let started = Instant::now();
        match write(&self.file, &data, self.identity) {
            Ok(()) => log::info!(
                "visuals: pipeline cache saved, {} KB in {:.1} ms",
                data.len() / 1024,
                ms(started)
            ),
            Err(error) => log::warn!("visuals: pipeline cache not saved: {error}"),
        }
        // Do not delete neighbouring files on an unvalidated name prefix.
        // Other driver versions are harmless misses; cache eviction belongs
        // to a separate disk-retention policy.
    }
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid or untrusted pipeline cache",
    )
}

fn private_directory(dir: &Path) -> io::Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(dir)?;
    if !metadata.file_type().is_dir() {
        return Err(invalid());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(invalid());
        }
    }
    // On Windows the app's per-user cache directory inherits its user's
    // ACL. This portable layer does not inspect or strengthen Windows ACLs.
    Ok(metadata)
}

fn read(file: &Path, identity: u64) -> io::Result<Option<Vec<u8>>> {
    let dir = file.parent().ok_or_else(invalid)?;
    let directory = match private_directory(dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let metadata = match fs::symlink_metadata(file) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_file() || metadata.len() > MAX_BYTES {
        return Err(invalid());
    }
    let input = File::open(file)?;
    let opened = input.metadata()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.dev() != opened.dev()
            || metadata.ino() != opened.ino()
            || opened.uid() != directory.uid()
            || opened.permissions().mode() & 0o077 != 0
        {
            return Err(invalid());
        }
    }
    #[cfg(not(unix))]
    let _ = (directory, opened);
    let mut bytes = Vec::new();
    input.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid());
    }
    decode(&bytes, identity).map(Some).ok_or_else(invalid)
}

fn decode(bytes: &[u8], identity: u64) -> Option<Vec<u8>> {
    if bytes.get(..8)? != MAGIC {
        return None;
    }
    let number = |at| Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?));
    if number(8)? != identity {
        return None;
    }
    let payload = bytes.get(HEADER..)?;
    if number(16)? != payload.len() as u64 || payload.is_empty() {
        return None;
    }
    if number(24)? != checksum(bytes.get(..24)?, payload) {
        return None;
    }
    Some(payload.to_vec())
}

fn encode(data: &[u8], identity: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(HEADER + data.len());
    bytes.extend(MAGIC);
    bytes.extend(identity.to_le_bytes());
    bytes.extend((data.len() as u64).to_le_bytes());
    bytes.extend(checksum(&bytes, data).to_le_bytes());
    bytes.extend(data);
    bytes
}

fn write(file: &Path, data: &[u8], identity: u64) -> io::Result<()> {
    if data.is_empty() || data.len() as u64 > MAX_BYTES - HEADER as u64 {
        return Err(invalid());
    }
    let dir = file.parent().ok_or_else(invalid)?;
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)?;
    secure_existing_directory(dir)?;
    private_directory(dir)?;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let temp = file.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    // A failed exclusive create does not grant us ownership of this path.
    let mut output = options.open(&temp)?;
    let cleanup = Temporary(temp);
    output.write_all(&encode(data, identity))?;
    output.sync_all()?;
    drop(output);
    fs::rename(&cleanup.0, file)
}

/// Older builds made gpu/ with default permissions inside the private app
/// cache. Tighten only that owned child of a private directory before writing
/// a newly generated cache. Never load the old payload first, follow a
/// directory symlink, or chmod an unrelated shared directory.
fn secure_existing_directory(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = fs::symlink_metadata(dir)?;
        if !metadata.file_type().is_dir() {
            return Err(invalid());
        }
        if metadata.permissions().mode() & 0o077 != 0 {
            let parent = private_directory(dir.parent().ok_or_else(invalid)?)?;
            if metadata.uid() != parent.uid() {
                return Err(invalid());
            }
            let directory = File::open(dir)?;
            let opened = directory.metadata()?;
            if metadata.dev() != opened.dev() || metadata.ino() != opened.ino() {
                return Err(invalid());
            }
            directory.set_permissions(fs::Permissions::from_mode(0o700))?;
        }
    }
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.0)
            && error.kind() != io::ErrorKind::NotFound
        {
            log::warn!("visuals: pipeline cache temporary-file cleanup: {error}");
        }
    }
}

/// FNV-1a covers both the identity/length header and the entire payload.
/// It detects accidental corruption, not deliberate forgery.
fn checksum(header: &[u8], data: &[u8]) -> u64 {
    header
        .iter()
        .chain(data)
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
        })
}

fn fnv1a(parts: &[&str]) -> u64 {
    parts
        .iter()
        .flat_map(|p| p.bytes().chain([0]))
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
}

/// Drivers reorder entries. Keep the existing cache when its wgpu header
/// and length indicate that no new pipelines were compiled. This is only
/// a write-elision heuristic; read always checks the full payload checksum.
fn same_cache(loaded: &[u8], data: &[u8]) -> bool {
    const WGPU_HEADER: usize = 64;
    loaded.len() == data.len()
        && loaded[..WGPU_HEADER.min(loaded.len())] == data[..WGPU_HEADER.min(data.len())]
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "cache regressions assert synthetic files and GPU setup"
)]
mod tests {
    use super::*;

    #[test]
    fn envelope_rejects_legacy_corrupt_truncated_and_wrong_identity_data() {
        let data = vec![73; 256];
        let bytes = encode(&data, 123);
        assert_eq!(decode(&bytes, 123), Some(data.clone()));
        assert!(decode(&bytes, 124).is_none());
        assert!(decode(&data, 123).is_none());
        for index in 0..bytes.len() {
            let mut changed = bytes.clone();
            changed[index] ^= 1;
            assert!(decode(&changed, 123).is_none(), "changed byte {index}");
            assert!(
                decode(&bytes[..index], 123).is_none(),
                "truncated at {index}"
            );
        }
        let mut extra = bytes;
        extra.push(0);
        assert!(decode(&extra, 123).is_none());
    }

    fn directory() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "encore-pipelines-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn private_atomic_file_round_trips_without_temporary_files() {
        let dir = directory();
        let file = dir.join("cache.bin");
        assert!(read(&file, 123).expect("missing").is_none());
        write(&file, &[1, 2, 3], 123).expect("save");
        assert_eq!(read(&file, 123).expect("load"), Some(vec![1, 2, 3]));
        assert_eq!(fs::read_dir(&dir).expect("files").count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&file).expect("file").permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(&dir).expect("dir").permissions().mode() & 0o777,
                0o700
            );
        }
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn permissive_files_directories_and_symlinks_are_rejected() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = directory();
        let file = dir.join("cache.bin");
        write(&file, &[1, 2, 3], 123).expect("save");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).expect("file mode");
        assert!(read(&file, 123).is_err());
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).expect("file mode");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).expect("dir mode");
        assert!(read(&file, 123).is_err());
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).expect("dir mode");
        let link = dir.join("link.bin");
        symlink(&file, &link).expect("symlink");
        assert!(read(&link, 123).is_err());
        let linkdir = dir.join("linked-dir");
        symlink(&dir, &linkdir).expect("directory symlink");
        assert!(read(&linkdir.join("cache.bin"), 123).is_err());
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn old_cache_directory_is_secured_only_inside_a_private_parent() {
        use std::os::unix::fs::PermissionsExt;
        let parent = directory();
        let dir = parent.join("gpu");
        let file = dir.join("cache.bin");
        write(&file, &[1, 2, 3], 123).expect("save");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).expect("legacy directory");
        assert!(read(&file, 123).is_err());
        write(&file, &[4, 5, 6], 123).expect("replace with newly generated data");
        assert_eq!(
            read(&file, 123).expect("private reload"),
            Some(vec![4, 5, 6])
        );
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).expect("shared parent");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).expect("shared child");
        assert!(write(&file, &[7], 123).is_err());
        assert_eq!(
            fs::metadata(&dir)
                .expect("unchanged child")
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        fs::remove_dir_all(parent).expect("cleanup");
    }

    #[test]
    fn oversized_cache_is_rejected_before_reading_the_payload() {
        let dir = directory();
        let file = dir.join("cache.bin");
        write(&file, &[1, 2, 3], 123).expect("save");
        OpenOptions::new()
            .write(true)
            .open(&file)
            .expect("file")
            .set_len(MAX_BYTES + 1)
            .expect("oversized sparse file");
        assert!(read(&file, 123).is_err());
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn gpu_cache_is_reloaded_only_through_the_validated_envelope() {
        let _one = crate::gpu_test_lock();
        let dir = directory();
        let gpu = match crate::Gpu::with_pipeline_cache(&dir) {
            Ok(gpu) => gpu,
            Err(error) => {
                eprintln!("skipped, no GPU: {error:#}");
                return;
            }
        };
        if !gpu
            .device
            .features()
            .contains(wgpu::Features::PIPELINE_CACHE)
        {
            eprintln!("skipped, no pipeline cache on {}", gpu.adapter());
            return;
        }
        drop(gpu);
        let files: Vec<_> = fs::read_dir(&dir)
            .expect("cache files")
            .map(|entry| entry.expect("entry").path())
            .collect();
        assert_eq!(files.len(), 1);
        let encoded = fs::read(&files[0]).expect("cache envelope");
        assert_eq!(&encoded[..8], MAGIC);
        let gpu = crate::Gpu::with_pipeline_cache(&dir).expect("second GPU");
        drop(gpu);
        assert_eq!(fs::read_dir(&dir).expect("cache files").count(), 1);
        // Corruption is rejected before a third device is allowed to see it.
        let mut damaged = encoded;
        let last = damaged.len() - 1;
        damaged[last] ^= 1;
        fs::write(&files[0], damaged).expect("synthetic corruption");
        assert!(read(&files[0], 0).is_err());
        crate::Gpu::with_pipeline_cache(&dir).expect("GPU falls back after damage");
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn driver_hash_distinguishes_part_boundaries() {
        assert_eq!(fnv1a(&[]), 0xcbf2_9ce4_8422_2325);
        assert_ne!(fnv1a(&["ab", "c"]), fnv1a(&["a", "bc"]));
    }
}
