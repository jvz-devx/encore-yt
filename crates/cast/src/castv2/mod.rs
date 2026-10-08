//! Google Cast v2 (the protocol of Chromecasts, Nest speakers and Android
//! TV): a sender over TLS that launches the Default Media Receiver and
//! drives its media session.

mod client;
pub mod messages;
pub mod proto;

pub use client::{Client, Event};
pub use messages::{App, DEFAULT_MEDIA_RECEIVER, Media, MediaStatus, ReceiverStatus};
