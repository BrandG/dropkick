//! Dropkick core.
//!
//! This crate knows nothing about Wayland, COSMIC, Win32, or networking. It
//! holds the utilities and their state; the platform crates provide adapters.
//!
//! The rule is enforced mechanically, not by good intentions:
//!   * `forbid(unsafe_code)` below,
//!   * zero dependencies,
//!   * a `deny.toml` at the workspace root that fails the build if any crate
//!     capable of opening a socket enters the dependency tree.
//!
//! It is also why the interaction grammar is testable without a compositor: the
//! tools take [`key::KeyPress`], not keysyms.

#![forbid(unsafe_code)]

pub mod app;
pub mod calculator;
pub mod key;
pub mod scratchpad;
pub mod text;

pub use app::{App, Response, ToolId, TOOLS};
pub use key::{Key, KeyPress, Mods};

/// Version reported by `Help -> version`. Dropkick never checks this against a
/// remote: there is nothing to check it against.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_populated() {
        assert!(!super::VERSION.is_empty());
    }
}
