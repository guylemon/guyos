//! GuyOS Core library — hexagonal layering described in **`docs/adr/0002-hexagonal-boundaries-and-ownership.md`**.
//!
//! ## Stable API
//!
//! Consume this crate through **named** re-exports from the crate root only:
//!
//! - [`Chat`] — UniFFI chat adapter (`adapters/inbound`); [`ChatMessage`] — domain record re-exported here
//! - [`ChatError`], [`Result`] — shared error surface (`error`)
//!
//! Internal layers (`domain`, `ports`, `application`, `adapters`, `infrastructure`)
//! are `pub(crate)` or private; their paths may change without a semver guarantee.
//! Layout and phased moves: **`docs/planning/phase-0-module-layout.md`**.

mod error;

pub(crate) mod adapters;
pub(crate) mod application;
pub(crate) mod domain;
pub(crate) mod infrastructure;
pub(crate) mod ports;

pub use crate::adapters::inbound::Chat;
pub use crate::domain::ChatMessage;
pub use crate::error::ChatError;

pub type Result<T> = std::result::Result<T, ChatError>;

uniffi::setup_scaffolding!();
