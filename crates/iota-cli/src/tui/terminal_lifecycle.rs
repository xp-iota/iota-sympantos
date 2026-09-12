use crossterm::terminal::disable_raw_mode;

pub(super) fn restore_terminal_state() {
    let _ = disable_raw_mode();
}

/// Restores terminal state on drop so that panics and early returns always
/// leave the terminal in a usable state.
pub(super) struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal_state();
    }
}

pub(super) fn install_terminal_panic_hook() {
    // Install a panic hook that restores the terminal before printing the
    // panic message, so the user's shell is not left in raw mode.
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal_state();
        original_hook(info);
    }));
}
