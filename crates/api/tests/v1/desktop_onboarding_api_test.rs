//! Layer-5 black-box `HTTP` tests for embedded Studio `SPA` serving, `RFC 7807` bearer enforcement,
//! agent manifest discovery, and `Discord` onboarding config persistence.

use std::{path::PathBuf, sync::Arc};

use api::build_router;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use framework::{
    desktop::dotenv::read_dotenv_file, testing::test_context::build_lazy_test_context,
};
use http_body_util::BodyExt;
use tower::ServiceExt;
use uuid::Uuid;

const OWNER_BEARER: &str = "Bearer test-owner-bearer-token";

fn setup_desktop_router() -> (Router, PathBuf, PathBuf) {
    let (mut ctx, _counters) = build_lazy_test_context().unwrap();
    let temp_root = std::env::temp_dir().join(format!("vcp-spa-suite-{}", Uuid::new_v4()));
    let dist_dir = temp_root.join("dist");
    let assets_dir = dist_dir.join("assets");
    std::fs::create_dir_all(&assets_dir).unwrap();
    std::fs::write(
        dist_dir.join("index.html"),
        "<!doctype html><title>VoiceCloneStudio</title>",
    )
    .unwrap();
    std::fs::write(assets_dir.join("main.js"), "console.log('studio-ready');").unwrap();
    std::fs::write(temp_root.join("secret.txt"), "top-secret").unwrap();

    let env_file = temp_root.join("studio.env");
    std::fs::write(&env_file, "DESKTOP_STANDALONE_MODE=true\n").unwrap();

    {
        let cfg = Arc::make_mut(&mut ctx.config);
        cfg.desktop.standalone_mode = true;
        cfg.desktop.web_dist_dir = Some(dist_dir);
        cfg.desktop.studio_env_path = Some(env_file.clone());
    }

    (build_router(ctx), temp_root, env_file)
}

#[tokio::test]
async fn serves_embedded_studio_spa_and_blocks_path_traversal() {
    let (router, temp_root, _env_file) = setup_desktop_router();

    let root_resp = router
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(root_resp.status(), StatusCode::OK);
    let root_body = root_resp.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&root_body).contains("<title>VoiceCloneStudio</title>"));

    let js_resp = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/assets/main.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(js_resp.status(), StatusCode::OK);
    assert_eq!(
        js_resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("text/javascript; charset=utf-8")
    );

    let trav_resp = router
        .oneshot(
            Request::builder()
                .uri("/../secret.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let trav_bytes = trav_resp.into_body().collect().await.unwrap().to_bytes();
    assert!(!String::from_utf8_lossy(&trav_bytes).contains("top-secret"));

    let _ = std::fs::remove_dir_all(&temp_root);
}

#[tokio::test]
async fn enforces_bearer_auth_and_rejects_sentinel_backdoor_on_system_endpoints() {
    let (router, temp_root, _env_file) = setup_desktop_router();

    let unauth_status = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/system/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauth_status.status(), StatusCode::UNAUTHORIZED);

    let sentinel_attempt = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/system/discord-config")
                .header(
                    header::AUTHORIZATION,
                    "Bearer dev-owner-bearer-token-change-me",
                )
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(sentinel_attempt.status(), StatusCode::UNAUTHORIZED);

    let _ = std::fs::remove_dir_all(&temp_root);
}

#[tokio::test]
async fn exposes_agent_manifest_and_persists_discord_onboarding_config() {
    let (router, temp_root, env_file) = setup_desktop_router();

    let manifest_resp = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/agent/manifest")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(manifest_resp.status(), StatusCode::OK);
    let manifest_json: serde_json::Value = serde_json::from_slice(
        &manifest_resp
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes(),
    )
    .unwrap();
    assert_eq!(manifest_json["standalone_desktop_mode"], true);
    assert_eq!(manifest_json["cli_binary"], "make");

    let status_resp = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/system/status")
                .header(header::AUTHORIZATION, OWNER_BEARER)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(status_resp.status(), StatusCode::OK);

    let bad_json_resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/system/discord-config")
                .header(header::AUTHORIZATION, OWNER_BEARER)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{not-valid-json"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad_json_resp.status(), StatusCode::BAD_REQUEST);

    let save_resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/system/discord-config")
                .header(header::AUTHORIZATION, OWNER_BEARER)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "discord_application_id": "111111111111111111",
                        "discord_bot_token": "MTIzNDU2.valid-looking-bot-secret",
                        "discord_owner_user_id": "222222222222222222",
                        "verify_online": false
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(save_resp.status(), StatusCode::OK);

    let persisted = read_dotenv_file(&env_file).unwrap();
    assert_eq!(
        persisted.get("DISCORD_APPLICATION_ID").map(String::as_str),
        Some("111111111111111111")
    );
    assert_eq!(
        persisted.get("DISCORD_OWNER_USER_ID").map(String::as_str),
        Some("222222222222222222")
    );

    let _ = std::fs::remove_dir_all(&temp_root);
}
