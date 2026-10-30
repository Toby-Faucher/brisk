//! Client identity (self-signed RSA-2048 cert, key, uniqueid) and the pinned server cert.
//! Every file is mode 0600, written to a temp file and renamed into place.

use rand_core::{OsRng, RngCore};
use rsa::pkcs1v15::{Signature, SigningKey};
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey, LineEnding};
use rsa::{RsaPrivateKey, RsaPublicKey};
use sha2::Sha256;
use std::fs::{self, DirBuilder};
use std::io::Write;
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;
use x509_cert::Certificate;
use x509_cert::builder::{Builder, CertificateBuilder, Profile};
use x509_cert::der::{DecodePem, EncodePem};
use x509_cert::name::Name;
use x509_cert::serial_number::SerialNumber;
use x509_cert::spki::{DecodePublicKey, EncodePublicKey, SubjectPublicKeyInfoOwned};
use x509_cert::time::Validity;

const CERT: &str = "client.pem";
const KEY: &str = "client.key";
const UNIQUE_ID: &str = "uniqueid";
const SERVER: &str = "server.pem";

/// The client's pairing identity: certificate, key and the unique id the host knows it by.
pub struct Identity {
    /// Self-signed client certificate, PEM.
    pub cert_pem: String,
    /// Private key for the certificate, PEM.
    pub key_pem: String,
    /// Id sent as `uniqueid` on every request.
    pub unique_id: String,
}

/// The server cert received in pairing phase 1; the only cert later HTTPS calls accept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerCert {
    /// The certificate, PEM.
    pub pem: String,
}

impl ServerCert {
    /// Reads the saved server cert from `dir`. Fails with a setup error if the client is not paired yet or the file is not a certificate.
    pub fn load(dir: &Path) -> crate::Result<ServerCert> {
        let pem = fs::read_to_string(dir.join(SERVER)).map_err(|e| {
            crate::Error::Setup(format!(
                "not paired ({}: {e}); run `brisk-probe pair`",
                dir.join(SERVER).display()
            ))
        })?;
        Certificate::from_pem(&pem).map_err(|e| crate::Error::Setup(format!("{SERVER}: {e}")))?;
        Ok(ServerCert { pem })
    }
    /// Writes the server cert into `dir`. Fails if the file cannot be written.
    pub fn save(&self, dir: &Path) -> crate::Result<()> {
        write_secret(dir, SERVER, &self.pem)
    }
}

/// `$XDG_CONFIG_HOME/brisk`, else `$HOME/.config/brisk`: the default `--config` of the binaries.
pub fn default_config_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(|d| Path::new(&d).join("brisk"))
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".config/brisk")))
}

/// Loads the existing identity in `dir`; never creates or overwrites anything.
pub fn load_identity(dir: &Path) -> crate::Result<Identity> {
    load(dir)?.ok_or_else(|| {
        crate::Error::Setup(format!(
            "no valid identity in {}; run `brisk-probe pair`",
            dir.display()
        ))
    })
}

/// Loads the identity in `dir`, or generates and stores a new one if a file is missing or the
/// identity does not parse or match (a half-written identity is replaced). Any other I/O error
/// (EACCES, EIO, ...) is returned and never leads to overwriting an existing key.
pub fn load_or_create_identity(dir: &Path) -> crate::Result<Identity> {
    if let Some(id) = load(dir)? {
        return Ok(id);
    }
    let id = generate().map_err(|e| crate::Error::Setup(format!("identity generation: {e}")))?;
    DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    write_secret(dir, KEY, &id.key_pem)?;
    write_secret(dir, CERT, &id.cert_pem)?;
    write_secret(dir, UNIQUE_ID, &id.unique_id)?;
    Ok(id)
}

/// `Ok(None)`: a file is missing, or the identity is unparsable/mismatched. `Err`: any other I/O error.
fn load(dir: &Path) -> crate::Result<Option<Identity>> {
    let read = |n| match fs::read_to_string(dir.join(n)) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(crate::Error::Io(e).context(dir.join(n).display().to_string())),
    };
    let (Some(cert_pem), Some(key_pem), Some(unique_id)) =
        (read(CERT)?, read(KEY)?, read(UNIQUE_ID)?)
    else {
        return Ok(None);
    };
    let id = Identity {
        cert_pem,
        key_pem,
        unique_id,
    };
    let ok = || -> Option<bool> {
        let cert = Certificate::from_pem(&id.cert_pem).ok()?;
        let spki =
            x509_cert::der::Encode::to_der(&cert.tbs_certificate.subject_public_key_info).ok()?;
        let key = RsaPrivateKey::from_pkcs8_pem(&id.key_pem).ok()?;
        Some(
            RsaPublicKey::from_public_key_der(&spki).ok()? == key.to_public_key()
                && !id.unique_id.is_empty(),
        )
    };
    Ok(ok().unwrap_or(false).then_some(id))
}

fn generate() -> Result<Identity, Box<dyn std::error::Error + Send + Sync>> {
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
    let mut f = tempfile::NamedTempFile::new_in(dir)?; // mode 0600
    f.write_all(data.as_bytes())?;
    f.as_file().sync_all()?;
    f.persist(dir.join(name)).map_err(|e| e.error)?;
    fs::File::open(dir)?.sync_all()?;
    Ok(())
}
