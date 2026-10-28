# RetroAchievements hash for GameCube and Wii discs

Question: how does brisk turn a Wii or GameCube file (Toby's Mario Kart Wii is a `.wbfs`) into the hash the RA game list expects?
Earlier notes call this unknown. It is not: rcheevos implements both.

## Sources read (all fetched, 2026-10-28)

- rcheevos `src/hash/rc_hash_disc.c` and `src/hash/rc_hash.c` (develop branch, raw.githubusercontent.com/RetroAchievements/rcheevos/develop/src/hash/). Note: the old `src/rhash/` path is gone, files now live in `src/hash/`.
- Dolphin `Source/Core/DolphinTool/VerifyCommand.cpp` and `Source/Core/Core/AchievementManager.cpp` (master).
- docs.retroachievements.org/developer-docs/game-identification.html: only a short GameCube paragraph, nothing on Wii, containers or disc IDs. The code is the authority.
- Not reached: the RA web API docs on whether game lists carry disc IDs. Not checked at all.

## GameCube (console 16), verified from code

Accepted when bytes 0x1C..0x20 are `C2 33 9F 3D`. MD5 (lowercase hex) over, in order:

1. Header: the first `0x2440 + 0x20 + apploader_body + apploader_trailer` bytes, capped at 1 MiB. The two apploader sizes are big-endian u32 at 0x2454 and 0x2458.
2. For each of the 18 main.dol segments (7 code, 11 data) with a non-zero size: that many bytes at the segment's file offset. The DOL starts at the big-endian u32 at 0x420. Segment offsets are u32 at DOL+0x00.., sizes u32 at DOL+0x90... The DOL header itself is not hashed.

Implemented in `crates/brisk-hostd/src/rahash.rs` (`ra_hash_gamecube_iso`), plus a std-only streaming MD5 tested on the RFC 1321 vectors.
Deliberate difference: rcheevos ignores short reads and hashes what it got; brisk returns an error.

## Wii (console 19), verified from code

rcheevos does support Wii discs. Accepted when bytes 0x18..0x1C are `5D 1C 9E A3`. (`.wad` WiiWare hits a stub that hashes nothing.)
The `.iso` extension tries PS2, PSP, PS3, 3DO, Sega CD, GameCube, then Wii in turn. The extension table has no `wbfs`, `rvz`, `gcm` or `gcz` entry.

MD5 over:

1. The first 0x80 bytes of the disc (main header).
2. The 4 bytes at 0x4E000 (region code).
3. The partition table at 0x40000 (4 groups, up to 16 partitions in total). Offsets are u32 values shifted left 2. For each partition that is not type 1 (update):
   a. The title metadata: size and offset are read at partition+0x2A4, size capped at 0x7C00.
   b. If the disc is encrypted (byte 0x61 is 0): up to 1024 clusters, each 0x7C00 bytes read at `part_offset + i*0x8000 + 0x400`. These are the RAW ENCRYPTED bytes. rcheevos does not decrypt anything.
   c. If decrypted (byte 0x61 non-zero): the GameCube partition routine at the partition offset with offsets shifted by 2.

Consequence: for a normal retail (encrypted) dump, no AES is needed. The hash only needs the disc image as raw bytes, as a plain ISO would present them.

## Containers, verified from Dolphin

Dolphin's `AchievementManager::CalculateHash` feeds rcheevos a custom file reader backed by Dolphin's `Volume::Read(..., PARTITION_NONE)`. So Dolphin reads WBFS, RVZ, WIA, GCZ, CISO and ISO as the raw underlying disc image and rcheevos hashes that. This is why one game gets one hash regardless of container.

`dolphin-tool verify -i <file> -a rchash` prints the RA hash (VerifyCommand.cpp). It exists only in builds with `USE_RETRO_ACHIEVEMENTS`; I did not check that the Arch package enables it. `dolphin-tool` is not installed on this machine, so nothing was run.
`dolphin-tool convert -f iso` can also write a plain ISO (options: rvz, iso, gcz, wia).

## What a pure Rust WBFS reader would need (not implemented)

No AES, no compression. WBFS is a block map: header (magic `WBFS`, sector count, HD sector shift, WBFS sector shift), then disc 0's info at one HD sector in: the 0x100-byte disc header, then a big-endian u16 table. Entry i is the WBFS sector holding the i-th block of the virtual ISO (0 = absent, reads as zeros). A `Read(offset, len)` is then a table lookup per block. discid.rs already parses the header and the shifts; the table layout beyond that is from memory of wbfs_file, NOT checked against Dolphin's WbfsBlob in this run.
That reader plus the Wii routine above (about 100 lines) would give the hash with no new dependency. The one risk: scrubbed WBFS files have absent blocks, and Dolphin returns zeros there. rcheevos would hash those zeros, so the reader must do the same. This is the same on any container, but is untested.
RVZ/WIA need zstd/bzip2/lzma plus Dolphin's exception lists and junk-data regeneration. Do not do this. If the file is a decrypted-partition or heavily scrubbed image the hash might also differ from the retail one; unverified.

## Decision table for the `.wbfs`

| Option | Cost | Status |
|---|---|---|
| `dolphin-tool verify -a rchash` on the file | needs a Dolphin build with RA support | exact RA answer by construction, unverified here |
| `dolphin-tool convert -f iso`, then brisk Wii routine | 7.9 GB temp file | not needed now that WBFS is read directly |
| Pure Rust WBFS reader + Wii routine in brisk | no dependency | written, passes synthetic tests, no real-disc comparison yet |
| Look up by 6-character disc ID | no hashing | unknown: not checked whether RA's game list carries disc IDs. The hash-based lookup is the documented route. |

## Wii routine and WBFS reader (written, 2nd pass)

`ra_hash_wii(path)` in `rahash.rs` handles a Wii ISO or WBFS file. The WBFS reader follows Dolphin's `WbfsBlob.cpp` (read this run):
file size must equal sector count << HD shift; WBFS sector >= 0x8000; disc 0 info sits at one HD sector in, the u16 big-endian table starts 0x100 later and has `ceil(143432*2*0x8000 / wbfs_sector)` entries; virtual offset o is at `table[o >> shift] << shift` plus the offset inside the block. Dolphin does not treat table entry 0 as absent: it reads the start of the WBFS file, and brisk copies that.
One rcheevos quirk is copied as written: the partition data offset at +0x2B8 is used as an absolute disc offset, though in the disc format it is relative to the partition. If the develop code differs from what Dolphin 2609 bundles, the hash would differ. Only a real golden hash settles that.
Tests: synthetic Wii image with a hand-computed expectation, the same image wrapped in a WBFS with a scrambled block map, and 11 truncated or hostile cases. There is also an `#[ignore]` test that reads `BRISK_RA_WII_FILE` and `BRISK_RA_WII_HASH`.

## VM probe: NOT done

The plan was to run `dolphin-tool verify -a rchash` on the games VM for a golden hash. The sandbox denied the remote shell command (class: remote shell writes), so nothing was run there: no `which dolphin-tool`, no `--help`, no hash. Whether the Arch Dolphin build has `rchash` is still unknown, and the Wii routine has never been compared with a real disc.

## Recommended next step

0. Status: the Wii routine exists; only the golden comparison is missing.
1. On a machine with Dolphin, run `dolphin-tool verify -i "Mario Kart Wii.wbfs" -a rchash`. This gives the true hash and a golden value.
2. Check it against the RA game list entry for Mario Kart Wii.
3. Run the ignored test in `rahash.rs` with the file and that hash. A match makes the Wii routine verified; a mismatch means the +0x2B8 quirk or the WBFS handling needs fixing.

## Verified vs not

- Verified (read in code): GameCube algorithm, Wii algorithm, that Wii hashes raw encrypted clusters, that Dolphin hands rcheevos the container-decoded raw disc, that `dolphin-tool verify -a rchash` exists.
- Verified by test: MD5 vectors, the GameCube routine against a synthetic ISO derived by hand, truncated and hostile inputs.
- Not verified: the Wii routine against a real disc, any hash against a real disc or RA's server, the Arch Dolphin package's RA support, the WBFS table details, disc IDs in the RA API.
