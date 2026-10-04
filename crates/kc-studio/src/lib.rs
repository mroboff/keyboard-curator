//! ZMK Studio transport and RPC, for live editing of bindings and layers.
//!
//! The protocol can change which behaviour each key has and manage layers;
//! everything else in a layout still needs a firmware build. This crate
//! speaks the protocol and converts between its numbers and the project's
//! bindings.

pub mod client;
pub mod codec;
pub mod framing;
pub mod proto;
pub mod serial;

pub use client::{Client, DeviceBehavior, StudioError};
pub use codec::{compare, BehaviorTable, Change, Comparison};
