//! Pure-Rust QUIC crypto for quinn, server side only.
//!
//! quinn's own rustls glue (`quinn_proto::crypto::rustls`) is compiled only with its `ring` or
//! `aws-lc-rs` features (C/asm), and no pure-Rust rustls provider implements `rustls::quic`
//! (rustls-rustcrypto 0.0.2-alpha: `quic: None` and `todo!()`; rustls-pure 0.15: `quic: None`;
//! checked 2026-10-24). This module fills the gap:
//! - [`Aes128GcmQuic`]: `rustls::quic::Algorithm` for AES-128-GCM packet protection and AES header
//!   protection (RFC 9001 sections 5.3-5.4), on RustCrypto `aes-gcm` and `aes`;
//! - [`provider`]: rustls-rustcrypto with only `TLS13_AES_128_GCM_SHA256`, which carries it;
//! - [`ServerCrypto`]: quinn's `crypto::ServerConfig` and `crypto::Session` over
//!   `rustls::quic::ServerConnection`, adapted from quinn-proto 0.11.19 `src/crypto/rustls.rs`
//!   (MIT OR Apache-2.0): the server half, with the retry tag on `aes-gcm` instead of ring;
//! - [`ResetKey`] (HMAC-SHA256) and [`TokenKey`] (HKDF-SHA256 to AES-256-GCM), the equivalents of
//!   quinn-proto's `ring_like.rs`.

use std::any::Any;
use std::io;
use std::sync::Arc;

use aes::cipher::BlockEncrypt;
use aes_gcm::aead::AeadInPlace;
use aes_gcm::{Aes128Gcm, Aes256Gcm, KeyInit};
use bytes::BytesMut;
use hmac::{Hmac, Mac};
use quinn_proto::crypto::{
    self, CryptoError, ExportKeyingMaterialError, KeyPair, Keys, UnsupportedVersion,
};
use quinn_proto::transport_parameters::TransportParameters;
use quinn_proto::{ConnectionId, Side, TransportError, TransportErrorCode};
use rustls::crypto::cipher::{AeadKey, Iv, Nonce};
use rustls::crypto::{CipherSuiteCommon, CryptoProvider};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::quic::{self as rq, Connection, KeyChange, Secrets, Suite, Version};
use rustls::{SupportedCipherSuite, Tls13CipherSuite};
use sha2::Sha256;

const TAG_LEN: usize = 16;

/// AES-128-GCM for QUIC packets plus AES-128 header protection.
pub struct Aes128GcmQuic;

impl rq::Algorithm for Aes128GcmQuic {
    fn packet_key(&self, key: AeadKey, iv: Iv) -> Box<dyn rq::PacketKey> {
        Box::new(PacketKey {
            key: Aes128Gcm::new_from_slice(key.as_ref())
                .expect("rustls sizes keys by aead_key_len"),
            iv,
        })
    }

    fn header_protection_key(&self, key: AeadKey) -> Box<dyn rq::HeaderProtectionKey> {
        Box::new(HeaderKey(
            aes::Aes128::new_from_slice(key.as_ref()).expect("rustls sizes keys by aead_key_len"),
        ))
    }

    fn aead_key_len(&self) -> usize {
        16
    }
}

struct PacketKey {
    key: Aes128Gcm,
    iv: Iv,
}

impl rq::PacketKey for PacketKey {
    fn encrypt_in_place(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut [u8],
    ) -> Result<rq::Tag, rustls::Error> {
        let nonce = Nonce::new(&self.iv, packet_number).0;
        let tag = self
            .key
            .encrypt_in_place_detached(&nonce.into(), header, payload)
            .map_err(|_| rustls::Error::EncryptError)?;
        Ok(rq::Tag::from(tag.as_slice()))
    }

    fn decrypt_in_place<'a>(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &'a mut [u8],
    ) -> Result<&'a [u8], rustls::Error> {
        let n = payload
            .len()
            .checked_sub(TAG_LEN)
            .ok_or(rustls::Error::DecryptError)?;
        let nonce = Nonce::new(&self.iv, packet_number).0;
        let (msg, tag) = payload.split_at_mut(n);
        self.key
            .decrypt_in_place_detached(&nonce.into(), header, msg, aes_gcm::Tag::from_slice(tag))
            .map_err(|_| rustls::Error::DecryptError)?;
        Ok(&payload[..n])
    }

    fn tag_len(&self) -> usize {
        TAG_LEN
    }

    /// RFC 9001 section 6.6 limits for AEAD_AES_128_GCM.
    fn confidentiality_limit(&self) -> u64 {
        1 << 23
    }

    fn integrity_limit(&self) -> u64 {
        1 << 52
    }
}

struct HeaderKey(aes::Aes128);

impl HeaderKey {
    /// RFC 9001 section 5.4.1, as rustls's ring backend does it.
    fn xor(
        &self,
        sample: &[u8],
        first: &mut u8,
        packet_number: &mut [u8],
        masked: bool,
    ) -> Result<(), rustls::Error> {
        let sample: [u8; 16] = sample
            .try_into()
            .map_err(|_| rustls::Error::General("sample of invalid length".into()))?;
        if packet_number.len() > 4 {
            return Err(rustls::Error::General("packet number too long".into()));
        }
        let mut mask = aes::Block::from(sample);
        self.0.encrypt_block(&mut mask);
        let bits = if *first & 0x80 != 0 { 0x0f } else { 0x1f };
        let plain = if masked {
            *first ^ (mask[0] & bits)
        } else {
            *first
        };
        let pn_len = (plain & 0x03) as usize + 1;
        *first ^= mask[0] & bits;
        for (d, m) in packet_number.iter_mut().zip(&mask[1..]).take(pn_len) {
            *d ^= m;
        }
        Ok(())
    }
}

impl rq::HeaderProtectionKey for HeaderKey {
    fn encrypt_in_place(
        &self,
        sample: &[u8],
        first: &mut u8,
        packet_number: &mut [u8],
    ) -> Result<(), rustls::Error> {
        self.xor(sample, first, packet_number, false)
    }

    fn decrypt_in_place(
        &self,
        sample: &[u8],
        first: &mut u8,
        packet_number: &mut [u8],
    ) -> Result<(), rustls::Error> {
        self.xor(sample, first, packet_number, true)
    }

    fn sample_len(&self) -> usize {
        16
    }
}

const BASE: &Tls13CipherSuite = match rustls_rustcrypto::TLS13_AES_128_GCM_SHA256 {
    SupportedCipherSuite::Tls13(s) => s,
    _ => panic!("TLS13_AES_128_GCM_SHA256 is a TLS 1.3 suite"),
};

/// rustls-rustcrypto's `TLS13_AES_128_GCM_SHA256` with QUIC support.
pub static SUITE: Tls13CipherSuite = Tls13CipherSuite {
    common: CipherSuiteCommon {
        suite: BASE.common.suite,
        hash_provider: BASE.common.hash_provider,
        confidentiality_limit: BASE.common.confidentiality_limit,
    },
    hkdf_provider: BASE.hkdf_provider,
    aead_alg: BASE.aead_alg,
    quic: Some(&Aes128GcmQuic),
};

/// rustls-rustcrypto (key exchange, signatures, randomness) with [`SUITE`] as the only suite.
pub fn provider() -> CryptoProvider {
    CryptoProvider {
        cipher_suites: vec![SupportedCipherSuite::Tls13(&SUITE)],
        ..rustls_rustcrypto::provider()
    }
}

fn initial_suite() -> Suite {
    SUITE.quic_suite().expect("SUITE has quic")
}

/// quinn server crypto: TLS 1.3 with one certificate, no client auth, no 0-RTT.
pub struct ServerCrypto {
    inner: Arc<rustls::ServerConfig>,
}

impl ServerCrypto {
    pub fn new(
        certs: Vec<CertificateDer<'static>>,
        key: PrivateKeyDer<'static>,
        alpn: &[&[u8]],
    ) -> Result<Self, rustls::Error> {
        let mut c = rustls::ServerConfig::builder_with_provider(Arc::new(provider()))
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_no_client_auth()
            .with_single_cert(certs, key)?;
        c.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
        c.max_early_data_size = 0;
        Ok(Self { inner: Arc::new(c) })
    }
}

fn interpret_version(version: u32) -> Result<Version, UnsupportedVersion> {
    match version {
        0xff00_001d..=0xff00_0020 => Ok(Version::V1Draft),
        0x0000_0001 | 0xff00_0021..=0xff00_0022 => Ok(Version::V1),
        _ => Err(UnsupportedVersion),
    }
}

fn initial_keys(version: Version, dst_cid: &ConnectionId, side: Side) -> Keys {
    let side = match side {
        Side::Client => rustls::Side::Client,
        Side::Server => rustls::Side::Server,
    };
    keys(initial_suite().keys(dst_cid, side, version))
}

fn keys(k: rq::Keys) -> Keys {
    Keys {
        header: KeyPair {
            local: Box::new(Hp(k.local.header)),
            remote: Box::new(Hp(k.remote.header)),
        },
        packet: KeyPair {
            local: Box::new(Pk(k.local.packet)),
            remote: Box::new(Pk(k.remote.packet)),
        },
    }
}

const RETRY_KEY_DRAFT: [u8; 16] = [
    0xcc, 0xce, 0x18, 0x7e, 0xd0, 0x9a, 0x09, 0xd0, 0x57, 0x28, 0x15, 0x5a, 0x6c, 0xb9, 0x6b, 0xe1,
];
const RETRY_NONCE_DRAFT: [u8; 12] = [
    0xe5, 0x49, 0x30, 0xf9, 0x7f, 0x21, 0x36, 0xf0, 0x53, 0x0a, 0x8c, 0x1c,
];
const RETRY_KEY_V1: [u8; 16] = [
    0xbe, 0x0c, 0x69, 0x0b, 0x9f, 0x66, 0x57, 0x5a, 0x1d, 0x76, 0x6b, 0x54, 0xe3, 0x68, 0xc8, 0x4e,
];
const RETRY_NONCE_V1: [u8; 12] = [
    0x46, 0x15, 0x99, 0xd3, 0x5d, 0x63, 0x2b, 0xf2, 0x23, 0x98, 0x25, 0xbb,
];

/// RFC 9001 section 5.8: the AES-128-GCM tag over the retry pseudo-packet.
fn retry_tag(version: Version, orig_dst_cid: &[u8], packet: &[u8]) -> [u8; 16] {
    let (key, nonce) = match version {
        Version::V1Draft => (RETRY_KEY_DRAFT, RETRY_NONCE_DRAFT),
        _ => (RETRY_KEY_V1, RETRY_NONCE_V1),
    };
    let mut pseudo = Vec::with_capacity(1 + orig_dst_cid.len() + packet.len());
    pseudo.push(orig_dst_cid.len() as u8);
    pseudo.extend_from_slice(orig_dst_cid);
    pseudo.extend_from_slice(packet);
    let tag = Aes128Gcm::new(&key.into())
        .encrypt_in_place_detached(&nonce.into(), &pseudo, &mut [])
        .expect("empty plaintext");
    tag.into()
}

impl crypto::ServerConfig for ServerCrypto {
    fn start_session(
        self: Arc<Self>,
        version: u32,
        params: &TransportParameters,
    ) -> Box<dyn crypto::Session> {
        // quinn calls this only after `initial_keys` accepted `version`.
        let version = interpret_version(version).expect("version checked by initial_keys");
        let mut tp = Vec::new();
        params.write(&mut tp);
        Box::new(TlsSession {
            version,
            got_handshake_data: false,
            next_secrets: None,
            inner: Connection::Server(
                rq::ServerConnection::new(self.inner.clone(), version, tp)
                    .expect("TLS 1.3 config with max_early_data_size 0"),
            ),
        })
    }

    fn initial_keys(
        &self,
        version: u32,
        dst_cid: &ConnectionId,
    ) -> Result<Keys, UnsupportedVersion> {
        Ok(initial_keys(
            interpret_version(version)?,
            dst_cid,
            Side::Server,
        ))
    }

    fn retry_tag(&self, version: u32, orig_dst_cid: &ConnectionId, packet: &[u8]) -> [u8; 16] {
        let version = interpret_version(version).expect("version checked by initial_keys");
        retry_tag(version, orig_dst_cid, packet)
    }
}

/// quinn client crypto over a caller-built `rustls::ClientConfig` (TLS 1.3 with [`provider`]).
/// Only the bridge's tests use it, as the browser stand-in.
pub struct ClientCrypto(pub Arc<rustls::ClientConfig>);

impl crypto::ClientConfig for ClientCrypto {
    fn start_session(
        self: Arc<Self>,
        version: u32,
        server_name: &str,
        params: &TransportParameters,
    ) -> Result<Box<dyn crypto::Session>, quinn_proto::ConnectError> {
        let version = interpret_version(version)
            .map_err(|_| quinn_proto::ConnectError::UnsupportedVersion)?;
        let name = rustls::pki_types::ServerName::try_from(server_name.to_owned())
            .map_err(|_| quinn_proto::ConnectError::InvalidServerName(server_name.into()))?;
        let mut tp = Vec::new();
        params.write(&mut tp);
        Ok(Box::new(TlsSession {
            version,
            got_handshake_data: false,
            next_secrets: None,
            inner: Connection::Client(
                rq::ClientConnection::new(self.0.clone(), version, name, tp)
                    .map_err(|e| quinn_proto::ConnectError::InvalidServerName(e.to_string()))?,
            ),
        }))
    }
}

/// What `quinn::Connection::handshake_data` returns for our connections.
pub struct HandshakeData {
    pub protocol: Option<Vec<u8>>,
    pub server_name: Option<String>,
}

struct TlsSession {
    version: Version,
    got_handshake_data: bool,
    next_secrets: Option<Secrets>,
    inner: Connection,
}

impl TlsSession {
    fn side(&self) -> Side {
        match self.inner {
            Connection::Client(_) => Side::Client,
            Connection::Server(_) => Side::Server,
        }
    }

    fn server_name(&self) -> Option<&str> {
        match &self.inner {
            Connection::Server(s) => s.server_name(),
            Connection::Client(_) => None,
        }
    }
}

impl crypto::Session for TlsSession {
    fn initial_keys(&self, dst_cid: &ConnectionId, side: Side) -> Keys {
        initial_keys(self.version, dst_cid, side)
    }

    fn handshake_data(&self) -> Option<Box<dyn Any>> {
        self.got_handshake_data.then(|| -> Box<dyn Any> {
            Box::new(HandshakeData {
                protocol: self.inner.alpn_protocol().map(<[u8]>::to_vec),
                server_name: self.server_name().map(str::to_owned),
            })
        })
    }

    fn peer_identity(&self) -> Option<Box<dyn Any>> {
        None // no client certificates
    }

    fn early_crypto(&self) -> Option<(Box<dyn crypto::HeaderKey>, Box<dyn crypto::PacketKey>)> {
        let k = self.inner.zero_rtt_keys()?;
        Some((Box::new(Hp(k.header)), Box::new(Pk(k.packet))))
    }

    fn early_data_accepted(&self) -> Option<bool> {
        match &self.inner {
            Connection::Client(c) => Some(c.is_early_data_accepted()),
            Connection::Server(_) => None,
        }
    }

    fn is_handshaking(&self) -> bool {
        self.inner.is_handshaking()
    }

    fn read_handshake(&mut self, buf: &[u8]) -> Result<bool, TransportError> {
        self.inner
            .read_hs(buf)
            .map_err(|e| match self.inner.alert() {
                Some(alert) => TransportError {
                    code: TransportErrorCode::crypto(alert.into()),
                    frame: None,
                    reason: e.to_string(),
                },
                None => TransportError {
                    code: TransportErrorCode::PROTOCOL_VIOLATION,
                    frame: None,
                    reason: format!("TLS error: {e}"),
                },
            })?;
        // rustls has no explicit "ClientHello seen" signal; quinn-proto uses the same test.
        if !self.got_handshake_data
            && (self.inner.alpn_protocol().is_some()
                || self.server_name().is_some()
                || !self.is_handshaking())
        {
            self.got_handshake_data = true;
            return Ok(true);
        }
        Ok(false)
    }

    fn transport_parameters(&self) -> Result<Option<TransportParameters>, TransportError> {
        match self.inner.quic_transport_parameters() {
            None => Ok(None),
            Some(buf) => TransportParameters::read(self.side(), &mut io::Cursor::new(buf))
                .map(Some)
                .map_err(Into::into),
        }
    }

    fn write_handshake(&mut self, buf: &mut Vec<u8>) -> Option<Keys> {
        let k = match self.inner.write_hs(buf)? {
            KeyChange::Handshake { keys } => keys,
            KeyChange::OneRtt { keys, next } => {
                self.next_secrets = Some(next);
                keys
            }
        };
        Some(keys(k))
    }

    fn next_1rtt_keys(&mut self) -> Option<KeyPair<Box<dyn crypto::PacketKey>>> {
        let k = self.next_secrets.as_mut()?.next_packet_keys();
        Some(KeyPair {
            local: Box::new(Pk(k.local)),
            remote: Box::new(Pk(k.remote)),
        })
    }

    fn is_valid_retry(&self, orig_dst_cid: &ConnectionId, header: &[u8], payload: &[u8]) -> bool {
        let Some(n) = payload.len().checked_sub(16) else {
            return false;
        };
        let mut packet = header.to_vec();
        packet.extend_from_slice(&payload[..n]);
        retry_tag(self.version, orig_dst_cid, &packet)[..] == payload[n..]
    }

    fn export_keying_material(
        &self,
        output: &mut [u8],
        label: &[u8],
        context: &[u8],
    ) -> Result<(), ExportKeyingMaterialError> {
        self.inner
            .export_keying_material(output, label, Some(context))
            .map(|_| ())
            .map_err(|_| ExportKeyingMaterialError)
    }
}

/// quinn's header-key interface over a rustls one (quinn-proto `impl HeaderKey for Box<dyn ..>`).
struct Hp(Box<dyn rq::HeaderProtectionKey>);

impl crypto::HeaderKey for Hp {
    fn decrypt(&self, pn_offset: usize, packet: &mut [u8]) {
        let (header, sample) = packet.split_at_mut(pn_offset + 4);
        let (first, rest) = header.split_at_mut(1);
        let pn_end = Ord::min(pn_offset + 3, rest.len());
        self.0
            .decrypt_in_place(
                &sample[..self.0.sample_len()],
                &mut first[0],
                &mut rest[pn_offset - 1..pn_end],
            )
            .expect("quinn passes a full sample and at most 4 pn bytes");
    }

    fn encrypt(&self, pn_offset: usize, packet: &mut [u8]) {
        let (header, sample) = packet.split_at_mut(pn_offset + 4);
        let (first, rest) = header.split_at_mut(1);
        let pn_end = Ord::min(pn_offset + 3, rest.len());
        self.0
            .encrypt_in_place(
                &sample[..self.0.sample_len()],
                &mut first[0],
                &mut rest[pn_offset - 1..pn_end],
            )
            .expect("quinn passes a full sample and at most 4 pn bytes");
    }

    fn sample_size(&self) -> usize {
        self.0.sample_len()
    }
}

/// quinn's packet-key interface over a rustls one.
struct Pk(Box<dyn rq::PacketKey>);

impl crypto::PacketKey for Pk {
    fn encrypt(&self, packet: u64, buf: &mut [u8], header_len: usize) {
        let (header, payload_tag) = buf.split_at_mut(header_len);
        let (payload, tag) = payload_tag.split_at_mut(payload_tag.len() - self.0.tag_len());
        let t = self
            .0
            .encrypt_in_place(packet, header, payload)
            .expect("AES-GCM encryption does not fail");
        tag.copy_from_slice(t.as_ref());
    }

    fn decrypt(
        &self,
        packet: u64,
        header: &[u8],
        payload: &mut BytesMut,
    ) -> Result<(), CryptoError> {
        let n = self
            .0
            .decrypt_in_place(packet, header, payload.as_mut())
            .map_err(|_| CryptoError)?
            .len();
        payload.truncate(n);
        Ok(())
    }

    fn tag_len(&self) -> usize {
        self.0.tag_len()
    }

    fn confidentiality_limit(&self) -> u64 {
        self.0.confidentiality_limit()
    }

    fn integrity_limit(&self) -> u64 {
        self.0.integrity_limit()
    }
}

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut b);
    b
}

/// Stateless-reset key: HMAC-SHA256 under a random 64-byte key.
pub struct ResetKey(Hmac<Sha256>);

impl ResetKey {
    pub fn random() -> Self {
        Self(
            <Hmac<Sha256> as Mac>::new_from_slice(&random::<64>())
                .expect("HMAC takes any key length"),
        )
    }
}

impl crypto::HmacKey for ResetKey {
    fn sign(&self, data: &[u8], out: &mut [u8]) {
        let mut m = self.0.clone();
        m.update(data);
        out.copy_from_slice(&m.finalize().into_bytes());
    }

    fn signature_len(&self) -> usize {
        32
    }

    fn verify(&self, data: &[u8], signature: &[u8]) -> Result<(), CryptoError> {
        let mut m = self.0.clone();
        m.update(data);
        m.verify_slice(signature).map_err(|_| CryptoError)
    }
}

/// Address-validation token key: HKDF-SHA256 over a random master key, expanded per token
/// into an AES-256-GCM key used once with a zero nonce (as quinn-proto does).
pub struct TokenKey(hkdf::Hkdf<Sha256>);

impl TokenKey {
    pub fn random() -> Self {
        Self(hkdf::Hkdf::new(None, &random::<64>()))
    }
}

impl crypto::HandshakeTokenKey for TokenKey {
    fn aead_from_hkdf(&self, random_bytes: &[u8]) -> Box<dyn crypto::AeadKey> {
        let mut k = [0u8; 32];
        self.0
            .expand(random_bytes, &mut k)
            .expect("32 bytes is a valid HKDF-SHA256 length");
        Box::new(TokenAead(Aes256Gcm::new(&k.into())))
    }
}

struct TokenAead(Aes256Gcm);

impl crypto::AeadKey for TokenAead {
    fn seal(&self, data: &mut Vec<u8>, additional_data: &[u8]) -> Result<(), CryptoError> {
        self.0
            .encrypt_in_place(&[0u8; 12].into(), additional_data, data)
            .map_err(|_| CryptoError)
    }

    fn open<'a>(
        &self,
        data: &'a mut [u8],
        additional_data: &[u8],
    ) -> Result<&'a mut [u8], CryptoError> {
        let n = data.len().checked_sub(TAG_LEN).ok_or(CryptoError)?;
        let (msg, tag) = data.split_at_mut(n);
        self.0
            .decrypt_in_place_detached(
                &[0u8; 12].into(),
                additional_data,
                msg,
                aes_gcm::Tag::from_slice(tag),
            )
            .map_err(|_| CryptoError)?;
        Ok(&mut data[..n])
    }
}

/// quinn server config: [`ServerCrypto`], a fresh [`TokenKey`], and `transport`.
pub fn server_config(
    crypto: ServerCrypto,
    transport: quinn::TransportConfig,
) -> quinn::ServerConfig {
    let mut c = quinn::ServerConfig::new(Arc::new(crypto), Arc::new(TokenKey::random()));
    c.transport_config(Arc::new(transport));
    c
}

/// quinn endpoint config with a fresh [`ResetKey`].
pub fn endpoint_config() -> quinn::EndpointConfig {
    quinn::EndpointConfig::new(Arc::new(ResetKey::random()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quinn_proto::crypto::{HandshakeTokenKey as _, HmacKey as _};

    /// draft-ietf-quic-v2-10 A.3 server Initial (the vector rustls tests its ring backend with):
    /// covers our packet key (AES-128-GCM with the pn nonce) and header protection end to end.
    #[test]
    fn server_initial_matches_quic_v2_vector() {
        let icid = [0x83, 0x94, 0xc8, 0xf0, 0x3e, 0x51, 0x57, 0x08];
        let server = rq::Keys::initial(
            Version::V2,
            &SUITE,
            &Aes128GcmQuic,
            &icid,
            rustls::Side::Server,
        );
        let mut payload = [
            0x02, 0x00, 0x00, 0x00, 0x00, 0x06, 0x00, 0x40, 0x5a, 0x02, 0x00, 0x00, 0x56, 0x03,
            0x03, 0xee, 0xfc, 0xe7, 0xf7, 0xb3, 0x7b, 0xa1, 0xd1, 0x63, 0x2e, 0x96, 0x67, 0x78,
            0x25, 0xdd, 0xf7, 0x39, 0x88, 0xcf, 0xc7, 0x98, 0x25, 0xdf, 0x56, 0x6d, 0xc5, 0x43,
            0x0b, 0x9a, 0x04, 0x5a, 0x12, 0x00, 0x13, 0x01, 0x00, 0x00, 0x2e, 0x00, 0x33, 0x00,
            0x24, 0x00, 0x1d, 0x00, 0x20, 0x9d, 0x3c, 0x94, 0x0d, 0x89, 0x69, 0x0b, 0x84, 0xd0,
            0x8a, 0x60, 0x99, 0x3c, 0x14, 0x4e, 0xca, 0x68, 0x4d, 0x10, 0x81, 0x28, 0x7c, 0x83,
            0x4d, 0x53, 0x11, 0xbc, 0xf3, 0x2b, 0xb9, 0xda, 0x1a, 0x00, 0x2b, 0x00, 0x02, 0x03,
            0x04,
        ];
        let plain_payload = payload;
        let mut header = [
            0xd1, 0x6b, 0x33, 0x43, 0xcf, 0x00, 0x08, 0xf0, 0x67, 0xa5, 0x50, 0x2a, 0x42, 0x62,
            0xb5, 0x00, 0x40, 0x75, 0x00, 0x01,
        ];
        let plain_header = header;
        let tag = server
            .local
            .packet
            .encrypt_in_place(1, &header, &mut payload)
            .unwrap();
        let (first, rest) = header.split_at_mut(1);
        let n = rest.len();
        server
            .local
            .header
            .encrypt_in_place(&payload[2..18], &mut first[0], &mut rest[n - 2..])
            .unwrap();
        let mut packet = header.to_vec();
        packet.extend(payload);
        packet.extend(tag.as_ref());
        let expected = [
            0xdc, 0x6b, 0x33, 0x43, 0xcf, 0x00, 0x08, 0xf0, 0x67, 0xa5, 0x50, 0x2a, 0x42, 0x62,
            0xb5, 0x00, 0x40, 0x75, 0xd9, 0x2f, 0xaa, 0xf1, 0x6f, 0x05, 0xd8, 0xa4, 0x39, 0x8c,
            0x47, 0x08, 0x96, 0x98, 0xba, 0xee, 0xa2, 0x6b, 0x91, 0xeb, 0x76, 0x1d, 0x9b, 0x89,
            0x23, 0x7b, 0xbf, 0x87, 0x26, 0x30, 0x17, 0x91, 0x53, 0x58, 0x23, 0x00, 0x35, 0xf7,
            0xfd, 0x39, 0x45, 0xd8, 0x89, 0x65, 0xcf, 0x17, 0xf9, 0xaf, 0x6e, 0x16, 0x88, 0x6c,
            0x61, 0xbf, 0xc7, 0x03, 0x10, 0x6f, 0xba, 0xf3, 0xcb, 0x4c, 0xfa, 0x52, 0x38, 0x2d,
            0xd1, 0x6a, 0x39, 0x3e, 0x42, 0x75, 0x75, 0x07, 0x69, 0x80, 0x75, 0xb2, 0xc9, 0x84,
            0xc7, 0x07, 0xf0, 0xa0, 0x81, 0x2d, 0x8c, 0xd5, 0xa6, 0x88, 0x1e, 0xaf, 0x21, 0xce,
            0xda, 0x98, 0xf4, 0xbd, 0x23, 0xf6, 0xfe, 0x1a, 0x3e, 0x2c, 0x43, 0xed, 0xd9, 0xce,
            0x7c, 0xa8, 0x4b, 0xed, 0x85, 0x21, 0xe2, 0xe1, 0x40,
        ];
        assert_eq!(packet[..], expected[..]);

        // The client's remote keys are the server's local keys: undo both layers.
        let client = rq::Keys::initial(
            Version::V2,
            &SUITE,
            &Aes128GcmQuic,
            &icid,
            rustls::Side::Client,
        );
        let (hdr, body) = packet.split_at_mut(20);
        let (first, rest) = hdr.split_at_mut(1);
        let n = rest.len();
        client
            .remote
            .header
            .decrypt_in_place(&body[2..18], &mut first[0], &mut rest[n - 2..])
            .unwrap();
        assert_eq!(hdr, plain_header);
        let mut wrong_pn = body.to_vec();
        assert!(
            client
                .remote
                .packet
                .decrypt_in_place(2, hdr, &mut wrong_pn)
                .is_err()
        );
        let plain = client.remote.packet.decrypt_in_place(1, hdr, body).unwrap();
        assert_eq!(plain, plain_payload);
    }

    /// RFC 9001 A.4: the Retry packet's integrity tag.
    #[test]
    fn retry_tag_matches_rfc9001_vector() {
        let odcid = [0x83, 0x94, 0xc8, 0xf0, 0x3e, 0x51, 0x57, 0x08];
        let retry = [
            0xff, 0x00, 0x00, 0x00, 0x01, 0x00, 0x08, 0xf0, 0x67, 0xa5, 0x50, 0x2a, 0x42, 0x62,
            0xb5, 0x74, 0x6f, 0x6b, 0x65, 0x6e,
        ];
        assert_eq!(
            retry_tag(Version::V1, &odcid, &retry),
            [
                0x04, 0xa2, 0x65, 0xba, 0x2e, 0xff, 0x4d, 0x82, 0x90, 0x58, 0xfb, 0x3f, 0x0f, 0x24,
                0x96, 0xba
            ]
        );
    }

    #[test]
    fn token_key_round_trips_and_rejects_tampering() {
        let k = TokenKey::random();
        let mut data = b"token".to_vec();
        k.aead_from_hkdf(b"r1").seal(&mut data, b"ad").unwrap();
        let mut copy = data.clone();
        assert_eq!(
            k.aead_from_hkdf(b"r1").open(&mut copy, b"ad").unwrap(),
            b"token"
        );
        let mut copy = data.clone();
        assert!(k.aead_from_hkdf(b"r2").open(&mut copy, b"ad").is_err());
        let mut copy = data.clone();
        assert!(k.aead_from_hkdf(b"r1").open(&mut copy, b"xx").is_err());
        assert!(k.aead_from_hkdf(b"r1").open(&mut [0u8; 15], b"ad").is_err());
    }

    #[test]
    fn reset_key_verifies_its_own_signature_only() {
        let k = ResetKey::random();
        let mut sig = [0u8; 32];
        k.sign(b"cid", &mut sig);
        assert!(k.verify(b"cid", &sig).is_ok());
        assert!(k.verify(b"cie", &sig).is_err());
        assert!(ResetKey::random().verify(b"cid", &sig).is_err());
    }

    /// RFC 9001 5.4.1: long headers mask 4 bits of the first byte, short headers 5, and only the
    /// packet number's own bytes (its length is in the unmasked first byte) are masked.
    #[test]
    fn header_protection_masks_4_or_5_bits_and_only_pn_len_bytes() {
        use aes::cipher::BlockEncrypt as _;
        let aes = aes::Aes128::new(&[7u8; 16].into());
        let hp = HeaderKey(aes.clone());
        // A sample whose mask has bit 0x10 set, so 4- and 5-bit masking differ.
        let (sample, mask) = (0..=255u8)
            .map(|b| {
                let s = [b; 16];
                let mut m = aes::Block::from(s);
                aes.encrypt_block(&mut m);
                (s, m)
            })
            .find(|(_, m)| m[0] & 0x10 != 0 && m[2] != 0)
            .unwrap();
        for (first, bits) in [(0xc0u8, 0x0f), (0x40, 0x1f)] {
            let (mut f, mut pn) = (first, [0u8; 4]); // pn length bits 00: one byte
            rq::HeaderProtectionKey::encrypt_in_place(&hp, &sample, &mut f, &mut pn).unwrap();
            assert_eq!(f, first ^ (mask[0] & bits), "first byte {first:#x}");
            assert_eq!(pn, [mask[1], 0, 0, 0], "only the 1-byte packet number");
            rq::HeaderProtectionKey::decrypt_in_place(&hp, &sample, &mut f, &mut pn).unwrap();
            assert_eq!((f, pn), (first, [0; 4]));
        }
    }

    #[test]
    fn header_protection_rejects_bad_sample_and_long_pn() {
        let hp = HeaderKey(aes::Aes128::new(&[0u8; 16].into()));
        let mut first = 0xc3;
        assert!(hp.xor(&[0u8; 15], &mut first, &mut [0; 4], false).is_err());
        assert!(hp.xor(&[0u8; 16], &mut first, &mut [0; 5], false).is_err());
        assert_eq!(first, 0xc3, "unchanged on error");
    }
}
