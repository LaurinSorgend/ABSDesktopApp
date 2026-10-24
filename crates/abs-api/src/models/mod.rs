//! Serde models of the server responses used by the client.
//!
//! Every struct uses `#[serde(default)]` and ignores unknown fields, so new or
//! missing server fields don't break decoding. Use [`crate::Raw`] where the full
//! JSON has to be kept.

mod browse;
mod item;
mod library;
mod progress;
mod session;
mod user;

pub use browse::*;
pub use item::*;
pub use library::*;
pub use progress::*;
pub use session::*;
pub use user::*;
