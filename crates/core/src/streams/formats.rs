//! Select playable audio formats in preference order.

use anyhow::{Result, bail};
use serde_json::Value;

/// The audio formats wanted, best first: Opus (WebM) and AAC-LC (MP4),
/// which the engine decodes. Never HE-AAC (139, 599): it has no decoder
/// for it.
const ITAGS: [u64; 7] = [774, 141, 251, 140, 250, 249, 600];

#[derive(Debug, PartialEq)]
pub struct Format {
    pub itag: u32,
    pub url: Option<String>,
    pub cipher: Option<String>,
}

/// The best wanted audio format of a playable response.
pub fn best_audio(response: &Value) -> Result<Format> {
    Ok(audio_formats(response)?.remove(0))
}

/// The wanted audio formats of a playable response, best first; at least one.
pub fn audio_formats(response: &Value) -> Result<Vec<Format>> {
    let status = crate::parse::at(response, &["playabilityStatus", "status"])
        .and_then(Value::as_str)
        .unwrap_or("none");
    if status != "OK" {
        let reason = crate::parse::at(response, &["playabilityStatus", "reason"])
            .and_then(Value::as_str)
            .unwrap_or("");
        bail!("not playable ({status}): {reason}");
    }
    let formats = crate::parse::at(response, &["streamingData", "adaptiveFormats"])
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let usable = |f: &&Value| {
        // Dynamic-range-compressed copies share the itag; skip them.
        f.get("isDrc").and_then(Value::as_bool) != Some(true)
            && f.get("drmFamilies").is_none()
            && (f.get("url").is_some() || f.get("signatureCipher").is_some())
    };
    let chosen: Vec<&Value> = ITAGS
        .iter()
        .filter_map(|itag| {
            formats
                .iter()
                .filter(usable)
                .find(|f| f.get("itag").and_then(Value::as_u64) == Some(*itag))
        })
        .collect();
    if chosen.is_empty() {
        bail!(if formats.is_empty() {
            "no formats (SABR only?)"
        } else {
            "no audio format with a URL that Encore plays"
        });
    }
    Ok(chosen
        .into_iter()
        .map(|f| {
            let text = |key: &str| f.get(key).and_then(Value::as_str).map(str::to_owned);
            Format {
                itag: f.get("itag").and_then(Value::as_u64).unwrap_or(0) as u32,
                url: text("url"),
                cipher: text("signatureCipher"),
            }
        })
        .collect())
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "tests inspect synthetic player responses"
)]
mod tests {
    use super::*;
    use serde_json::json;

    fn response(itags: &[u64]) -> Value {
        let formats: Vec<Value> = itags
            .iter()
            .map(|itag| json!({"itag": itag, "url": format!("https://example.invalid/{itag}")}))
            .collect();
        json!({
            "playabilityStatus": {"status": "OK"},
            "streamingData": {"adaptiveFormats": formats},
        })
    }

    /// HE-AAC (139, 599) has no decoder in the engine: never picked, even
    /// when it is all there is.
    #[test]
    fn never_picks_he_aac() {
        let picked = audio_formats(&response(&[139, 599, 140, 249])).expect("formats");
        let itags: Vec<u32> = picked.iter().map(|f| f.itag).collect();
        assert_eq!(itags, [140, 249]);
        assert!(audio_formats(&response(&[139, 599])).is_err());
    }

    /// Premium's formats come first: 774 (Opus ~256 kbps), then 141 (AAC
    /// 256 kbps), then 251, whatever order the response lists them in.
    #[test]
    fn premium_formats_first() {
        let picked = audio_formats(&response(&[249, 251, 141, 140, 774])).expect("formats");
        let itags: Vec<u32> = picked.iter().map(|f| f.itag).collect();
        assert_eq!(itags, [774, 141, 251, 140, 249]);
    }

    /// DRC copies and DRM-protected formats (a TV experiment) are skipped.
    #[test]
    fn skips_drc_and_drm() {
        let response = json!({
            "playabilityStatus": {"status": "OK"},
            "streamingData": {"adaptiveFormats": [
                {"itag": 774, "url": "https://example.invalid/a", "drmFamilies": ["WIDEVINE"]},
                {"itag": 141, "url": "https://example.invalid/b", "isDrc": true},
                {"itag": 141, "signatureCipher": "s=x&url=https%3A%2F%2Fexample.invalid%2Fc"},
            ]},
        });
        let picked = audio_formats(&response).expect("formats");
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].itag, 141);
        assert!(picked[0].cipher.is_some());
    }
}
