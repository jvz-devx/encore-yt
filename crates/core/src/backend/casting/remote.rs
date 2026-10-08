//! The remote task owns the receiver and serializes network commands.
//! Stamped events cannot update a replacement session or a newer song.

use std::time::Duration;

use encore_cast::castv2::Media;
use encore_cast::discovery::Policy;
use encore_cast::relay::Source;
use encore_cast::session::{Connection, Session, Status};
use tokio::sync::mpsc;

use crate::backend::Internal;

pub(in crate::backend) enum Request {
    Load {
        generation: u64,
        source: Source,
        metadata: Box<Media>,
        position: f64,
        playing: bool,
        volume: f64,
    },
    Pause(bool),
    Seek(f64),
    Volume(f64),
    Stop,
}

pub(in crate::backend) enum Message {
    Ready {
        aac: bool,
    },
    Busy(String),
    Status {
        generation: u64,
        status: Status,
    },
    Ended {
        generation: u64,
        status: Status,
        error: Option<String>,
        loaded: bool,
    },
}

pub(in crate::backend) struct Remote {
    commands: mpsc::UnboundedSender<Request>,
    task: tokio::task::JoinHandle<()>,
}

impl Remote {
    #[cfg(test)]
    pub fn test_link() -> (Self, mpsc::UnboundedReceiver<Request>) {
        let (commands, rx) = mpsc::unbounded_channel();
        (
            Self {
                commands,
                task: tokio::spawn(std::future::pending()),
            },
            rx,
        )
    }
    pub fn start(
        device: encore_cast::Device,
        policy: Policy,
        takeover: bool,
        stamp: u64,
        tx: mpsc::UnboundedSender<Internal>,
    ) -> Self {
        let (commands, rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(run(device, policy, takeover, stamp, tx, rx));
        Self { commands, task }
    }

    pub fn send(&self, request: Request) {
        let _ = self.commands.send(request);
    }

    pub async fn shutdown(mut self) {
        self.send(Request::Stop);
        // Shutdown must not wait for a disappeared receiver's full timeout.
        let _ = tokio::time::timeout(Duration::from_secs(1), &mut self.task).await;
    }
}

impl Drop for Remote {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn run(
    device: encore_cast::Device,
    policy: Policy,
    takeover: bool,
    stamp: u64,
    tx: mpsc::UnboundedSender<Internal>,
    mut commands: mpsc::UnboundedReceiver<Request>,
) {
    let send = |message| {
        let _ = tx.send(Internal::Cast { stamp, message });
    };
    let mut session = match Session::connect(&device, policy, takeover).await {
        Ok(Connection::Ready(session)) => session,
        Ok(Connection::Busy(app)) => {
            send(Message::Busy(app));
            return;
        }
        Err(error) => {
            log::warn!("connecting cast session: {error:#}");
            send(Message::Ended {
                generation: 0,
                status: Status::default(),
                error: Some("Couldn't connect to the device. Try again.".into()),
                loaded: false,
            });
            return;
        }
    };
    send(Message::Ready {
        aac: !session.supports("audio/webm") && session.supports("audio/mp4"),
    });
    let mut status = Status::default();
    let mut loaded = false;
    let mut generation = 0;
    let mut finished = false;
    let mut timer = tokio::time::interval(Duration::from_millis(500));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let result = tokio::select! {
            request = commands.recv() => match request {
                Some(Request::Load { generation: next, source, metadata, position, playing, volume }) => {
                    generation = next;
                    finished = false;
                    // Preserve the requested handoff even if LOAD fails.
                    status.position = position;
                    status.playing = playing;
                    match session.load(source, &metadata, position, playing, volume).await {
                        Ok(next) => { loaded = true; status = next; send(Message::Status { generation, status: status.clone() }); Ok(()) }
                        Err(error) => { let _ = session.stop().await; Err(error) }
                    }
                }
                Some(Request::Pause(paused)) if loaded => {
                    let result = session.pause(paused).await;
                    if result.is_ok() { status.playing = !paused; send(Message::Status { generation, status: status.clone() }); }
                    result
                }
                Some(Request::Seek(position)) if loaded => {
                    let result = session.seek(position).await;
                    if result.is_ok() { status.position = position; send(Message::Status { generation, status: status.clone() }); }
                    result
                }
                Some(Request::Volume(volume)) if loaded => session.volume(volume).await,
                Some(Request::Stop) | None => {
                    // Refresh position before STOP, whose status may reset it.
                    if loaded && let Ok(next) = session.poll().await { status = next; }
                    let error = session.stop().await.err().map(|error| {
                        log::warn!("stopping cast session: {error:#}");
                        "The device didn't confirm that casting stopped.".into()
                    });
                    send(Message::Ended { generation, status, error, loaded });
                    return;
                }
                _ => Ok(()),
            },
            _ = timer.tick(), if loaded && !finished => {
                match session.poll().await {
                    Ok(next) => { status = next; finished = status.finished; send(Message::Status { generation, status: status.clone() }); Ok(()) }
                    Err(error) => Err(error),
                }
            }
        };
        if let Err(error) = result {
            log::warn!("cast session ended: {error:#}");
            // A refused control can leave buffered audio running. Stop only
            // our own session before handing playback back locally.
            let _ = session.stop().await;
            send(Message::Ended {
                generation,
                status,
                error: Some("Casting stopped. Try connecting again.".into()),
                loaded,
            });
            return;
        }
    }
}
