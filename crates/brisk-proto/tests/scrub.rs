use brisk_proto::scrub::{scrub, scrub_with_encodings};

fn s(b: &[u8]) -> Vec<Vec<u8>> {
    vec![b.to_vec()]
}

#[test]
fn scrub_replaces_all_occurrences() {
    let out = scrub(b"..KEY..KEY..", &s(b"KEY"));
    assert_eq!(out, b"..\0\0\0..\0\0\0..");
}

#[test]
fn scrub_keeps_length() {
    let buf = b"abcSECRETdef".to_vec();
    assert_eq!(scrub(&buf, &s(b"SECRET")).len(), buf.len());
    assert_eq!(scrub(&buf, &[]), buf);
}

#[test]
fn empty_secret_is_ignored() {
    let buf = b"hello".to_vec();
    assert_eq!(scrub(&buf, &[vec![]]), buf);
}

#[test]
fn secret_longer_than_buffer_is_noop() {
    assert_eq!(scrub(b"ab", &s(b"abc")), b"ab");
}

#[test]
fn start_end_and_whole_buffer() {
    assert_eq!(scrub(b"KEYxxKEY", &s(b"KEY")), b"\0\0\0xx\0\0\0");
    assert_eq!(scrub(b"KEY", &s(b"KEY")), b"\0\0\0");
}

#[test]
fn partial_occurrence_is_left_alone() {
    assert_eq!(scrub(b"KEKEY", &s(b"KEY")), b"KE\0\0\0");
    assert_eq!(scrub(b"KE", &s(b"KEY")), b"KE");
}

#[test]
fn overlapping_secrets_fully_replaced() {
    let secrets = vec![b"abcd".to_vec(), b"cdef".to_vec(), b"cd".to_vec()];
    assert_eq!(scrub(b"xabcdefx", &secrets), b"x\0\0\0\0\0\0x");
    // self-overlap
    assert_eq!(scrub(b"aaaa", &s(b"aaa")), b"\0\0\0\0");
}

#[test]
fn multiple_secrets() {
    let secrets = vec![b"PIN1".to_vec(), b"rikey".to_vec()];
    assert_eq!(scrub(b"PIN1-rikey-PIN1", &secrets), b"\0\0\0\0-\0\0\0\0\0-\0\0\0\0");
}

#[test]
fn hex_forms_scrubbed() {
    let key = vec![0xde, 0xad, 0xbe, 0xef];
    let buf = b"rikey=DEADBEEF&x=deadbeef&y=DeAdBeEf".to_vec();
    let out = scrub_with_encodings(&buf, &[key]);
    assert_eq!(out, b"rikey=\0\0\0\0\0\0\0\0&x=\0\0\0\0\0\0\0\0&y=DeAdBeEf");
    // plain scrub does not know the hex form
    assert_eq!(scrub(&buf, &[vec![0xde, 0xad, 0xbe, 0xef]]), buf);
}

fn contains(h: &[u8], n: &[u8]) -> bool {
    h.windows(n.len()).any(|w| w == n)
}

#[test]
fn no_secret_substring_survives() {
    let mut st = 0x1234_5678_9abc_def0u64;
    let mut next = move || {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        st
    };
    for _ in 0..500 {
        // nonzero bytes so the filler can't recreate a secret
        let mut rnd = |n: usize| -> Vec<u8> { (0..n).map(|_| (next() % 255 + 1) as u8).collect() };
        let secrets: Vec<Vec<u8>> = (0..3).map(|_| rnd(4 + 12)).collect();
        let mut buf = rnd(200);
        for (i, sec) in secrets.iter().enumerate() {
            let at = i * 60 + (next() % 40) as usize; // distinct, non-colliding slots
            buf[at..at + sec.len()].copy_from_slice(sec);
        }
        for sec in &secrets {
            assert!(contains(&buf, sec), "secret not planted");
        }
        let out = scrub(&buf, &secrets);
        assert_eq!(out.len(), buf.len());
        for sec in &secrets {
            assert!(!contains(&out, sec));
        }
    }
}

#[test]
fn suffix_prefix_overlap_fully_scrubbed() {
    let secrets = vec![b"AAAABBBB".to_vec(), b"BBBBCCCC".to_vec()];
    let buf = b"..AAAABBBBCCCC..".to_vec();
    assert!(contains(&buf, &secrets[0]) && contains(&buf, &secrets[1]));
    let out = scrub(&buf, &secrets);
    assert_eq!(out, b"..\0\0\0\0\0\0\0\0\0\0\0\0..");
}
