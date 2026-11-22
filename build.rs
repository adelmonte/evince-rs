fn main() {
    let mut libs = vec!["poppler-glib", "libtiff-4", "libarchive", "synctex"];
    for (feature, lib) in [("DJVU", "ddjvuapi"), ("PS", "libspectre"), ("XPS", "libgxps")] {
        if std::env::var_os(format!("CARGO_FEATURE_{feature}")).is_some() {
            libs.push(lib);
        }
    }
    for lib in libs {
        if let Err(e) = pkg_config::Config::new().probe(lib) {
            panic!("required system library {lib} not found: {e}");
        }
    }
}
