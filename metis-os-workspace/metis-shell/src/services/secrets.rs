//! Credential storage now lives in the shared `metis-secrets` crate so the
//! settings app can read/write the same keyring items. Re-exported here so the
//! existing `crate::services::secrets::...` call sites keep compiling.

pub use metis_secrets::*;
