# discid notes

Code: `crates/brisk-hostd/src/discid.rs`. It reads a game file's header and says what it is. No emulator runs.

## Checked against primary sources

Dolphin source, read on 2026-10-28:

- WBFS header (https://raw.githubusercontent.com/dolphin-emu/dolphin/master/Source/Core/DiscIO/WbfsBlob.h): packed struct of `u32 magic` ("WBFS"), `u32 hd_sector_count`, `u8 hd_sector_shift`, `u8 wbfs_sector_shift`, 2 padding bytes, `u8 disc_table[500]`. So the shift is at offset 8 and the disc table at 0x0C.
- WBFS reader (https://raw.githubusercontent.com/dolphin-emu/dolphin/master/Source/Core/DiscIO/WbfsBlob.cpp): HD sector size is `1 << hd_sector_shift`. Slot 0 is in use when `disc_table[0] != 0`. The block map for disc 0 is read from one HD sector plus 0x100 (`WII_DISC_HEADER_SIZE`), which means the 0x100-byte disc header copy sits at the start of the disc info, at file offset one HD sector. The `u32` count is byte-swapped, so the header fields are big-endian.
- WIA/RVZ (https://raw.githubusercontent.com/dolphin-emu/dolphin/master/Source/Core/DiscIO/WIABlob.h): `WIAHeader1` is 0x48 bytes. `WIAHeader2` starts with `disc_type`, `compression_type`, `compression_level`, `chunk_size` (4 bytes each), then `disc_header[0x80]`. So the disc header is at file offset 0x58. Magics: WIA `"WIA\x01"`, RVZ `"RVZ\x01"` (bytes at offset 0).

Not re-checked here, taken from `emulator-metadata-and-achievements.md` section C, which cites https://wiibrew.org/wiki/Wii_disc: the ID at 0..6, Wii magic 0x5D1C9EA3 at 0x18, GameCube magic 0xC2339F3D at 0x1C, title at 0x20 (64 bytes).

## Not verified

- Dolphin's own reader also checks that the file size equals `hd_sector_count << hd_sector_shift`. We do not; a short WBFS is only refused when the header copy would lie past the end.
- Region letters: `E` USA, `J` Japan, `K` Korea, the rest of the PAL letters are from memory (not looked up).
- Cartridge detection (from memory, no source fetched): iNES `"NES\x1A"` plus a 16-byte header; SNES copier header is 512 bytes when file size mod 1024 is 512; Game Boy logo at 0x104, CGB flag at 0x143 (0x80/0xC0 means GBC), title at 0x134; GBA logo start `24 FF AE 51` at 4, fixed byte 0x96 at 0xB2, title at 0xA0, game code at 0xAC; Mega Drive `"SEGA"` at 0x100, overseas title at 0x150. Interleaved Mega Drive dumps (SMD) and headerless SNES without a `.sfc`/`.smc` extension are not detected by content.
- The RetroAchievements hash. The rcheevos header (https://raw.githubusercontent.com/RetroAchievements/rcheevos/master/include/rc_hash.h) uses a 33-byte buffer for it, which fits a 32-character digest (MD5-sized), not SHA-1. The per-system rules were not read. So `Identity.hash` is a plain SHA-1 of the file after a known copier header is skipped. It does not match RetroAchievements and must not be sent to them.
- WIA/RVZ `disc_type` (GameCube or Wii) is not read; the system comes from the magic words in the embedded header.
- The Wii hash method on RetroAchievements is still unknown.

## Using it in Stage 2 and 3

- Stage 2 (detect by header): call `identify(path)` once per file and trust `system` and `container` over the file name. `System::Unknown` with `Ok` means "not something we know"; `Err(Invalid/Truncated)` means a known format that is damaged, so show it as broken, not as unknown.
- Stage 3 (identify the game): use `game_id` as the key for the Wii and GameCube metadata lookup (GameTDB-style IDs) and `title` as the fallback label. For cartridges there is no ID; use `hash` against a database that takes SHA-1 (ScreenScraper does), and compute an MD5 separately if RetroAchievements matching is wanted.
- Hashing a cartridge streams the whole file; run it off the request path and cache it by path, size and mtime.
