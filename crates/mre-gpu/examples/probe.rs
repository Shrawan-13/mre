//! Prints the adapter wgpu would use, without opening a window.
//!
//! ```text
//! cargo run --release -p mre-gpu --example probe
//! ```
//!
//! This is the honest answer to "are we using the GPU?": a wgpu backend is
//! only hardware-accelerated when the adapter type says so. Headless CI often
//! resolves to lavapipe or SwiftShader, which is a CPU adapter behind a GPU
//! API, and must be reported as such.

fn main() {
    match mre_gpu::probe() {
        Ok(info) => {
            println!("GPU adapter: {info}");
            println!("hardware_accelerated: {}", !info.software);
        }
        Err(error) => {
            println!("GPU adapter: none ({error})");
            println!("hardware_accelerated: false");
        }
    }
}
