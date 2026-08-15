fn main() {
    // Use the vendored protoc so builds don't require a system protobuf install.
    std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path().unwrap());
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
