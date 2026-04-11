use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let static_dir = manifest_dir.join("static");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let out_path = out_dir.join("asset_fingerprint.rs");

    println!("cargo:rerun-if-changed=static");

    let mut hasher = Sha256::new();
    if static_dir.is_dir() {
        let mut paths = Vec::new();
        collect_files(&static_dir, &mut paths);
        paths.sort();
        for path in paths {
            let rel = path
                .strip_prefix(&static_dir)
                .expect("path under static_dir");
            hasher.update(rel.to_string_lossy().as_bytes());
            hasher.update([0]);
            let bytes = fs::read(&path).unwrap_or_else(|e| {
                panic!("failed to read {}: {e}", path.display());
            });
            hasher.update(&bytes);
        }
    } else {
        hasher.update(b"no-static-dir");
    }

    let digest = hasher.finalize();
    let fingerprint = hex::encode(&digest[..8]);

    let code = format!(
        "/// Compile-time fingerprint of all files under `static/` (for cache-busting URLs).\npub const ASSET_FINGERPRINT: &str = \"{fingerprint}\";\n"
    );
    fs::write(&out_path, code).expect("write asset_fingerprint.rs");
}

fn collect_files(dir: &Path, acc: &mut Vec<PathBuf>) {
    let read_dir = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for ent in read_dir.flatten() {
        let p = ent.path();
        if p.is_dir() {
            collect_files(&p, acc);
        } else if p.is_file() {
            acc.push(p);
        }
    }
}
