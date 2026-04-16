fn main() {
    // Link advapi32 on Windows to satisfy libzmq_sys dependencies
    println!("cargo:rustc-link-lib=advapi32");
}
