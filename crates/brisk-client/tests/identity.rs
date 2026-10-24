use brisk_client::identity::*;
use std::os::unix::fs::PermissionsExt;

fn mode(p: &std::path::Path) -> u32 {
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[test]
fn identity_roundtrip_is_0600() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("identity");
    let a = load_or_create_identity(&dir).unwrap();
    let files: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(files.len(), 3, "{files:?}");
    for f in &files {
        assert_eq!(mode(f), 0o600, "{f:?}");
    }
    assert_eq!(mode(&dir), 0o700);
    let b = load_or_create_identity(&dir).unwrap();
    assert_eq!((&a.cert_pem, &a.key_pem, &a.unique_id), (&b.cert_pem, &b.key_pem, &b.unique_id));
    // Usable for pairing (cert and key parse, RSA).
    let rng = Box::new(brisk_client::SystemRng);
    brisk_proto::pair::PairingClient::new(&b.cert_pem, &b.key_pem, &b.unique_id, rng).unwrap();
}

#[test]
fn half_written_identity_is_not_loaded() {
    let t = tempfile::tempdir().unwrap();
    let a = load_or_create_identity(t.path()).unwrap();
    std::fs::remove_file(t.path().join("client.key")).unwrap();
    let b = load_or_create_identity(t.path()).unwrap();
    assert_ne!(a.cert_pem, b.cert_pem, "cert without key must be regenerated");
    assert!(t.path().join("client.key").exists());
    // A key that does not match the cert is also a broken identity.
    let c = load_or_create_identity(tempfile::tempdir().unwrap().path()).unwrap();
    std::fs::write(t.path().join("client.key"), &c.key_pem).unwrap();
    let d = load_or_create_identity(t.path()).unwrap();
    assert_ne!(d.cert_pem, b.cert_pem);
    // No temp files left behind.
    let names: Vec<_> = std::fs::read_dir(t.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names.len(), 3, "{names:?}");
}
