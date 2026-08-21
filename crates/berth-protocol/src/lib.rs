//! Computer-session protocol types.
//!
//! JSON vocabulary from `spec/computer-session.md`. This crate is types,
//! validation, quoting, and coordinate mapping only — no HTTP, Docker, or CLI.

mod action;
mod coord;
mod lease;
mod quote;

pub use action::{
    Ack, AckKind, AckResult, Action, ActionBatch, ActionBatchKind, Button, Frame, FrameKind,
};
pub use coord::{Point, Region, scale_coordinates, scale_region};
pub use lease::{
    Class, DEFAULT_ALLOWLIST, Density, Egress, Isolation, Lease, LeaseRequest, License, MvpError,
    Network, ObjectStore, Os, Resources, Term, Workspace, default_min_seconds,
    network_from_allowlist_key, parse_allowlist, validate_mvp,
};
pub use quote::{
    DENSITY_MULT_ISOLATED, DENSITY_MULT_SHARED, OS_MULT_LINUX, P_CPU, P_DISK, P_MEM, Quote,
    USD_PER_GAS, density_mult, os_mult,
};
