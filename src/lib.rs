//! `jlcpcb-mcp`: a Model Context Protocol server for finding and evaluating
//! JLCPCB / LCSC electronic components for PCB assembly.
//!
//! The crate is split so that everything except [`server`] is plain async
//! Rust and can be tested without an MCP client.

pub mod error;
pub mod http;
pub mod jlc;
pub mod lcsc;
pub mod model;
pub mod official;
pub mod pcb;
pub mod server;
pub mod service;
pub mod values;

pub use error::ApiError;
pub use server::JlcpcbServer;
pub use service::{Config, Service};
