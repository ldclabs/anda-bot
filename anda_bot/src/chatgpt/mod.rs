//! ChatGPT plan authorization shared by all local Anda clients.
use anda_core::BoxError;
use axum::{
    Router,
    extract::{Query, State},
    response::{Html, IntoResponse},
    routing::get,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

pub mod api;
pub mod model;
mod oidc;
pub mod setup;
mod store;
#[cfg(test)]
mod tests;
use store::{Accounts, CredentialStore, Profile, Tokens};

pub const ISSUER: &str = "https://auth.openai.com";
pub const API_BASE: &str = "https://api.openai.com/v1";
pub const USAGE_URL: &str = "https://chatgpt.com/settings/usage";
const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
const PLAN_SCOPE: &str = "chatgpt.tokens.use.direct";
const FLOW_SECONDS: u64 = 600;

pub(crate) fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub(crate) fn random_id() -> String {
    URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AccountView {
    pub id: String,
    pub label: String,
    pub email: Option<String>,
    pub connected: bool,
    pub plan_enabled: bool,
}
#[derive(Serialize, Deserialize)]
pub struct AccountsView {
    pub active: Option<String>,
    pub accounts: Vec<AccountView>,
    pub usage_url: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct LoginView {
    pub flow_id: String,
    pub status: String,
    pub expires_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_url: Option<String>,
    pub account_id: Option<String>,
    pub error: Option<String>,
}
struct PendingLogin {
    view: LoginView,
    state: String,
    nonce: String,
    verifier: Zeroizing<String>,
    client_id: String,
    profile_id: Option<String>,
    redirect_uri: String,
    cancel: CancellationToken,
}
#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct CatalogModel {
    pub slug: String,
    pub display_name: String,
    #[serde(default)]
    pub visibility: String,
}
#[derive(Deserialize)]
struct Catalog {
    models: Vec<CatalogModel>,
}
#[derive(Deserialize)]
struct TokenResponse {
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    token_type: String,
    #[serde(default)]
    expires_in: u64,
    #[serde(default)]
    scope: Option<String>,
}
#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    jwks_uri: String,
    revocation_endpoint: String,
}
#[derive(Clone)]
struct Endpoints {
    issuer: String,
    api: String,
}
impl Default for Endpoints {
    fn default() -> Self {
        Self {
            issuer: ISSUER.into(),
            api: API_BASE.into(),
        }
    }
}

pub struct ChatGptService {
    store: CredentialStore,
    accounts: Mutex<Accounts>,
    flows: Mutex<BTreeMap<String, PendingLogin>>,
    sessions: Mutex<BTreeMap<String, CancellationToken>>,
    jwks: Mutex<Option<(u64, oidc::Jwks)>>,
    http: reqwest::Client,
    endpoints: Endpoints,
}
impl ChatGptService {
    pub fn open(
        home: &Path,
        secret: &[u8; 32],
        http: reqwest::Client,
    ) -> Result<Arc<Self>, BoxError> {
        let store = CredentialStore::open(home, secret)?;
        let accounts = store.load()?;
        Ok(Arc::new(Self {
            store,
            accounts: Mutex::new(accounts),
            flows: Mutex::new(BTreeMap::new()),
            sessions: Mutex::new(BTreeMap::new()),
            jwks: Mutex::new(None),
            http,
            endpoints: Endpoints::default(),
        }))
    }
    pub async fn accounts(&self) -> AccountsView {
        let accounts = self.accounts.lock().await;
        AccountsView {
            active: accounts.active.clone(),
            accounts: accounts
                .profiles
                .values()
                .map(|p| AccountView {
                    id: p.id.clone(),
                    label: p.label.clone(),
                    email: p.email.clone(),
                    connected: p.tokens.is_some(),
                    plan_enabled: p
                        .tokens
                        .as_ref()
                        .is_some_and(|t| t.scopes.iter().any(|s| s == PLAN_SCOPE)),
                })
                .collect(),
            usage_url: USAGE_URL.into(),
        }
    }
    pub async fn select(&self, id: &str) -> Result<AccountsView, BoxError> {
        let mut accounts = self.accounts.lock().await;
        if !accounts.profiles.contains_key(id) {
            return Err("ChatGPT account not found".into());
        }
        let mut next = accounts.clone();
        next.active = Some(id.into());
        self.store.save(&next)?;
        *accounts = next;
        drop(accounts);
        Ok(self.accounts().await)
    }
    pub async fn start_login(
        self: &Arc<Self>,
        profile_id: Option<String>,
        port: u16,
        consent: bool,
    ) -> Result<LoginView, BoxError> {
        let accounts = self.accounts.lock().await;
        let profile = profile_id
            .as_ref()
            .map(|id| accounts.profiles.get(id).ok_or("ChatGPT account not found"))
            .transpose()?;
        let client_id = profile
            .map(|p| p.client_id.clone())
            .or_else(|| accounts.pending_client_id.clone())
            .unwrap_or_else(|| "dynamic_agent_client".into());
        let host_id = accounts.host_id.clone();
        drop(accounts);
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
        let redirect_uri = format!(
            "http://127.0.0.1:{}/auth/callback",
            listener.local_addr()?.port()
        );
        let (flow_id, state, nonce, verifier) =
            (random_id(), random_id(), random_id(), random_id());
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let mut url =
            reqwest::Url::parse(&format!("{}/api/accounts/authorize", self.endpoints.issuer))?;
        url.query_pairs_mut().extend_pairs([
            ("client_id", client_id.as_str()),
            ("ext_agent_host_id", host_id.as_str()),
            ("response_type", "code"),
            ("redirect_uri", redirect_uri.as_str()),
            ("scope", SCOPES),
            ("resource", API_BASE),
            ("state", state.as_str()),
            ("nonce", nonce.as_str()),
            ("code_challenge_method", "S256"),
            ("code_challenge", challenge.as_str()),
        ]);
        if client_id == "dynamic_agent_client" {
            url.query_pairs_mut()
                .append_pair("agent_name_hint", "Anda Bot");
        }
        if consent {
            url.query_pairs_mut().append_pair("prompt", "consent");
        }
        // ID-token hints deliberately stay out of URLs returned to UI/terminal clients.
        let view = LoginView {
            flow_id: flow_id.clone(),
            status: "pending".into(),
            expires_at: unix_seconds() + FLOW_SECONDS,
            authorization_url: Some(url.to_string()),
            account_id: None,
            error: None,
        };
        let cancel = CancellationToken::new();
        {
            let mut flows = self.flows.lock().await;
            flows.retain(|_, f| {
                if f.view.expires_at < unix_seconds() {
                    f.cancel.cancel();
                    false
                } else {
                    true
                }
            });
            if flows
                .values()
                .any(|f| f.view.status == "pending" || f.view.status == "exchanging")
            {
                return Err("A ChatGPT sign-in is already in progress; cancel it first".into());
            }
            let mut saved = view.clone();
            saved.authorization_url = None;
            flows.insert(
                flow_id.clone(),
                PendingLogin {
                    view: saved,
                    state,
                    nonce,
                    verifier: Zeroizing::new(verifier),
                    client_id,
                    profile_id,
                    redirect_uri,
                    cancel: cancel.clone(),
                },
            );
        }
        let app = Router::new()
            .route("/auth/callback", get(callback))
            .with_state((self.clone(), flow_id.clone()));
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            let done = cancel.clone();
            let _=axum::serve(listener,app).with_graceful_shutdown(async move {
                tokio::select!{_=done.cancelled()=>{},_=tokio::time::sleep(Duration::from_secs(FLOW_SECONDS))=>{done.cancel();}}
            }).await;
            if let Some(service) = weak.upgrade()
                && let Some(flow) = service.flows.lock().await.get_mut(&flow_id)
                && flow.view.status == "pending"
            {
                flow.view.status = "expired".into();
            }
        });
        Ok(view)
    }
    pub async fn login_status(&self, id: &str) -> Result<LoginView, BoxError> {
        self.flows
            .lock()
            .await
            .get(id)
            .map(|f| f.view.clone())
            .ok_or_else(|| "ChatGPT login attempt not found".into())
    }
    pub async fn cancel_login(&self, id: &str) -> Result<LoginView, BoxError> {
        let mut flows = self.flows.lock().await;
        let flow = flows.get_mut(id).ok_or("ChatGPT login attempt not found")?;
        if matches!(flow.view.status.as_str(), "pending" | "exchanging") {
            flow.view.status = "cancelled".into();
            flow.cancel.cancel();
        }
        Ok(flow.view.clone())
    }
    async fn complete_login(&self, flow_id: &str, query: Callback) -> Result<(), BoxError> {
        let (nonce, verifier, client_id, profile_id, redirect, cancel) = {
            let mut flows = self.flows.lock().await;
            let f = flows.get_mut(flow_id).ok_or("unknown login attempt")?;
            if f.view.status != "pending"
                || f.view.expires_at <= unix_seconds()
                || query.state.as_deref() != Some(&f.state)
            {
                return Err("invalid or expired login callback".into());
            }
            if query.error.is_some() {
                f.view.status = "cancelled".into();
                f.view.error = Some("ChatGPT authorization was declined".into());
                f.cancel.cancel();
                return Err("authorization declined".into());
            }
            let checked: Result<String, &str> = if f.client_id != "dynamic_agent_client" {
                if query.client_id.as_deref().is_some_and(|v| v != f.client_id) {
                    Err("callback client ID does not match this account")
                } else {
                    Ok(f.client_id.clone())
                }
            } else {
                query
                    .client_id
                    .clone()
                    .filter(|v| {
                        !v.is_empty()
                            && v != "dynamic_agent_client"
                            && v.len() < 256
                            && v.chars()
                                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                    })
                    .ok_or("registration did not return an issued client ID")
            }
            .and_then(|client_id| {
                if query.code.as_ref().is_none_or(|v| v.is_empty()) {
                    Err("missing authorization code")
                } else {
                    Ok(client_id)
                }
            });
            // The state matched, so this callback ends the attempt either way.
            let client_id = match checked {
                Ok(client_id) => client_id,
                Err(error) => {
                    f.view.status = "failed".into();
                    f.view.error = Some(error.into());
                    f.cancel.cancel();
                    return Err(error.into());
                }
            };
            f.client_id = client_id.clone();
            f.view.status = "exchanging".into();
            (
                f.nonce.clone(),
                f.verifier.clone(),
                client_id,
                f.profile_id.clone(),
                f.redirect_uri.clone(),
                f.cancel.clone(),
            )
        };
        let result: Result<String, BoxError> = async {
            if profile_id.is_none() {
                let mut accounts = self.accounts.lock().await;
                let mut next = accounts.clone();
                next.pending_client_id = Some(client_id.clone());
                self.store.save(&next)?;
                *accounts = next;
            }
            let response = self
                .http
                .post(format!(
                    "{}/api/accounts/oauth/token",
                    self.endpoints.issuer
                ))
                .timeout(Duration::from_secs(30))
                .form(&[
                    ("grant_type", "authorization_code"),
                    ("client_id", client_id.as_str()),
                    ("code", query.code.as_deref().unwrap_or_default()),
                    ("code_verifier", verifier.as_str()),
                    ("redirect_uri", redirect.as_str()),
                    ("resource", API_BASE),
                ])
                .send()
                .await
                .map_err(|e| e.without_url())?;
            let token: TokenResponse = decode(response).await?;
            let identity = self
                .verify_identity(
                    token.id_token.as_deref().ok_or("missing ID token")?,
                    &client_id,
                    Some(&nonce),
                )
                .await?;
            let tokens = tokens_from_response(token, None)?;
            let mut accounts = self.accounts.lock().await;
            if cancel.is_cancelled() {
                return Err("sign-in was cancelled".into());
            }
            let id = if let Some(id) = profile_id {
                let old = accounts.profiles.get(&id).ok_or("account was removed")?;
                if old.subject != identity.sub
                    || old.issuer != identity.iss
                    || old.client_id != client_id
                {
                    return Err("sign-in returned a different account".into());
                }
                id
            } else {
                random_id()
            };
            let mut next = accounts.clone();
            let label = identity
                .email
                .clone()
                .unwrap_or_else(|| format!("ChatGPT {}", &id[..8]));
            next.profiles.insert(
                id.clone(),
                Profile {
                    id: id.clone(),
                    label,
                    issuer: identity.iss,
                    subject: identity.sub,
                    client_id,
                    email: identity.email,
                    tokens: Some(tokens),
                },
            );
            next.active = Some(id.clone());
            next.pending_client_id = None;
            let mut flows = self.flows.lock().await;
            let flow = flows.get_mut(flow_id).ok_or("sign-in expired")?;
            if cancel.is_cancelled() || flow.view.status != "exchanging" {
                return Err("sign-in was cancelled".into());
            }
            self.store.save(&next)?;
            flow.view.status = "completed".into();
            flow.view.account_id = Some(id.clone());
            *accounts = next;
            self.sessions.lock().await.entry(id.clone()).or_default();
            Ok(id)
        }
        .await;
        if let Some(f) = self.flows.lock().await.get_mut(flow_id)
            && f.view.status != "cancelled"
        {
            match &result {
                Ok(profile) => {
                    f.view.status = "completed".into();
                    f.view.account_id = Some(profile.clone());
                }
                Err(e) => {
                    f.view.status = "failed".into();
                    f.view.error = Some(e.to_string());
                }
            }
        }
        cancel.cancel();
        result.map(|_| ())
    }
    async fn discovery(&self) -> Result<Discovery, BoxError> {
        let discovery: Discovery = decode(
            self.http
                .get(format!(
                    "{}/.well-known/openid-configuration",
                    self.endpoints.issuer
                ))
                .timeout(Duration::from_secs(20))
                .send()
                .await?,
        )
        .await?;
        if discovery.issuer != ISSUER {
            return Err("OIDC discovery issuer mismatch".into());
        }
        for url in [&discovery.jwks_uri, &discovery.revocation_endpoint] {
            let url = reqwest::Url::parse(url)?;
            if url.scheme() != "https"
                || url.host_str() != Some("auth.openai.com")
                || url.port_or_known_default() != Some(443)
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err("untrusted OIDC endpoint".into());
            }
        }
        Ok(discovery)
    }
    async fn verify_identity(
        &self,
        token: &str,
        client_id: &str,
        nonce: Option<&str>,
    ) -> Result<oidc::Identity, BoxError> {
        let mut cache = self.jwks.lock().await;
        if let Some((at, keys)) = &*cache
            && *at + 3600 > unix_seconds()
            && let Ok(identity) = oidc::verify(token, keys, client_id, nonce)
        {
            return Ok(identity);
        }
        let discovery = self.discovery().await?;
        let keys: oidc::Jwks = decode(
            self.http
                .get(discovery.jwks_uri)
                .timeout(Duration::from_secs(20))
                .send()
                .await?,
        )
        .await?;
        let result = oidc::verify(token, &keys, client_id, nonce);
        *cache = Some((unix_seconds(), keys));
        result
    }
    pub(crate) async fn access(
        &self,
        id: &str,
        force: bool,
    ) -> Result<(Zeroizing<String>, CancellationToken), BoxError> {
        // Held over refresh and atomic publication: refresh tokens rotate and may not race.
        let mut accounts = self.accounts.lock().await;
        let profile = accounts
            .profiles
            .get(id)
            .ok_or("ChatGPT account not found")?;
        let saved = profile.tokens.as_ref().ok_or("ChatGPT sign-in required")?;
        if !saved.scopes.iter().any(|s| s == PLAN_SCOPE) {
            return Err(
                "ChatGPT plan permission is disabled; enable it in account settings".into(),
            );
        }
        if force || saved.expires_at <= unix_seconds() + 60 {
            let client_id = profile.client_id.clone();
            let subject = profile.subject.clone();
            let old = saved.clone();
            let response = self
                .http
                .post(format!(
                    "{}/api/accounts/oauth/token",
                    self.endpoints.issuer
                ))
                .timeout(Duration::from_secs(30))
                .form(&[
                    ("grant_type", "refresh_token"),
                    ("client_id", client_id.as_str()),
                    ("refresh_token", old.refresh_token.as_str()),
                    ("resource", API_BASE),
                ])
                .send()
                .await
                .map_err(|e| e.without_url())?;
            let token = decode::<TokenResponse>(response).await;
            match token {
                Ok(token) => {
                    let verified = match &token.id_token {
                        Some(id_token) => self
                            .verify_identity(id_token, &client_id, None)
                            .await
                            .map(|identity| identity.sub),
                        None => Ok(subject.clone()),
                    };
                    if verified.as_ref().is_ok_and(|sub| *sub != subject) {
                        return Err("refreshed token belongs to another account".into());
                    }
                    let mut updated = tokens_from_response(token, Some(&old))?;
                    if verified.is_err() {
                        // The old refresh token is already spent. Keep the rotated one, but
                        // never use an unverified access token: the next call refreshes again.
                        updated.access_token.clear();
                        updated.id_token = old.id_token.clone();
                        updated.expires_at = 0;
                    }
                    let mut next = accounts.clone();
                    next.profiles.get_mut(id).unwrap().tokens = Some(updated);
                    self.store.save(&next)?;
                    *accounts = next;
                    verified?;
                }
                Err(error) => {
                    if error.downcast_ref::<ProviderError>().is_some_and(|e| {
                        matches!(
                            e.code.as_str(),
                            "invalid_grant"
                                | "invalid_refresh_token"
                                | "token_expired"
                                | "refresh_token_expired"
                                | "refresh_token_invalid"
                                | "refresh_token_invalidated"
                                | "refresh_token_reused"
                        )
                    }) {
                        let mut next = accounts.clone();
                        next.profiles.get_mut(id).unwrap().tokens = None;
                        self.store.save(&next)?;
                        *accounts = next;
                        if let Some(cancel) = self.sessions.lock().await.remove(id) {
                            cancel.cancel();
                        }
                    }
                    return Err(error);
                }
            }
        }
        let tokens = accounts
            .profiles
            .get(id)
            .and_then(|p| p.tokens.as_ref())
            .ok_or("ChatGPT sign-in required")?;
        if !tokens.scopes.iter().any(|s| s == PLAN_SCOPE) {
            return Err("ChatGPT plan permission is disabled".into());
        }
        let access = Zeroizing::new(tokens.access_token.clone());
        let cancel = self
            .sessions
            .lock()
            .await
            .entry(id.into())
            .or_default()
            .clone();
        Ok((access, cancel))
    }
    pub async fn models(&self, id: &str) -> Result<Vec<CatalogModel>, BoxError> {
        let (token, _) = self.access(id, false).await?;
        let catalog: Catalog = decode(
            self.http
                .get(format!("{}/models", self.endpoints.api))
                .bearer_auth(token.as_str())
                .timeout(Duration::from_secs(30))
                .send()
                .await?,
        )
        .await?;
        Ok(catalog
            .models
            .into_iter()
            .filter(|m| m.visibility == "list")
            .collect())
    }
    /// Move a session to an owner-only transfer file; the source stops refreshing it.
    pub async fn export_session(&self, id: &str, path: &Path) -> Result<(), BoxError> {
        if !path.is_absolute() || path.exists() {
            return Err("Choose an absolute path for a new transfer file".into());
        }
        let mut accounts = self.accounts.lock().await;
        let profile = accounts
            .profiles
            .get(id)
            .ok_or("ChatGPT account not found")?;
        if profile.tokens.is_none() {
            return Err("Sign in before exporting this account".into());
        }
        let bytes = Zeroizing::new(serde_json::to_vec(profile)?);
        // create_new prevents an accidental overwrite, including a symlink target.
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        use std::io::Write;
        file.write_all(&bytes)?;
        file.sync_all()?;
        let mut next = accounts.clone();
        next.profiles.get_mut(id).unwrap().tokens = None;
        if let Err(error) = self.store.save(&next) {
            let _ = std::fs::remove_file(path);
            return Err(error);
        }
        *accounts = next;
        if let Some(cancel) = self.sessions.lock().await.remove(id) {
            cancel.cancel();
        }
        Ok(())
    }
    /// Import only from an explicitly supplied, protected local file. Host ID is retained.
    pub async fn import_session(&self, path: &Path) -> Result<String, BoxError> {
        if !path.is_absolute() {
            return Err("An absolute transfer-file path is required".into());
        }
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.is_file() || metadata.len() > 1024 * 1024 {
            return Err("Invalid ChatGPT transfer file".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err("Transfer file must have owner-only permissions (chmod 600)".into());
            }
        }
        let bytes = Zeroizing::new(std::fs::read(path)?);
        let mut profile: Profile =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid ChatGPT transfer file")?;
        if profile.issuer != ISSUER
            || profile.subject.is_empty()
            || profile.client_id == "dynamic_agent_client"
            || profile
                .tokens
                .as_ref()
                .is_none_or(|t| t.refresh_token.is_empty())
        {
            return Err("Invalid ChatGPT transfer registration".into());
        }
        let mut accounts = self.accounts.lock().await;
        if accounts.profiles.values().any(|p| {
            p.client_id == profile.client_id && p.subject == profile.subject && p.tokens.is_some()
        }) {
            return Err("This ChatGPT session is already connected on this host".into());
        }
        profile.id = random_id();
        let id = profile.id.clone();
        let mut next = accounts.clone();
        next.profiles.insert(id.clone(), profile);
        next.active = Some(id.clone());
        self.store.save(&next)?;
        *accounts = next;
        drop(accounts);
        // Move, rather than copy: remove the transfer artifact after durable import.
        std::fs::remove_file(path)?;
        Ok(id)
    }

    pub async fn logout(&self, id: &str) -> Result<serde_json::Value, BoxError> {
        // Clear local tokens first so no refresh can rotate them, then revoke without
        // holding the account lock across network calls.
        let (tokens, client_id) = {
            let mut accounts = self.accounts.lock().await;
            let profile = accounts
                .profiles
                .get(id)
                .ok_or("ChatGPT account not found")?;
            let taken = (profile.tokens.clone(), profile.client_id.clone());
            let mut next = accounts.clone();
            next.profiles.get_mut(id).unwrap().tokens = None;
            self.store.save(&next)?;
            *accounts = next;
            taken
        };
        if let Some(cancel) = self.sessions.lock().await.remove(id) {
            cancel.cancel();
        }
        let mut revoked = tokens.is_none();
        if let Some(tokens) = &tokens
            && let Ok(discovery) = self.discovery().await
        {
            for attempt in 0..2 {
                let result = self
                    .http
                    .post(&discovery.revocation_endpoint)
                    .timeout(Duration::from_secs(10))
                    .form(&[
                        ("token", tokens.refresh_token.as_str()),
                        ("token_type_hint", "refresh_token"),
                        ("client_id", client_id.as_str()),
                    ])
                    .send()
                    .await;
                if result.as_ref().is_ok_and(|r| r.status().is_success()) {
                    revoked = true;
                    break;
                }
                if result.as_ref().is_ok_and(|r| r.status().is_client_error()) {
                    break;
                }
                if attempt == 0 {
                    tokio::time::sleep(Duration::from_millis(300)).await;
                }
            }
        }
        Ok(serde_json::json!({"revocation_confirmed":revoked,"usage_url":USAGE_URL}))
    }
}
fn tokens_from_response(response: TokenResponse, old: Option<&Tokens>) -> Result<Tokens, BoxError> {
    let scopes: Vec<String> = response
        .scope
        .map(|s| s.split_whitespace().map(str::to_owned).collect())
        .or_else(|| old.map(|t| t.scopes.clone()))
        .unwrap_or_default();
    let enabled = scopes.iter().any(|s| s == PLAN_SCOPE);
    let refresh = response.refresh_token.unwrap_or_default();
    if enabled
        && (!response.token_type.eq_ignore_ascii_case("bearer")
            || response.access_token.is_empty()
            || response.expires_in == 0
            || refresh.is_empty())
    {
        return Err("invalid OAuth token response or missing rotating refresh token".into());
    }
    Ok(Tokens {
        access_token: response.access_token,
        refresh_token: refresh,
        id_token: response
            .id_token
            .or_else(|| old.map(|t| t.id_token.clone()))
            .unwrap_or_default(),
        expires_at: unix_seconds().saturating_add(response.expires_in),
        scopes,
    })
}
#[derive(Deserialize)]
struct Callback {
    state: Option<String>,
    code: Option<String>,
    client_id: Option<String>,
    error: Option<String>,
}
async fn callback(
    State((service, id)): State<(Arc<ChatGptService>, String)>,
    Query(query): Query<Callback>,
) -> impl IntoResponse {
    let (status, message) = match service.complete_login(&id, query).await {
        Ok(()) => (
            axum::http::StatusCode::OK,
            "ChatGPT is connected. You can close this tab and return to Anda Bot.",
        ),
        Err(_) => (
            axum::http::StatusCode::BAD_REQUEST,
            "Sign-in could not be completed. Return to Anda Bot for details or start a new attempt.",
        ),
    };
    (
        status,
        [
            ("cache-control", "no-store"),
            ("referrer-policy", "no-referrer"),
            (
                "content-security-policy",
                "default-src 'none'; frame-ancestors 'none'",
            ),
        ],
        Html(message),
    )
}

#[derive(Debug, Serialize)]
pub struct ProviderError {
    pub code: String,
    pub status: u16,
    pub request_id: Option<String>,
    pub param: Option<String>,
}
impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let recovery = match self.code.as_str() {
            "subscription_sharing_usage_limit_exceeded" => {
                "ChatGPT plan usage limit reached; manage usage at https://chatgpt.com/settings/usage"
            }
            "subscription_sharing_user_not_eligible" => {
                "This ChatGPT account or workspace is not eligible for plan usage"
            }
            "subscription_sharing_unsupported_capability" => {
                "The request includes an unsupported ChatGPT plan capability"
            }
            "invalid_grant"
            | "invalid_refresh_token"
            | "refresh_token_expired"
            | "refresh_token_reused" => "ChatGPT sign-in required",
            _ if self.status == 403 => {
                "ChatGPT access is restricted by account, workspace, or serving policy"
            }
            _ => "ChatGPT request failed",
        };
        write!(
            f,
            "{recovery} (HTTP {}, code {}, request {}, parameter {})",
            self.status,
            self.code,
            self.request_id.as_deref().unwrap_or("unknown"),
            self.param.as_deref().unwrap_or("unknown")
        )
    }
}
impl std::error::Error for ProviderError {}
pub(super) async fn read_bytes(
    response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, BoxError> {
    use futures::StreamExt;
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if bytes.len() + chunk.len() > limit {
            return Err("ChatGPT response exceeded the size limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
pub(super) async fn decode<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, BoxError> {
    let status = response.status();
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let bytes = read_bytes(response, 1024 * 1024).await?;
    if !status.is_success() {
        return Err(Box::new(response_error(
            status.as_u16(),
            request_id,
            &bytes,
        )));
    }
    serde_json::from_slice(&bytes).map_err(|_| "ChatGPT returned an invalid response".into())
}
pub(super) fn response_error(
    status: u16,
    request_id: Option<String>,
    bytes: &[u8],
) -> ProviderError {
    let value: serde_json::Value = serde_json::from_slice(bytes).unwrap_or_default();
    let code = value
        .get("error")
        .and_then(|e| {
            e.as_str()
                .or_else(|| e.get("code").and_then(|v| v.as_str()))
        })
        .unwrap_or("request_failed")
        .chars()
        .take(128)
        .collect();
    let param = value
        .pointer("/error/param")
        .and_then(|v| v.as_str())
        .map(|s| s.chars().take(128).collect());
    ProviderError {
        code,
        status,
        request_id,
        param,
    }
}
