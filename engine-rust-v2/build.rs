fn main() {
    // Link Windows advapi32 library for security descriptor functions
    println!("cargo:rustc-link-lib=advapi32");
}
