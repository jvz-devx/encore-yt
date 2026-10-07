//! Small differences between the desktops ytfast builds for.

/// Keeps a helper (mpv, yt-dlp) from opening a console window on Windows.
pub fn no_console(command: &mut tokio::process::Command) {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = command;
}
