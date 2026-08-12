fn main() {
    #[cfg(target_os = "linux")]
    napi_build::setup();
}
