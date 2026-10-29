//! GameStream pairing, client side, mirroring Sunshine `src/nvhttp.cpp` phase handlers.
//!
//! Phase N's `on_*` takes the hex field of the server's reply to phase N and returns the
//! query params of phase N+1. Any error poisons the session (like Sunshine's `fail_pair`).

use aes::Aes128;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit, generic_array::GenericArray};
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs1v15::{Signature, SigningKey, VerifyingKey};
use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey};
use rsa::signature::{SignatureEncoding, Signer, Verifier};
use rsa::{RsaPrivateKey, RsaPublicKey};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use x509_cert::Certificate;
use x509_cert::der::{DecodePem, Encode};

/// Sent as `devicename` in phase 1; Sunshine requires the param.
const DEVICE_NAME: &str = "brisk";

#[derive(Debug, Clone, PartialEq, Eq)]
/// Why pairing failed.
pub enum PairError {
    /// A reply arrived in the wrong phase.
    OutOfOrder,
    /// A hex field is not hex.
    BadHex,
    /// A field has the wrong length.
    BadLength,
    /// A certificate does not parse.
    BadCert,
    /// The client key does not parse.
    BadKey,
    /// Server secret does not match the phase-2 hash: wrong PIN or MITM.
    HashMismatch,
    /// The server's signature over its secret does not verify.
    BadSignature,
    /// The server answered `paired` other than 1.
    NotPaired,
}

impl std::fmt::Display for PairError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "pairing failed: {self:?}")
    }
}
impl std::error::Error for PairError {}

/// Keys derived from the salt and PIN.
pub struct PairKeys {
    /// AES-128 key for the challenge phases.
    pub aes_key: [u8; 16],
}

/// SHA-256(salt || pin)[..16], as Sunshine `crypto::gen_aes_key`.
pub fn derive_aes_key(salt: [u8; 16], pin: &str) -> PairKeys {
    let h = Sha256::new()
        .chain_update(salt)
        .chain_update(pin.as_bytes())
        .finalize();
    PairKeys {
        aes_key: h[..16].try_into().unwrap(),
    }
}

/// AES-128-ECB, no padding. Panics if `data.len() % 16 != 0` (all callers pass whole blocks).
pub(crate) fn aes_ecb_encrypt(key: &[u8; 16], data: &[u8]) -> Vec<u8> {
    assert!(
        data.len().is_multiple_of(16),
        "aes_ecb_encrypt: partial block"
    );
    let c = Aes128::new(key.into());
    let mut out = data.to_vec();
    out.as_chunks_mut::<16>()
        .0
        .iter_mut()
        .for_each(|b| c.encrypt_block(GenericArray::from_mut_slice(b)));
    out
}

/// AES-128-ECB decryption, no padding; `data` must be whole blocks.
pub fn aes_ecb_decrypt(key: &[u8; 16], data: &[u8]) -> Result<Vec<u8>, PairError> {
    if !data.len().is_multiple_of(16) {
        return Err(PairError::BadLength);
    }
    let c = Aes128::new(key.into());
    let mut out = data.to_vec();
    out.as_chunks_mut::<16>()
        .0
        .iter_mut()
        .for_each(|b| c.decrypt_block(GenericArray::from_mut_slice(b)));
    Ok(out)
}

/// AES(SHA-256(server_challenge || client_cert_sig || client_secret)): the phase-3 param,
/// checked by Sunshine `clientpairingsecret` against what it decrypted in `serverchallengeresp`.
pub fn server_challenge_resp(
    key: &[u8; 16],
    server_challenge: &[u8],
    client_cert_sig: &[u8],
    client_secret: &[u8; 16],
) -> Vec<u8> {
    let h = Sha256::new()
        .chain_update(server_challenge)
        .chain_update(client_cert_sig)
        .chain_update(client_secret)
        .finalize();
    aes_ecb_encrypt(key, &h)
}

/// Uppercase hex, as Sunshine `util::hex_vec(_, true)`.
pub fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02X}")).collect()
}

/// Parses hex (either case); odd lengths and non-hex bytes are refused.
pub fn unhex(s: &str) -> Result<Vec<u8>, PairError> {
    // from_str_radix alone would accept "+F"
    if !s.len().is_multiple_of(2) || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(PairError::BadHex);
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| PairError::BadHex))
        .collect()
}

/// Randomness for pairing salt, challenge and secret.
pub trait Rng {
    /// Fills `buf` with random bytes.
    fn fill(&mut self, buf: &mut [u8]);
}

/// Parsed cert: (raw signature bits, public key).
fn parse_cert(pem: &str) -> Result<(Vec<u8>, RsaPublicKey), PairError> {
    let c = Certificate::from_pem(pem).map_err(|_| PairError::BadCert)?;
    let spki = c
        .tbs_certificate
        .subject_public_key_info
        .to_der()
        .map_err(|_| PairError::BadCert)?;
    let pk = RsaPublicKey::from_public_key_der(&spki).map_err(|_| PairError::BadCert)?;
    Ok((c.signature.raw_bytes().to_vec(), pk))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Start,
    SentCert,
    SentChallenge,
    SentResp,
    SentSecret,
    Done,
    Failed,
}

/// Client side of the PIN pairing handshake, one phase per reply.
pub struct PairingClient {
    cert_pem: String,
    cert_sig: Vec<u8>,
    key: SigningKey<Sha256>,
    unique_id: String,
    rng: Box<dyn Rng>,
    phase: Phase,
    aes_key: [u8; 16],
    server_sig: Vec<u8>,
    server_pk: Option<RsaPublicKey>,
    client_challenge: [u8; 16],
    server_hash: Vec<u8>,
    client_secret: [u8; 16],
}

impl PairingClient {
    /// A client using our certificate and key (PKCS#8 or PKCS#1 PEM).
    pub fn new(
        client_cert_pem: &str,
        client_key_pem: &str,
        unique_id: &str,
        rng: Box<dyn Rng>,
    ) -> Result<Self, PairError> {
        let (cert_sig, _) = parse_cert(client_cert_pem)?;
        let key = RsaPrivateKey::from_pkcs8_pem(client_key_pem)
            .or_else(|_| RsaPrivateKey::from_pkcs1_pem(client_key_pem))
            .map_err(|_| PairError::BadKey)?;
        Ok(Self {
            cert_pem: client_cert_pem.to_owned(),
            cert_sig,
            key: SigningKey::new(key),
            unique_id: unique_id.to_owned(),
            rng,
            phase: Phase::Start,
            aes_key: [0; 16],
            server_sig: vec![],
            server_pk: None,
            client_challenge: [0; 16],
            server_hash: vec![],
            client_secret: [0; 16],
        })
    }

    fn params(&self, kv: &[(&str, &str)]) -> Vec<(String, String)> {
        std::iter::once(("uniqueid", self.unique_id.as_str()))
            .chain(kv.iter().copied())
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect()
    }

    /// Checks the phase and marks the session failed until the caller completes the step.
    fn enter(&mut self, expect: Phase) -> Result<(), PairError> {
        let ok = self.phase == expect;
        self.phase = Phase::Failed;
        ok.then_some(()).ok_or(PairError::OutOfOrder)
    }

    /// Phase 1 (HTTP): starts a fresh session. Sunshine blocks this request until the PIN is entered.
    pub fn getservercert(&mut self, salt: [u8; 16], pin: &str) -> Vec<(String, String)> {
        self.aes_key = derive_aes_key(salt, pin).aes_key;
        self.phase = Phase::SentCert;
        let cert = hex(self.cert_pem.as_bytes());
        self.params(&[
            ("devicename", DEVICE_NAME),
            ("phrase", "getservercert"),
            ("salt", &hex(&salt)),
            ("clientcert", &cert),
        ])
    }

    /// `plaincert` (hex PEM; empty means the server is already pairing) -> phase 2 `clientchallenge`.
    pub fn on_servercert(
        &mut self,
        plaincert_hex: &str,
    ) -> Result<Vec<(String, String)>, PairError> {
        self.enter(Phase::SentCert)?;
        let pem = String::from_utf8(unhex(plaincert_hex)?).map_err(|_| PairError::BadCert)?;
        let (sig, pk) = parse_cert(&pem)?;
        self.server_sig = sig;
        self.server_pk = Some(pk);
        self.rng.fill(&mut self.client_challenge);
        let enc = aes_ecb_encrypt(&self.aes_key, &self.client_challenge);
        self.phase = Phase::SentChallenge;
        Ok(self.params(&[("clientchallenge", &hex(&enc))]))
    }

    /// `challengeresponse` = AES(hash(32) || server_challenge(16)) -> phase 3 `serverchallengeresp`.
    pub fn on_clientchallenge_resp(
        &mut self,
        hex_resp: &str,
    ) -> Result<Vec<(String, String)>, PairError> {
        self.enter(Phase::SentChallenge)?;
        let dec = aes_ecb_decrypt(&self.aes_key, &unhex(hex_resp)?)?;
        if dec.len() != 48 {
            return Err(PairError::BadLength);
        }
        self.server_hash = dec[..32].to_vec();
        let mut secret = [0u8; 16];
        self.rng.fill(&mut secret);
        let resp = server_challenge_resp(&self.aes_key, &dec[32..], &self.cert_sig, &secret);
        self.client_secret = secret;
        self.phase = Phase::SentResp;
        Ok(self.params(&[("serverchallengeresp", &hex(&resp))]))
    }

    /// `pairingsecret` = server_secret(16) || RSA-SHA256 sig -> phase 4 `clientpairingsecret`.
    /// Verifies the server knew the PIN (phase-2 hash) and holds the cert's key.
    pub fn on_serverchallenge_resp(
        &mut self,
        hex_resp: &str,
    ) -> Result<Vec<(String, String)>, PairError> {
        self.enter(Phase::SentResp)?;
        let ps = unhex(hex_resp)?;
        if ps.len() <= 16 {
            return Err(PairError::BadLength);
        }
        let (secret, sig) = ps.split_at(16);
        let h = Sha256::new()
            .chain_update(self.client_challenge)
            .chain_update(&self.server_sig)
            .chain_update(secret)
            .finalize();
        // Constant time: the hash commits to the PIN-derived exchange.
        if !bool::from(h.as_slice().ct_eq(self.server_hash.as_slice())) {
            return Err(PairError::HashMismatch);
        }
        let pk = self.server_pk.clone().ok_or(PairError::OutOfOrder)?;
        let sig = Signature::try_from(sig).map_err(|_| PairError::BadSignature)?;
        VerifyingKey::<Sha256>::new(pk)
            .verify(secret, &sig)
            .map_err(|_| PairError::BadSignature)?;
        let mine = self.client_secret;
        let out = [&mine[..], &self.key.sign(&mine).to_vec()].concat();
        self.phase = Phase::SentSecret;
        Ok(self.params(&[("clientpairingsecret", &hex(&out))]))
    }

    /// Phase 4 reply carries only `paired`; "1" -> phase 5 params, sent over HTTPS with the client cert.
    pub fn on_pairingsecret_resp(
        &mut self,
        paired: &str,
    ) -> Result<Vec<(String, String)>, PairError> {
        self.enter(Phase::SentSecret)?;
        if paired != "1" {
            return Err(PairError::NotPaired);
        }
        self.phase = Phase::Done;
        Ok(self.params(&[("phrase", "pairchallenge")]))
    }
}
