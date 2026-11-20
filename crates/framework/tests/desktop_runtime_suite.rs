#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{collections::BTreeMap, path::Path};

use domain::hardware::ModelArtifactSpec;
use framework::{
    config::{AppConfig, DesktopLauncherEnv},
    desktop::{
        discord_verify::verify_discord_bot_token_at,
        dotenv::{parse_dotenv_text, upsert_dotenv_file},
        models::{inspect_models_status, pull_missing_models},
        paths::resolve_desktop_paths,
        probe::parse_nvidia_smi_csv,
    },
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method, path},
};

#[test]
fn standalone_desktop_mode_boots_without_discord_and_detects_when_configured() {
    let vars = BTreeMap::from([
        ("DESKTOP_STANDALONE_MODE".to_owned(), "true".to_owned()),
        (
            "DATABASE_URL".to_owned(),
            "postgres://voice_pipe:local@127.0.0.1:55432/voice_pipe".to_owned(),
        ),
        (
            "WEB_DIST_DIR".to_owned(),
            "C:\\Program Files\\VoiceCloneStudio\\web\\dist".to_owned(),
        ),
        ("WHISPER_USE_GPU".to_owned(), "false".to_owned()),
    ]);

    let cfg = AppConfig::from_map(&vars).unwrap();
    assert!(cfg.desktop.standalone_mode);
    assert!(cfg.desktop.auto_migrate);
    assert!(!cfg.whisper_use_gpu);
    assert!(!cfg.is_discord_configured());
    assert_eq!(cfg.discord_oauth_urls(), None);
    assert_eq!(
        cfg.desktop.web_dist_dir.as_deref(),
        Some(Path::new("C:\\Program Files\\VoiceCloneStudio\\web\\dist"))
    );

    let mut with_discord = vars;
    with_discord.insert(
        "DISCORD_APPLICATION_ID".to_owned(),
        "111111111111111111".to_owned(),
    );
    with_discord.insert(
        "DISCORD_OWNER_USER_ID".to_owned(),
        "222222222222222222".to_owned(),
    );
    with_discord.insert(
        "DISCORD_BOT_TOKEN".to_owned(),
        "MTIzNDU2Nzg5MDEyMzQ1Njc4.real-token-shape".to_owned(),
    );

    let configured = AppConfig::from_map(&with_discord).unwrap();
    assert!(configured.is_discord_configured());
    let (user_install, bot_dm) = configured.discord_oauth_urls().unwrap();
    assert!(user_install.contains("client_id=111111111111111111"));
    assert!(user_install.contains("integration_type=1"));
    assert!(bot_dm.contains("integration_type=0"));
}

#[test]
fn resolves_windows_appdata_and_portable_bundle_paths_deterministically() {
    let env_appdata = DesktopLauncherEnv::from_map(&BTreeMap::from([(
        "LOCALAPPDATA".to_owned(),
        "C:\\Users\\Jane Doe\\AppData\\Local".to_owned(),
    )]));
    let bundle_root = Path::new("C:\\Program Files\\VoiceCloneStudio");

    let standard = resolve_desktop_paths(bundle_root, &env_appdata, false);
    assert!(
        standard
            .data_root
            .to_string_lossy()
            .contains("VoiceCloneStudio")
    );
    assert_eq!(standard.web_dist_dir, bundle_root.join("web").join("dist"));

    let portable = resolve_desktop_paths(bundle_root, &env_appdata, true);
    assert_eq!(portable.data_root, bundle_root.join("data"));
    assert_eq!(
        portable.env_file,
        bundle_root.join("data").join("config").join("studio.env")
    );
}

#[test]
fn dotenv_upsert_round_trips_windows_paths_with_spaces_without_backslash_corruption() {
    let tmp_dir = std::env::temp_dir().join(format!("vcp-dotenv-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp_dir).unwrap();
    let env_path = tmp_dir.join("studio.env");
    std::fs::write(
        &env_path,
        "# Studio config\nAPI_BIND_ADDR='127.0.0.1:8080'\nDISCORD_BOT_TOKEN=\"unconfigured\"\n",
    )
    .unwrap();

    let win_space_path =
        "C:\\Users\\Jane Doe\\AppData\\Local\\Programs\\VoiceCloneStudio\\web\\dist";
    let updates = BTreeMap::from([
        ("WEB_DIST_DIR".to_owned(), win_space_path.to_owned()),
        (
            "DISCORD_BOT_TOKEN".to_owned(),
            "real-bot-token-xyz".to_owned(),
        ),
    ]);
    upsert_dotenv_file(&env_path, &updates).unwrap();

    let pass1 = parse_dotenv_text(&std::fs::read_to_string(&env_path).unwrap());
    assert_eq!(
        pass1.get("WEB_DIST_DIR").map(String::as_str),
        Some(win_space_path)
    );

    // Re-upsert from parsed map to prove zero backslash doubling across multiple saves
    upsert_dotenv_file(&env_path, &pass1).unwrap();
    let pass2 = parse_dotenv_text(&std::fs::read_to_string(&env_path).unwrap());
    assert_eq!(
        pass2.get("WEB_DIST_DIR").map(String::as_str),
        Some(win_space_path)
    );
    assert_eq!(
        pass2.get("API_BIND_ADDR").map(String::as_str),
        Some("127.0.0.1:8080")
    );
    let _ = std::fs::remove_dir_all(&tmp_dir);

    let csv = "0, NVIDIA GeForce RTX 3070, 8192, 7412, 560.94, GPU-aaa\n1, NVIDIA GeForce RTX 3090, 24576, 23800, 560.94, GPU-bbb\n";
    let gpus = parse_nvidia_smi_csv(csv);
    assert_eq!(gpus.len(), 2);
    assert_eq!(gpus[1].vram_total_mib, 24_576);
    assert_eq!(gpus[1].uuid.as_deref(), Some("GPU-bbb"));
}

#[tokio::test]
async fn resumable_model_puller_handles_206_and_416_and_verifies_discord_token() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/models/vad.onnx"))
        .and(header("range", "bytes=5-"))
        .respond_with(ResponseTemplate::new(206).set_body_bytes(b"WORLD_ONNX".as_slice()))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/models/stale.bin"))
        .and(header("range", "bytes=9-"))
        .respond_with(ResponseTemplate::new(416))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/models/stale.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"FRESH_FULL".as_slice()))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v10/users/@me"))
        .and(header("authorization", "Bot valid-token-123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "111111111111111111",
            "username": "Translation"
        })))
        .mount(&mock_server)
        .await;

    let tmp_models = std::env::temp_dir().join(format!("vcp-models-test-{}", uuid::Uuid::new_v4()));
    let vad_dir = tmp_models.join("vad");
    tokio::fs::create_dir_all(&vad_dir).await.unwrap();
    tokio::fs::write(vad_dir.join("silero_vad.onnx.part"), b"HELLO")
        .await
        .unwrap();
    tokio::fs::write(vad_dir.join("stale.bin.part"), b"OLD9BYTES")
        .await
        .unwrap();

    let artifacts = vec![
        ModelArtifactSpec {
            component: "silero_vad".to_owned(),
            relative_path: "vad/silero_vad.onnx".to_owned(),
            download_url: format!("{}/models/vad.onnx", mock_server.uri()),
            approx_size_mib: 1,
            required: true,
        },
        ModelArtifactSpec {
            component: "stale_check".to_owned(),
            relative_path: "vad/stale.bin".to_owned(),
            download_url: format!("{}/models/stale.bin", mock_server.uri()),
            approx_size_mib: 1,
            required: true,
        },
    ];

    let http = reqwest::Client::new();
    let pull_report = pull_missing_models(&http, &tmp_models, &artifacts, None)
        .await
        .unwrap();
    assert_eq!(pull_report.downloaded_files, 2);
    assert_eq!(
        tokio::fs::read(vad_dir.join("silero_vad.onnx"))
            .await
            .unwrap(),
        b"HELLOWORLD_ONNX"
    );
    assert_eq!(
        tokio::fs::read(vad_dir.join("stale.bin")).await.unwrap(),
        b"FRESH_FULL"
    );
    assert!(inspect_models_status(&tmp_models, &artifacts).all_required_present);

    let bot = verify_discord_bot_token_at(&http, &mock_server.uri(), "valid-token-123")
        .await
        .unwrap();
    assert_eq!(bot.username, "Translation");

    let _ = tokio::fs::remove_dir_all(&tmp_models).await;
}
