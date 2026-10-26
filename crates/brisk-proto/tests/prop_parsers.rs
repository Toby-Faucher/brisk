//! Property tests: every parser that reads attacker-influenced bytes (docs/fuzz-inventory.md)
//! takes arbitrary input without panicking, and its peak heap use stays proportional to the input
//! (a counting allocator below measures it), so a declared length can never make it allocate.
//! Inputs are small, so each case is quick; `PROPTEST_CASES=5000 cargo nextest run -p brisk-proto`
//! for a longer run.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use brisk_proto::http::{
    HttpError, check_status, parse_applist, parse_field, parse_launch, parse_serverinfo,
};
use proptest::prelude::*;

/// Counts live heap bytes per thread, so parallel tests in one process do not disturb each other.
struct Counting;

thread_local! {
    static LIVE: Cell<isize> = const { Cell::new(0) };
    static PEAK: Cell<isize> = const { Cell::new(0) };
}

fn track(delta: isize) {
    // try_with: the allocator also runs while thread-locals are being torn down.
    let _ = LIVE.try_with(|l| {
        let v = l.get() + delta;
        l.set(v);
        let _ = PEAK.try_with(|p| p.set(p.get().max(v)));
    });
}

// SAFETY: every call is forwarded unchanged to the system allocator; only a counter is added.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        // SAFETY: same contract as the caller's (non-zero size layout).
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            track(l.size() as isize);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: `p` came from `alloc` above with the same layout.
        unsafe { System.dealloc(p, l) };
        track(-(l.size() as isize));
    }
}

#[global_allocator]
static A: Counting = Counting;

/// Peak extra heap bytes on this thread while `f` runs.
fn peak<R>(f: impl FnOnce() -> R) -> (R, usize) {
    let start = LIVE.with(Cell::get);
    PEAK.with(|p| p.set(start));
    let r = f();
    (r, (PEAK.with(Cell::get) - start).max(0) as usize)
}

/// What any parser here may use for an input of `n` bytes.
fn budget(n: usize) -> usize {
    16 * n + (64 << 10)
}

/// Found in this pass: the XML parser stored the joined element path of every text node, so a
/// deeply nested reply (or many text nodes under one long element name) cost memory and time
/// quadratic in its size: 257 MB and 20 s (debug) for a 64 KiB reply.
#[test]
fn deep_or_long_xml_paths_cost_no_quadratic_memory() {
    let mut deep = String::from(r#"<root status_code="200">"#);
    while deep.len() < 64_000 {
        deep.push_str("<a>x");
    }
    let mut long = format!(r#"<root status_code="200"><{}>"#, "n".repeat(30_000));
    while long.len() < 64_000 {
        long.push_str("x<b/>");
    }
    for x in [deep, long] {
        let (r, used) = peak(|| check_status(&x));
        assert_eq!(r, Err(HttpError::BadXml));
        assert!(used <= budget(x.len()), "used {used} bytes for {}", x.len());
    }
}

fn bytes(max: usize) -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..max)
}

proptest! {
    #[test]
    fn http_parsers_take_any_text(s in "\\PC{0,600}") {
        let (_, used) = peak(|| {
            let _ = check_status(&s);
            let _ = parse_field(&s, "paired");
            let _ = parse_serverinfo(&s);
            let _ = parse_applist(&s);
            let _ = parse_launch(&s);
        });
        prop_assert!(used <= budget(s.len()));
    }
}
