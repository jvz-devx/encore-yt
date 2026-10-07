//! What the window talks to: the backend, or in the UI tests
//! (`crate::ui_tests`) a pair of channels standing in for it, so a test
//! hands the app events and reads the commands it sent without a network,
//! audio or D-Bus.

use encore_core::backend::{Backend, Command, Event};
use encore_core::desktop::Now;

pub enum Link {
    Live(Backend),
    #[cfg(test)]
    Fake {
        commands: tokio::sync::mpsc::UnboundedSender<Command>,
        events: std::sync::mpsc::Receiver<Event>,
        now: tokio::sync::watch::Receiver<Now>,
    },
}

impl Link {
    pub fn send(&self, command: Command) {
        match self {
            Link::Live(backend) => backend.send(command),
            #[cfg(test)]
            Link::Fake { commands, .. } => {
                let _ = commands.send(command);
            }
        }
    }

    /// The next event that came in, if any.
    pub fn next_event(&self) -> Option<Event> {
        match self {
            Link::Live(backend) => backend.events.try_recv().ok(),
            #[cfg(test)]
            Link::Fake { events, .. } => events.try_recv().ok(),
        }
    }

    /// A sender for commands from other threads (MPRIS, the command line).
    pub fn commands(&self) -> tokio::sync::mpsc::UnboundedSender<Command> {
        match self {
            Link::Live(backend) => backend.commands(),
            #[cfg(test)]
            Link::Fake { commands, .. } => commands.clone(),
        }
    }

    /// The queue and playback state as last sent.
    pub fn now(&self) -> tokio::sync::watch::Receiver<Now> {
        match self {
            Link::Live(backend) => backend.now.clone(),
            #[cfg(test)]
            Link::Fake { now, .. } => now.clone(),
        }
    }

    /// The real backend, for the desktop services that run on its runtime.
    pub fn live(&self) -> Option<&Backend> {
        match self {
            Link::Live(backend) => Some(backend),
            #[cfg(test)]
            Link::Fake { .. } => None,
        }
    }

    /// Saves the session and stops playback (see `Backend::shutdown`).
    pub fn shutdown(&self) {
        if let Some(backend) = self.live() {
            backend.shutdown();
        }
    }
}
