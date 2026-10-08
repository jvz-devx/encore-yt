//! Platform key retrieval and Chromium cookie cryptography. Never logs key or cookie values.

use super::browser::Browser;
use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use anyhow::Result;
use sha2::Digest;
use std::process::Command;

/// The browser's "Safe Storage" password and where it came from: the Secret
/// Service (libsecret), else KWallet. `None` when neither has one.
#[cfg(not(target_os = "macos"))]
pub(super) fn safe_storage_password(browser: &Browser) -> Result<Option<(Vec<u8>, &'static str)>> {
    if let Some(password) = secret_service_password(browser.keyring) {
        return Ok(Some((password, "the Secret Service")));
    }
    match kwallet::password(browser.vendor) {
        Ok(Some(password)) => Ok(Some((password, "KWallet"))),
        Ok(None) => Ok(None),
        Err(error) => Err(error.context("reading KWallet")),
    }
}

/// `secret-tool lookup application <application>`, without the trailing
/// newline. `None` when it is empty or `secret-tool` is missing.
#[cfg(not(target_os = "macos"))]
fn secret_service_password(application: &str) -> Option<Vec<u8>> {
    let output = Command::new("secret-tool")
        .args(["lookup", "application", application])
        .output()
        .ok()?;
    let mut password = output.stdout;
    while password.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
        password.pop();
    }
    Some(password).filter(|p| !p.is_empty())
}

/// PBKDF2 rounds Chromium uses for its macOS key (1 on Linux).
#[cfg(any(target_os = "macos", test))]
pub(super) const MAC_ITERATIONS: u32 = 1003;

/// The Keychain's Safe Storage password for `browser`:
/// `security find-generic-password -w -a <vendor> -s <keychain>`, without
/// the trailing newline. `None` when there is none or access was denied.
#[cfg(target_os = "macos")]
pub(super) fn keychain_password(browser: &Browser) -> Option<Vec<u8>> {
    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-w",
            "-a",
            browser.vendor,
            "-s",
            browser.keychain,
        ])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let mut password = output.stdout;
    while password.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
        password.pop();
    }
    Some(password).filter(|p| !p.is_empty())
}

/// KDE's wallet over D-Bus (`org.kde.KWallet`), as Chromium uses it on KDE.
#[cfg(not(target_os = "macos"))]
mod kwallet {
    use anyhow::{Result, bail};
    use zbus::blocking::{Connection, Proxy};

    /// How Encore names itself to kwalletd (its access prompt shows it).
    const APP_ID: &str = crate::APP_NAME;

    /// kwalletd6 (Plasma 6), then kwalletd5.
    const DAEMONS: &[(&str, &str)] = &[
        ("org.kde.kwalletd6", "/modules/kwalletd6"),
        ("org.kde.kwalletd5", "/modules/kwalletd5"),
    ];

    /// The password in folder "<name> Keys", entry "<name> Safe Storage" of
    /// the network wallet. `None` when no wallet daemon runs or the entry
    /// is missing or empty.
    pub fn password(name: &str) -> Result<Option<Vec<u8>>> {
        let Ok(bus) = Connection::session() else {
            return Ok(None);
        };
        for (service, path) in DAEMONS {
            let Ok(proxy) = Proxy::new(&bus, *service, *path, "org.kde.KWallet") else {
                continue;
            };
            // No such daemon (and none to start): try the next.
            let Ok(enabled) = proxy.call::<_, _, bool>("isEnabled", &()) else {
                continue;
            };
            if !enabled {
                return Ok(None);
            }
            let wallet = proxy
                .call::<_, _, String>("networkWallet", &())
                .ok()
                .filter(|w| !w.is_empty())
                .unwrap_or_else(|| "kdewallet".into());
            return read(&proxy, &wallet, name);
        }
        Ok(None)
    }

    fn read(proxy: &Proxy<'_>, wallet: &str, name: &str) -> Result<Option<Vec<u8>>> {
        // Asks the person to unlock the wallet if it is closed.
        let handle: i32 = proxy.call("open", &(wallet, 0i64, APP_ID))?;
        if handle < 0 {
            bail!("the wallet “{wallet}” didn't open");
        }
        let folder = format!("{name} Keys");
        let entry = format!("{name} Safe Storage");
        let found = proxy.call::<_, _, bool>(
            "hasEntry",
            &(handle, folder.as_str(), entry.as_str(), APP_ID),
        );
        let password = found.and_then(|found| {
            if found {
                proxy.call::<_, _, String>(
                    "readPassword",
                    &(handle, folder.as_str(), entry.as_str(), APP_ID),
                )
            } else {
                Ok(String::new())
            }
        });
        if let Err(error) = proxy.call::<_, _, i32>("close", &(handle, false, APP_ID)) {
            log::warn!("couldn't close browser keyring handle: {error}");
        }
        Ok(Some(password?.into_bytes()).filter(|p| !p.is_empty()))
    }
}

/// The keys a Chromium profile's values are encrypted with. Linux: `v10`
/// with the fixed password, `v11` with the Safe Storage one. macOS: `v10`
/// with the Keychain's.
pub(super) struct Keys {
    pub(super) v10: Option<[u8; 16]>,
    pub(super) v11: Option<[u8; 16]>,
}

pub(super) fn derive_key(password: &[u8], iterations: u32) -> [u8; 16] {
    let mut key = [0u8; 16];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password, b"saltysalt", iterations, &mut key);
    key
}

pub(super) fn decrypt(encrypted: &[u8], keys: &Keys, host: &str, version: i64) -> Option<String> {
    let key = match encrypted.get(..3)? {
        b"v10" => keys.v10.as_ref()?,
        b"v11" => keys.v11.as_ref()?,
        _ => return None,
    };
    let body = &encrypted[3..];
    let decryptor = cbc::Decryptor::<aes::Aes128>::new(key.into(), &[b' '; 16].into());
    let plain = decryptor.decrypt_padded_vec_mut::<Pkcs7>(body).ok()?;
    // Schema 24 prefixes the value with SHA-256 of its host.
    let plain = if version >= 24 {
        if plain.get(..32)? != &sha2::Sha256::digest(host.as_bytes())[..] {
            return None;
        }
        &plain[32..]
    } else {
        &plain[..]
    };
    String::from_utf8(plain.to_vec()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::BlockEncryptMut;

    #[test]
    fn schema_24_rejects_a_missing_or_wrong_host_digest() {
        let key = derive_key(b"synthetic password", 1);
        let keys = Keys {
            v10: Some(key),
            v11: None,
        };
        for plain in [b"short".to_vec(), vec![0; 40]] {
            let body = cbc::Encryptor::<aes::Aes128>::new(&key.into(), &[b' '; 16].into())
                .encrypt_padded_vec_mut::<Pkcs7>(&plain);
            let encrypted = [b"v10".as_slice(), &body].concat();
            assert_eq!(decrypt(&encrypted, &keys, ".youtube.com", 24), None);
        }
    }

    /// A value encrypted the way Chrome does on macOS (schema 24): the
    /// Keychain password through PBKDF2 (1003 rounds), AES-128-CBC with an
    /// IV of spaces, `v10`, the host's SHA-256 in front. Synthetic password
    /// and value.
    #[test]
    fn decrypts_a_macos_value() {
        let key = derive_key(b"synthetic keychain password", MAC_ITERATIONS);
        let host = ".youtube.com";
        let mut plain = sha2::Sha256::digest(host.as_bytes()).to_vec();
        plain.extend_from_slice(b"synthetic-value");
        let body = cbc::Encryptor::<aes::Aes128>::new(&key.into(), &[b' '; 16].into())
            .encrypt_padded_vec_mut::<Pkcs7>(&plain);
        let encrypted = [b"v10".as_slice(), &body].concat();
        let keys = Keys {
            v10: Some(key),
            v11: None,
        };
        assert_eq!(
            decrypt(&encrypted, &keys, host, 24).as_deref(),
            Some("synthetic-value")
        );
        let wrong = Keys {
            v10: Some(derive_key(b"synthetic keychain password", 1)),
            v11: None,
        };
        assert_ne!(
            decrypt(&encrypted, &wrong, host, 24).as_deref(),
            Some("synthetic-value")
        );
    }
}
