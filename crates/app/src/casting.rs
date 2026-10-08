//! Device picker state and commands. The backend owns discovery and playback.

use encore_core::backend::Command;
use encore_core::casting::{Device, Session, State};
use gpui_kit::*;

use crate::app::MusicApp;

#[derive(Default)]
pub struct CastUi {
    pub state: State,
    pub open: bool,
    pending: Option<Device>,
}

impl MusicApp {
    pub fn show_cast(&mut self, open: bool, cx: &mut Context<Self>) {
        self.cast.open = open;
        self.send(Command::CastScan(open));
        cx.notify();
    }

    pub fn choose_cast(&mut self, device: Device, takeover: bool, cx: &mut Context<Self>) {
        if self.player.current().is_none() {
            return;
        }
        if self.cast.state.session.is_some() && !takeover {
            return;
        }
        self.cast.pending = Some(device.clone());
        self.cast.state.error = None;
        self.cast.state.session = Some(Session::Connecting(device.clone()));
        log::info!("cast: chose {} ({:?})", device.name, device.kind);
        self.send(Command::CastConnect {
            id: device.id,
            kind: device.kind,
            takeover,
        });
        cx.notify();
    }

    pub fn cast_changed(&mut self, mut state: State, cx: &mut Context<Self>) {
        if let Some(pending) = &self.cast.pending {
            if state.session.is_some() || state.error.is_some() {
                self.cast.pending = None;
            } else {
                // Discovery reports queued before Connect cannot erase the
                // immediate Connecting feedback while the request is pending.
                state.session = Some(Session::Connecting(pending.clone()));
            }
        }
        if matches!(state.session, Some(Session::Confirm { .. })) && !self.cast.open {
            self.cast.open = true;
            self.send(Command::CastScan(true));
        }
        if let Some(error) = &state.error {
            self.error = Some(error.clone());
        }
        self.cast.state = state;
        cx.notify();
    }
}
