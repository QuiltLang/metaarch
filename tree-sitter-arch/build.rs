fn main() {
    let src_dir = std::path::Path::new("src");
    let mut c_config = cc::Build::new();
    c_config
        .std("c11")
        .include(src_dir)
        .file(src_dir.join("parser.c"))
        .compile("tree-sitter-arch");
    println!("cargo:rerun-if-changed=src/parser.c");
}
