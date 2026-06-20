//! Configuration is owned by the shared `metis-config` crate (pure serde + fs, no
//! GTK) so the settings app can reuse it. This module re-exports everything so the
//! existing `crate::config::...` call sites across the shell keep compiling.

pub use metis_config::*;
