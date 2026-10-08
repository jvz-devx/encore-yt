//! A small element tree over quick-xml, for UPnP device descriptions and
//! SOAP replies: element local names (namespace prefixes dropped), their
//! text with entities resolved, and their children.

use anyhow::{Context, Result};
use quick_xml::Reader;
use quick_xml::events::Event;

const MAX_XML_BYTES: usize = 2 * 1024 * 1024;
const MAX_XML_DEPTH: usize = 64;

/// Device descriptions and SOAP replies are small. Bound the body while
/// reading, before building either a string or the recursively owned tree.
pub(crate) async fn read_body(mut response: reqwest::Response) -> Result<String> {
    anyhow::ensure!(
        response
            .content_length()
            .is_none_or(|len| len <= MAX_XML_BYTES as u64),
        "device XML body is too large"
    );
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.context("read device XML body")? {
        anyhow::ensure!(
            chunk.len() <= MAX_XML_BYTES - body.len(),
            "device XML body is too large"
        );
        body.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

#[derive(Clone, Debug, Default)]
pub struct Node {
    pub name: String,
    pub text: String,
    pub children: Vec<Node>,
}

impl Node {
    pub fn parse(xml: &str) -> Result<Node> {
        anyhow::ensure!(xml.len() <= MAX_XML_BYTES, "device XML body is too large");
        let mut reader = Reader::from_str(xml);
        let mut stack = vec![Node::default()];
        // Text arrives in pieces around entity references; they are joined
        // raw and unescaped when the element ends.
        let mut raw = vec![String::new()];
        loop {
            match reader.read_event().context("XML")? {
                Event::Start(start) => {
                    anyhow::ensure!(
                        stack.len() <= MAX_XML_DEPTH,
                        "device XML is too deeply nested"
                    );
                    stack.push(Node {
                        name: local(start.local_name().as_ref()),
                        ..Node::default()
                    });
                    raw.push(String::new());
                }
                Event::Empty(start) => {
                    anyhow::ensure!(
                        stack.len() <= MAX_XML_DEPTH,
                        "device XML is too deeply nested"
                    );
                    let node = Node {
                        name: local(start.local_name().as_ref()),
                        ..Node::default()
                    };
                    stack
                        .last_mut()
                        .context("missing XML root")?
                        .children
                        .push(node);
                }
                Event::Text(text) => raw
                    .last_mut()
                    .context("missing XML text root")?
                    .push_str(&text.decode()?),
                Event::CData(data) => {
                    let data = data.decode()?;
                    raw.last_mut()
                        .context("missing XML text root")?
                        .push_str(&quick_xml::escape::escape(&*data));
                }
                Event::GeneralRef(entity) => {
                    let name = entity.decode()?;
                    raw.last_mut()
                        .context("missing XML text root")?
                        .push_str(&format!("&{name};"));
                }
                Event::End(_) => {
                    let mut node = stack.pop().context("unbalanced XML")?;
                    let text = raw.pop().context("unbalanced XML text")?;
                    node.text = quick_xml::escape::unescape(text.trim())?.into_owned();
                    stack
                        .last_mut()
                        .context("unbalanced XML")?
                        .children
                        .push(node);
                }
                Event::Eof => break,
                _ => {}
            }
        }
        let mut root = stack.pop().context("empty XML")?;
        anyhow::ensure!(stack.is_empty(), "unclosed XML element");
        root.children.pop().context("no root element")
    }

    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    /// The text of the child `name`, or "" when there is none.
    pub fn text_of(&self, name: &str) -> &str {
        self.child(name).map_or("", |c| c.text.as_str())
    }

    /// The first element called `name` at any depth, this one included.
    pub fn find(&self, name: &str) -> Option<&Node> {
        if self.name == name {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(name))
    }

    /// Every element called `name` at any depth.
    pub fn find_all<'a>(&'a self, name: &str, out: &mut Vec<&'a Node>) {
        if self.name == name {
            out.push(self);
        }
        for child in &self.children {
            child.find_all(name, out);
        }
    }
}

fn local(name: &[u8]) -> String {
    String::from_utf8_lossy(name).into_owned()
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
    fn rejects_oversized_and_deeply_nested_xml() {
        let nested = format!(
            "{}{}",
            "<x>".repeat(MAX_XML_DEPTH + 1),
            "</x>".repeat(MAX_XML_DEPTH + 1)
        );
        assert!(Node::parse(&nested).is_err());
        assert!(Node::parse(&"x".repeat(MAX_XML_BYTES + 1)).is_err());
        let allowed = format!(
            "{}{}",
            "<x>".repeat(MAX_XML_DEPTH),
            "</x>".repeat(MAX_XML_DEPTH)
        );
        assert!(Node::parse(&allowed).is_ok());
        let too_deep_leaf = format!(
            "{}<leaf/>{}",
            "<x>".repeat(MAX_XML_DEPTH),
            "</x>".repeat(MAX_XML_DEPTH)
        );
        assert!(Node::parse(&too_deep_leaf).is_err());
    }

    #[tokio::test]
    async fn rejects_a_large_advertised_body_without_waiting_for_its_bytes() {
        use tokio::io::AsyncWriteExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = tokio::io::BufReader::new(stream);
            crate::http::read_request(&mut reader).await.unwrap();
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                MAX_XML_BYTES + 1
            );
            reader
                .into_inner()
                .write_all(head.as_bytes())
                .await
                .unwrap();
        });
        let response = reqwest::get(format!("http://{address}/description"))
            .await
            .unwrap();
        assert!(
            read_body(response)
                .await
                .unwrap_err()
                .to_string()
                .contains("too large")
        );
        server.await.unwrap();
    }

    #[test]
    fn names_lose_their_prefix_and_text_its_entities() {
        let xml = r#"<?xml version="1.0"?>
            <s:Envelope xmlns:s="x"><s:Body><u:R xmlns:u="y">
            <Meta>&lt;DIDL-Lite&gt;a &amp; b&lt;/DIDL-Lite&gt;</Meta><Empty/>
            </u:R></s:Body></s:Envelope>"#;
        let root = Node::parse(xml).unwrap();
        assert_eq!(root.name, "Envelope");
        let r = root.find("R").unwrap();
        assert_eq!(r.text_of("Meta"), "<DIDL-Lite>a & b</DIDL-Lite>");
        assert!(r.child("Empty").is_some());
        assert_eq!(r.text_of("Missing"), "");
    }
}
