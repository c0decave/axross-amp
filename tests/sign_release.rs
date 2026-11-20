//! scripts/sign_release.py signs what src/update.rs verifies, refuses a key
//! that is not pinned, and reads the pinned keys as the program has them.
//! (Needs Python with the cryptography package - AMP_PYTHON; without it
//! the test says so and passes: the maintainer's machine has it.)

use std::path::Path;
use std::process::Command;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn python() -> Option<String> {
    let python = std::env::var("AMP_PYTHON").unwrap_or_else(|_| "python3".into());
    let ok = Command::new(&python)
        .args(["-c", "import cryptography"])
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        eprintln!("no Python with cryptography: sign_release.py not tried");
    }
    ok.then_some(python)
}

fn script() -> std::path::PathBuf {
    Path::new(ROOT).join("scripts/sign_release.py")
}

#[test]
fn the_script_signs_what_the_updater_verifies_and_only_with_a_pinned_key() {
    let Some(python) = python() else { return };
    let dir = std::env::temp_dir().join(format!("amp-sign-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // a key of the test's own, pinned in a copy of TRUSTED_KEYS' shape
    let key = dir.join("key.pem");
    let out = Command::new(&python)
        .arg("-c")
        .arg(
            "import os,sys\n\
             from cryptography.hazmat.primitives import serialization as s\n\
             from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey\n\
             k=Ed25519PrivateKey.generate()\n\
             fd=os.open(sys.argv[1],os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)\n\
             os.write(fd,k.private_bytes(s.Encoding.PEM,s.PrivateFormat.PKCS8,s.NoEncryption()))\n\
             print(k.public_key().public_bytes(s.Encoding.Raw,s.PublicFormat.Raw).hex())",
        )
        .arg(&key)
        .output()
        .unwrap();
    assert!(out.status.success());
    let hex = String::from_utf8(out.stdout).unwrap().trim().to_string();
    let public: [u8; 32] = (0..32)
        .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let bytes: Vec<String> = public.iter().map(|b| format!("0x{b:02x}")).collect();
    let pinned = dir.join("update.rs");
    std::fs::write(
        &pinned,
        format!(
            "pub const TRUSTED_KEYS: &[[u8; 32]] = &[\n    [{}],\n];\n",
            bytes.join(", ")
        ),
    )
    .unwrap();
    let sums = dir.join("SHA256SUMS");
    let line = format!("{}  axross-amp-1.0.0-linux-x86_64\n", "ab".repeat(32));
    std::fs::write(&sums, &line).unwrap();
    let sign = |pinned_from: &Path| {
        Command::new(&python)
            .arg(script())
            .args(["sign"])
            .arg(&sums)
            .arg("--key")
            .arg(&key)
            .arg("--pinned-from")
            .arg(pinned_from)
            .output()
            .unwrap()
    };
    let done = sign(&pinned);
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let sig = std::fs::read(dir.join("SHA256SUMS.sig")).unwrap();
    assert_eq!(sig.len(), 64);
    axross_amp::update::verify_signature(line.as_bytes(), &sig, &[public]).unwrap();
    assert!(
        axross_amp::update::verify_signature(
            line.as_bytes(),
            &sig,
            axross_amp::update::TRUSTED_KEYS
        )
        .is_err()
    );
    // the program's own pinned key is not this one: refused
    std::fs::remove_file(dir.join("SHA256SUMS.sig")).unwrap();
    let refused = sign(&Path::new(ROOT).join("src/update.rs"));
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("not pinned"));
    assert!(!dir.join("SHA256SUMS.sig").exists());
    // the script reads TRUSTED_KEYS as the program has them
    let read = Command::new(&python)
        .args(["-B", "-c"])
        .arg("import sys; sys.path.insert(0, sys.argv[1]); import sign_release as s; print(' '.join(k.hex() for k in s.pinned_keys()))")
        .arg(Path::new(ROOT).join("scripts"))
        .output()
        .unwrap();
    let theirs = String::from_utf8(read.stdout).unwrap();
    let ours: Vec<String> = axross_amp::update::TRUSTED_KEYS
        .iter()
        .map(|k| axross_amp::update::hex(k))
        .collect();
    assert_eq!(theirs.trim(), ours.join(" "));
    let _ = std::fs::remove_dir_all(&dir);
}
