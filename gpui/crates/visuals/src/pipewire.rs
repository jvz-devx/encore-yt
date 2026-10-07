//! A PipeWire tap on mpv's output, through the PipeWire command line tools
//! (no libpipewire headers needed at build time).
//!
//! mpv's playback stream is the node named "ytfast" (`--audio-client-name`);
//! with Smooth mixes or Audition there are several. `pw-record` opens an
//! unlinked capture node ("ytfast-visuals-<our pid>", `--target 0`) and writes raw f32
//! stereo at 48 kHz to its stdout; a linker thread connects every "ytfast"
//! node's output ports to it with `pw-link`, so decks mix into one signal.
//! The Rust engine (ytfast-audio) plays all decks through one stream of this
//! process, which pipewire-pulse names after cpal's PulseAudio client
//! ("cpal-pulseaudio-<our pid>"); it is tapped the same way.
//!
//! The capture node's media class is `Stream/Input/Audio/Analyzer`, not
//! `Stream/Input/Audio`: WirePlumber leaves it alone (no fallback to the
//! microphone) and pipewire-pulse doesn't list it as a recording, so KDE
//! shows no "microphone in use" indicator (checked: m8-tray-both.png).
//!
//! While mpv is paused nothing flows and the reader sleeps on the pipe. The
//! app keeps a tap only while Now Playing shows and a song plays.

use std::collections::HashSet;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use anyhow::{Context as _, Result};
use serde_json::Value;

/// Starts `pw-record` on a new, unlinked analyzer node; samples arrive on
/// the returned stdout until the child is killed.
pub fn record() -> Result<(Child, ChildStdout)> {
    let mut child = Command::new("pw-record")
        .args([
            "--raw",
            "--format",
            "f32",
            "--rate",
            "48000",
            "--channels",
            "2",
        ])
        .args(["--latency", "10ms", "--target", "0"])
        .arg("-P")
        .arg(format!(
            "{{ node.name = {} media.class = Stream/Input/Audio/Analyzer \
             node.dont-reconnect = true }}",
            node_name()
        ))
        .arg("-")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("starting pw-record")?;
    let stdout = child.stdout.take().context("pw-record stdout")?;
    Ok((child, stdout))
}

/// Links new mpv nodes to our capture node until `stop`: every second
/// until one is linked, then every three (a new mpv process, or a second
/// deck for Smooth mixes, still gets linked).
pub fn link_loop(stop: &AtomicBool, linked: &AtomicUsize) {
    let mut done: HashSet<u64> = HashSet::new();
    while !stop.load(Ordering::Relaxed) {
        if let Some(graph) = Graph::read()
            && let Some(ours) = graph.our_node()
        {
            for mpv in graph.mpv_nodes() {
                if !done.contains(&mpv) && graph.link(mpv, ours) {
                    log::info!("visuals: linked mpv's PipeWire node {mpv}");
                    done.insert(mpv);
                }
            }
            done.retain(|id| graph.has_node(*id));
            linked.store(done.len(), Ordering::Relaxed);
        }
        let wait = if done.is_empty() { 10 } else { 30 };
        for _ in 0..wait {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Our capture node's name, unique per app process.
fn node_name() -> String {
    format!("ytfast-visuals-{}", std::process::id())
}

/// One `pw-dump` of the graph.
struct Graph(Vec<Value>);

impl Graph {
    fn read() -> Option<Self> {
        let output = Command::new("pw-dump")
            .stderr(Stdio::null())
            .output()
            .ok()?;
        serde_json::from_slice(&output.stdout).ok().map(Self)
    }

    fn nodes(&self) -> impl Iterator<Item = (u64, &Value)> {
        self.0
            .iter()
            .filter(|o| o["type"] == "PipeWire:Interface:Node")
            .filter_map(|o| Some((o["id"].as_u64()?, &o["info"]["props"])))
    }

    fn has_node(&self, id: u64) -> bool {
        self.nodes().any(|(n, _)| n == id)
    }

    /// mpv's playback streams and the Rust engine's.
    fn mpv_nodes(&self) -> Vec<u64> {
        let engine = format!("cpal-pulseaudio-{}", std::process::id());
        self.nodes()
            .filter(|(_, p)| {
                (p["node.name"] == "ytfast" || p["node.name"] == engine.as_str())
                    && p["media.class"] == "Stream/Output/Audio"
            })
            .map(|(id, _)| id)
            .collect()
    }

    fn our_node(&self) -> Option<u64> {
        let name = node_name();
        self.nodes()
            .find(|(_, p)| p["node.name"] == name.as_str())
            .map(|(id, _)| id)
    }

    /// Port ids of a node in one direction, with their channel.
    fn ports(&self, node: u64, direction: &str) -> Vec<(u64, String)> {
        self.0
            .iter()
            .filter(|o| o["type"] == "PipeWire:Interface:Port")
            .filter(|o| o["info"]["direction"] == direction)
            .filter(|o| o["info"]["props"]["node.id"].as_u64() == Some(node))
            .filter_map(|o| {
                let channel = o["info"]["props"]["audio.channel"].as_str()?;
                Some((o["id"].as_u64()?, channel.to_owned()))
            })
            .collect()
    }

    /// Links FL to FL and FR to FR (a mono output to both).
    fn link(&self, from: u64, to: u64) -> bool {
        let outputs = self.ports(from, "output");
        let inputs = self.ports(to, "input");
        let mut any = false;
        for (input, channel) in &inputs {
            let source = outputs
                .iter()
                .find(|(_, c)| c == channel || c == "MONO")
                .map(|(id, _)| id);
            if let Some(output) = source {
                any |= Command::new("pw-link")
                    .args([output.to_string(), input.to_string()])
                    .stderr(Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success());
            }
        }
        any
    }
}
