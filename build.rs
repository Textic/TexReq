fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // Windows defaults to a 1 MB main-thread stack, which debug builds of
        // GPUI / gpui-kit exceed during theme and component layout/rendering.
        println!("cargo:rustc-link-arg-bins=/STACK:8388608");
    }
}
