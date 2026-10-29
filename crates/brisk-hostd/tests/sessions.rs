//! Session history over HTTP: one session's record + series, and the series PUT.
mod common;
use axum::http::StatusCode;
use brisk_hostd::http::{AppState, router};
use brisk_hostd::sessions::{MAX_POINTS, MAX_SERIES};
use common::{call, data_cfg};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

struct T {
    app: axum::Router,
    dir: tempfile::TempDir,
}

fn rec(started: u64) -> String {
    format!(
        r#"{{"appid":10,"name":"g{started}","started":{started},"ended":{},"host":"bawn","preset":"fast"}}"#,
        started + 60_000
    )
}

/// hostd over a data dir holding sessions `ids` (old-style records: no codec/stats).
fn t(ids: impl IntoIterator<Item = u64>) -> T {
    let dir = tempfile::tempdir().unwrap();
    let lines: String = ids.into_iter().map(|i| rec(i) + "\n").collect();
    std::fs::write(dir.path().join("sessions.jsonl"), lines).unwrap();
    T {
        app: router(Arc::new(AppState::new(data_cfg(dir.path())))),
        dir,
    }
}

impl T {
    async fn raw(&self, m: &str, u: &str, secret: bool, body: String) -> (StatusCode, Value) {
        call(&self.app, m, u, secret, Some(body)).await
    }
    async fn put(&self, id: u64, v: Value) -> StatusCode {
        self.raw(
            "PUT",
            &format!("/api/sessions/{id}/series"),
            true,
            v.to_string(),
        )
        .await
        .0
    }
    async fn get(&self, id: u64) -> (StatusCode, Value) {
        self.raw("GET", &format!("/api/sessions/{id}"), true, String::new())
            .await
    }
    fn file(&self, id: u64) -> std::path::PathBuf {
        self.dir.path().join(format!("sessions/{id}.series.json"))
    }
}

#[tokio::test]
async fn put_then_get_merges_by_t() {
    let t = t([1000]);
    let (st, v) = t.get(1000).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["session"]["name"], "g1000");
    assert_eq!(v["series"], json!([]), "no series yet");
    let pts = json!({"points": [[0, 20.5, 60, 18.2], [5, 22, 59.5, 19]]});
    assert_eq!(t.put(1000, pts).await, StatusCode::NO_CONTENT);
    // the partial last bucket comes again, complete; a new one is appended; out of order is fine
    let pts = json!({"points": [[10, 30, 58, 20], [5, 25, 60, 20]]});
    assert_eq!(t.put(1000, pts).await, StatusCode::NO_CONTENT);
    let (_, v) = t.get(1000).await;
    assert_eq!(
        v["series"],
        json!([
            [0.0, 20.5, 60.0, 18.2],
            [5.0, 25.0, 60.0, 20.0],
            [10.0, 30.0, 58.0, 20.0]
        ])
    );
    let mode = std::fs::metadata(t.file(1000))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    // the list never carries series, only a sparkline of the delay
    let (_, l) = t.raw("GET", "/api/sessions", true, String::new()).await;
    assert!(l["sessions"][0].get("series").is_none(), "{l}");
    assert_eq!(l["sessions"][0]["spark"], json!([20.5, 25.0, 30.0]));
}

/// The list's sparkline: at most 24 mean-binned delays; none without a series.
#[tokio::test]
async fn list_rows_carry_a_24_point_spark() {
    let t = t([1000, 2000]);
    let pts: Vec<[f64; 4]> = (0..120)
        .map(|i| [i as f64 * 5.0, i as f64, 60.0, 1.0])
        .collect();
    assert_eq!(
        t.put(1000, json!({ "points": pts })).await,
        StatusCode::NO_CONTENT
    );
    let (_, l) = t.raw("GET", "/api/sessions", true, String::new()).await;
    let rows = l["sessions"].as_array().unwrap();
    assert_eq!(rows[0]["started"], 2000);
    assert_eq!(rows[0]["spark"], json!([]));
    let spark: Vec<f64> = serde_json::from_value(rows[1]["spark"].clone()).unwrap();
    assert_eq!(spark.len(), 24);
    assert_eq!((spark[0], spark[23]), (2.0, 117.0)); // means of 0..=4 and 115..=119
    // GET of one session is unchanged: no spark there
    assert!(t.get(1000).await.1["session"].get("spark").is_none());
}

/// Concurrent PUTs to one session all land (read-merge-write is serialized).
#[tokio::test]
async fn concurrent_puts_all_merge() {
    let t = Arc::new(t([1000]));
    let jobs: Vec<_> = (0..16)
        .map(|i| {
            let t = t.clone();
            tokio::spawn(async move {
                let pts = json!({"points": [[i as f64 * 5.0, 1.0, 60.0, 1.0]]});
                t.put(1000, pts).await
            })
        })
        .collect();
    for j in jobs {
        assert_eq!(j.await.unwrap(), StatusCode::NO_CONTENT);
    }
    assert_eq!(t.get(1000).await.1["series"].as_array().unwrap().len(), 16);
}

#[tokio::test]
async fn put_validation() {
    let t = t([1000]);
    let bad = [
        json!({"points": [[0, null, 60, 1]]}), // JSON's NaN
        json!({"points": [[0, 1, 60]]}),
        json!({"points": [[0, 1, 60, 1, 5]]}),
        json!({"points": [[-5, 1, 60, 1]]}),
        json!({"points": [[0, 1e300, 60, 1]]}),
        json!({"points": [[0, 1, 60, "x"]]}),
        json!({"pts": []}),
        json!([]),
    ];
    for b in bad {
        assert_eq!(t.put(1000, b.clone()).await, StatusCode::BAD_REQUEST, "{b}");
    }
    let huge = r#"{"points":[[0,1e400,60,1]]}"#.to_string();
    let (st, _) = t.raw("PUT", "/api/sessions/1000/series", true, huge).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    // too many points in one request, and too many once merged
    let many = |from: usize, n: usize| json!({"points": (from..from + n).map(|i| [i as f64 * 5.0, 1.0, 60.0, 1.0]).collect::<Vec<_>>()});
    assert_eq!(
        t.put(1000, many(0, MAX_POINTS + 1)).await,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        t.put(1000, many(0, MAX_POINTS - 1)).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        t.put(1000, many(MAX_POINTS, 2)).await,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        t.get(1000).await.1["series"].as_array().unwrap().len(),
        MAX_POINTS - 1
    );
    // over 1 MiB of body
    let pad = format!(r#"{{"points":[],"x":"{}"}}"#, "a".repeat(1 << 20));
    let (st, _) = t.raw("PUT", "/api/sessions/1000/series", true, pad).await;
    assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE);
    // a session hostd never recorded: no file is made for it
    assert_eq!(t.put(77, many(0, 1)).await, StatusCode::NOT_FOUND);
    assert!(!t.file(77).exists());
    assert_eq!(t.get(77).await.0, StatusCode::NOT_FOUND);
    let (st, _) = t.raw("GET", "/api/sessions/abc", true, String::new()).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn needs_the_secret() {
    let t = t([1000]);
    let (st, _) = t
        .raw("GET", "/api/sessions/1000", false, String::new())
        .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let body = json!({"points": [[0, 1, 60, 1]]}).to_string();
    let (st, _) = t.raw("PUT", "/api/sessions/1000/series", false, body).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    assert!(!t.file(1000).exists());
}

#[tokio::test]
async fn keeps_series_of_the_newest_sessions_only() {
    let ids: Vec<u64> = (1..=MAX_SERIES as u64 + 3).map(|i| i * 1000).collect();
    let t = t(ids.clone());
    for &id in &ids {
        assert_eq!(
            t.put(id, json!({"points": [[0, 1, 60, 1]]})).await,
            StatusCode::NO_CONTENT
        );
    }
    let n = std::fs::read_dir(t.dir.path().join("sessions"))
        .unwrap()
        .count();
    assert_eq!(n, MAX_SERIES);
    for id in &ids[..3] {
        assert!(!t.file(*id).exists(), "{id} evicted");
        assert_eq!(t.get(*id).await.1["series"], json!([]), "record stays");
    }
    assert!(t.file(ids[3]).exists());
    // a crashed write's temp file is swept by the next PUT
    let tmp = t.dir.path().join("sessions/.9000.123.tmp");
    std::fs::write(&tmp, "[").unwrap();
    // a corrupt file reads as empty and is replaced by the next PUT
    std::fs::write(t.file(ids[5]), "{oops").unwrap();
    assert_eq!(t.get(ids[5]).await.1["series"], json!([]));
    assert_eq!(
        t.put(ids[5], json!({"points": [[5, 2, 60, 1]]})).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        t.get(ids[5]).await.1["series"],
        json!([[5.0, 2.0, 60.0, 1.0]])
    );
    assert!(!tmp.exists(), "leftover temp file swept");
}
