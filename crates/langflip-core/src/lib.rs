//! Platform-independent core domain logic and OS adapter traits for LangFlip.
//!
//! Zero OS-specific dependencies: all logic in this crate compiles and tests
//! natively on any development host.

pub mod buffer;
pub mod config;
pub mod engine;
pub mod hotkey;
pub mod key;
pub mod layout;
pub mod planner;
pub mod ports;
