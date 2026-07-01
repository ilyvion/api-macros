//! Registers the `APIM_*` environment variables read by this proc-macro so that Cargo
//! rebuilds this crate (and, transitively, every consumer that expands the macro) when
//! any of them change. Without this, `std::env::var` reads inside the macro expansion
//! are invisible to Cargo's fingerprinting and stale codegen can silently persist.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=apim_env_var_list.txt");

    for var in include_str!("apim_env_var_list.txt").lines() {
        println!("cargo:rerun-if-env-changed={var}");
    }
}
