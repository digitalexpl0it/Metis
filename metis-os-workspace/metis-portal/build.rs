fn main() {
    println!("cargo:rustc-check-cfg=cfg(has_libeis)");
    if pkg_config::Config::new()
        .atleast_version("1.0")
        .probe("libeis-1.0")
        .is_ok()
    {
        println!("cargo:rustc-cfg=has_libeis");
    }
}
