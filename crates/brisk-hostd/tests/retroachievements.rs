use brisk_hostd::retroachievements::*;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const KEY: &str = "SECRETKEY123";
const FX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ra");
const HASH: &str = "aaaa1111aaaa1111aaaa1111aaaa1111";

fn fx(name: &str) -> String {
    std::fs::read_to_string(format!("{FX}/{name}")).unwrap()
}

type Log = Arc<Mutex<Vec<String>>>;

/// Mock RA: first route whose substring is in the request line answers (status, body bytes).
async fn mock(routes: Vec<(&'static str, u16, Vec<u8>)>) -> (String, Log) {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let log: Log = Arc::default();
    let lg = log.clone();
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        loop {
            let (mut c, _) = l.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let n = c.read(&mut buf).await.unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).to_string();
            let line = head.lines().next().unwrap_or("").to_string();
            lg.lock().unwrap().push(line.clone());
            let (code, body) = match routes.iter().find(|(p, _, _)| line.contains(p)) {
                Some(_) if !line.contains(&format!("y={KEY}")) => (401, vec![]),
                Some((_, c, b)) => (*c, b.clone()),
                None => (404, vec![]),
            };
            let h = format!(
                "HTTP/1.1 {code} X\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            let _ = c.write_all(h.as_bytes()).await;
            let _ = c.write_all(&body).await;
        }
    });
    (base, log)
}

struct T {
    _d: tempfile::TempDir,
    cfg: RaConfig,
}

fn setup(base: &str, creds: Option<&str>) -> T {
    let d = tempfile::tempdir().unwrap();
    let key_file = d.path().join("retroachievements");
    if let Some(c) = creds {
        std::fs::write(&key_file, c).unwrap();
    }
    let cfg = RaConfig {
        data_dir: d.path().join("data"),
        key_file,
        base: base.into(),
        hash_ttl: Duration::from_secs(3600),
        progress_ttl: Duration::from_secs(3600),
        min_interval: Duration::ZERO,
    };
    T { _d: d, cfg }
}

fn ok_routes() -> Vec<(&'static str, u16, Vec<u8>)> {
    vec![
        ("API_GetGameList.php", 200, fx("gamelist.json").into_bytes()),
        (
            "API_GetGameInfoAndUserProgress.php",
            200,
            fx("progress-38107.json").into_bytes(),
        ),
    ]
}

fn good() -> Option<&'static str> {
    Some("toby_ra\nSECRETKEY123\n")
}

#[tokio::test]
async fn happy_path() {
    let (base, log) = mock(ok_routes()).await;
    let t = setup(&base, good());
    let p = progress_for(&t.cfg, CONSOLE_WII, "AAAA1111AAAA1111aaaa1111aaaa1111")
        .await
        .unwrap();
    assert_eq!(
        (p.game_id, p.title.as_str(), p.console_id),
        (38107, "Mario Kart Wii", 19)
    );
    assert_eq!(
        (p.num_achievements, p.num_awarded, p.num_awarded_hardcore),
        (2, 1, 1)
    );
    // sorted by display order; unlocked time present or none
    assert_eq!(p.achievements[0].title, "First");
    assert_eq!(
        p.achievements[0].unlocked.as_deref(),
        Some("2026-03-12 17:47:29")
    );
    assert_eq!(
        p.achievements[0].unlocked_hardcore.as_deref(),
        Some("2026-03-12 17:47:29")
    );
    assert_eq!(p.achievements[0].badge_name, "250336");
    assert_eq!(p.achievements[1].unlocked, None);
    assert_eq!(p.achievements[1].points, 5);
    let l = log.lock().unwrap();
    assert!(l[0].contains("i=19&h=1"), "{l:?}");
    assert!(l[1].contains("u=toby_ra&g=38107"), "{l:?}");
}

#[tokio::test]
async fn missing_or_bad_credentials() {
    let (base, log) = mock(ok_routes()).await;
    let t = setup(&base, None);
    assert_eq!(progress_for(&t.cfg, 19, HASH).await, Err(RaError::Off));
    for bad in ["", "onlyuser\n", "u\nk&evil=1\n", "u v\nkey\n"] {
        let t = setup(&base, Some(bad));
        assert_eq!(
            progress_for(&t.cfg, 19, HASH).await,
            Err(RaError::BadCredentials),
            "{bad:?}"
        );
    }
    assert!(log.lock().unwrap().is_empty());
}

#[tokio::test]
async fn hash_not_in_list_and_list_cached() {
    let (base, log) = mock(ok_routes()).await;
    let t = setup(&base, good());
    assert_eq!(
        resolve_hash(&t.cfg, 19, "ffff").await,
        Err(RaError::NotFound)
    );
    assert_eq!(
        resolve_hash(&t.cfg, 19, "bbbb2222bbbb2222bbbb2222bbbb2222").await,
        Ok(38107)
    );
    assert_eq!(log.lock().unwrap().len(), 1, "list is downloaded once");
    let f = t.cfg.data_dir.join("ra/hashes-19.json");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(f).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[tokio::test]
async fn http_errors_are_typed_and_never_leak_the_key() {
    let cases = [
        (401, RaError::Unauthorized),
        (403, RaError::Unauthorized),
        (429, RaError::RateLimited),
        (500, RaError::Network("HTTP 500".into())),
    ];
    for (code, want) in cases {
        let (base, log) = mock(vec![("API_", code, b"{\"key\":\"SECRETKEY123\"}".to_vec())]).await;
        let t = setup(&base, good());
        let e = progress_for(&t.cfg, 19, HASH).await.unwrap_err();
        assert_eq!(e, want);
        assert_eq!(log.lock().unwrap().len(), 1, "no retries");
        assert!(!format!("{e} {e:?}").contains(KEY));
    }
}

#[tokio::test]
async fn malformed_and_huge_responses() {
    let (base, _) = mock(vec![("API_", 200, b"<html>nope</html>".to_vec())]).await;
    let t = setup(&base, good());
    let e = progress_for(&t.cfg, 19, HASH).await.unwrap_err();
    assert_eq!(e, RaError::BadResponse);
    assert!(!format!("{e:?}").contains(KEY));

    let (base, _) = mock(vec![("API_", 200, vec![b' '; 9 << 20])]).await;
    let t = setup(&base, good());
    assert_eq!(progress_for(&t.cfg, 19, HASH).await, Err(RaError::TooLarge));
}

#[tokio::test]
async fn unreachable_server_is_a_network_error() {
    let t = setup("http://127.0.0.1:1", good());
    let e = progress_for(&t.cfg, 19, HASH).await.unwrap_err();
    assert!(matches!(e, RaError::Network(_)), "{e:?}");
    assert!(!format!("{e} {e:?}").contains(KEY));
}

#[tokio::test]
async fn progress_cache_hit_and_expiry() {
    let (base, log) = mock(ok_routes()).await;
    let mut t = setup(&base, good());
    let a = game_progress(&t.cfg, 38107).await.unwrap();
    let b = game_progress(&t.cfg, 38107).await.unwrap();
    assert_eq!(a, b);
    assert_eq!(log.lock().unwrap().len(), 1);
    // the cache holds no credentials
    let text = std::fs::read_to_string(t.cfg.data_dir.join("ra/game-38107.json")).unwrap();
    assert!(!text.contains(KEY));
    t.cfg.progress_ttl = Duration::ZERO;
    game_progress(&t.cfg, 38107).await.unwrap();
    assert_eq!(log.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn cache_write_failure_still_answers_and_leaves_no_temp() {
    let (base, log) = mock(ok_routes()).await;
    let t = setup(&base, good());
    // a directory where the cache file goes makes the final rename fail
    let dest = t.cfg.data_dir.join("ra/game-38107.json");
    std::fs::create_dir_all(&dest).unwrap();
    let a = game_progress(&t.cfg, 38107).await.unwrap();
    assert_eq!(game_progress(&t.cfg, 38107).await.unwrap(), a);
    assert_eq!(log.lock().unwrap().len(), 2, "nothing was cached");
    let names: Vec<_> = std::fs::read_dir(t.cfg.data_dir.join("ra"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["game-38107.json"]);
}

#[tokio::test]
async fn min_interval_spaces_requests() {
    let (base, _) = mock(ok_routes()).await;
    let mut t = setup(&base, good());
    t.cfg.min_interval = Duration::from_millis(300);
    let s = std::time::Instant::now();
    progress_for(&t.cfg, 19, HASH).await.unwrap(); // two requests
    assert!(
        s.elapsed() >= Duration::from_millis(300),
        "{:?}",
        s.elapsed()
    );
}

#[test]
fn debug_of_config_has_no_key() {
    let t = setup("http://127.0.0.1:1", good());
    assert!(!format!("{:?}", t.cfg).contains(KEY));
}
