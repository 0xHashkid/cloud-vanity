fn main() {
    #[cfg(feature = "gpu")]
    build_cuda_libs();
}

#[cfg(feature = "gpu")]
fn build_cuda_libs() {
    println!("cargo::rerun-if-changed=kernels/");

    cc::Build::new()
        .cuda(true)
        .file("kernels/utils.cu")
        .file("kernels/vanity.cu")
        .file("kernels/base58.cu")
        .file("kernels/sha256.cu")
        .flag("-cudart=static")
        .flag("-gencode=arch=compute_89,code=compute_89")
        .flag("-gencode=arch=compute_75,code=sm_75")  // Turing RTX
        .flag("-gencode=arch=compute_80,code=sm_80")  // Ampere (A100, RTX 30××)
        .flag("-gencode=arch=compute_86,code=sm_86")  // Ampere (RTX 30×× Ti)
        .flag("-gencode=arch=compute_89,code=sm_89")  // Ada Lovelace (RTX 40××)
        .flag("-gencode=arch=compute_90,code=sm_90")  // Hopper (H100)
        .flag("-gencode=arch=compute_75,code=compute_75")
        .flag("-gencode=arch=compute_80,code=compute_80")
        .flag("-gencode=arch=compute_86,code=compute_86")
        .flag("-gencode=arch=compute_89,code=compute_89")
        .flag("-gencode=arch=compute_90,code=compute_90")
    
        .compile("libvanity.a");

    // Add link directory
    println!("cargo:rustc-link-search=native=/usr/local/cuda/lib64");
    println!("cargo:rustc-link-lib=cudart");
    println!("cargo:rustc-link-lib=cuda");

    // Emit the location of the compiled library
    let out_dir = std::env::var("OUT_DIR").unwrap();
    println!("cargo:rustc-link-search=native={}", out_dir);
}
