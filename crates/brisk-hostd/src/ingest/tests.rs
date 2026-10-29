use super::*;
use crate::discid::Container;
use std::os::unix::fs::symlink;

pub(crate) fn disc(id: &[u8; 6], wii: bool, title: &str) -> Vec<u8> {
    let mut h = vec![0u8; 0x100];
    h[..6].copy_from_slice(id);
    h[if wii { 0x18 } else { 0x1C }..][..4].copy_from_slice(if wii {
        &[0x5D, 0x1C, 0x9E, 0xA3]
    } else {
        &[0xC2, 0x33, 0x9F, 0x3D]
    });
    h[0x20..0x20 + title.len()].copy_from_slice(title.as_bytes());
    h
}

fn gba(title: &str) -> Vec<u8> {
    let mut h = vec![0u8; 0x200];
    h[4..8].copy_from_slice(&[0x24, 0xFF, 0xAE, 0x51]);
    h[0xB2] = 0x96;
    h[0xA0..0xA0 + title.len()].copy_from_slice(title.as_bytes());
    h[0xAC..0xB0].copy_from_slice(b"AGBE");
    h
}

fn put(root: &Path, rel: &str, bytes: &[u8]) -> PathBuf {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, bytes).unwrap();
    p
}

fn names(s: &Scan, root: &Path) -> Vec<String> {
    s.found
        .iter()
        .map(|f| {
            f.path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

#[test]
fn scan_trusts_the_header_and_skips_the_rest() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    let wii = disc(b"RMCE01", true, "Mario Kart Wii");
    put(r, "wii/mk.iso", &wii);
    // a Wii disc named like a GBA game is still a Wii disc
    put(r, "liar.gba", &disc(b"GALE01", false, "Melee"));
    put(r, "a/b/c/deep.iso", &wii); // depth 3 dir: found
    put(r, "a/b/c/d/too-deep.iso", &wii);
    put(r, "wii/half.iso.part", &wii);
    put(r, ".uploads/x.iso", &wii);
    put(r, ".hidden/x.iso", &wii);
    put(r, "notes.txt", &wii);
    put(r, "zeros.iso", &[0u8; 100]); // known extension, nothing we know
    let mut bad = b"WBFS".to_vec();
    bad.resize(20, 0);
    put(r, "bad.wbfs", &bad);
    let s = scan(r);
    assert_eq!(names(&s, r), ["a/b/c/deep.iso", "liar.gba", "wii/mk.iso"]);
    let liar = &s.found[1].id;
    assert_eq!(
        (liar.system, liar.game_id.as_deref()),
        (System::GameCube, Some("GALE01"))
    );
    assert_eq!(
        (s.unknown, s.broken, s.scanned, s.truncated),
        (1, 1, 5, false)
    );
}

#[test]
fn scan_follows_no_symlinks_except_a_top_level_folder() {
    let d = tempfile::tempdir().unwrap();
    let (root, out) = (d.path().join("games"), d.path().join("out"));
    std::fs::create_dir_all(&root).unwrap();
    let wii = disc(b"RMCE01", true, "A");
    put(&out, "linked/in-top.iso", &wii);
    put(&out, "deep/in-deep.iso", &wii);
    let secret = put(&out, "file.iso", &wii);
    symlink(out.join("linked"), root.join("wii")).unwrap(); // the ~/Games/wii case
    std::fs::create_dir_all(root.join("x")).unwrap();
    symlink(out.join("deep"), root.join("x/escape")).unwrap();
    symlink(&secret, root.join("x/file.iso")).unwrap();
    symlink(&root, root.join("x/loop")).unwrap();
    let s = scan(&root);
    assert_eq!(names(&s, &root), ["wii/in-top.iso"]);
}

#[test]
fn scan_stops_at_the_entry_cap() {
    let d = tempfile::tempdir().unwrap();
    for i in 0..MAX_ENTRIES + 5 {
        std::fs::write(d.path().join(format!("{i}.txt")), b"").unwrap();
    }
    let s = scan(d.path());
    assert!(s.truncated && s.found.is_empty());
    assert!(!scan(&d.path().join("missing")).truncated);
}

fn found(path: &Path, bytes: &[u8]) -> Found {
    std::fs::write(path, bytes).unwrap();
    Found {
        path: path.into(),
        id: discid::identify(path).unwrap(),
    }
}

#[test]
fn merge_dedupes_by_id_and_path_and_aliases_old_slugs() {
    let d = tempfile::tempdir().unwrap();
    let mk = disc(b"RMCE01", true, "Mario Kart Wii");
    let a = found(&d.path().join("a.iso"), &mk);
    let mut recs = vec![];
    let m = merge(&mut recs, std::slice::from_ref(&a), "scan");
    assert_eq!(m.added.len(), 1);
    assert_eq!(recs[0].id.as_str(), "emu:dolphin:RMCE01");
    assert_eq!(
        (recs[0].name.as_str(), recs[0].system.as_deref()),
        ("Mario Kart Wii", Some("wii"))
    );
    // same file again, and the same disc at another path
    let b = found(&d.path().join("copy.iso"), &mk);
    let m = merge(&mut recs, &[a, b], "scan");
    assert_eq!((m.added.len(), m.duplicates, recs.len()), (0, 2, 1));

    // an older slug record for the same path keeps its id and gains the new one
    let p = d.path().join("old.iso");
    let mut old = record(
        &found(&p, &disc(b"GALE01", false, "Melee")).path,
        &discid::identify(&p).unwrap(),
        "manual",
    )
    .unwrap();
    old.id = GameId::emu("dolphin", "melee").unwrap();
    let mut recs = vec![old];
    let m = merge(
        &mut recs,
        &[Found {
            path: p.clone(),
            id: discid::identify(&p).unwrap(),
        }],
        "scan",
    );
    assert_eq!((m.added.len(), m.aliased, recs.len()), (0, 1, 1));
    assert_eq!(recs[0].id.as_str(), "emu:dolphin:melee");
    assert_eq!(recs[0].aliases[0].as_str(), "emu:dolphin:GALE01");
    assert_eq!(
        crate::emu::resolve(&recs, "emu:dolphin:GALE01")
            .unwrap()
            .as_str(),
        "emu:dolphin:melee"
    );
    // and a second pass changes nothing
    let m = merge(
        &mut recs,
        &[Found {
            path: p.clone(),
            id: discid::identify(&p).unwrap(),
        }],
        "scan",
    );
    assert_eq!((m.aliased, m.duplicates), (0, 1));
    assert_eq!(recs[0].aliases.len(), 1);
}

#[test]
fn same_file_through_a_symlink_is_one_game() {
    let d = tempfile::tempdir().unwrap();
    let a = found(&d.path().join("a.iso"), &disc(b"RMCE01", true, "M"));
    symlink(d.path(), d.path().join("lnk")).unwrap();
    let via = Found {
        path: d.path().join("lnk/a.iso"),
        id: a.id.clone(),
    };
    let mut recs = vec![];
    merge(&mut recs, &[a, via], "scan");
    assert_eq!(recs.len(), 1);
}

#[test]
fn cartridges_key_on_the_hash_and_have_no_emulator_yet() {
    let d = tempfile::tempdir().unwrap();
    let g = found(&d.path().join("g.gba"), &gba("MINISH"));
    let k = key(&g.id).unwrap();
    assert_eq!(k.len(), 16);
    assert!(g.id.hash.as_ref().unwrap().starts_with(&k));
    let mut recs = vec![];
    let m = merge(&mut recs, &[g], "scan");
    assert_eq!((m.unplayable, recs.len()), (1, 0));
}

fn cfg(d: &Path) -> Config {
    let mut c = Config::from_env(d.join("dist"));
    c.games_dir = d.join("Games");
    c.data_dir = d.join("data");
    c
}

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

#[test]
fn add_dry_paths() {
    let d = tempfile::tempdir().unwrap();
    let c = cfg(d.path());
    let wii = put(
        d.path(),
        "in/Mario.iso",
        &disc(b"RMCE01", true, "Mario Kart Wii"),
    );
    let code = |a: &[&str]| add_cmd(&c, &args(a));
    assert_eq!(code(&[]).0, 2);
    assert_eq!(code(&["--bogus", "x"]).0, 2);
    assert_eq!(code(&["a", "b"]).0, 2);
    assert_eq!(code(&["--move", "--copy", wii.to_str().unwrap()]).0, 2);
    assert_eq!(code(&["/nonexistent.iso"]).0, 1);
    let junk = put(d.path(), "in/junk.iso", &[0u8; 64]);
    assert!(
        code(&[junk.to_str().unwrap()])
            .1
            .contains("not a game file")
    );
    let cart = put(d.path(), "in/g.gba", &gba("X"));
    assert!(
        code(&[cart.to_str().unwrap()])
            .1
            .contains("no emulator for gba")
    );
    assert!(!c.data_dir.join("library.json").exists());

    // in place
    let (rc, msg) = code(&[wii.to_str().unwrap()]);
    assert_eq!((rc, msg.as_str()), (0, "added emu:dolphin:RMCE01"));
    assert_eq!(code(&[wii.to_str().unwrap()]).1, "already in the library");
    assert_eq!(emu::load(&c.data_dir)[0].file, wii.canonicalize().unwrap());
    assert!(wii.exists());
}

#[test]
fn add_move_copy_link_put_the_file_in_the_system_folder() {
    let d = tempfile::tempdir().unwrap();
    let c = cfg(d.path());
    let a = put(d.path(), "in/a.iso", &disc(b"RMCE01", true, "A"));
    let b = put(d.path(), "in/b.iso", &disc(b"RSPE01", true, "B"));
    let l = put(d.path(), "in/l.iso", &disc(b"GALE01", false, "L"));
    // a file already sitting where the copy would go: the copy gets a suffix
    put(d.path(), "Games/wii/a.iso", b"other");
    assert_eq!(add_cmd(&c, &args(&["--copy", a.to_str().unwrap()])).0, 0);
    assert!(a.exists());
    assert_eq!(add_cmd(&c, &args(&["--move", b.to_str().unwrap()])).0, 0);
    assert!(!b.exists() && c.games_dir.join("wii/b.iso").is_file());
    assert_eq!(add_cmd(&c, &args(&[l.to_str().unwrap(), "--link"])).0, 0);
    assert!(c.games_dir.join("gamecube/l.iso").is_symlink());
    let recs = emu::load(&c.data_dir);
    assert_eq!(recs.len(), 3);
    let copy = recs
        .iter()
        .find(|r| r.id.as_str().ends_with("RMCE01"))
        .unwrap();
    assert_eq!(copy.file, c.games_dir.join("wii/a (1).iso"));
    assert_eq!(
        std::fs::read(c.games_dir.join("wii/a.iso")).unwrap(),
        b"other"
    );
}

#[test]
fn a_damaged_library_is_never_overwritten() {
    let d = tempfile::tempdir().unwrap();
    let c = cfg(d.path());
    std::fs::create_dir_all(&c.data_dir).unwrap();
    std::fs::write(c.data_dir.join("library.json"), "{oops").unwrap();
    let a = put(d.path(), "in/a.iso", &disc(b"RMCE01", true, "A"));
    assert_eq!(add_cmd(&c, &args(&[a.to_str().unwrap()])).0, 1);
    assert_eq!(
        std::fs::read_to_string(c.data_dir.join("library.json")).unwrap(),
        "{oops"
    );
}

#[test]
fn forget_keeps_the_file_and_is_idempotent() {
    let d = tempfile::tempdir().unwrap();
    let c = cfg(d.path());
    let a = put(d.path(), "in/a.iso", &disc(b"RMCE01", true, "A"));
    add_cmd(&c, &args(&[a.to_str().unwrap()]));
    let id = GameId::parse("emu:dolphin:RMCE01").unwrap();
    assert_eq!(forget(&c.data_dir, &id), Ok(true));
    assert_eq!(forget(&c.data_dir, &id), Ok(false));
    assert!(a.exists() && emu::load(&c.data_dir).is_empty());
}

#[test]
fn removed_list_skips_scan_and_add_clears_it() {
    let d = tempfile::tempdir().unwrap();
    let c = cfg(d.path());
    let a = put(&c.games_dir, "wii/a.iso", &disc(b"RMCE01", true, "A"));
    scan_library(&c).unwrap();
    let id = emu::load(&c.data_dir)[0].id.clone();
    assert!(forget(&c.data_dir, &id).unwrap());
    let r = scan_library(&c).unwrap();
    assert_eq!((r.merged.removed, r.merged.added.len()), (1, 0));
    assert!(emu::load(&c.data_dir).is_empty());
    // the same file under another name is skipped by id
    let b = put(&c.games_dir, "wii/b.iso", &disc(b"RMCE01", true, "A"));
    std::fs::remove_file(&a).unwrap();
    assert_eq!(scan_library(&c).unwrap().merged.removed, 1);
    // the command is deliberate: it clears the entry
    assert_eq!(add_cmd(&c, &args(&[b.to_str().unwrap()])).0, 0);
    assert_eq!(emu::load(&c.data_dir).len(), 1);
    assert!(tombs_load(&c.data_dir).is_empty());
}

#[test]
fn removed_list_is_tolerant_and_bounded() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("removed.json"), "{oops").unwrap();
    assert!(tombs_load(d.path()).is_empty());
    let t: Vec<Tomb> = (0..MAX_TOMBS + 10)
        .map(|i| Tomb {
            path: format!("/x/{i}").into(),
            id: format!("emu:dolphin:{i}"),
        })
        .collect();
    tombs_save(d.path(), t);
    let l = tombs_load(d.path());
    assert_eq!(l.len(), MAX_TOMBS);
    assert_eq!(
        l.last().unwrap().path,
        PathBuf::from(format!("/x/{}", MAX_TOMBS + 9))
    );
    use std::os::unix::fs::PermissionsExt;
    let m = std::fs::metadata(d.path().join("removed.json")).unwrap();
    assert_eq!(m.permissions().mode() & 0o777, 0o600);
}

#[test]
fn move_copies_when_hard_links_are_refused_but_retries_a_name_clash() {
    let d = tempfile::tempdir().unwrap();
    let src = put(d.path(), "in/a.iso", b"data");
    let dir = d.path().join("out");
    let eperm = |_: &Path, _: &Path| Err(io::Error::from_raw_os_error(libc::EPERM));
    let p = place_with(&src, &dir, "a.iso", Mode::Move, &eperm).unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), b"data");
    assert!(!src.exists());
    // a clash stays a retry with a suffix
    let src2 = put(d.path(), "in/b.iso", b"two");
    let p2 = place(&src2, &dir, "a.iso", Mode::Move).unwrap();
    assert_eq!(p2, dir.join("a (1).iso"));
    assert_eq!(std::fs::read(&p).unwrap(), b"data");
}

#[test]
fn a_big_rescan_canonicalizes_each_path_once() {
    use std::cell::Cell;
    let n = 3000;
    let calls = Cell::new(0usize);
    let f = |p: &Path| {
        calls.set(calls.get() + 1);
        Some(p.to_path_buf())
    };
    let mk = || -> Vec<Found> {
        (0..n)
            .map(|i| Found {
                path: format!("/g/wii/{i}.iso").into(),
                id: Identity {
                    system: System::Wii,
                    game_id: Some(format!("A{i:05}")),
                    title: Some(format!("T{i}")),
                    region: None,
                    container: Container::Iso,
                    hash: None,
                },
            })
            .collect()
    };
    let d = tempfile::tempdir().unwrap();
    let tombs: Vec<Tomb> = (0..n)
        .map(|i| Tomb {
            path: format!("/old/{i}.iso").into(),
            id: format!("emu:dolphin:Z{i:05}"),
        })
        .collect();
    tombs_save(d.path(), tombs);
    let mut recs = vec![];
    let mut canon = Canon::new(&f);
    let m = register_with(d.path(), &mut recs, mk(), "scan", true, &mut canon);
    assert_eq!(m.added.len(), n);
    // a second pass: every file is known now
    let m = register_with(d.path(), &mut recs, mk(), "scan", true, &mut canon);
    assert_eq!((m.added.len(), m.duplicates), (0, n));
    assert!(calls.get() <= 2 * n, "{} canonicalize calls", calls.get());
}

#[test]
fn unparseable_records_are_backed_up_before_the_first_rewrite() {
    let d = tempfile::tempdir().unwrap();
    let text = r#"{"v":1,"games":[{"id":"emu:dolphin:AAAAAA","emulator":"dolphin","file":"/x/a.iso","name":"A","source":"scan"},{"from":"a newer version"}]}"#;
    std::fs::write(d.path().join("library.json"), text).unwrap();
    assert_eq!(load_strict(d.path()).unwrap().len(), 1);
    update(d.path(), |_| (true, ())).unwrap();
    let bads: Vec<_> = std::fs::read_dir(d.path())
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("library.json.bad-")
        })
        .collect();
    assert_eq!(bads.len(), 1);
    assert_eq!(std::fs::read_to_string(bads[0].path()).unwrap(), text);
    // the rewrite dropped the odd record, and a second rewrite makes no second copy
    assert_eq!(raw_count(d.path()), 1);
    update(d.path(), |_| (true, ())).unwrap();
    assert_eq!(
        std::fs::read_dir(d.path())
            .unwrap()
            .flatten()
            .filter(|e| e
                .file_name()
                .to_string_lossy()
                .starts_with("library.json.bad-"))
            .count(),
        1
    );
}

#[test]
fn a_damaged_removed_list_is_set_aside_and_the_scan_refuses() {
    let d = tempfile::tempdir().unwrap();
    let c = cfg(d.path());
    put(&c.games_dir, "wii/a.iso", &disc(b"RMCE01", true, "A"));
    std::fs::create_dir_all(&c.data_dir).unwrap();
    std::fs::write(c.data_dir.join("removed.json"), "{oops").unwrap();
    let e = scan_library(&c).err().unwrap();
    assert!(e.contains("removed.json"), "{e}");
    assert!(emu::load(&c.data_dir).is_empty());
    assert!(!c.data_dir.join("removed.json").exists());
    assert!(std::fs::read_dir(&c.data_dir).unwrap().flatten().any(|e| {
        e.file_name()
            .to_string_lossy()
            .starts_with("removed.json.bad-")
    }));
    // an explicit "include removed" scan does not need the list
    std::fs::write(c.data_dir.join("removed.json"), "{oops").unwrap();
    assert_eq!(scan_library_with(&c, true).unwrap().merged.added.len(), 1);
}

#[test]
fn a_3ds_file_in_the_scan_gets_an_azahar_record_named_after_the_file() {
    let d = tempfile::tempdir().unwrap();
    let mut h = vec![0u8; 0x200];
    h[0x100..0x104].copy_from_slice(b"NCCH");
    h[0x118..0x120].copy_from_slice(&0x0004_0000_0012_3400u64.to_le_bytes());
    let p = d.path().join("3ds");
    std::fs::create_dir(&p).unwrap();
    std::fs::write(p.join("Some Game.cxi"), &h).unwrap();
    let s = scan(d.path());
    assert_eq!(s.found.len(), 1);
    let r = record(&s.found[0].path, &s.found[0].id, "scan").unwrap();
    assert_eq!(r.id.as_str(), "emu:azahar:0004000000123400");
    assert_eq!(
        (r.emulator.as_str(), r.system.as_deref(), r.name.as_str()),
        ("azahar", Some("3ds"), "Some Game")
    );
}
