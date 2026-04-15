fn main() {
    // Ensure advapi32 is linked on Windows for functions used by libzmq
    println!("cargo:rustc-link-lib=advapi32");
}
