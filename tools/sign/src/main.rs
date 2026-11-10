//! `horizon-sign keygen`: make the signing key (once) in %USERPROFILE%\.horizon-mods\minisign.key
//! and print the public key, as the registry entry's `key` and the repo's minisign.pub.
//! `horizon-sign <file>...`: write `<file>.minisig` next to each file (prehashed Ed25519, as
//! minisign does by default), and check it as the launcher does.
//! `horizon-sign pubkey`: print the public key again.
mod blake2b;

use base64::{Engine, engine::general_purpose::STANDARD as B64};
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{Ed25519KeyPair, KeyPair};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const HEADER: &str = "horizon-sign secret key v1 (keep private; not a minisign file)";

fn key_path() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .expect("no home folder");
    PathBuf::from(home).join(".horizon-mods").join("minisign.key")
}

struct Key {
    id: [u8; 8],
    pair: Ed25519KeyPair,
}

fn load() -> Key {
    let text = std::fs::read_to_string(key_path())
        .unwrap_or_else(|_| panic!("no key at {}: run `horizon-sign keygen` first", key_path().display()));
    let line = text.lines().nth(1).expect("key file has no key line");
    let raw = B64.decode(line.trim()).expect("key line is not base64");
    let mut id = [0u8; 8];
    id.copy_from_slice(&raw[..8]);
    let pair = Ed25519KeyPair::from_pkcs8(&raw[8..]).expect("key is not an Ed25519 PKCS#8 key");
    Key { id, pair }
}

fn public_text(key: &Key) -> String {
    let mut raw = b"Ed".to_vec();
    raw.extend_from_slice(&key.id);
    raw.extend_from_slice(key.pair.public_key().as_ref());
    let id_hex: String = key.id.iter().rev().map(|b| format!("{b:02X}")).collect();
    format!("untrusted comment: minisign public key {id_hex}\n{}\n", B64.encode(raw))
}

fn keygen() {
    let path = key_path();
    if path.exists() {
        eprintln!("A key already exists at {}; keeping it.", path.display());
    } else {
        let rng = SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).expect("key generation failed");
        let mut id = [0u8; 8];
        rng.fill(&mut id).expect("random failed");
        let mut raw = id.to_vec();
        raw.extend_from_slice(pkcs8.as_ref());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("{HEADER}\n{}\n", B64.encode(raw))).unwrap();
        eprintln!("New key written to {}", path.display());
    }
    print!("{}", public_text(&load()));
}

fn sign(file: &str) {
    let key = load();
    let data = std::fs::read(file).unwrap_or_else(|e| panic!("{file}: {e}"));
    // Prehashed minisign signature ("ED"): Ed25519 over the file's BLAKE2b-512, the kind the
    // launcher's streaming check takes.
    let sig = key.pair.sign(&blake2b::blake2b_512(&data));
    let mut sig_raw = b"ED".to_vec();
    sig_raw.extend_from_slice(&key.id);
    sig_raw.extend_from_slice(sig.as_ref());
    let name = std::path::Path::new(file).file_name().unwrap().to_string_lossy().into_owned();
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let trusted = format!("timestamp:{stamp}\tfile:{name}");
    // The global signature: over the signature and the trusted comment.
    let mut global_msg = sig.as_ref().to_vec();
    global_msg.extend_from_slice(trusted.as_bytes());
    let global = key.pair.sign(&global_msg);
    let text = format!(
        "untrusted comment: signature from horizon-sign secret key\n{}\ntrusted comment: {trusted}\n{}\n",
        B64.encode(sig_raw),
        B64.encode(global.as_ref())
    );
    let out = format!("{file}.minisig");
    std::fs::write(&out, &text).unwrap();
    // Checked with minisign-verify, as the launcher checks it.
    let public = minisign_verify::PublicKey::decode(&public_text(&key)).expect("public key");
    let signature = minisign_verify::Signature::decode(&text).expect("signature");
    public.verify(&data, &signature, false).expect("the new signature does not verify");
    let mut stream = public.verify_stream(&signature).expect("not a streaming signature");
    stream.update(&data);
    stream.finalize().expect("the new signature does not verify as a stream");
    println!("{out}: signed and verified");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("keygen") => keygen(),
        Some("pubkey") => print!("{}", public_text(&load())),
        Some(_) => args.iter().for_each(|f| sign(f)),
        None => eprintln!("usage: horizon-sign keygen | pubkey | <file>..."),
    }
}
