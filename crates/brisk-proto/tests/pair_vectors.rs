//! Vectors from Sunshine tests/unit/test_http_pairing.cpp (lines 50-95, 166-189), verbatim.

use brisk_proto::pair::*;
use rsa::pkcs1v15::{Signature, VerifyingKey};
use rsa::pkcs8::DecodePublicKey;
use rsa::signature::Verifier;
use rsa::RsaPublicKey;
use sha2::{Digest, Sha256};
use x509_cert::der::{DecodePem, Encode};
use x509_cert::Certificate;

const PRIVATE_KEY: &str = "-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQDLePNlWN06FLlM
ujWzIX8UICO7SWfH5DXlafVjpxwi/WCkdO6FxixqRNGu71wMvJXFbDlNR8fqX2xo
+eq17J3uFKn+qdjmP3L38bkqxhoJ/nCrXkeGyCTQ+Daug63ZYSJeW2Mmf+LAR5/i
/fWYfXpSlbcf5XJQPEWvENpLqWu+NOU50dJXIEVYpUXRx2+x4ZbwkH7tVJm94L+C
OUyiJKQPyWgU2aFsyJGwHFfePfSUpfYHqbHZV/ILpY59VJairBwE99bx/mBvMI7a
hBmJTSDuDffJcPDhFF5kZa0UkQPrPvhXcQaSRti7v0VonEQj8pTSnGYr9ktWKk92
wxDyn9S3AgMBAAECggEAbEhQ14WELg2rUz7hpxPTaiV0fo4hEcrMN+u8sKzVF3Xa
QYsNCNoe9urq3/r39LtDxU3D7PGfXYYszmz50Jk8ruAGW8WN7XKkv3i/fxjv8JOc
6EYDMKJAnYkKqLLhCQddX/Oof2udg5BacVWPpvhX6a1NSEc2H6cDupfwZEWkVhMi
bCC3JcNmjFa8N7ow1/5VQiYVTjpxfV7GY1GRe7vMvBucdQKH3tUG5PYXKXytXw/j
KDLaECiYVT89KbApkI0zhy7I5g3LRq0Rs5fmYLCjVebbuAL1W5CJHFJeFOgMKvnO
QSl7MfHkTnzTzUqwkwXjgNMGsTosV4UloL9gXVF6GQKBgQD5fI771WETkpaKjWBe
6XUVSS98IOAPbTGpb8CIhSjzCuztNAJ+0ey1zklQHonMFbdmcWTkTJoF3ECqAos9
vxB4ROg+TdqGDcRrXa7Twtmhv66QvYxttkaK3CqoLX8CCTnjgXBCijo6sCpo6H1T
+y55bBDpxZjNFT5BV3+YPBfWQwKBgQDQyNt+saTqJqxGYV7zWQtOqKORRHAjaJpy
m5035pky5wORsaxQY8HxbsTIQp9jBSw3SQHLHN/NAXDl2k7VAw/axMc+lj9eW+3z
2Hv5LVgj37jnJYEpYwehvtR0B4jZnXLyLwShoBdRPkGlC5fs9+oWjQZoDwMLZfTg
eZVOJm6SfQKBgQDfxYcB/kuKIKsCLvhHaSJpKzF6JoqRi6FFlkScrsMh66TCxSmP
0n58O0Cqqhlyge/z5LVXyBVGOF2Pn6SAh4UgOr4MVAwyvNp2aprKuTQ2zhSnIjx4
k0sGdZ+VJOmMS/YuRwUHya+cwDHp0s3Gq77tja5F38PD/s/OD8sUIqJGvQKBgBfI
6ghy4GC0ayfRa+m5GSqq14dzDntaLU4lIDIAGS/NVYDBhunZk3yXq99Mh6/WJQVf
Uc77yRsnsN7ekeB+as33YONmZm2vd1oyLV1jpwjfMcdTZHV8jKAGh1l4ikSQRUoF
xTdMb5uXxg6xVWtvisFq63HrU+N2iAESmMnAYxRZAoGAVEFJRRjPrSIUTCCKRiTE
br+cHqy6S5iYRxGl9riKySBKeU16fqUACIvUqmqlx4Secj3/Hn/VzYEzkxcSPwGi
qMgdS0R+tacca7NopUYaaluneKYdS++DNlT/m+KVHqLynQr54z1qBlThg9KGrpmM
LGZkXtQpx6sX7v3Kq56PkNk=
-----END PRIVATE KEY-----";
const PUBLIC_CERT: &str = "-----BEGIN CERTIFICATE-----
MIIC6zCCAdOgAwIBAgIBATANBgkqhkiG9w0BAQsFADA5MQswCQYDVQQGEwJJVDEW
MBQGA1UECgwNR2FtZXNPbldoYWxlczESMBAGA1UEAwwJbG9jYWxob3N0MB4XDTIy
MDQwOTA5MTYwNVoXDTQyMDQwNDA5MTYwNVowOTELMAkGA1UEBhMCSVQxFjAUBgNV
BAoMDUdhbWVzT25XaGFsZXMxEjAQBgNVBAMMCWxvY2FsaG9zdDCCASIwDQYJKoZI
hvcNAQEBBQADggEPADCCAQoCggEBAMt482VY3ToUuUy6NbMhfxQgI7tJZ8fkNeVp
9WOnHCL9YKR07oXGLGpE0a7vXAy8lcVsOU1Hx+pfbGj56rXsne4Uqf6p2OY/cvfx
uSrGGgn+cKteR4bIJND4Nq6DrdlhIl5bYyZ/4sBHn+L99Zh9elKVtx/lclA8Ra8Q
2kupa7405TnR0lcgRVilRdHHb7HhlvCQfu1Umb3gv4I5TKIkpA/JaBTZoWzIkbAc
V9499JSl9gepsdlX8guljn1UlqKsHAT31vH+YG8wjtqEGYlNIO4N98lw8OEUXmRl
rRSRA+s++FdxBpJG2Lu/RWicRCPylNKcZiv2S1YqT3bDEPKf1LcCAwEAATANBgkq
hkiG9w0BAQsFAAOCAQEAqPBqzvDjl89pZMll3Ge8RS7HeDuzgocrhOcT2jnk4ag7
/TROZuISjDp6+SnL3gPEt7E2OcFAczTg3l/wbT5PFb6vM96saLm4EP0zmLfK1FnM
JDRahKutP9rx6RO5OHqsUB+b4jA4W0L9UnXUoLKbjig501AUix0p52FBxu+HJ90r
HlLs3Vo6nj4Z/PZXrzaz8dtQ/KJMpd/g/9xlo6BKAnRk5SI8KLhO4hW6zG0QA56j
X4wnh1bwdiidqpcgyuKossLOPxbS786WmsesaAWPnpoY6M8aija+ALwNNuWWmyMg
9SVDV76xJzM36Uq7Kg3QJYTlY04WmPIdJHkCtXWf9g==
-----END CERTIFICATE-----";

const SALT: &str = "ff5dc6eda99339a8a0793e216c4257c4";
const PIN: &str = "5338";
const CLIENT_CHALLENGE: &str = "741CD3D6890C16DA39D53BCA0893AAF0";
const SERVER_CHALLENGE_RESP: &str = "920BABAE9F7599AA1CA8EC87FB3454C91872A7D8D5127DDC176C2FDAE635CF7A";
const SERVER_CHALLENGE: &str = "AAAAAAAAAAAAAAAA";
const CLIENT_PAIRING_SECRET: &str = concat!(
    "000102030405060708090A0B0C0D0EFF", // secret
    "9BB74D8DE2FF006C3F47FC45EFDAA97D433783AFAB3ACD85CA7ED2330BB2A7BD18A5B044AF8CAC177116FAE8A6E8E44653A8944A0F8EA138B2E013756D847D2C4FC52F736E2E7E9B4154712B18F8307B2A161E010F0587744163E42ECA9EA548FC435756EDCF1FEB94037631ABB72B29DDAC0EA5E61F2DBFCC3B20AA021473CC85AC98D88052CA6618ED1701EFBF142C18D5E779A3155B84DF65057D4823EC194E6DF14006793E8D7A3DCCE20A911636C4E01ECA8B54B9DE9F256F15DE9A980EA024B30D77579140D45EC220C738164BDEEEBF7364AE94A5FF9B784B40F2E640CE8603017DEEAC7B2AD77B807C643B7B349C110FE15F94C7B3D37FF15FDFBE26",
);

fn salt() -> [u8; 16] {
    unhex(SALT).unwrap().try_into().unwrap()
}

/// Hands out queued chunks in order: client challenge, then client secret.
struct QueueRng(Vec<Vec<u8>>);
impl Rng for QueueRng {
    fn fill(&mut self, buf: &mut [u8]) {
        buf.copy_from_slice(&self.0.remove(0));
    }
}

fn client() -> PairingClient {
    let rng = QueueRng(vec![b"CLIENT CHALLENGE".to_vec(), unhex("000102030405060708090A0B0C0D0EFF").unwrap()]);
    PairingClient::new(PUBLIC_CERT, PRIVATE_KEY, "1234", Box::new(rng)).unwrap()
}

fn param<'a>(p: &'a [(String, String)], k: &str) -> &'a str {
    &p.iter().find(|(n, _)| n == k).unwrap_or_else(|| panic!("missing {k}")).1
}

fn cert_sig() -> Vec<u8> {
    Certificate::from_pem(PUBLIC_CERT).unwrap().signature.raw_bytes().to_vec()
}

fn sign(data: &[u8]) -> Vec<u8> {
    use rsa::pkcs8::DecodePrivateKey;
    use rsa::signature::{SignatureEncoding, Signer};
    let key = rsa::RsaPrivateKey::from_pkcs8_pem(PRIVATE_KEY).unwrap();
    rsa::pkcs1v15::SigningKey::<Sha256>::new(key).sign(data).to_vec()
}

#[test]
fn aes_key_from_salt_and_pin() {
    let k = derive_aes_key(salt(), PIN);
    assert_eq!(hex(&aes_ecb_encrypt(&k.aes_key, b"CLIENT CHALLENGE")), CLIENT_CHALLENGE);
    assert_eq!(aes_ecb_decrypt(&k.aes_key, &unhex(CLIENT_CHALLENGE).unwrap()).unwrap(), b"CLIENT CHALLENGE");
}

#[test]
fn wrong_pin_changes_ciphertext() {
    let k = derive_aes_key(salt(), "0000");
    assert_ne!(hex(&aes_ecb_encrypt(&k.aes_key, b"CLIENT CHALLENGE")), CLIENT_CHALLENGE);
}

#[test]
fn server_challenge_resp_vector() {
    // The Sunshine vector uses an 8-byte server challenge (test override), so it cannot travel
    // through a real 48-byte challengeresponse; check the function PairingClient uses directly.
    let k = derive_aes_key(salt(), PIN);
    let secret: [u8; 16] = unhex("000102030405060708090A0B0C0D0EFF").unwrap().try_into().unwrap();
    let resp = server_challenge_resp(&k.aes_key, &unhex(SERVER_CHALLENGE).unwrap(), &cert_sig(), &secret);
    assert_eq!(hex(&resp), SERVER_CHALLENGE_RESP);
}

#[test]
fn phase_sequence_with_fixed_rng() {
    let key = derive_aes_key(salt(), PIN).aes_key;
    let mut c = client();

    let p1 = c.getservercert(salt(), PIN);
    assert_eq!(param(&p1, "phrase"), "getservercert");
    assert_eq!(param(&p1, "salt"), SALT.to_uppercase());
    assert_eq!(unhex(param(&p1, "clientcert")).unwrap(), PUBLIC_CERT.as_bytes());
    assert_eq!(param(&p1, "uniqueid"), "1234");

    // Sunshine test setup: the server uses the same key and cert as the client.
    let p2 = c.on_servercert(&hex(PUBLIC_CERT.as_bytes())).unwrap();
    assert_eq!(param(&p2, "clientchallenge"), CLIENT_CHALLENGE);

    // Simulated server, as nvhttp.cpp clientchallenge().
    let server_secret = [0x5Au8; 16];
    let server_challenge = [0xAAu8; 16];
    let challenge = aes_ecb_decrypt(&key, &unhex(param(&p2, "clientchallenge")).unwrap()).unwrap();
    let hash = Sha256::new().chain_update(&challenge).chain_update(cert_sig()).chain_update(server_secret).finalize();
    let resp = aes_ecb_encrypt(&key, &[hash.as_slice(), &server_challenge].concat());
    let p3 = c.on_clientchallenge_resp(&hex(&resp)).unwrap();
    let secret = unhex("000102030405060708090A0B0C0D0EFF").unwrap();
    let want = server_challenge_resp(&key, &server_challenge, &cert_sig(), &secret.clone().try_into().unwrap());
    assert_eq!(param(&p3, "serverchallengeresp"), hex(&want));
    // nvhttp.cpp clientpairingsecret() same_hash check, done server side.
    let clienthash = aes_ecb_decrypt(&key, &want).unwrap();
    let expect = Sha256::new().chain_update(server_challenge).chain_update(cert_sig()).chain_update(&secret).finalize();
    assert_eq!(clienthash, expect.as_slice());

    // As nvhttp.cpp serverchallengeresp(): serversecret || sign256(serversecret).
    let pairingsecret = [&server_secret[..], &sign(&server_secret)].concat();
    let p4 = c.on_serverchallenge_resp(&hex(&pairingsecret)).unwrap();
    let cps = param(&p4, "clientpairingsecret");
    assert_eq!(cps, CLIENT_PAIRING_SECRET);
    let cps = unhex(cps).unwrap();
    let cert = Certificate::from_pem(PUBLIC_CERT).unwrap();
    let pk = RsaPublicKey::from_public_key_der(&cert.tbs_certificate.subject_public_key_info.to_der().unwrap()).unwrap();
    VerifyingKey::<Sha256>::new(pk).verify(&cps[..16], &Signature::try_from(&cps[16..]).unwrap()).unwrap();

    let p5 = c.on_pairingsecret_resp("1").unwrap();
    assert_eq!(param(&p5, "phrase"), "pairchallenge");
    assert_eq!(param(&p5, "uniqueid"), "1234");
}

/// Client after phase 2, with the phase-2 hash committing to server secret [1; 16].
fn at_phase3() -> PairingClient {
    let key = derive_aes_key(salt(), PIN).aes_key;
    let mut c = client();
    c.getservercert(salt(), PIN);
    let p2 = c.on_servercert(&hex(PUBLIC_CERT.as_bytes())).unwrap();
    let challenge = aes_ecb_decrypt(&key, &unhex(param(&p2, "clientchallenge")).unwrap()).unwrap();
    let hash = Sha256::new().chain_update(&challenge).chain_update(cert_sig()).chain_update([1u8; 16]).finalize();
    c.on_clientchallenge_resp(&hex(&aes_ecb_encrypt(&key, &[hash.as_slice(), &[0u8; 16]].concat()))).unwrap();
    c
}

#[test]
fn tampered_server_secret_rejected() {
    // Correctly signed, but not the secret the phase-2 hash committed to.
    let other = [2u8; 16];
    let r = at_phase3().on_serverchallenge_resp(&hex(&[&other[..], &sign(&other)].concat()));
    assert_eq!(r, Err(PairError::HashMismatch));
    // Committed secret, bad signature.
    let mut sig = sign(&[1u8; 16]);
    sig[0] ^= 1;
    let r = at_phase3().on_serverchallenge_resp(&hex(&[&[1u8; 16][..], &sig].concat()));
    assert_eq!(r, Err(PairError::BadSignature));
}

#[test]
fn out_of_order_calls_error() {
    let mut c = client();
    assert_eq!(c.on_pairingsecret_resp("1"), Err(PairError::OutOfOrder));
    let mut c = client();
    assert_eq!(c.on_servercert(&hex(PUBLIC_CERT.as_bytes())), Err(PairError::OutOfOrder));
    // A failed phase poisons the session, like Sunshine's fail_pair.
    let mut c = client();
    c.getservercert(salt(), PIN);
    assert!(c.on_servercert("").is_err());
    assert_eq!(c.on_servercert(&hex(PUBLIC_CERT.as_bytes())), Err(PairError::OutOfOrder));
}

#[test]
fn empty_and_garbage_inputs_error_not_panic() {
    for bad in ["", "ABC", "WRONG", "NOSIGNATURE", "00", "+F", "\u{e9}0"] {
        let mut c = client();
        c.getservercert(salt(), PIN);
        assert!(c.on_servercert(bad).is_err(), "servercert {bad:?}");

        let mut c = client();
        c.getservercert(salt(), PIN);
        c.on_servercert(&hex(PUBLIC_CERT.as_bytes())).unwrap();
        assert!(c.on_clientchallenge_resp(bad).is_err(), "challengeresponse {bad:?}");

        assert!(at_phase3().on_serverchallenge_resp(bad).is_err(), "pairingsecret {bad:?}");
        let secret_only = format!("{}{bad}", "01".repeat(16));
        assert!(at_phase3().on_serverchallenge_resp(&secret_only).is_err(), "no signature {bad:?}");
    }
    let key = derive_aes_key(salt(), PIN).aes_key;
    assert!(aes_ecb_decrypt(&key, &[0u8; 15]).is_err());
    let mut c = client();
    c.getservercert(salt(), PIN);
    assert!(c.on_servercert(&hex(b"-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n")).is_err());
    assert!(PairingClient::new("junk", PRIVATE_KEY, "1", Box::new(QueueRng(vec![]))).is_err());
}
