use base64::Engine;
use minisign_verify::{PublicKey, Signature};
use serde_json::Value;
use std::path::PathBuf;

#[test]
#[ignore = "requires a freshly built release installer"]
fn built_release_signature_matches_embedded_public_key() {
    let installer = required_path("QUOTADOCK_RELEASE_INSTALLER");
    let signature_path = required_path("QUOTADOCK_RELEASE_SIGNATURE");
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let config: Value = serde_json::from_str(
        &std::fs::read_to_string(manifest_dir.join("tauri.conf.json"))
            .expect("read tauri.conf.json"),
    )
    .expect("parse tauri.conf.json");
    let encoded_public_key = config["plugins"]["updater"]["pubkey"]
        .as_str()
        .expect("plugins.updater.pubkey");
    let public_key_text = decode_base64_text(encoded_public_key);
    let signature_text = decode_base64_text(
        std::fs::read_to_string(signature_path)
            .expect("read signature")
            .trim(),
    );
    let public_key = PublicKey::decode(&public_key_text).expect("decode public key");
    let signature = Signature::decode(&signature_text).expect("decode signature");
    let installer_bytes = std::fs::read(installer).expect("read installer");

    public_key
        .verify(&installer_bytes, &signature, true)
        .expect("verify release signature");
}

fn required_path(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .unwrap_or_else(|| panic!("{name} must point to an existing file"))
}

fn decode_base64_text(value: &str) -> String {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value)
        .expect("decode base64");
    String::from_utf8(bytes).expect("decoded value is UTF-8")
}
