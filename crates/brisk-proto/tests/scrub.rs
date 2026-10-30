//! The fixture scrubber blanks every secret and its encodings.

use brisk_proto::scrub::{scrub, scrub_with_encodings};

/// (buffer, secrets, scrubbed buffer)
type Case<'a> = (&'a [u8], &'a [&'a [u8]], &'a [u8]);

#[test]
fn scrub_cases() {
    let cases: &[Case] = &[
        (b"..KEY..KEY..", &[b"KEY"], b"..\0\0\0..\0\0\0.."), // every occurrence
        (b"KEYxxKEY", &[b"KEY"], b"\0\0\0xx\0\0\0"),         // at the start and the end
        (b"KEY", &[b"KEY"], b"\0\0\0"),                      // the whole buffer
        (b"KEKEY", &[b"KEY"], b"KE\0\0\0"),                  // a partial match is left alone
        (b"KE", &[b"KEY"], b"KE"),
        (b"ab", &[b"abc"], b"ab"),    // secret longer than the buffer
        (b"hello", &[b""], b"hello"), // empty secret ignored
        (b"abcSECRETdef", &[], b"abcSECRETdef"), // no secrets
        (b"aaaa", &[b"aaa"], b"\0\0\0\0"), // self-overlap
        (b"xabcdefx", &[b"abcd", b"cdef", b"cd"], b"x\0\0\0\0\0\0x"), // overlapping secrets
        (
            b"PIN1-rikey-PIN1",
            &[b"PIN1", b"rikey"],
            b"\0\0\0\0-\0\0\0\0\0-\0\0\0\0",
        ),
        // One secret's suffix is the next one's prefix: both are blanked in full.
        (
            b"..AAAABBBBCCCC..",
            &[b"AAAABBBB", b"BBBBCCCC"],
            b"..\0\0\0\0\0\0\0\0\0\0\0\0..",
        ),
    ];
    for &(buf, secrets, want) in cases {
        let secrets: Vec<Vec<u8>> = secrets.iter().map(|s| s.to_vec()).collect();
        assert_eq!(
            scrub(buf, &secrets),
            want,
            "{:?}",
            String::from_utf8_lossy(buf)
        );
    }
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
