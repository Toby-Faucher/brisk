//! Vectors are hand-derived from moonlight-common-c Input.h/InputStream.c (#pragma pack(1),
//! header = size u32 BE (len-4), magic u32 LE). No recorded real-client capture or Sunshine
//! byte vector exists (Sunshine test_input.cpp builds packets from structs, not bytes),
//! so these are NOT independently verified against a live host.
use brisk_proto::input::*;

fn one(ev: InputEvent) -> Vec<u8> {
    let mut v = encode_input(&ev).unwrap();
    assert_eq!(v.len(), 1);
    v.remove(0)
}

#[test]
fn gamepad_full_deflection_bytes() {
    // NV_MULTI_CONTROLLER_PACKET, 34 bytes:
    // size BE32 = 30            00 00 00 1E
    // magic LE32 = 0x0C (GEN5)  0C 00 00 00
    // headerB LE16 0x001A       1A 00
    // controllerNumber 0        00 00
    // activeGamepadMask 1       01 00
    // midB LE16 0x0014          14 00
    // buttonFlags 0x1234        34 12
    // lt, rt = 255              FF FF
    // lx 32767                  FF 7F
    // ly -32768                 00 80
    // rx -32768                 00 80
    // ry 32767                  FF 7F
    // tailA 0x009C              9C 00
    // buttonFlags2 0x0010       10 00   (buttons = 0x0010_1234)
    // tailB 0x0055              55 00
    let want = [
        0, 0, 0, 0x1E, 0x0C, 0, 0, 0, 0x1A, 0, 0, 0, 1, 0, 0x14, 0, 0x34, 0x12, 0xFF, 0xFF, 0xFF,
        0x7F, 0, 0x80, 0, 0x80, 0xFF, 0x7F, 0x9C, 0, 0x10, 0, 0x55, 0,
    ];
    let got = one(InputEvent::Gamepad {
        index: 0,
        active_mask: 1,
        buttons: 0x0010_1234,
        lt: 255,
        rt: 255,
        lx: i16::MAX,
        ly: i16::MIN,
        rx: i16::MIN,
        ry: i16::MAX,
    });
    assert_eq!(got, want);
}

#[test]
fn mouse_move_rel_bytes() {
    // size 8 BE; magic 7 LE; dx=-2 BE (FF FE); dy=258 BE (01 02)
    assert_eq!(
        one(InputEvent::MouseMoveRel { dx: -2, dy: 258 }),
        [0, 0, 0, 8, 7, 0, 0, 0, 0xFF, 0xFE, 1, 2]
    );
}

#[test]
fn mouse_move_clamps_to_i16() {
    // InputStream.c: each axis saturates to i16 per packet, remainder in following packets.
    // dx = 40000 -> 32767 then 7233 (0x1C41); dy = -70000 -> -32768, -32768, -4464 (0xEE90)
    let v = encode_input(&InputEvent::MouseMoveRel {
        dx: 40000,
        dy: -70000,
    })
    .unwrap();
    let d = |p: &[u8]| {
        (
            i16::from_be_bytes([p[8], p[9]]),
            i16::from_be_bytes([p[10], p[11]]),
        )
    };
    assert_eq!(v.len(), 3);
    assert_eq!(d(&v[0]), (32767, -32768));
    assert_eq!(d(&v[1]), (7233, -32768));
    assert_eq!(d(&v[2]), (0, -4464));
    assert!(encode_input(&InputEvent::MouseMoveRel { dx: 0, dy: 0 })
        .unwrap()
        .is_empty());
    // Untrusted huge deltas are clamped to +-8 * 32767: at most 8 packets, never a queue flood.
    let v = encode_input(&InputEvent::MouseMoveRel {
        dx: i32::MIN,
        dy: i32::MAX,
    })
    .unwrap();
    assert_eq!(v.len(), 8);
    let sum = |axis: usize| {
        v.iter()
            .map(|p| d(p))
            .map(|t| [t.0, t.1][axis] as i32)
            .sum::<i32>()
    };
    assert_eq!((sum(0), sum(1)), (-MAX_MOUSE_DELTA, MAX_MOUSE_DELTA));
}

#[test]
fn mouse_button_bytes() {
    // size 5; magic 8 (press) / 9 (release), Gen5+ = action 7/8 + 1; button u8
    assert_eq!(
        one(InputEvent::MouseButton {
            button: 3,
            down: true
        }),
        [0, 0, 0, 5, 8, 0, 0, 0, 3]
    );
    assert_eq!(
        one(InputEvent::MouseButton {
            button: 1,
            down: false
        }),
        [0, 0, 0, 5, 9, 0, 0, 0, 1]
    );
    for b in [0, 6, 255] {
        assert_eq!(
            encode_input(&InputEvent::MouseButton {
                button: b,
                down: true
            }),
            Err(InputError::BadMouseButton(b))
        );
    }
}

#[test]
fn scroll_bytes() {
    // size 10; magic 0x0A; amt BE twice (-120 = FF 88); zero3
    assert_eq!(
        one(InputEvent::Scroll { amount: -120 }),
        [0, 0, 0, 10, 0x0A, 0, 0, 0, 0xFF, 0x88, 0xFF, 0x88, 0, 0]
    );
    assert!(encode_input(&InputEvent::Scroll { amount: 0 })
        .unwrap()
        .is_empty());
    assert_eq!(one(InputEvent::Scroll { amount: i16::MIN }).len(), 14);
}

#[test]
fn key_down_up() {
    // size 10; magic 3 down / 4 up; flags 0; keyCode LE 0x8041 ('A'); modifiers; zero2
    assert_eq!(
        one(InputEvent::Key {
            vk: 0x41,
            down: true,
            modifiers: 1
        }),
        [0, 0, 0, 10, 3, 0, 0, 0, 0, 0x41, 0x80, 1, 0, 0]
    );
    assert_eq!(
        one(InputEvent::Key {
            vk: 0x41,
            down: false,
            modifiers: 0
        }),
        [0, 0, 0, 10, 4, 0, 0, 0, 0, 0x41, 0x80, 0, 0, 0]
    );
    assert_eq!(
        encode_input(&InputEvent::Key {
            vk: 0x41,
            down: true,
            modifiers: 0x20
        }),
        Err(InputError::BadModifiers(0x20))
    );
    // Limelight.h defines 4 modifier bits; 0x10 is not one of them.
    assert_eq!(
        encode_input(&InputEvent::Key {
            vk: 0x41,
            down: true,
            modifiers: 0x10
        }),
        Err(InputError::BadModifiers(0x10))
    );
}

#[test]
fn multi_gamepad_index_ge_4_rejected() {
    // Classic GFE wraps `% 4`; Sunshine (Input.c `% MAX_GAMEPADS`, input.cpp gamepads[16])
    // accepts 0..16 and ignores >= 16 ("ControllerNumber out of range"). We target Sunshine:
    // 4..15 encode, >= 16 is rejected rather than silently wrapped onto another pad.
    let pad = |index| InputEvent::Gamepad {
        index,
        active_mask: 0,
        buttons: 0,
        lt: 0,
        rt: 0,
        lx: 0,
        ly: 0,
        rx: 0,
        ry: 0,
    };
    for i in [4, 15] {
        assert_eq!(one(pad(i))[10], i); // controllerNumber LE low byte
    }
    for i in [16, 255] {
        assert_eq!(encode_input(&pad(i)), Err(InputError::GamepadIndex(i)));
    }
}
