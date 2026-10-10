//! Session headers and Netscape cookie encoding, without browser or keyring access.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, PartialEq, Eq)]
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
    pub(crate) cookies: Vec<Cookie>,
    /// Only Encore's own imported/pasted files are renewed on disk. Browser
    /// databases and files supplied outside those slots remain read-only.
    pub(crate) persist: Option<PathBuf>,
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
    /// The root-page `Cookie` header for music.youtube.com.
    pub fn header(&self) -> String {
        self.header_at("music.youtube.com", "/")
    }

    /// Cookies scoped to the request's host and path. In particular, a
    /// www.youtube.com response must not overwrite a Music-only cookie.
    pub(crate) fn header_for(&self, url: &reqwest::Url) -> String {
        if url.scheme() != "https" {
            return String::new();
        }
        self.header_at(url.host_str().unwrap_or_default(), url.path())
    }

    fn header_at(&self, host: &str, path: &str) -> String {
        let mut chosen: Vec<&Cookie> = Vec::new();
        let now = unix_now();
        for cookie in self.cookies.iter().filter(|c| c.applies(host, path, now)) {
            match chosen.iter_mut().find(|c| c.name == cookie.name) {
                Some(existing)
                    if (specificity(&cookie.host), cookie.path.len())
                        > (specificity(&existing.host), existing.path.len()) =>
                {
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
        self.cookie("SAPISID")
            .or_else(|| self.cookie("__Secure-3PAPISID"))
    }

    /// A youtube.com cookie's value (`__Secure-1PAPISID` and
    /// `__Secure-3PAPISID` for the other SAPISIDHASH schemes).
    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.cookie_at(name, "music.youtube.com", "/")
    }

    pub(crate) fn cookie_for(&self, name: &str, url: &reqwest::Url) -> Option<&str> {
        if url.scheme() != "https" {
            return None;
        }
        self.cookie_at(name, url.host_str()?, url.path())
    }

    fn cookie_at(&self, name: &str, host: &str, path: &str) -> Option<&str> {
        let now = unix_now();
        self.cookies
            .iter()
            .filter(|c| c.name == name && c.applies(host, path, now))
            .max_by_key(|c| (specificity(&c.host), c.path.len()))
            .map(|c| c.value.as_str())
    }

    /// Merge trusted YouTube responses as a browser's cookie jar does. Keep
    /// the domain/path/expiry instead of a name-only playback overlay.
    pub(crate) fn renew(&mut self, url: &reqwest::Url, headers: &[String]) -> bool {
        let Some(host) = url.host_str() else {
            return false;
        };
        if url.scheme() != "https" || !youtube_domain(host) {
            return false;
        }
        let now = unix_now();
        let mut changed = false;
        for header in headers {
            let Ok(parsed) = cookie::Cookie::parse(header.as_str()) else {
                continue;
            };
            // Prevent an invalid header from injecting a Netscape file row or
            // another request header. Never include the raw header in an error.
            if !valid_pair(parsed.name(), parsed.value()) {
                continue;
            }
            let domain = match parsed.domain() {
                Some(domain) => {
                    let domain = domain.to_ascii_lowercase();
                    if !youtube_domain(&domain) || !domain_matches(host, &domain) {
                        continue;
                    }
                    format!(".{domain}")
                }
                None => host.to_owned(),
            };
            let path = parsed
                .path()
                .filter(|p| p.starts_with('/') && !p.contains(['\t', '\n', '\r']))
                .unwrap_or_else(|| default_path(url.path()))
                .to_owned();
            let expires = parsed
                .max_age()
                .map(|age| now.saturating_add(age.whole_seconds()))
                .or_else(|| parsed.expires_datetime().map(|t| t.unix_timestamp()))
                .unwrap_or(0);
            let deleted = match parsed.max_age() {
                Some(age) => age.whole_seconds() <= 0,
                None => parsed
                    .expires_datetime()
                    .is_some_and(|t| t.unix_timestamp() <= now),
            };
            let existing = self.cookies.iter().position(|c| {
                c.name == parsed.name()
                    && c.host.trim_start_matches('.') == domain.trim_start_matches('.')
                    && c.path == path
            });
            if deleted {
                if let Some(index) = existing {
                    self.cookies.remove(index);
                    changed = true;
                }
                continue;
            }
            let renewed = Cookie {
                host: domain,
                name: parsed.name().into(),
                value: parsed.value().into(),
                path,
                secure: parsed.secure().unwrap_or(false),
                expires,
            };
            if let Some(index) = existing {
                if self.cookies[index] != renewed {
                    self.cookies[index] = renewed;
                    changed = true;
                }
            } else {
                self.cookies.push(renewed);
                changed = true;
            }
        }
        changed
    }

    pub(crate) fn persist(&self) -> Result<()> {
        if let Some(path) = &self.persist {
            self.write_netscape(path)?;
        }
        Ok(())
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

impl Cookie {
    fn applies(&self, host: &str, path: &str, now: i64) -> bool {
        let domain = self.host.trim_start_matches('.');
        let matches_host = if self.host.starts_with('.') {
            domain_matches(host, domain)
        } else {
            host == domain
        };
        matches_host
            && (self.expires == 0 || self.expires > now)
            && path.starts_with(&self.path)
            && (self.path.ends_with('/')
                || path.len() == self.path.len()
                || path.as_bytes().get(self.path.len()) == Some(&b'/'))
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .try_into()
        .unwrap_or(i64::MAX)
}

fn domain_matches(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

fn youtube_domain(host: &str) -> bool {
    domain_matches(host, "youtube.com")
}

fn default_path(path: &str) -> &str {
    path.rsplit_once('/')
        .map(|(parent, _)| parent)
        .filter(|parent| !parent.is_empty())
        .unwrap_or("/")
}

fn valid_pair(name: &str, value: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_graphic() && !b"()<>@,;:\\\"/[]?={}".contains(&b))
        && value.bytes().all(|b| !b.is_ascii_control() && b != b';')
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
pub(crate) fn parse_netscape(text: &str) -> Vec<Cookie> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim_end_matches('\r');
            let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
            if line.starts_with('#') {
                return None;
            }
            let mut fields = line.split('\t');
            let host = fields.next()?;
            let subdomains = fields.next()?;
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
                host: if subdomains.eq_ignore_ascii_case("TRUE") && !host.starts_with('.') {
                    format!(".{host}")
                } else {
                    host.to_owned()
                },
                name: name.to_owned(),
                value: value.to_owned(),
                path: path.to_owned(),
                secure: secure.eq_ignore_ascii_case("TRUE"),
                expires: expiry(expires),
            })
        })
        .collect()
}

/// A cookie file's expiry in whole seconds. Exporters write integers or
/// fractions; anything else (or nothing) is a session cookie, as before:
/// dropping the line would lose the sign-in cookies over a formatting quirk.
fn expiry(field: &str) -> i64 {
    let field = field.trim();
    field
        .parse::<i64>()
        .ok()
        .or_else(|| {
            field
                .parse::<f64>()
                .ok()
                .filter(|seconds| seconds.is_finite())
                .map(|seconds| seconds as i64)
        })
        .unwrap_or(0)
        .max(0)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test assertions report fixture failures"
)]
mod tests {
    use super::*;

    fn session(rows: &str) -> Session {
        Session {
            source: "Synthetic cookies".into(),
            profile: "synthetic".into(),
            cookies: parse_netscape(rows),
            persist: None,
        }
    }

    fn renew(session: &mut Session, url: &str, headers: &[&str]) -> bool {
        session.renew(
            &url.parse().unwrap(),
            &headers.iter().map(|h| (*h).into()).collect::<Vec<_>>(),
        )
    }

    #[test]
    fn response_cookies_replace_and_delete_the_sessions_cookies() {
        let mut s = session(
            ".youtube.com\tTRUE\t/\tTRUE\t0\tSID\tkeep\n.youtube.com\tTRUE\t/\tTRUE\t0\tSIDCC\told\n.youtube.com\tTRUE\t/\tTRUE\t0\tYEC\tremove\n",
        );
        assert!(renew(
            &mut s,
            "https://www.youtube.com/watch?v=synthetic",
            &[
                "SIDCC=new; Domain=.youtube.com; Path=/; Secure; Max-Age=3600",
                "YEC=gone; Domain=.youtube.com; Path=/; Max-Age=0",
                "SESSION=keep; Domain=.youtube.com; Path=/; Secure",
            ]
        ));
        assert_eq!(s.header(), "SID=keep; SIDCC=new; SESSION=keep");
        assert!(
            s.cookies
                .iter()
                .any(|c| c.name == "SIDCC" && c.expires > unix_now())
        );
        assert!(renew(
            &mut s,
            "https://music.youtube.com/",
            &["SIDCC=gone; Domain=.youtube.com; Path=/; Expires=Thu, 01 Jan 1970 00:00:00 GMT",]
        ));
        assert_eq!(s.header(), "SID=keep; SESSION=keep");
        // Max-Age overrides Expires, including an Expires in the past.
        assert!(renew(
            &mut s,
            "https://music.youtube.com/",
            &[
                "SIDCC=valid; Domain=.youtube.com; Path=/; Max-Age=3600; Expires=Thu, 01 Jan 1970 00:00:00 GMT",
            ]
        ));
        assert_eq!(s.cookie("SIDCC"), Some("valid"));
        assert!(!renew(
            &mut s,
            "https://music.youtube.com/",
            &["SESSION=keep; Domain=.youtube.com; Path=/; Secure",]
        ));
    }

    #[test]
    fn music_and_playback_cookies_keep_their_scopes() {
        let mut s = session(
            ".youtube.com\tTRUE\t/\tTRUE\t0\tSAPISID\tshared\nmusic.youtube.com\tFALSE\t/\tTRUE\t0\tSAPISID\tmusic\n",
        );
        renew(
            &mut s,
            "https://www.youtube.com/watch",
            &["SAPISID=playback; Path=/; Secure", "WWW=host-only; Secure"],
        );
        assert_eq!(s.header(), "SAPISID=music");
        assert_eq!(s.sapisid(), Some("music"));
        let playback: reqwest::Url = "https://www.youtube.com/watch".parse().unwrap();
        assert_eq!(s.header_for(&playback), "SAPISID=playback; WWW=host-only");
        assert_eq!(s.cookie_for("SAPISID", &playback), Some("playback"));
        // A path cookie's default directory is /youtubei/v1, not /.
        renew(
            &mut s,
            "https://music.youtube.com/youtubei/v1/browse",
            &["API=only-here; Secure"],
        );
        assert!(!s.header().contains("API="));
        assert!(
            s.header_for(
                &"https://music.youtube.com/youtubei/v1/next"
                    .parse()
                    .unwrap()
            )
            .contains("API=only-here")
        );
        assert!(
            !s.header_for(&"https://music.youtube.com/youtubei/v10".parse().unwrap())
                .contains("API=")
        );
        assert!(
            s.header_for(&"http://music.youtube.com/".parse().unwrap())
                .is_empty()
        );
    }

    #[test]
    fn response_cookies_cannot_escape_their_domain_or_inject_a_file_row() {
        let mut s = session(".youtube.com\tTRUE\t/\tTRUE\t0\tSAPISID\tkeep\n");
        let invalid = [
            "SAPISID=bad; Domain=google.com; Path=/",
            "SAPISID=bad; Domain=notyoutube.com; Path=/",
            "SAPISID=bad; Domain=music.youtube.com; Path=/",
            "SAPISID=bad; Domain=com; Path=/",
            "=bad; Domain=.youtube.com; Path=/",
            "SAPISID=bad\trow; Domain=.youtube.com; Path=/",
            "SAPISID=bad\r\nrow; Domain=.youtube.com; Path=/",
        ];
        assert!(!renew(&mut s, "https://www.youtube.com/", &invalid));
        assert!(!renew(
            &mut s,
            "https://notyoutube.com/",
            &["SAPISID=bad; Domain=.youtube.com; Path=/"]
        ));
        assert!(!renew(
            &mut s,
            "http://music.youtube.com/",
            &["SAPISID=bad; Domain=.youtube.com; Path=/"]
        ));
        assert_eq!(s.header(), "SAPISID=keep");
    }

    #[test]
    fn cookie_file_domain_flags_and_expiry_survive_a_round_trip() {
        let s = session(
            "youtube.com\tTRUE\t/\tTRUE\t0\tSAPISID\tdomain\nmusic.youtube.com\tFALSE\t/\tTRUE\t1\tEXPIRED\told\n.google.com\tTRUE\t/\tTRUE\t0\tSID\tgoogle\n",
        );
        assert_eq!(s.header(), "SAPISID=domain");
        assert_eq!(s.cookies[0].host, ".youtube.com");
        // Session cookies (expiry 0) are intentionally retained after restart.
        assert_eq!(s.sapisid(), Some("domain"));
        assert!(!format!("{s:?}").contains("domain"));
    }

    #[test]
    fn imported_cookies_require_a_domain_boundary() {
        let rows = "notyoutube.com\tTRUE\t/\tTRUE\t0\tPREF\tx\n.youtube.com\tTRUE\t/\tTRUE\t0\tPREF\tx\naccounts.google.com\tTRUE\t/\tTRUE\t0\tPREF\tx\n";
        let cookies = parse_netscape(rows);
        let hosts: Vec<_> = cookies.iter().map(|cookie| cookie.host.as_str()).collect();
        assert_eq!(hosts, [".youtube.com", ".accounts.google.com"]);
    }

    #[test]
    fn odd_expiries_keep_the_cookie() {
        // Exporters write fractional seconds or leave session cookies blank;
        // the sign-in cookies must survive either.
        let rows = ".youtube.com\tTRUE\t/\tTRUE\t1791449919.537\tSAPISID\ta\n.youtube.com\tTRUE\t/\tTRUE\t\tHSID\tb\n.youtube.com\tTRUE\t/\tTRUE\tbad\tSID\tc\n";
        let expiries: Vec<_> = parse_netscape(rows)
            .iter()
            .map(|cookie| (cookie.name.clone(), cookie.expires))
            .collect();
        assert_eq!(
            expiries,
            [
                ("SAPISID".to_owned(), 1_791_449_919),
                ("HSID".to_owned(), 0),
                ("SID".to_owned(), 0),
            ]
        );
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
