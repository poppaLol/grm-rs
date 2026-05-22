//! Split-ready service API contract artifacts for GRM.
//!
//! This crate intentionally contains the protobuf source contract rather than a
//! daemon, transport policy, or generated client. It is client-facing and can be
//! split from the monorepo later without depending on private daemon internals.

use std::path::{Path, PathBuf};

pub const PROTO_PACKAGE: &str = "grm.service.v1";
pub const PROTO_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/proto");

pub const PROTO_FILES: &[&str] = &[
    "grm/service/v1/common.proto",
    "grm/service/v1/schema.proto",
    "grm/service/v1/node.proto",
    "grm/service/v1/edge.proto",
    "grm/service/v1/query.proto",
    "grm/service/v1/batch.proto",
    "grm/service/v1/admin.proto",
    "grm/service/v1/service.proto",
];

pub fn proto_root() -> &'static Path {
    Path::new(PROTO_ROOT)
}

pub fn proto_files() -> impl Iterator<Item = PathBuf> {
    PROTO_FILES.iter().map(|file| proto_root().join(file))
}
