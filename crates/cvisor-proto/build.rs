fn main() {
    // protoc resolution: honor a caller-set PROTOC, else a system protoc on
    // PATH, else the vendored binary. The vendored one is glibc-linked, so on
    // musl (Alpine) it needs a system protoc (apk add protobuf) or gcompat; nix
    // and other C-free environments set PROTOC to a packaged protoc.
    if std::env::var_os("PROTOC").is_none() {
        let system = std::process::Command::new("protoc")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !system {
            std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path().unwrap());
        }
    }
    // Emit the file descriptor set so the daemon can serve gRPC reflection.
    let descriptor =
        std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("cvisor_descriptor.bin");
    tonic_build::configure()
        .build_client(true)
        .build_server(true)
        .file_descriptor_set_path(&descriptor)
        .compile_protos(&["proto/cvisor.proto"], &["proto"])
        .expect("compiling cvisor.proto");
    println!("cargo:rerun-if-changed=proto/cvisor.proto");
}
