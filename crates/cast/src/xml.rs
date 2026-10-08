//! A small element tree over quick-xml, for UPnP device descriptions and
//! SOAP replies: element local names (namespace prefixes dropped), their
//! text with entities resolved, and their children.

use anyhow::{Context, Result};
use quick_xml::Reader;
use quick_xml::events::Event;

#[derive(Clone, Debug, Default)]
pub struct Node {
    pub name: String,
    pub text: String,
    pub children: Vec<Node>,
}

impl Node {
    pub fn parse(xml: &str) -> Result<Node> {
        let mut reader = Reader::from_str(xml);
        let mut stack = vec![Node::default()];
        // Text arrives in pieces around entity references; they are joined
        // raw and unescaped when the element ends.
        let mut raw = vec![String::new()];
        loop {
            match reader.read_event().context("XML")? {
                Event::Start(start) => {
                    stack.push(Node {
                        name: local(start.local_name().as_ref()),
                        ..Node::default()
                    });
                    raw.push(String::new());
                }
                Event::Empty(start) => {
                    let node = Node {
                        name: local(start.local_name().as_ref()),
                        ..Node::default()
                    };
                    stack.last_mut().expect("root").children.push(node);
                }
                Event::Text(text) => raw.last_mut().expect("root").push_str(&text.decode()?),
                Event::CData(data) => {
                    let data = data.decode()?;
                    raw.last_mut()
                        .expect("root")
                        .push_str(&quick_xml::escape::escape(&*data));
                }
                Event::GeneralRef(entity) => {
                    let name = entity.decode()?;
                    raw.last_mut().expect("root").push_str(&format!("&{name};"));
                }
                Event::End(_) => {
                    let mut node = stack.pop().context("unbalanced XML")?;
                    let text = raw.pop().unwrap_or_default();
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
mod tests {
    use super::*;

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
