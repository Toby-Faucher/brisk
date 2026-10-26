//! Input events (any field values) through the Moonlight packet encoder and the control sealer.
#![no_main]
#[path = "common.rs"]
mod common;

use brisk_proto::control::{Control, ControlMsg};
use brisk_proto::input::{InputEvent, encode_input};
use libfuzzer_sys::fuzz_target;

fn event(b: &[u8]) -> Option<InputEvent> {
    let i32_at = |i: usize| Some(i32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?));
    let i16_at = |i: usize| Some(i16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?));
    Some(match b.first()? % 5 {
        0 => InputEvent::MouseMoveRel {
            dx: i32_at(1)?,
            dy: i32_at(5)?,
        },
        1 => InputEvent::MouseButton {
            button: *b.get(1)?,
            down: *b.get(2)? & 1 == 1,
        },
        2 => InputEvent::Scroll { amount: i16_at(1)? },
        3 => InputEvent::Key {
            vk: *b.get(1)?,
            down: *b.get(2)? & 1 == 1,
            modifiers: *b.get(3)?,
        },
        _ => InputEvent::Gamepad {
            index: *b.get(1)?,
            active_mask: u16::from_le_bytes(b.get(2..4)?.try_into().ok()?),
            buttons: u32::from_le_bytes(b.get(4..8)?.try_into().ok()?),
            lt: *b.get(8)?,
            rt: *b.get(9)?,
            lx: i16_at(10)?,
            ly: i16_at(12)?,
            rx: i16_at(14)?,
            ry: i16_at(16)?,
        },
    })
}

fuzz_target!(|d: &[u8]| {
    let mut ctl = Control::new(common::KEY);
    for c in common::chunks(d) {
        let Some(ev) = event(c) else { continue };
        if let Ok(ps) = encode_input(&ev) {
            assert!(ps.len() <= 8, "one event is at most 8 packets");
            for p in &ps {
                assert_eq!(
                    u32::from_be_bytes(p[..4].try_into().unwrap()) as usize,
                    p.len() - 4
                );
            }
        }
        if let Ok(ms) = ControlMsg::input(&ev) {
            for m in ms {
                ctl.encode(&m).expect("input fits a control message");
            }
        }
    }
});
