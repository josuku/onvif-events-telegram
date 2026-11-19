use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not defined"));

    // Workspace root (parent of this crate)
    let workspace_dir = manifest_dir
        .parent()
        .expect("Cannot determine workspace root");

    let (library_name, source) = runtime_info(workspace_dir);

    println!("cargo:rerun-if-changed={}", source.display());

    if !source.exists() {
        panic!("ONNX Runtime library not found: {}", source.display());
    }

    let target_dir = target_directory(&manifest_dir);
    fs::create_dir_all(&target_dir).expect("Failed to create target directory");

    let destination = target_dir.join(library_name);

    fs::copy(&source, &destination).expect("Failed to copy ONNX Runtime library");

    println!(
        "cargo:warning=Copied {} -> {}",
        source.display(),
        destination.display()
    );
}

fn runtime_info(workspace_dir: &Path) -> (&'static str, PathBuf) {
    let runtime_root = workspace_dir.join("ultralytics-runtime");

    if cfg!(target_os = "windows") {
        (
            "onnxruntime.dll",
            runtime_root.join("windows").join("onnxruntime.dll"),
        )
    } else if cfg!(target_os = "linux") {
        (
            "libonnxruntime.so",
            runtime_root.join("linux").join("libonnxruntime.so"),
        )
    } else if cfg!(target_os = "macos") {
        (
            "libonnxruntime.dylib",
            runtime_root.join("macos").join("libonnxruntime.dylib"),
        )
    } else {
        panic!("Unsupported operating system");
    }
}

fn target_directory(manifest_dir: &Path) -> PathBuf {
    let profile = env::var("PROFILE").expect("PROFILE not defined");

    env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest_dir.join("../target"))
        .join(profile)
}
