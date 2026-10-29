//! Moonlight input packets (plaintext bodies, Gen7 / Sunshine appversion 7.1.431.-1).
//!
//! Layouts follow moonlight-common-c `Input.h` (`#pragma pack(1)`) and `InputStream.c`.
//! Every packet is `NV_INPUT_HEADER { size: u32 BE, magic: u32 LE }` + body, where
//! `size` = total length - 4. Per-field endianness is noted on each variant; Moonlight
//! mixes big- and little-endian. The control stream (Task 8) encrypts each returned
//! packet as one input message; nothing here does crypto.
//!
//! | event        | magic (LE u32) | total | body (after header)                                         |
//! |--------------|----------------|-------|-------------------------------------------------------------|
//! | MouseMoveRel | 0x00000007     | 12    | dx i16 BE, dy i16 BE                                        |
//! | MouseButton  | 0x08 dn/0x09 up| 9     | button u8 (1=L 2=M 3=R 4=X1 5=X2)                           |
//! | Scroll       | 0x0000000A     | 14    | amt i16 BE, amt i16 BE (copy), zero i16                     |
//! | Key          | 0x03 dn/0x04 up| 14    | flags u8 (0), vk u16 LE (|0x8000), modifiers u8, zero u16   |
//! | Gamepad      | 0x0000000C     | 34    | see [`InputEvent::Gamepad`]; all fields LE                  |

/// Moonlight `LI_WHEEL_DELTA`: one wheel notch in high-res scroll units.
pub const WHEEL_DELTA: i16 = 120;
/// Sunshine tracks 16 gamepads (`platf::MAX_GAMEPADS`, width of `activeGamepadMask`).
pub const MAX_GAMEPADS: u8 = 16;
/// SHIFT|CTRL|ALT|META: the 4 bits Limelight.h:699-702 defines.
const MODIFIER_MASK: u8 = 0x0F;
/// Largest |delta| one MouseMoveRel event may carry; more is clamped, so one (untrusted) event is
/// at most 8 packets and cannot fill the ENet send queue (`MAX_IN_FLIGHT`).
pub const MAX_MOUSE_DELTA: i32 = i16::MAX as i32 * 8;

#[derive(Debug, Clone, PartialEq, Eq)]
/// One input event from the browser, before encoding.
pub enum InputEvent {
    /// Relative motion. Deltas beyond i16 are split over several packets like
    /// moonlight's input thread (each axis saturates per packet, remainder follows); each axis is
    /// first clamped to +-[`MAX_MOUSE_DELTA`].
    MouseMoveRel {
        /// Horizontal motion.
        dx: i32,
        /// Vertical motion.
        dy: i32,
    },
    /// `button` 1..=5; `down` = press.
    MouseButton {
        /// Button number.
        button: u8,
        /// Pressed, not released.
        down: bool,
    },
    /// High-resolution vertical scroll (120 = one notch). 0 sends nothing.
    Scroll {
        /// Scroll amount, 120 per notch.
        amount: i16,
    },
    /// `vk` is the Windows virtual-key code; the encoder sets the 0x8000 flag real
    /// clients send (Sunshine masks it off). `modifiers` bits: 1 shift, 2 ctrl, 4 alt, 8 meta.
    Key {
        /// Windows virtual-key code.
        vk: u8,
        /// Pressed, not released.
        down: bool,
        /// Modifier bits.
        modifiers: u8,
    },
    /// Multi-controller packet. `buttons` low 16 bits -> `buttonFlags`, high 16 ->
    /// Sunshine's `buttonFlags2`. `index` 0..16; `active_mask` bit per connected pad
    /// (Sunshine allocates the pad when its bit is set). Layout, all LE:
    /// headerB 0x001A, controller u16, mask u16, midB 0x0014, buttons u16, lt u8, rt u8,
    /// lx,ly,rx,ry i16, tailA 0x009C, buttons2 u16, tailB 0x0055.
    Gamepad {
        /// Pad index, 0..16.
        index: u8,
        /// Bit per connected pad.
        active_mask: u16,
        /// Button bits.
        buttons: u32,
        /// Left trigger.
        lt: u8,
        /// Right trigger.
        rt: u8,
        /// Left stick x.
        lx: i16,
        /// Left stick y.
        ly: i16,
        /// Right stick x.
        rx: i16,
        /// Right stick y.
        ry: i16,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// An event the wire format cannot carry.
pub enum InputError {
    /// Mouse button outside 1..=5.
    BadMouseButton(u8),
    /// Modifier bits outside shift, ctrl, alt and meta.
    BadModifiers(u8),
    /// Gamepad index of 16 or more.
    GamepadIndex(u8),
}

impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InputError::BadMouseButton(b) => write!(f, "bad mouse button {b}"),
            InputError::BadModifiers(m) => write!(f, "bad key modifiers {m:#04x}"),
            InputError::GamepadIndex(i) => write!(f, "gamepad index {i} out of range"),
        }
    }
}
impl std::error::Error for InputError {}

fn packet(magic: u32, body: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(8 + body.len());
    p.extend_from_slice(&(body.len() as u32 + 4).to_be_bytes());
    p.extend_from_slice(&magic.to_le_bytes());
    p.extend_from_slice(body);
    p
}

/// Encode one event into zero or more plaintext input packets (one per control message).
pub fn encode_input(ev: &InputEvent) -> Result<Vec<Vec<u8>>, InputError> {
    Ok(match *ev {
        InputEvent::MouseMoveRel { dx, dy } => {
            let [mut dx, mut dy] = [dx, dy].map(|d| d.clamp(-MAX_MOUSE_DELTA, MAX_MOUSE_DELTA));
            let mut out = Vec::new();
            while dx != 0 || dy != 0 {
                let (cx, cy) = (
                    dx.clamp(i16::MIN as i32, i16::MAX as i32),
                    dy.clamp(i16::MIN as i32, i16::MAX as i32),
                );
                dx -= cx;
                dy -= cy;
                let ([x0, x1], [y0, y1]) = ((cx as i16).to_be_bytes(), (cy as i16).to_be_bytes());
                out.push(packet(7, &[x0, x1, y0, y1]));
            }
            out
        }
        InputEvent::MouseButton { button, down } => {
            if !(1..=5).contains(&button) {
                return Err(InputError::BadMouseButton(button));
            }
            vec![packet(if down { 8 } else { 9 }, &[button])]
        }
        InputEvent::Scroll { amount } => {
            if amount == 0 {
                return Ok(vec![]);
            }
            let a = amount.to_be_bytes();
            vec![packet(0x0A, &[a[0], a[1], a[0], a[1], 0, 0])]
        }
        InputEvent::Key {
            vk,
            down,
            modifiers,
        } => {
            if modifiers & !MODIFIER_MASK != 0 {
                return Err(InputError::BadModifiers(modifiers));
            }
            let k = (0x8000u16 | vk as u16).to_le_bytes();
            vec![packet(
                if down { 3 } else { 4 },
                &[0, k[0], k[1], modifiers, 0, 0],
            )]
        }
        InputEvent::Gamepad {
            index,
            active_mask,
            buttons,
            lt,
            rt,
            lx,
            ly,
            rx,
            ry,
        } => {
            if index >= MAX_GAMEPADS {
                return Err(InputError::GamepadIndex(index));
            }
            let mut b = Vec::with_capacity(26);
            b.extend_from_slice(&0x001Au16.to_le_bytes());
            b.extend_from_slice(&(index as u16).to_le_bytes());
            b.extend_from_slice(&active_mask.to_le_bytes());
            b.extend_from_slice(&0x0014u16.to_le_bytes());
            b.extend_from_slice(&(buttons as u16).to_le_bytes());
            b.extend_from_slice(&[lt, rt]);
            for s in [lx, ly, rx, ry] {
                b.extend_from_slice(&s.to_le_bytes());
            }
            b.extend_from_slice(&0x009Cu16.to_le_bytes());
            b.extend_from_slice(&((buttons >> 16) as u16).to_le_bytes());
            b.extend_from_slice(&0x0055u16.to_le_bytes());
            vec![packet(0x0C, &b)]
        }
    })
}
