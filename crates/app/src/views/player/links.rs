//! A song's artists and album as one line of links to their pages, used by
//! the player bar and Now Playing. One text run, so the line ellipsises as
//! a whole.

use std::ops::Range;

use encore_core::model::{Target, Track};
use gpui_kit::*;

use crate::app::MusicApp;
use crate::theme::Colors;

/// "Artist & Artist • Album": runs with a page are links (the full text
/// colour and an underline under the pointer), the rest plain. The caller
/// sets the type and colour. `id` keeps the links of two places apart.
pub fn track_links(
    id: &'static str,
    track: &Track,
    app: &MusicApp,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> Stateful<Div> {
    let mut text = String::new();
    let mut links: Vec<(Range<usize>, Target)> = Vec::new();
    for run in &track.artists {
        push(&mut text, &mut links, &run.text, run.target.clone());
    }
    if let Some(album) = &track.album {
        if !text.is_empty() {
            text.push_str(" • ");
        }
        push(&mut text, &mut links, &album.text, album.target.clone());
    }
    let hovered = app
        .player
        .link_hover
        .as_ref()
        .filter(|(place, _)| *place == id)
        .map(|(_, i)| *i);
    let highlights = hovered
        .and_then(|i| links.get(i))
        .map(|(range, _)| {
            (
                range.clone(),
                HighlightStyle {
                    color: Some(c.text),
                    underline: Some(UnderlineStyle {
                        thickness: px(1.),
                        color: Some(c.text),
                        wavy: false,
                    }),
                    ..Default::default()
                },
            )
        })
        .into_iter()
        .collect::<Vec<_>>();
    let ranges: Vec<Range<usize>> = links.iter().map(|(r, _)| r.clone()).collect();
    let targets: Vec<Target> = links.into_iter().map(|(_, t)| t).collect();
    let entity = cx.entity().downgrade();
    let hover_ranges = ranges.clone();
    let open = cx.entity().downgrade();
    let text = InteractiveText::new(
        SharedString::from(format!("{id}-links")),
        StyledText::new(text).with_highlights(highlights),
    )
    .on_hover(move |index, _, _, cx| {
        let link = index.and_then(|ix| hover_ranges.iter().position(|r| r.contains(&ix)));
        let _ = entity.update(cx, |this, cx| {
            let now = link.map(|i| (id, i));
            if this.player.link_hover != now {
                this.player.link_hover = now;
                cx.notify();
            }
        });
    })
    .on_click(ranges, move |i, _, cx| {
        cx.stop_propagation();
        let Some(target) = targets.get(i).cloned() else {
            return;
        };
        let _ = open.update(cx, |this, cx| this.open_link(target, cx));
    });
    // The text reports hover only while the pointer is over it; leaving the
    // line clears it.
    let left = cx.entity().downgrade();
    div()
        .id(SharedString::from(format!("{id}-line")))
        .min_w_0()
        .truncate()
        .child(text)
        .on_hover(move |hovered, _, cx| {
            if !*hovered {
                let _ = left.update(cx, |this, cx| {
                    if this.player.link_hover.is_some_and(|(place, _)| place == id) {
                        this.player.link_hover = None;
                        cx.notify();
                    }
                });
            }
        })
}

fn push(
    text: &mut String,
    links: &mut Vec<(Range<usize>, Target)>,
    run: &str,
    target: Option<Target>,
) {
    let start = text.len();
    text.push_str(run);
    if let Some(target) = target {
        links.push((start..text.len(), target));
    }
}
