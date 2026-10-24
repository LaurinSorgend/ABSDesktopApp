//! Client for the Audiobookshelf HTTP API: auth with refreshing tokens, typed models
//! and the endpoints the desktop app uses.

mod auth;
mod client;
mod endpoints;
mod error;
pub mod jwt;
pub mod models;
mod raw;
mod realtime;
mod version;

pub use auth::TokenSink;
pub use client::{AbsClient, http_client};
pub use endpoints::{ItemFilter, ItemsQuery};
pub use error::{Error, Result, SinkError};
pub use raw::Raw;
pub use realtime::{Realtime, RealtimeEvent};
pub use reqwest;
pub use version::{Feature, ServerVersion};
