fn main() {
    // No build-time processing needed - we use WASM witness calculators at runtime
    println!("cargo:rerun-if-changed=build.rs");
}
