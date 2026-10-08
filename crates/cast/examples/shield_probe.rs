//! Shield-only status and volume cleanup for the maintainer-authorized pass.
//! Never launches or loads media. Refuses all non-Shield discovery results.
#![allow(clippy::print_stdout, reason = "check evidence")]

use std::time::Duration;

use anyhow::{Context, Result, ensure};
use encore_cast::Device;
use encore_cast::castv2::Client;
use encore_cast::discovery::{self, Policy};

#[tokio::main]
async fn main() -> Result<()> {
    let policy = Policy::from_env();
    ensure!(policy.shield_only, "ENCORE_CAST_SHIELD_ONLY=1 is required");
    let devices = discovery::scan(policy, Duration::from_secs(3)).await?;
    ensure!(
        devices.len() == 1,
        "need exactly one discovered Shield, found {}",
        devices.len()
    );
    let device = devices.into_iter().next().context("no Shield")?;
    policy.check(&device)?;
    let Device::Cast(device) = device else {
        anyhow::bail!("not a Cast device")
    };
    let client = Client::connect(device.addr).await?;
    let args: Vec<_> = std::env::args().skip(1).collect();
    let status = match args.as_slice() {
        [] => client.receiver_status().await?,
        [action, level, muted] if action == "restore" => {
            client
                .receiver_volume(level.parse()?, muted.parse()?)
                .await?
        }
        _ => anyhow::bail!("usage: shield_probe [restore LEVEL MUTED]"),
    };
    println!(
        "{}",
        serde_json::json!({ "name": device.name, "model": device.model, "volume": status.volume, "muted": status.muted,
        "apps": status.apps.iter().map(|a| serde_json::json!({ "id": a.app_id, "name": a.display_name, "idle": a.is_idle_screen })).collect::<Vec<_>>() })
    );
    Ok(())
}
