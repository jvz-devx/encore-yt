//! Whether the OS says the connection is metered (M33), so background
//! extras such as hover prefetch can stay off it.
//!
//! Linux asks NetworkManager over the system bus (`Metered`). Windows has
//! `NetworkCostType` and macOS has no simple public API; neither is read
//! here, so they report "not metered". The answer is cheap but blocking:
//! ask from a background thread.

/// NetworkManager's `NMMetered` value: 1 is "yes", 3 "guess yes" (2 "no",
/// 4 "guess no", 0 unknown).
pub fn from_network_manager(value: u32) -> bool {
    matches!(value, 1 | 3)
}

/// Whether the connection is metered now; false when the OS can't say.
/// Blocks on the system bus for a moment.
pub fn is_metered() -> bool {
    #[cfg(target_os = "linux")]
    {
        network_manager().is_some_and(from_network_manager)
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

#[cfg(target_os = "linux")]
fn network_manager() -> Option<u32> {
    use zbus::blocking::{Connection, Proxy};
    let bus = Connection::system().ok()?;
    let proxy = Proxy::new(
        &bus,
        "org.freedesktop.NetworkManager",
        "/org/freedesktop/NetworkManager",
        "org.freedesktop.NetworkManager",
    )
    .ok()?;
    proxy.get_property::<u32>("Metered").ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_yes_and_guess_yes_are_metered() {
        assert!(from_network_manager(1));
        assert!(from_network_manager(3));
        for other in [0, 2, 4, 99] {
            assert!(!from_network_manager(other), "{other}");
        }
    }
}
