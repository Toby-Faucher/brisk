//! Browser input frames through the bridge's InputQueue, interleaved with the session's own calls.
#![no_main]
#[path = "common.rs"]
mod common;

use brisk_bridge::input::{InputQueue, MAX_PADS, MAX_PENDING, parse};
use libfuzzer_sys::fuzz_target;

/// Droppable events, plus one release per key (254 vks) and button (5), plus a neutral state and
/// a removal per pad.
const BOUND: usize = MAX_PENDING + 254 + 5 + 2 * MAX_PADS as usize;

fuzz_target!(|d: &[u8]| {
    let mut q = InputQueue::default();
    for c in common::chunks(d) {
        let _ = parse(c);
        let (op, body) = c.split_first().unwrap_or((&0, &[]));
        let stream = u64::from(op >> 4);
        match op & 0x0f {
            0..=7 => q.on_message_from(stream, body),
            8 => {
                q.pop();
            }
            9 => q.release_all(),
            10 => q.end_stream(stream),
            11 => {
                if let Some(e) = q.pop() {
                    q.timed_out(e);
                }
            }
            12 => q.detach_pads(),
            13 => {
                let _ = q.take_detached();
            }
            14 => {
                q.release_all();
                while q.pop().is_some() {}
                assert!(!q.held(), "held after release_all and a drain");
            }
            _ => {
                let _ = q.shutdown();
                assert!(q.is_empty() && !q.held(), "state left after shutdown");
            }
        }
        assert!(q.len() <= BOUND, "queue grew to {}", q.len());
    }
});
