//! Account edit requests, acknowledgements and frontend actions.

use crate::model::{LikeStatus, Track};

/// A write the backend makes to the account.
#[derive(Clone, Debug)]
pub enum Edit {
    Rate {
        video_id: String,
        status: LikeStatus,
    },
    /// Save an album (its audio playlist) or a playlist to the library, or remove it.
    Save {
        playlist_id: String,
        save: bool,
    },
    Subscribe {
        channel_id: String,
        subscribe: bool,
    },
    Create {
        title: String,
        description: String,
        video_ids: Vec<String>,
    },
    Add {
        playlist_id: String,
        video_ids: Vec<String>,
    },
    Remove {
        playlist_id: String,
        video_id: String,
        set_video_id: String,
    },
    /// Move an entry before `before` (the end when `None`).
    Move {
        playlist_id: String,
        set_video_id: String,
        before: Option<String>,
    },
    Details {
        playlist_id: String,
        title: Option<String>,
        description: Option<String>,
    },
    Delete {
        playlist_id: String,
    },
}

/// What a successful edit returned.
#[derive(Clone, Debug)]
pub enum Done {
    Ok,
    /// A new playlist's id.
    Created(String),
    /// Songs added to a playlist: (video id, entry id).
    Added(Vec<(String, String)>),
}

/// Why an edit didn't happen.
#[derive(Clone, Debug)]
pub enum Failure {
    /// YouTube Music answered and said no (detail for Copy).
    Refused(String),
    AlreadyInPlaylist,
    Offline,
    SignedOut,
}

/// What the interface asks for.
#[derive(Clone, Debug)]
pub enum AccountAction {
    /// Give `track` this rating (`Indifferent` removes a like or dislike).
    Rate {
        track: Track,
        status: LikeStatus,
    },
    Save {
        playlist_id: String,
        title: String,
        save: bool,
    },
    Subscribe {
        channel_id: String,
        name: String,
        subscribe: bool,
    },
    Create {
        title: String,
        description: String,
        tracks: Vec<Track>,
    },
    Add {
        playlist_id: String,
        tracks: Vec<Track>,
    },
    Remove {
        playlist_id: String,
        set_video_id: String,
    },
    /// Move the entry `set_video_id` to where the entry `onto` is.
    Move {
        playlist_id: String,
        set_video_id: String,
        onto: String,
    },
    Details {
        playlist_id: String,
        title: String,
        description: String,
    },
    Delete {
        playlist_id: String,
    },
    Dialog(Option<Dialog>),
}

/// A dialog over the window.
#[derive(Clone, Debug)]
pub enum Dialog {
    /// Name and describe a new playlist, holding `tracks` (may be empty).
    NewPlaylist {
        title: String,
        description: String,
        tracks: Vec<Track>,
    },
    EditPlaylist {
        playlist_id: String,
        title: String,
        description: String,
    },
    DeletePlaylist {
        playlist_id: String,
        title: String,
    },
    /// Choose one of the account's playlists for `tracks`.
    AddToPlaylist {
        tracks: Vec<Track>,
        filter: String,
        selected: usize,
    },
}
