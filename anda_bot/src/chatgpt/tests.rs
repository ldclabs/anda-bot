use super::*;
use axum::{Form, Json, routing::post};
use cose2::{Signer, crypto::RingSigner, iana};
use std::sync::atomic::{AtomicUsize, Ordering};

fn signed_identity(client: &str, nonce: &str, subject: &str) -> String {
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","kid":"test-key"}"#);
    let payload=URL_SAFE_NO_PAD.encode(serde_json::to_vec(&serde_json::json!({"iss":ISSUER,"aud":client,"sub":subject,"nonce":nonce,"exp":unix_seconds()+3600,"iat":unix_seconds(),"email":"test@example.invalid"})).unwrap());
    let signing_input = format!("{header}.{payload}");
    let signer = RingSigner::rsa_from_pkcs8(
        iana::AlgorithmRS256,
        include_bytes!("testdata/oidc-test-only.der"),
        None,
    )
    .unwrap();
    format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(signer.sign(signing_input.as_bytes()).unwrap())
    )
}
fn jwks() -> oidc::Jwks {
    serde_json::from_str(include_str!("testdata/jwks.json")).unwrap()
}
fn service(home: &Path) -> Arc<ChatGptService> {
    ChatGptService::open(
        home,
        &[3; 32],
        reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap(),
    )
    .unwrap()
}
fn profile(id: &str) -> Profile {
    Profile {
        id: id.into(),
        label: "Test".into(),
        issuer: ISSUER.into(),
        subject: "subject".into(),
        client_id: "oaiapp_test".into(),
        email: Some("test@example.invalid".into()),
        tokens: Some(Tokens {
            access_token: "access-1".into(),
            refresh_token: "refresh-1".into(),
            id_token: signed_identity("oaiapp_test", "nonce", "subject"),
            expires_at: unix_seconds() + 3600,
            scopes: SCOPES.split_whitespace().map(str::to_owned).collect(),
        }),
    }
}

#[test]
fn chatgpt_oidc_verifies_real_rsa_and_rejects_tampering() {
    let token = signed_identity("oaiapp_test", "nonce", "subject");
    assert!(oidc::verify(&token, &jwks(), "oaiapp_test", Some("nonce")).is_ok());
    assert!(oidc::verify(&token, &jwks(), "other", Some("nonce")).is_err());
    assert!(oidc::verify(&token, &jwks(), "oaiapp_test", Some("other")).is_err());
    let mut parts: Vec<String> = token.split('.').map(str::to_owned).collect();
    parts[1]=URL_SAFE_NO_PAD.encode(br#"{"iss":"https://auth.openai.com","sub":"attacker","aud":"oaiapp_test","exp":9999999999,"nonce":"nonce"}"#);
    assert!(oidc::verify(&parts.join("."), &jwks(), "oaiapp_test", Some("nonce")).is_err());
}

#[tokio::test]
async fn chatgpt_login_binds_callback_pkce_and_persists_registration() {
    let home = tempfile::tempdir().unwrap();
    let mut service = service(home.path());
    let expected = Arc::new(Mutex::new(BTreeMap::<String, String>::new()));
    let captured = expected.clone();
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    let base=crate::test_support::spawn_http_mock(Router::new().route("/api/accounts/oauth/token",post(move |Form(form):Form<BTreeMap<String,String>>|{let expected=captured.clone();let calls=calls.clone();async move {
        calls.fetch_add(1,Ordering::SeqCst);let expected=expected.lock().await;
        assert_eq!(form.get("client_id").unwrap(),"oaiapp_test");assert_eq!(form.get("resource").unwrap(),API_BASE);
        assert_eq!(form.get("code_verifier"),expected.get("verifier"));assert_eq!(form.get("redirect_uri"),expected.get("redirect_uri"));
        Json(serde_json::json!({"access_token":"access","refresh_token":"refresh","id_token":signed_identity("oaiapp_test",expected.get("nonce").unwrap(),"subject"),"scope":SCOPES,"token_type":"Bearer","expires_in":3600}))
    }}))).await;
    Arc::get_mut(&mut service).unwrap().endpoints.issuer = base;
    *service.jwks.lock().await = Some((unix_seconds(), jwks()));
    let flow = service.start_login(None, 0, false).await.unwrap();
    let url = reqwest::Url::parse(flow.authorization_url.as_ref().unwrap()).unwrap();
    let query: BTreeMap<String, String> = url.query_pairs().into_owned().collect();
    assert_eq!(query.get("client_id").unwrap(), "dynamic_agent_client");
    assert!(query["redirect_uri"].starts_with("http://127.0.0.1:"));
    let verifier = service.flows.lock().await[&flow.flow_id]
        .verifier
        .to_string();
    assert_eq!(
        query["code_challenge"],
        URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
    );
    *expected.lock().await = BTreeMap::from([
        ("verifier".into(), verifier),
        ("nonce".into(), query["nonce"].clone()),
        ("redirect_uri".into(), query["redirect_uri"].clone()),
    ]);
    assert!(
        service
            .complete_login(
                &flow.flow_id,
                Callback {
                    state: Some("wrong".into()),
                    code: Some("code".into()),
                    client_id: Some("oaiapp_test".into()),
                    error: None
                }
            )
            .await
            .is_err()
    );
    assert_eq!(count.load(Ordering::SeqCst), 0);
    service
        .complete_login(
            &flow.flow_id,
            Callback {
                state: Some(query["state"].clone()),
                code: Some("code".into()),
                client_id: Some("oaiapp_test".into()),
                error: None,
            },
        )
        .await
        .unwrap();
    let account = service.accounts().await.accounts.pop().unwrap();
    assert!(account.plan_enabled);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(
        service
            .complete_login(
                &flow.flow_id,
                Callback {
                    state: Some(query["state"].clone()),
                    code: Some("code".into()),
                    client_id: Some("oaiapp_test".into()),
                    error: None
                }
            )
            .await
            .is_err()
    );
    let retry = service
        .start_login(Some(account.id), 0, false)
        .await
        .unwrap();
    let url = reqwest::Url::parse(retry.authorization_url.as_ref().unwrap()).unwrap();
    let query: BTreeMap<String, String> = url.query_pairs().into_owned().collect();
    assert_eq!(query["client_id"], "oaiapp_test");
    assert!(!query.contains_key("agent_name_hint"));
    assert!(!query.contains_key("id_token_hint"));
    service.cancel_login(&retry.flow_id).await.unwrap();
}

#[tokio::test]
async fn chatgpt_concurrent_requests_rotate_once_and_publish_atomically() {
    let home = tempfile::tempdir().unwrap();
    let mut service = service(home.path());
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    let base=crate::test_support::spawn_http_mock(Router::new().route("/api/accounts/oauth/token",post(move |Form(form):Form<BTreeMap<String,String>>|{let calls=calls.clone();async move {
        assert_eq!(form["refresh_token"],"refresh-1");assert!(!form.contains_key("scope"));calls.fetch_add(1,Ordering::SeqCst);
        Json(serde_json::json!({"access_token":"access-2","refresh_token":"refresh-2","token_type":"Bearer","expires_in":3600}))
    }}))).await;
    Arc::get_mut(&mut service).unwrap().endpoints.issuer = base;
    let mut initial = profile("p");
    initial.tokens.as_mut().unwrap().expires_at = 0;
    service
        .accounts
        .lock()
        .await
        .profiles
        .insert("p".into(), initial);
    let (a, b) = tokio::join!(service.access("p", false), service.access("p", false));
    assert_eq!(a.unwrap().0.as_str(), "access-2");
    assert_eq!(b.unwrap().0.as_str(), "access-2");
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(
        service.store.load().unwrap().profiles["p"]
            .tokens
            .as_ref()
            .unwrap()
            .refresh_token,
        "refresh-2"
    );
}

#[tokio::test]
async fn chatgpt_refresh_failure_preserves_transient_session_but_clears_revocation() {
    for (status, code, keep) in [
        (503, "temporarily_unavailable", true),
        (400, "invalid_grant", false),
    ] {
        let home = tempfile::tempdir().unwrap();
        let mut service = service(home.path());
        let base = crate::test_support::spawn_http_mock(Router::new().route(
            "/api/accounts/oauth/token",
            post(move || async move {
                (
                    axum::http::StatusCode::from_u16(status).unwrap(),
                    Json(serde_json::json!({"error":code})),
                )
            }),
        ))
        .await;
        Arc::get_mut(&mut service).unwrap().endpoints.issuer = base;
        let mut initial = profile("p");
        initial.tokens.as_mut().unwrap().expires_at = 0;
        service
            .accounts
            .lock()
            .await
            .profiles
            .insert("p".into(), initial);
        assert!(service.access("p", false).await.is_err());
        assert_eq!(
            service.accounts.lock().await.profiles["p"].tokens.is_some(),
            keep
        );
    }
}

#[tokio::test]
async fn chatgpt_transfer_moves_session_and_preserves_destination_host() {
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let service = service(source.path());
    let dest = super::tests::service(target.path());
    service
        .accounts
        .lock()
        .await
        .profiles
        .insert("p".into(), profile("p"));
    let host = dest.accounts.lock().await.host_id.clone();
    let transfer = source.path().join("transfer.json");
    service.export_session("p", &transfer).await.unwrap();
    assert!(service.accounts.lock().await.profiles["p"].tokens.is_none());
    let imported = dest.import_session(&transfer).await.unwrap();
    assert!(!transfer.exists());
    assert_eq!(dest.accounts.lock().await.host_id, host);
    assert!(
        dest.accounts.lock().await.profiles[&imported]
            .tokens
            .is_some()
    );
}

#[test]
fn chatgpt_identity_only_grant_does_not_require_plan_tokens() {
    let response: TokenResponse =
        serde_json::from_value(serde_json::json!({"id_token":"id","scope":"openid profile email"}))
            .unwrap();
    let tokens = tokens_from_response(response, None).unwrap();
    assert!(!tokens.scopes.iter().any(|s| s == PLAN_SCOPE));
}

fn owner_api(service: Arc<ChatGptService>, home: &Path) -> (api::ChatGptApi, String) {
    let key = crate::identity::Ed25519Key::new([19; 32]);
    let mut claims = crate::identity::expiring_claims(Duration::from_secs(60)).unwrap();
    claims
        .extra
        .insert(crate::identity::iana::CWTClaimScope, "*");
    let token = key.sign_cwt(claims).unwrap();
    let api = api::ChatGptApi {
        service,
        home: home.into(),
        auth: anda_engine_server::handler::AppState {
            engines: Arc::new(Default::default()),
            default_engine: key.id(),
            start_time_ms: anda_engine::unix_ms(),
            extra_info: Arc::new(Default::default()),
            ed25519_pubkeys: Arc::new(vec![key.pubkey().into()]),
        },
        owner: key.id(),
        config_lock: Arc::new(Mutex::new(())),
        runtime: None,
        setup_complete: CancellationToken::new(),
    };
    (api, token)
}
#[tokio::test]
async fn chatgpt_controls_are_owner_only_and_never_return_credentials() {
    let home = tempfile::tempdir().unwrap();
    let service = service(home.path());
    service
        .accounts
        .lock()
        .await
        .profiles
        .insert("p".into(), profile("p"));
    let (api, token) = owner_api(service, home.path());
    let base = crate::test_support::spawn_http_mock(api.router()).await;
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let path = format!("{base}/daemon/chatgpt");
    assert_eq!(
        http.post(&path)
            .json(&api::Request::Accounts)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let result = http
        .post(&path)
        .bearer_auth(&token)
        .json(&api::Request::Accounts)
        .send()
        .await
        .unwrap();
    assert_eq!(result.status(), 200);
    let text = result.text().await.unwrap();
    assert!(!text.contains("access-1"));
    assert!(!text.contains("refresh-1"));
    assert!(!text.contains("id_token"));
    let result = http
        .post(&path)
        .bearer_auth(&token)
        .header("Origin", "chrome-extension://test")
        .json(&api::Request::TransferExport {
            profile_id: "p".into(),
            path: home.path().join("must-not-exist.json"),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(result.status(), 403);
    assert!(!home.path().join("must-not-exist.json").exists());
}
#[tokio::test]
async fn chatgpt_setup_activates_model_without_an_api_key_or_brain() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join("config.yaml"),
        "model:\n  active: ''\n  providers: []\n",
    )
    .unwrap();
    let mut service = service(home.path());
    let base=crate::test_support::spawn_http_mock(Router::new().route("/models",get(||async{Json(serde_json::json!({"models":[{"slug":"test-model","display_name":"Test model","visibility":"list"}]}))}))).await;
    Arc::get_mut(&mut service).unwrap().endpoints.api = base;
    service
        .accounts
        .lock()
        .await
        .profiles
        .insert("p".into(), profile("p"));
    let (api, token) = owner_api(service.clone(), home.path());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(setup::serve(api, addr, cancel.clone()));
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let base = format!("http://{addr}");
    for _ in 0..50 {
        if http
            .get(format!("{base}/daemon/status"))
            .send()
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let state: serde_json::Value = http
        .get(format!("{base}/daemon/status"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(state["needs_setup"], true);
    // Only the authenticated configuration response exposes setup reasons.
    assert!(state.get("setup_issues").is_none());
    assert_eq!(
        http.get(format!("{base}/daemon/config"))
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::FORBIDDEN
    );
    let config: serde_json::Value = http
        .get(format!("{base}/daemon/config"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(config["setup_issues"], serde_json::json!(["model.active"]));
    let response = http
        .post(format!("{base}/daemon/chatgpt"))
        .bearer_auth(&token)
        .json(&api::Request::ModelSelect {
            profile_id: "p".into(),
            model: "test-model".into(),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    assert!(
        tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    );
    let saved = std::fs::read_to_string(home.path().join("config.yaml")).unwrap();
    assert!(!saved.contains("access-1"));
    assert!(!saved.contains("refresh-1"));
    let config = crate::config::Config::from_contents(&saved).unwrap();
    assert!(config.setup_issues().is_empty());
    assert_eq!(config.model.active, "chatgpt:p:test-model");
    assert!(!home.path().join("db").exists());
    assert_eq!(
        config
            .models_with_chatgpt(http, Some(service))
            .get_model()
            .unwrap()
            .model_name(),
        "chatgpt:p:test-model"
    );
}

#[test]
fn chatgpt_setup_template_presets_load_with_only_an_api_key() {
    let template = crate::config::Config::default_template();
    let count = crate::config::Config::from_contents(template)
        .unwrap()
        .model
        .providers
        .len();
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    for index in 0..count {
        let mut config = crate::config::Config::from_contents(template).unwrap();
        let provider = &mut config.model.providers[index];
        provider.api_key = "test-onboarding-key".into();
        provider.disabled = false;
        config.model.active = provider.selection_id();
        assert!(config.setup_issues().is_empty(), "{}", config.model.active);
        let model = config.models(http.clone()).get_model().unwrap();
        assert_eq!(model.model_name(), config.model.active);
    }
}

#[tokio::test]
async fn chatgpt_tool_roundtrip_replays_calls_without_repeating_history() {
    use anda_core::{CompletionRequest, ContentPart};
    let home = tempfile::tempdir().unwrap();
    let mut service = service(home.path());
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let base=crate::test_support::spawn_http_mock(Router::new().route("/responses",post(move |headers:axum::http::HeaderMap,Json(body):Json<serde_json::Value>|{let count=count.clone();async move{
        assert_eq!(headers["authorization"],"Bearer access-1");assert_eq!(body["store"],false);assert_eq!(body["stream"],true);assert!(body.get("max_output_tokens").is_none());
        let output=if count.fetch_add(1,Ordering::SeqCst)==0{serde_json::json!([{"type":"function_call","namespace":"anda","name":"read_file","call_id":"call_one","arguments":"{}"}])}else{
            let input=body["input"].as_array().unwrap();assert_eq!(input.iter().filter(|v|v["type"]=="function_call").count(),1);assert_eq!(input.iter().filter(|v|v["type"]=="function_call_output").count(),1);assert!(input.iter().any(|v|v["call_id"]=="call_one"));
            serde_json::json!([{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Done"}]}])
        };
        let response=serde_json::json!({"type":"response.completed","response":{"id":"r","created_at":1,"model":"test-model","output":output,"status":"completed"}});
        ([("content-type","text/event-stream")],format!("data: {response}\n\n"))
    }}))).await;
    Arc::get_mut(&mut service).unwrap().endpoints.api = base;
    service
        .accounts
        .lock()
        .await
        .profiles
        .insert("p".into(), profile("p"));
    let config = crate::config::ModelProvider {
        model: "test-model".into(),
        auth: crate::config::ModelAuth::Chatgpt {
            profile: "p".into(),
        },
        ..Default::default()
    };
    let model = model::completion_model(service, "p".into(), &config);
    let first = model
        .completer
        .completion(CompletionRequest {
            prompt: "Read the file".into(),
            max_output_tokens: Some(100),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(first.tool_calls[0].name, "read_file");
    let second = model
        .completer
        .completion(CompletionRequest {
            raw_history: first.raw_history,
            content: vec![ContentPart::ToolOutput {
                name: "read_file".into(),
                output: serde_json::json!({"content":"hello"}),
                call_id: Some("call_one".into()),
                is_error: Some(false),
                remote_id: None,
            }],
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(second.content, "Done");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(second.raw_history.len(), 2);
}
