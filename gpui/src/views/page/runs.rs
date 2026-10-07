//! Subtitle runs as one line of text whose linked runs (an artist, an
//! album) open their page. A link under the pointer is underlined and drawn
//! in the text colour; the rest of the line keeps its colour.

use std::ops::Range;

use gpui_kit::*;
use ytfast::model::{Run, Target};

use crate::app::MusicApp;
use crate::theme::Colors;

/// A line of `runs` in `color`, its links clickable. `id` names the text
/// for hover tracking; it must be unique on the page.
pub fn runs_line(
    id: impl Into<SharedString>,
    runs: &[Run],
    color: Hsla,
    hover: Option<&(SharedString, usize)>,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    let id: SharedString = id.into();
    let mut text = String::new();
    let mut links: Vec<(Range<usize>, Target)> = Vec::new();
    for run in runs {
        let start = text.len();
        text.push_str(&run.text);
        if let Some(target) = &run.target {
            links.push((start..text.len(), target.clone()));
        }
    }
    if links.is_empty() {
        return div().text_color(color).child(text).into_any_element();
    }
    let hovered = hover.filter(|(h, _)| *h == id).map(|(_, i)| *i);
    let highlights: Vec<(Range<usize>, HighlightStyle)> = hovered
        .and_then(|i| links.get(i))
        .map(|(range, _)| {
            vec![(
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
            )]
        })
        .unwrap_or_default();
    let ranges: Vec<Range<usize>> = links.iter().map(|(r, _)| r.clone()).collect();
    let targets: Vec<Target> = links.into_iter().map(|(_, t)| t).collect();
    let app = cx.entity().downgrade();
    let hover_app = app.clone();
    let hover_id = id.clone();
    let hover_ranges = ranges.clone();
    let leave_app = app.clone();
    let leave_id = id.clone();
    div()
        .id(SharedString::from(format!("links:{id}")))
        .text_color(color)
        // The text only hears the pointer while it's over it: leaving
        // clears its link.
        .on_hover(move |inside, _, cx| {
            if *inside {
                return;
            }
            let _ = leave_app.update(cx, |app, cx| {
                if app
                    .pages
                    .link_hover
                    .as_ref()
                    .is_some_and(|(h, _)| *h == leave_id)
                {
                    app.pages.link_hover = None;
                    cx.notify();
                }
            });
        })
        .child(
            InteractiveText::new(
                ElementId::Name(id.clone()),
                StyledText::new(text).with_highlights(highlights),
            )
            .on_click(ranges, move |i, _, cx| {
                // The row or card under the link doesn't take this click.
                cx.stop_propagation();
                if let Some(target) = targets.get(i).cloned() {
                    let _ = app.update(cx, |app, cx| app.activate(target, cx));
                }
            })
            .on_hover(move |index, _, _, cx| {
                let link = index.and_then(|ix| hover_ranges.iter().position(|r| r.contains(&ix)));
                let _ = hover_app.update(cx, |app, cx| {
                    let next = link.map(|l| (hover_id.clone(), l));
                    let same_text = app
                        .pages
                        .link_hover
                        .as_ref()
                        .is_some_and(|(h, _)| *h == hover_id);
                    // Leaving a link clears it; leaving another text's
                    // link is that text's business.
                    if (next.is_some() || same_text) && app.pages.link_hover != next {
                        app.pages.link_hover = next;
                        cx.notify();
                    }
                });
            }),
        )
        .into_any_element()
}
