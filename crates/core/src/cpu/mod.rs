//! CPU specific operations (x86_64).
//!
//! Provides the low-level assembly necessary to jump entirely into the
//! guest application context, saving the host state and replacing it back
//! upon guest exit.

pub mod jump;
