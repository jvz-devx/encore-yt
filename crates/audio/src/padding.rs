//! Opus end padding in WebM. The encoder pads the last packet up to a whole
//! frame and records how much in the last BlockGroup's `DiscardPadding`
//! (nanoseconds). symphonia 0.6.1's Matroska reader ignores that element,
//! so without this each Opus track would end with up to 20 ms of padding
//! (13.5 ms in the test files), a dropout at every gapless join. FFmpeg (and
//! so mpv) trims it.

const BLOCK_GROUP: u32 = 0xA0;
const BLOCK: u32 = 0xA1;
const DISCARD_PADDING: u32 = 0x75A2;

/// The last BlockGroup's DiscardPadding in `tail` (the file's last bytes),
/// in nanoseconds.
pub fn discard_padding(tail: &[u8]) -> Option<i64> {
    (0..tail.len())
        .rev()
        .filter(|&i| tail[i] as u32 == BLOCK_GROUP)
        .find_map(|i| block_group(&tail[i..]))
}

/// Parses a BlockGroup at the start of `bytes`; `Some` only if its children
/// parse exactly to its end, include a Block and carry DiscardPadding.
fn block_group(bytes: &[u8]) -> Option<i64> {
    let (id, id_len) = element_id(bytes)?;
    if id != BLOCK_GROUP {
        return None;
    }
    let (size, size_len) = vint(&bytes[id_len..])?;
    let start = id_len + size_len;
    let body = bytes.get(start..start + usize::try_from(size).ok()?)?;
    let (mut at, mut has_block, mut padding) = (0, false, None);
    while at < body.len() {
        let (child, child_len) = element_id(&body[at..])?;
        let (len, len_len) = vint(&body[at + child_len..])?;
        let from = at + child_len + len_len;
        let value = body.get(from..from + usize::try_from(len).ok()?)?;
        match child {
            BLOCK => has_block = true,
            DISCARD_PADDING if (1..=8).contains(&value.len()) => padding = Some(signed(value)),
            _ => {}
        }
        at = from + value.len();
    }
    // At most one Opus frame (120 ms) of padding is plausible.
    padding.filter(|p| has_block && (0..=120_000_000).contains(p))
}

/// An element ID keeps its length marker: 1 to 4 bytes.
fn element_id(bytes: &[u8]) -> Option<(u32, usize)> {
    let first = *bytes.first()?;
    let len = first.leading_zeros() as usize + 1;
    if len > 4 {
        return None;
    }
    let id = bytes
        .get(..len)?
        .iter()
        .fold(0u32, |v, b| (v << 8) | *b as u32);
    Some((id, len))
}

/// A size: 1 to 8 bytes, length marker removed.
fn vint(bytes: &[u8]) -> Option<(u64, usize)> {
    let first = *bytes.first()?;
    let len = first.leading_zeros() as usize + 1;
    if len > 8 {
        return None;
    }
    let mask = if len == 8 { 0 } else { 0xFFu8 >> len };
    let value = bytes
        .get(1..len)?
        .iter()
        .fold((first & mask) as u64, |v, b| (v << 8) | *b as u64);
    Some((value, len))
}

fn signed(bytes: &[u8]) -> i64 {
    let unsigned = bytes.iter().fold(0u64, |v, b| (v << 8) | *b as u64);
    let shift = 64 - 8 * bytes.len() as u32;
    ((unsigned << shift) as i64) >> shift
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
    fn finds_the_padding_of_the_last_block_group() {
        // A BlockGroup with a 4-byte Block and DiscardPadding 13.5 ms,
        // after some unrelated bytes that contain 0xA0.
        let padding: i64 = 13_500_000;
        let mut group = vec![0xA1, 0x84, 0x81, 0x00, 0x00, 0x80];
        group.extend([0x75, 0xA2, 0x84]);
        group.extend(&(padding as u32).to_be_bytes());
        let mut tail = vec![0x12, 0xA0, 0x99, 0x00];
        tail.extend([0xA0, 0x80 | group.len() as u8]);
        tail.extend(group);
        assert_eq!(discard_padding(&tail), Some(padding));
    }

    #[test]
    fn ignores_bytes_that_do_not_parse_as_a_block_group() {
        assert_eq!(discard_padding(&[0xA0, 0x85, 0x75, 0xA2, 0x81, 0x10]), None);
    }
}
