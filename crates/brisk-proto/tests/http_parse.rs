use brisk_proto::http::*;

const SERVERINFO: &str = r#"<?xml version="1.0" encoding="utf-8"?><root status_code="200"><hostname>bawn</hostname><appversion>7.1.431.-1</appversion><GfeVersion>3.23.0.74</GfeVersion><uniqueid>0123456789ABCDEF</uniqueid><HttpsPort>47984</HttpsPort><ExternalPort>47989</ExternalPort><MaxLumaPixelsHEVC>1869449984</MaxLumaPixelsHEVC><mac>00:00:00:00:00:00</mac><LocalIP>192.168.1.5</LocalIP><ServerCodecModeSupport>259</ServerCodecModeSupport><PairStatus>PS</PairStatus><currentgame>0</currentgame><state>SUNSHINE_SERVER_FREE</state></root>"#;

#[test]
fn parses_serverinfo() {
    let s = parse_serverinfo(&SERVERINFO.replace("PS", "0")).unwrap();
    assert_eq!(s.app_version, "7.1.431.-1");
    assert_eq!(s.gfe_version, "3.23.0.74");
    assert_eq!(s.unique_id, "0123456789ABCDEF");
    assert_eq!(
        (s.https_port, s.current_game, s.codec_mode_support),
        (47984, 0, 259)
    );
    assert_eq!(s.server_state, "SUNSHINE_SERVER_FREE");
    assert!(!s.paired);
    assert!(
        parse_serverinfo(&SERVERINFO.replace("PS", "1"))
            .unwrap()
            .paired
    );
    assert!(parse_serverinfo(&SERVERINFO.replace("PS", "2")).is_err());
}

#[test]
fn applist_two_apps() {
    let x = r#"<?xml version="1.0" encoding="utf-8"?><root status_code="200"><App><IsHdrSupported>0</IsHdrSupported><AppTitle>Desktop &amp; Co</AppTitle><ID>1</ID></App><App><IsHdrSupported>0</IsHdrSupported><AppTitle>Steam Big Picture</AppTitle><ID>1093255277</ID></App></root>"#;
    let a = parse_applist(x).unwrap();
    assert_eq!(
        a,
        vec![
            App {
                id: 1,
                title: "Desktop & Co".into()
            },
            App {
                id: 1093255277,
                title: "Steam Big Picture".into()
            }
        ]
    );
    assert!(
        parse_applist(r#"<root status_code="200"/>"#)
            .unwrap()
            .is_empty()
    );
    assert!(parse_applist(r#"<root status_code="200"><App><ID>1</ID></App></root>"#).is_err());
}

#[test]
fn launch_extracts_rtsp_url() {
    let x = r#"<?xml version="1.0" encoding="utf-8"?><root status_code="200"><sessionUrl0>rtsp://192.168.1.5:48010</sessionUrl0><gamesession>1</gamesession></root>"#;
    let r = parse_launch(x).unwrap();
    assert_eq!(
        r,
        LaunchReply {
            session_url: "rtsp://192.168.1.5:48010".into(),
            game_session: 1
        }
    );
    assert!(parse_launch(&x.replace("rtsp://", "http://")).is_err());
    assert!(parse_launch(&x.replace("rtsp://", "rtspenc://")).is_ok());
}

#[test]
fn status_401_is_error_with_message() {
    let x = r#"<root status_code="401" status_message="The client is not authorized. Certificate verification failed."/>"#;
    let e = check_status(x).unwrap_err();
    assert_eq!(
        e,
        HttpError::Status {
            code: 401,
            message: "The client is not authorized. Certificate verification failed.".into()
        }
    );
    assert!(e.to_string().contains("not authorized"));
    let busy = r#"<root><resume>0</resume></root>"#;
    assert!(check_status(busy).is_err());
    assert!(check_status(r#"<root><cancel>1</cancel></root>"#.replace("<root>", "<root status_code=\"200\">").as_str()).is_ok());
    assert!(parse_launch(r#"<root status_code="400" status_message="An app is already running on this host"><resume>0</resume></root>"#).is_err());
}

#[test]
fn truncated_xml_errors_not_panics() {
    for cut in 0..SERVERINFO.len() {
        if SERVERINFO.is_char_boundary(cut) {
            let _ = parse_serverinfo(&SERVERINFO[..cut]);
            let _ = check_status(&SERVERINFO[..cut]);
        }
    }
    assert!(check_status(&SERVERINFO[..SERVERINFO.len() - 10]).is_err());
    assert!(check_status("").is_err());
    assert!(check_status("garbage").is_err());
    assert!(check_status("<root status_code=\"200\"><a></b></root>").is_err());
}

#[test]
fn oversized_body_rejected() {
    let big = format!(
        r#"<root status_code="200"><x>{}</x></root>"#,
        "a".repeat(64 * 1024)
    );
    assert_eq!(check_status(&big), Err(HttpError::TooLarge));
    let ok = format!(
        r#"<root status_code="200"><x>{}</x></root>"#,
        "a".repeat(64 * 1024 - 40)
    );
    assert!(check_status(&ok).is_ok());
}

#[test]
fn launch_query_has_required_params() {
    let q = launch_query(
        "UID",
        7,
        1920,
        1080,
        60,
        [0xAB; 16],
        42,
        &LaunchOpts::default(),
    );
    let g = |k: &str| {
        q.iter()
            .find(|(a, _)| a == k)
            .map(|(_, v)| v.as_str())
            .unwrap()
    };
    assert_eq!(g("mode"), "1920x1080x60");
    assert_eq!(g("rikey"), "AB".repeat(16));
    assert_eq!(
        (
            g("rikeyid"),
            g("appid"),
            g("localAudioPlayMode"),
            g("uniqueid")
        ),
        ("42", "7", "0", "UID")
    );
    assert_eq!((g("surroundAudioInfo"), g("corever")), ("196610", "1"));
    // Pad 0 attached, as moonlight sends its attached-pad mask. Sunshine 7.1.431 only exports it
    // to the app (SUNSHINE_CLIENT_GCMAP); pads attach on their multi-controller packets.
    assert_eq!(g("gcmap"), "1");
}

#[test]
fn pair_reply_fields() {
    let x = r#"<?xml version="1.0" encoding="utf-8"?><root status_code="200"><paired>1</paired><plaincert>2D2D</plaincert></root>"#;
    assert_eq!(parse_field(x, "plaincert").unwrap(), "2D2D");
    assert_eq!(parse_field(x, "paired").unwrap(), "1");
    assert_eq!(
        parse_field(x, "pairingsecret"),
        Err(HttpError::BadField("pairingsecret"))
    );
    let fail =
        r#"<root status_code="400" status_message="Pairing failed"><paired>0</paired></root>"#;
    assert!(matches!(
        parse_field(fail, "paired"),
        Err(HttpError::Status { code: 400, .. })
    ));
}

#[test]
fn codec_support_from_server_codec_mode_support() {
    use brisk_proto::sdp::Codec;
    let s = parse_serverinfo(&SERVERINFO.replace("PS", "0")).unwrap(); // 259 = H.264 | HEVC | HEVC Main10
    assert!(s.supports(Codec::H264) && s.supports(Codec::Hevc));
    let no_hevc = SERVERINFO.replace("PS", "0").replace(">259<", ">1<");
    let s = parse_serverinfo(&no_hevc).unwrap();
    assert!(s.supports(Codec::H264) && !s.supports(Codec::Hevc));
}
