//! Minimal owner inbox configuration, installed only after an explicit restart.
use super::Journal;
use anda_core::{BoxError, Principal};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::io::AsyncWriteExt;

const RUNTIME_FILE: &str = "brain-inbox.runtime.yaml";
const JOURNAL_KEY: &str = "inbox-setup/v1";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SetupPreview {
    pub schema_version: u32,
    pub preview_digest: String,
    pub state: String,
    pub expires_at: u64,
    pub runtime_file: String,
    pub config_file: String,
    pub restart_required: bool,
    #[serde(default)]
    pub changes: Vec<serde_json::Value>,
    #[serde(default)]
    pub managed_runtime: String,
}

#[derive(Clone, Deserialize, Serialize)]
struct SetupRecord {
    caller: String,
    config_digest: String,
    updated_config_digest: String,
    runtime: String,
    view: SetupPreview,
}

#[derive(Clone)]
pub struct InboxSetup {
    home: PathBuf,
    journal: Journal,
    lock: Arc<tokio::sync::Mutex<()>>,
    override_present: bool,
}

impl InboxSetup {
    pub fn new(home: PathBuf, journal: Journal, lock: Arc<tokio::sync::Mutex<()>>) -> Self {
        Self {
            home,
            journal,
            lock,
            override_present: std::env::var_os("BRAIN_RUNTIME_CONFIG").is_some(),
        }
    }

    pub async fn prepare(&self, caller: Principal) -> Result<SetupPreview, BoxError> {
        self.reject_override()?;
        let _guard = self.lock.lock().await;
        if let Some(record) = self.journal.read::<SetupRecord>(JOURNAL_KEY).await?
            && record.caller == caller.to_string()
            && record.view.state == "applying"
        {
            return Ok(record.view);
        }
        let bytes = tokio::fs::read(self.home.join(crate::config::CONFIG_FILE_NAME)).await?;
        let text =
            crate::util::text::read_text_file(&self.home.join(crate::config::CONFIG_FILE_NAME))
                .await?;
        let config = crate::config::Config::from_contents(&text)?;
        if config.brain.runtime_config.as_deref() == Some(Path::new(RUNTIME_FILE)) {
            let old = self
                .journal
                .read::<SetupRecord>(JOURNAL_KEY)
                .await?
                .ok_or("custom_runtime_config")?;
            if old.caller == caller.to_string()
                && tokio::fs::read(self.home.join(RUNTIME_FILE)).await? == old.runtime.as_bytes()
            {
                return Ok(old.view);
            }
        }
        if config.brain.runtime_config.is_some() {
            return Err("custom_runtime_config".into());
        }
        let runtime = runtime_config(caller)?;
        let runtime_path = self.home.join(RUNTIME_FILE);
        match tokio::fs::symlink_metadata(&runtime_path).await {
            Ok(_) => {
                let old = self
                    .journal
                    .read::<SetupRecord>(JOURNAL_KEY)
                    .await?
                    .ok_or("runtime_file_exists")?;
                if old.caller != caller.to_string()
                    || old.runtime != runtime
                    || tokio::fs::read(&runtime_path).await? != runtime.as_bytes()
                {
                    return Err("runtime_file_exists".into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let config_digest = digest(&bytes);
        let updated = update_config(&text)?;
        let preview_digest = anda_cognitive_nexus::content_digest(
            &json!({"caller":caller.to_string(),"config":config_digest,"runtime":runtime,"effect":"owner-inbox-only-v1"}),
        )?;
        let view = SetupPreview {
            schema_version: 1,
            preview_digest,
            state: "prepared".into(),
            expires_at: anda_engine::unix_ms() + 600_000,
            runtime_file: RUNTIME_FILE.into(),
            config_file: crate::config::CONFIG_FILE_NAME.into(),
            restart_required: true,
            changes: vec![
                json!({"path":"brain.runtime_config","before":null,"after":RUNTIME_FILE,"reformats_config":true,"private_backup":true}),
            ],
            managed_runtime: runtime.clone(),
        };
        self.journal
            .write(
                JOURNAL_KEY,
                &SetupRecord {
                    caller: caller.to_string(),
                    config_digest,
                    updated_config_digest: digest(updated.as_bytes()),
                    runtime,
                    view: view.clone(),
                },
            )
            .await?;
        Ok(view)
    }

    pub async fn commit(
        &self,
        caller: Principal,
        preview_digest: &str,
    ) -> Result<SetupPreview, BoxError> {
        self.reject_override()?;
        let _guard = self.lock.lock().await;
        let mut record = self
            .journal
            .read::<SetupRecord>(JOURNAL_KEY)
            .await?
            .ok_or("not_found")?;
        if record.caller != caller.to_string() || record.view.preview_digest != preview_digest {
            return Err("revision_conflict".into());
        }
        if record.view.state == "prepared" && anda_engine::unix_ms() > record.view.expires_at {
            return Err("preview_expired".into());
        }
        self.apply(&mut record).await?;
        Ok(record.view)
    }

    async fn apply(&self, record: &mut SetupRecord) -> Result<(), BoxError> {
        let path = self.home.join(crate::config::CONFIG_FILE_NAME);
        let bytes = tokio::fs::read(&path).await?;
        let runtime_path = self.home.join(RUNTIME_FILE);
        if digest(&bytes) == record.updated_config_digest {
            if tokio::fs::read(&runtime_path).await? != record.runtime.as_bytes() {
                return Err("revision_conflict".into());
            }
            record.view.state = "restart_required".into();
            self.journal.write(JOURNAL_KEY, record).await?;
            return Ok(());
        }
        if digest(&bytes) != record.config_digest {
            return Err("revision_conflict".into());
        }
        let text = crate::util::text::read_text_file(&path).await?;
        let updated = update_config(&text)?;
        if digest(updated.as_bytes()) != record.updated_config_digest {
            return Err("revision_conflict".into());
        }
        record.view.state = "applying".into();
        self.journal.write(JOURNAL_KEY, record).await?;
        // Each file is atomic, and the journal makes interruption between the
        // two writes recoverable. Backups contain credentials: create privately.
        let backup = self.home.join(format!(
            "config.before-memory-{}.yaml",
            &record.config_digest[7..23]
        ));
        create_exact(&backup, &bytes).await?;
        create_exact(&runtime_path, record.runtime.as_bytes()).await?;
        crate::engine::write_daemon_config_atomically(&path, updated.as_bytes()).await?;
        record.view.state = "restart_required".into();
        self.journal.write(JOURNAL_KEY, record).await?;
        Ok(())
    }

    /// Recovery resumes only a previously admitted apply, never a prepared UI preview.
    pub async fn recover(&self) -> Result<(), BoxError> {
        let _guard = self.lock.lock().await;
        if let Some(mut record) = self.journal.read::<SetupRecord>(JOURNAL_KEY).await?
            && record.view.state == "applying"
        {
            self.reject_override()?;
            self.apply(&mut record).await?;
        }
        Ok(())
    }

    pub async fn run(&self, cancel: tokio_util::sync::CancellationToken) {
        loop {
            let failed = match self.recover().await {
                Ok(()) => false,
                Err(error) => {
                    log::warn!("Inbox configuration needs review: {error}");
                    true
                }
            };
            tokio::select! {_=cancel.cancelled()=>break,_=tokio::time::sleep(std::time::Duration::from_secs(if failed {60} else {5}))=>{}}
        }
    }

    fn reject_override(&self) -> Result<(), BoxError> {
        if self.override_present {
            Err("external_config_override".into())
        } else {
            Ok(())
        }
    }
}

fn digest(bytes: &[u8]) -> String {
    format!(
        "sha256:{}",
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}
fn update_config(text: &str) -> Result<String, BoxError> {
    let mut value: serde_json::Value = serde_saphyr::from_str(text)?;
    let map = value.as_object_mut().ok_or("config must be an object")?;
    let brain = map.entry("brain").or_insert_with(|| json!({}));
    if brain.is_null() {
        *brain = json!({});
    }
    brain
        .as_object_mut()
        .ok_or("invalid brain configuration")?
        .insert("runtime_config".into(), RUNTIME_FILE.into());
    let result = serde_saphyr::to_string(&value)?;
    crate::config::Config::from_contents(&result)?;
    Ok(result)
}

fn runtime_config(caller: Principal) -> Result<String, BoxError> {
    if caller == Principal::anonymous() {
        return Err("unauthorized".into());
    }
    let config: anda_brain::runtime_api::config::RuntimeConfig = serde_json::from_value(
        json!({"format":"anda-brain:runtime-api-v1","spaces":{"anda_bot":{
            "bootstrap":true,"subjects":[{"credential":{"kind":"cwt_subject","subject":caller.to_string()},"principal":"kip:principal:bot-owner","observer":false,"audit_recipients":false}],"audience":["kip:principal:bot-owner"],"observers":[],
            "adapter":{"id":"attention_inbox_v1","controller_principal":"kip:principal:bot-attention-controller","recipient_principal":"kip:principal:bot-owner","message":"A memory changed / 一条记忆发生变化","question":"Does this memory need an update? / 这条记忆是否需要更新？","reply_timeout_ms":86400000,"context":null},"semantic":null,"utility":null,"trust":null,"learning":null
        }}}),
    )?;
    config.validate(|_| None)?;
    Ok(serde_saphyr::to_string(&config)?)
}

pub(super) async fn create_exact(path: &Path, bytes: &[u8]) -> Result<(), BoxError> {
    if tokio::fs::symlink_metadata(path).await.is_ok() {
        if tokio::fs::symlink_metadata(path)
            .await?
            .file_type()
            .is_symlink()
            || tokio::fs::read(path).await? != bytes
        {
            return Err("runtime_file_exists".into());
        }
        return Ok(());
    }
    let temp = path.with_extension(format!("{}.tmp", ic_auth_types::Xid::new()));
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&temp).await?;
    file.write_all(bytes).await?;
    file.sync_all().await?;
    drop(file);
    // Link creates the final name atomically without replacing an existing file.
    let result = tokio::fs::hard_link(&temp, path).await;
    let _ = tokio::fs::remove_file(&temp).await;
    match result {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if tokio::fs::read(path).await? == bytes {
                Ok(())
            } else {
                Err("runtime_file_exists".into())
            }
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn memory_setup_recovers_after_runtime_write_without_replacing_admitted_intent() {
        let home = tempfile::tempdir().unwrap();
        tokio::fs::write(home.path().join("config.yaml"), "addr: 127.0.0.1:8042\n")
            .await
            .unwrap();
        let mut setup = InboxSetup::new(
            home.path().into(),
            Journal::new(Arc::new(object_store::memory::InMemory::new())),
            Default::default(),
        );
        setup.override_present = false;
        let owner = crate::identity::Ed25519Key::new([83; 32]).id();
        let preview = setup.prepare(owner).await.unwrap();
        let mut record = setup
            .journal
            .read::<SetupRecord>(JOURNAL_KEY)
            .await
            .unwrap()
            .unwrap();
        record.view.state = "applying".into();
        setup.journal.write(JOURNAL_KEY, &record).await.unwrap();
        create_exact(&home.path().join(RUNTIME_FILE), record.runtime.as_bytes())
            .await
            .unwrap();
        assert_eq!(setup.prepare(owner).await.unwrap().state, "applying");
        let restarted = setup.clone();
        restarted.recover().await.unwrap();
        assert_eq!(
            restarted
                .commit(owner, &preview.preview_digest)
                .await
                .unwrap()
                .state,
            "restart_required"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for name in ["config.yaml", RUNTIME_FILE] {
                assert_eq!(
                    std::fs::metadata(home.path().join(name))
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o600
                )
            }
        }
        setup.override_present = true;
        assert!(
            setup
                .prepare(owner)
                .await
                .unwrap_err()
                .to_string()
                .contains("external_config_override")
        );
    }
    #[tokio::test]
    async fn memory_setup_previews_without_writing_and_applies_only_the_checked_config() {
        let home = tempfile::tempdir().unwrap();
        tokio::fs::write(
            home.path().join("config.yaml"),
            "addr: 127.0.0.1:8042\nmodel:\n  providers: []\n",
        )
        .await
        .unwrap();
        let mut service = InboxSetup::new(
            home.path().into(),
            Journal::new(Arc::new(object_store::memory::InMemory::new())),
            Default::default(),
        );
        service.override_present = false;
        let owner = crate::identity::Ed25519Key::new([81; 32]).id();
        let preview = service.prepare(owner).await.unwrap();
        assert!(!home.path().join(RUNTIME_FILE).exists());
        let view = service
            .commit(owner, &preview.preview_digest)
            .await
            .unwrap();
        assert_eq!(view.state, "restart_required");
        let cfg = crate::config::Config::from_file(&home.path().join("config.yaml"))
            .await
            .unwrap();
        assert_eq!(cfg.brain.runtime_config, Some(RUNTIME_FILE.into()));
        assert!(
            tokio::fs::read_to_string(home.path().join(RUNTIME_FILE))
                .await
                .unwrap()
                .contains(&owner.to_string())
        );
        assert_eq!(
            service
                .commit(owner, &preview.preview_digest)
                .await
                .unwrap()
                .state,
            "restart_required"
        );
    }
    #[tokio::test]
    async fn memory_setup_rejects_config_races_and_preserves_custom_runtime() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("config.yaml");
        tokio::fs::write(&path, "addr: 127.0.0.1:8042\n")
            .await
            .unwrap();
        let mut service = InboxSetup::new(
            home.path().into(),
            Journal::new(Arc::new(object_store::memory::InMemory::new())),
            Default::default(),
        );
        service.override_present = false;
        let owner = crate::identity::Ed25519Key::new([82; 32]).id();
        let preview = service.prepare(owner).await.unwrap();
        tokio::fs::write(&path, "brain:\n  runtime_config: custom.yaml\n")
            .await
            .unwrap();
        assert!(
            service
                .commit(owner, &preview.preview_digest)
                .await
                .is_err()
        );
        assert!(service.prepare(owner).await.is_err());
        assert!(!home.path().join(RUNTIME_FILE).exists());
    }
}
