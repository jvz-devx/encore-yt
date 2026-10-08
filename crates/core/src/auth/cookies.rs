//! Session headers and Netscape cookie encoding, without browser or keyring access.

use anyhow::{Context, Result};
use std::path::Path;

#[derive(Clone)]
pub struct Cookie {
    pub host: String,
    pub name: String,
    pub value: String,
    pub path: String,
    pub secure: bool,
    /// Unix seconds; 0 for a session cookie.
    pub expires: i64,
}

/// A browser's YouTube and Google cookies.
#[derive(Clone)]
pub struct Session {
    /// "Google Chrome (Default)", for the account menu.
    pub source: String,
    /// The [`super::Profile::id`] it came from.
    pub profile: String,
    pub(super) cookies: Vec<Cookie>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("source", &self.source)
            .field("cookies", &self.cookies.len())
            .finish()
    }
}

impl Session {
    /// The `Cookie` header for music.youtube.com: every youtube.com cookie,
    /// the most specific host winning a repeated name.
    pub fn header(&self) -> String {
        let mut chosen: Vec<&Cookie> = Vec::new();
        for cookie in self.cookies.iter().filter(|c| applies_to_music(&c.host)) {
            match chosen.iter_mut().find(|c| c.name == cookie.name) {
                Some(existing) if specificity(&cookie.host) > specificity(&existing.host) => {
                    *existing = cookie
                }
                Some(_) => {}
                None => chosen.push(cookie),
            }
        }
        let mut header = String::new();
        for cookie in chosen {
            if !header.is_empty() {
                header.push_str("; ");
            }
            header.push_str(&cookie.name);
            header.push('=');
            header.push_str(&cookie.value);
        }
        header
    }

    /// The value InnerTube's SAPISIDHASH authorization is computed from.
    pub fn sapisid(&self) -> Option<&str> {
        ["SAPISID", "__Secure-3PAPISID"].iter().find_map(|name| {
            self.cookies
                .iter()
                .find(|c| c.name == *name && applies_to_music(&c.host))
                .map(|c| c.value.as_str())
        })
    }

    /// A youtube.com cookie's value (`__Secure-1PAPISID` and
    /// `__Secure-3PAPISID` for the other SAPISIDHASH schemes).
    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.cookies
            .iter()
            .find(|c| c.name == name && applies_to_music(&c.host))
            .map(|c| c.value.as_str())
    }

    /// Writes the cookies as a Netscape cookie file (mode 0600).
    pub fn write_netscape(&self, path: &Path) -> Result<()> {
        let mut text = String::from("# Netscape HTTP Cookie File\n");
        for c in &self.cookies {
            let domain_flag = if c.host.starts_with('.') {
                "TRUE"
            } else {
                "FALSE"
            };
            let secure = if c.secure { "TRUE" } else { "FALSE" };
            text.push_str(&format!(
                "{}\t{domain_flag}\t{}\t{secure}\t{}\t{}\t{}\n",
                c.host, c.path, c.expires, c.name, c.value
            ));
        }
        crate::paths::write_atomic(path, text.as_bytes()).context("write private cookie file")
    }
}

pub(super) fn applies_to_music(host: &str) -> bool {
    host == "music.youtube.com"
        || host == ".music.youtube.com"
        || host == ".youtube.com"
        || host == "youtube.com"
}

fn specificity(host: &str) -> usize {
    host.trim_start_matches('.').len()
}

/// The cookies of a `Cookie` request header, `name=value; name=value`, as
/// youtube.com cookies. Tolerates a leading `Cookie:` and quotes around it
/// (from "Copy as cURL"). They are session cookies: a header carries no
/// expiry.
pub(super) fn parse_cookie_header(text: &str) -> Vec<Cookie> {
    let mut text = text.trim();
    let lower = text.to_ascii_lowercase();
    if let Some(at) = lower.find("cookie:") {
        text = &text[at + "cookie:".len()..];
    }
    let text = text
        .trim()
        .trim_start_matches(['\'', '"'])
        .split(['\'', '"', '\n', '\r'])
        .next()
        .unwrap_or_default();
    text.split(';')
        .filter_map(|pair| {
            let (name, value) = pair.trim().split_once('=')?;
            let name = name.trim();
            if name.is_empty() || name.contains(char::is_whitespace) {
                return None;
            }
            Some(Cookie {
                host: ".youtube.com".into(),
                name: name.into(),
                value: value.trim().into(),
                path: "/".into(),
                secure: true,
                expires: 0,
            })
        })
        .collect()
}

/// The youtube.com and google.com lines of a Netscape cookie file:
/// `host, subdomains, path, secure, expires, name, value`, tab-separated.
/// `#HttpOnly_` marks an HttpOnly cookie; other `#` lines are comments.
pub(super) fn parse_netscape(text: &str) -> Vec<Cookie> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim_end_matches('\r');
            let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
            if line.starts_with('#') {
                return None;
            }
            let mut fields = line.split('\t');
            let host = fields.next()?;
            let _subdomains = fields.next()?;
            let path = fields.next()?;
            let secure = fields.next()?;
            let expires = fields.next()?;
            let name = fields.next()?;
            let value = fields.next().unwrap_or_default();
            let domain = host.trim_start_matches('.');
            if !(matches!(domain, "youtube.com" | "google.com")
                || domain.ends_with(".youtube.com")
                || domain.ends_with(".google.com"))
            {
                return None;
            }
            Some(Cookie {
                host: host.to_owned(),
                name: name.to_owned(),
                value: value.to_owned(),
                path: path.to_owned(),
                secure: secure.eq_ignore_ascii_case("TRUE"),
                expires: expires.parse::<i64>().ok()?.max(0),
            })
        })
        .collect()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test assertions report fixture failures"
)]
mod tests {
    use super::*;

    #[test]
    fn imported_cookies_require_a_domain_boundary_and_valid_expiry() {
        let rows = "notyoutube.com\tTRUE\t/\tTRUE\t0\tPREF\tx\n.youtube.com\tTRUE\t/\tTRUE\t0\tPREF\tx\nmusic.youtube.com\tTRUE\t/\tTRUE\tbad\tPREF\tx\naccounts.google.com\tTRUE\t/\tTRUE\t0\tPREF\tx\n";
        let cookies = parse_netscape(rows);
        let hosts: Vec<_> = cookies.iter().map(|cookie| cookie.host.as_str()).collect();
        assert_eq!(hosts, [".youtube.com", "accounts.google.com"]);
    }

    #[test]
    fn reads_a_pasted_cookie_header() {
        let cookies = parse_cookie_header("Cookie: PREF=f6=1; SAPISID=abc/def; HSID=x=y\n");
        let pairs: Vec<_> = cookies
            .iter()
            .map(|c| (c.name.as_str(), c.value.as_str()))
            .collect();
        assert_eq!(
            pairs,
            [("PREF", "f6=1"), ("SAPISID", "abc/def"), ("HSID", "x=y")]
        );
        assert!(
            cookies
                .iter()
                .any(|c| applies_to_music(&c.host) && c.name == "SAPISID")
        );
        let curl = parse_cookie_header("-H 'cookie: SAPISID=a; SID=b' \\");
        assert_eq!(curl.len(), 2);
        assert!(parse_cookie_header("hello there").is_empty());
    }
}
