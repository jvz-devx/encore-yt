//! Shared tolerant traversal of InnerTube JSON values.

use serde_json::Value;

/// Follows object keys; a key that parses as a number indexes an array.
pub fn at<'a>(v: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cur = v;
    for key in path {
        cur = match key.parse::<usize>() {
            Ok(i) if cur.is_array() => cur.get(i)?,
            _ => cur.get(*key)?,
        };
    }
    Some(cur)
}

pub(super) fn str_at<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    at(v, path)?.as_str()
}

/// Depth-first search for the first value under `key`.
pub fn find<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(map) => {
            if let Some(found) = map.get(key) {
                return Some(found);
            }
            map.values().find_map(|child| find(child, key))
        }
        Value::Array(items) => items.iter().find_map(|child| find(child, key)),
        _ => None,
    }
}

pub(super) fn array(v: Option<&Value>) -> &[Value] {
    v.and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// The text of `{runs: [...]}` or `{simpleText}`.
pub fn text(v: Option<&Value>) -> String {
    let Some(v) = v else { return String::new() };
    if let Some(s) = v.get("simpleText").and_then(Value::as_str) {
        return s.to_owned();
    }
    array(v.get("runs"))
        .iter()
        .filter_map(|r| r.get("text").and_then(Value::as_str))
        .collect()
}
