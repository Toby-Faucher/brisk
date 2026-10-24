//! Client identity (self-signed RSA-2048 cert, key, uniqueid) and the pinned server cert.
//! Every file is mode 0600, written to a temp file and renamed into place.

use rand_core::{OsRng, RngCore};
use rsa::pkcs1v15::{Signature, SigningKey};
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey, LineEnding};
use rsa::{RsaPrivateKey, RsaPublicKey};
use sha2::Sha256;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;
use x509_cert::builder::{Builder, CertificateBuilder, Profile};
use x509_cert::der::{DecodePem, EncodePem};
use x509_cert::name::Name;
use x509_cert::serial_number::SerialNumber;
use x509_cert::spki::{DecodePublicKey, EncodePublicKey, SubjectPublicKeyInfoOwned};
use x509_cert::time::Validity;
use x509_cert::Certificate;

const CERT: &str = "client.pem";
const KEY: &str = "client.key";
const UNIQUE_ID: &str = "uniqueid";
const SERVER: &str = "server.pem";

pub struct Identity {
    pub cert_pem: String,
    pub key_pem: String,
    pub unique_id: String,
}

/// The server cert received in pairing phase 1; the only cert later HTTPS calls accept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerCert {
    pub pem: String,
}

impl ServerCert {
    pub fn load(dir: &Path) -> crate::Result<ServerCert> {
        let pem = fs::read_to_string(dir.join(SERVER)).map_err(|e| {
            format!(
                "not paired ({}: {e}); run `brisk-probe pair`",
                dir.join(SERVER).display()
            )
        })?;
        Certificate::from_pem(&pem).map_err(|e| format!("{SERVER}: {e}"))?;
        Ok(ServerCert { pem })
    }
    pub fn save(&self, dir: &Path) -> crate::Result<()> {
        write_secret(dir, SERVER, &self.pem)
    }
}

/// Loads the identity in `dir`, or generates and stores a new one if any part is missing,
/// unreadable or mismatched (a half-written identity is replaced, not an error).
pub fn load_or_create_identity(dir: &Path) -> crate::Result<Identity> {
    if let Some(id) = load(dir) {
        return Ok(id);
    }
    let id = generate()?;
    fs::create_dir_all(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    write_secret(dir, KEY, &id.key_pem)?;
    write_secret(dir, CERT, &id.cert_pem)?;
    write_secret(dir, UNIQUE_ID, &id.unique_id)?;
    Ok(id)
}

fn load(dir: &Path) -> Option<Identity> {
    let read = |n| fs::read_to_string(dir.join(n)).ok();
    let id = Identity {
        cert_pem: read(CERT)?,
        key_pem: read(KEY)?,
        unique_id: read(UNIQUE_ID)?,
    };
    let cert = Certificate::from_pem(&id.cert_pem).ok()?;
    let spki =
        x509_cert::der::Encode::to_der(&cert.tbs_certificate.subject_public_key_info).ok()?;
    let key = RsaPrivateKey::from_pkcs8_pem(&id.key_pem).ok()?;
    (RsaPublicKey::from_public_key_der(&spki).ok()? == key.to_public_key()
        && !id.unique_id.is_empty())
    .then_some(id)
}

fn generate() -> crate::Result<Identity> {
    let key = RsaPrivateKey::new(&mut OsRng, 2048)?;
    let signer = SigningKey::<Sha256>::new(key.clone());
    let spki =
        SubjectPublicKeyInfoOwned::try_from(key.to_public_key().to_public_key_der()?.as_bytes())?;
    let mut serial = [0u8; 16];
    OsRng.fill_bytes(&mut serial);
    serial[0] = (serial[0] & 0x7f) | 0x01; // positive, no leading zero
    let validity = Validity::from_now(Duration::from_secs(20 * 365 * 86400))?;
    let cert = CertificateBuilder::new(
        Profile::Root,
        SerialNumber::new(&serial)?,
        validity,
        Name::from_str("CN=brisk")?,
        spki,
        &signer,
    )?
    .build::<Signature>()?;
    let mut uid = [0u8; 8];
    OsRng.fill_bytes(&mut uid);
    Ok(Identity {
        cert_pem: cert.to_pem(LineEnding::LF)?,
        key_pem: key.to_pkcs8_pem(LineEnding::LF)?.to_string(),
        unique_id: brisk_proto::pair::hex(&uid),
    })
}

/// Atomic 0600 write: fresh temp file in `dir`, fsync, rename over `name`.
fn write_secret(dir: &Path, name: &str, data: &str) -> crate::Result<()> {
    let tmp = dir.join(format!(".{name}.tmp"));
    let _ = fs::remove_file(&tmp); // a stale temp file may have other perms
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(data.as_bytes())?;
    f.sync_all()?;
    fs::rename(&tmp, dir.join(name))?;
    fs::File::open(dir)?.sync_all()?;
    Ok(())
}
