use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=VLC_TTY_BUNDLE");
    println!("cargo:rerun-if-env-changed=VLC_TTY_BUNDLE_SHA256");
    println!("cargo:rerun-if-env-changed=VLC_TTY_BUNDLE_INFO");
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("bundle.rs");
    let source = match env::var("VLC_TTY_BUNDLE") {
        Ok(path) => {
            let path = fs::canonicalize(path).expect("Bundle payload must exist");
            println!("cargo:rerun-if-changed={}", path.display());
            let sha = env::var("VLC_TTY_BUNDLE_SHA256").expect("Bundle hash required");
            assert!(sha.len() == 64 && sha.bytes().all(|c| c.is_ascii_hexdigit()));
            let info = env::var("VLC_TTY_BUNDLE_INFO").expect("Bundle identity required");
            format!("const PAYLOAD: &[u8] = include_bytes!({:?});\nconst PAYLOAD_SHA: &str = {sha:?};\nconst BUNDLE_INFO: &str = {info:?};\n", path.to_string_lossy())
        }
        Err(_) => "const PAYLOAD: &[u8] = &[];\nconst PAYLOAD_SHA: &str = \"\";\nconst BUNDLE_INFO: &str = \"Development launcher without an embedded runtime\";\n".into(),
    };
    fs::write(output, source).unwrap();
}
