//! UPnP control: a SOAP envelope POSTed to a service's control URL with a
//! `SOAPACTION` header; the reply's `<u:{action}Response>` holds the out
//! arguments, and a fault carries a `UPnPError` code.

use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
use quick_xml::escape::escape;

use crate::xml::Node;

pub fn envelope(service: &str, action: &str, args: &[(&str, &str)]) -> String {
    let args: String = args
        .iter()
        .map(|(name, value)| format!("<{name}>{}</{name}>", escape(*value)))
        .collect();
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" "#,
            r#"s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">"#,
            r#"<s:Body><u:{action} xmlns:u="{service}">{args}</u:{action}></s:Body></s:Envelope>"#
        ),
        action = action,
        service = service,
        args = args,
    )
}

/// Calls `action` and returns the response element.
pub async fn call(
    control_url: &str,
    service: &str,
    action: &str,
    args: &[(&str, &str)],
) -> Result<Node> {
    static HTTP: OnceLock<reqwest::Client> = OnceLock::new();
    let http = match HTTP.get() {
        Some(http) => http,
        None => {
            // Another caller may initialize the same shared client first.
            let _ = HTTP.set(super::client()?);
            HTTP.get().context("SOAP client was not initialized")?
        }
    };
    let response = http
        .post(control_url)
        .header("Content-Type", r#"text/xml; charset="utf-8""#)
        .header("SOAPACTION", format!("\"{service}#{action}\""))
        .body(envelope(service, action, args))
        .send()
        .await
        .with_context(|| action.to_owned())?;
    let status = response.status();
    let body = response.text().await.context("read SOAP response")?;
    let root = Node::parse(&body).with_context(|| format!("{action}: HTTP {status}, not XML"))?;
    if !status.is_success() {
        let error = root.find("UPnPError");
        let code = error.map_or("", |e| e.text_of("errorCode"));
        let text = error.map_or("", |e| e.text_of("errorDescription"));
        bail!("{action}: HTTP {status}, UPnP error {code} {text}");
    }
    root.find(&format!("{action}Response"))
        .cloned()
        .with_context(|| format!("missing {action}Response in SOAP reply"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_envelope_escapes_arguments() {
        let xml = envelope(
            super::super::AV_TRANSPORT,
            "SetAVTransportURI",
            &[
                ("InstanceID", "0"),
                ("CurrentURIMetaData", "<DIDL-Lite>&</DIDL-Lite>"),
            ],
        );
        let root = Node::parse(&xml).unwrap();
        let action = root.find("SetAVTransportURI").unwrap();
        assert_eq!(
            action.text_of("CurrentURIMetaData"),
            "<DIDL-Lite>&</DIDL-Lite>"
        );
        assert!(xml.contains(r#"xmlns:u="urn:schemas-upnp-org:service:AVTransport:1""#));
    }
}
