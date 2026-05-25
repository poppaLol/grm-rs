use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let proto_root = manifest_dir.join("proto");
    let files = [
        "grm/service/v1/common.proto",
        "grm/service/v1/schema.proto",
        "grm/service/v1/node.proto",
        "grm/service/v1/edge.proto",
        "grm/service/v1/query.proto",
        "grm/service/v1/batch.proto",
        "grm/service/v1/admin.proto",
        "grm/service/v1/workspace.proto",
        "grm/service/v1/service.proto",
    ]
    .map(|file| proto_root.join(file));

    for file in files.iter() {
        println!("cargo:rerun-if-changed={}", file.display());
    }

    let protoc = protoc_bin_vendored::protoc_bin_path().unwrap();
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc);
    config
        .compile_protos(&files, &[proto_root])
        .expect("GRM service protobuf contract should compile");
}
