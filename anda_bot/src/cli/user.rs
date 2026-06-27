use anda_core::BoxError;
use clap::{Args, Subcommand};
use std::{path::PathBuf, str::FromStr, sync::Arc};

use crate::{
    config::{Config, DEFAULT_USER_ID, OWNER_USER_ID},
    daemon::Daemon,
    util::{
        key::{
            Ed25519Key, Ed25519PubKey, IdentityKeyStore, encode_ed25519_pubkey,
            load_or_init_identity_secret_with_store, os_identity_key_store, owner_identity_key,
            random_ed25519_privkey, trusted_user_identity_key, write_ed25519_secret_file,
            write_identity_secret_with_store,
        },
        text::read_text_file,
    },
};

#[derive(Args)]
pub struct UserCommand {
    #[command(subcommand)]
    command: Option<UserSubcommand>,
}

#[derive(Subcommand)]
enum UserSubcommand {
    /// List trusted users configured for this daemon.
    List,
    /// Generate a new Ed25519 keypair and add the public key to config.yaml.
    Create(UserCreateCommand),
    /// Add an existing Ed25519 public key to config.yaml.
    Import(UserImportCommand),
    /// Print the public key and principal for an Ed25519 private key file.
    Pubkey(UserPubkeyCommand),
}

#[derive(Args)]
pub struct UserCreateCommand {
    /// User id to add under top-level `users`.
    #[arg(value_name = "ID")]
    id: String,

    /// Where to save the generated private key instead of the system keyring.
    #[arg(long, value_name = "PATH")]
    key_path: Option<PathBuf>,

    /// Overwrite the private key when it already exists.
    #[arg(long)]
    overwrite_key: bool,
}

#[derive(Args)]
pub struct UserImportCommand {
    /// User id to add under top-level `users`.
    #[arg(value_name = "ID")]
    id: String,

    /// Existing Ed25519 public key, encoded as raw 32-byte base64url or COSE OKP.
    #[arg(long)]
    pubkey: String,
}

#[derive(Args)]
pub struct UserPubkeyCommand {
    /// Ed25519 private key file.
    #[arg(value_name = "PATH")]
    key_path: PathBuf,
}

pub async fn run(daemon: &Daemon, cmd: UserCommand) -> Result<(), BoxError> {
    run_with_store(daemon, cmd, os_identity_key_store()).await
}

async fn run_with_store(
    daemon: &Daemon,
    cmd: UserCommand,
    identity_store: Arc<dyn IdentityKeyStore>,
) -> Result<(), BoxError> {
    match cmd.command.unwrap_or(UserSubcommand::List) {
        UserSubcommand::List => list_users(daemon, identity_store).await,
        UserSubcommand::Create(cmd) => create_user(daemon, cmd, identity_store).await,
        UserSubcommand::Import(cmd) => import_user(daemon, cmd).await,
        UserSubcommand::Pubkey(cmd) => print_pubkey(cmd).await,
    }
}

async fn list_users(
    daemon: &Daemon,
    identity_store: Arc<dyn IdentityKeyStore>,
) -> Result<(), BoxError> {
    let cfg = load_cli_config(daemon).await?;
    let owner_key_ref = owner_identity_key(&daemon.home);
    let owner_secret =
        load_or_init_identity_secret_with_store(&owner_key_ref, identity_store).await?;
    let owner_key = Ed25519Key::new(owner_secret);
    let owner_pubkey = owner_key.pubkey();

    println!("Trusted users:");
    println!("- default, owner");
    println!("  principal: {}", owner_pubkey.id().to_text());
    println!("  pubkey: {}", encode_ed25519_pubkey(&owner_pubkey));
    println!("  private_key: {}", owner_key_ref.location());

    let mut listed = 0usize;
    for (index, user) in cfg.users.iter().enumerate() {
        if user.is_empty() {
            continue;
        }
        let pubkey = user
            .pubkey()
            .map_err(|err| format!("users[{index}].pubkey: {err}"))?;
        let id = user.id().unwrap_or_else(|| pubkey.id().to_text());
        println!("- {id}");
        println!("  principal: {}", pubkey.id().to_text());
        println!("  pubkey: {}", encode_ed25519_pubkey(&pubkey));
        listed += 1;
    }

    if listed == 0 {
        println!(
            "No additional users are configured in {}.",
            daemon.config_file_path().display()
        );
    }
    Ok(())
}

async fn create_user(
    daemon: &Daemon,
    cmd: UserCreateCommand,
    identity_store: Arc<dyn IdentityKeyStore>,
) -> Result<(), BoxError> {
    let id = validate_new_user_id(&cmd.id)?;
    let cfg_text = load_config_text(daemon).await?;
    let cfg = Config::from_contents(&cfg_text)?;
    ensure_user_id_available(&cfg, &id)?;

    let secret = random_ed25519_privkey();
    let key = Ed25519Key::new(secret);
    let pubkey = encode_ed25519_pubkey(&key.pubkey());
    let updated = add_user_to_config_text(&cfg_text, &id, &pubkey)?;
    let private_key_location = if let Some(key_path) = cmd.key_path {
        if !cmd.overwrite_key && tokio::fs::metadata(&key_path).await.is_ok() {
            return Err(format!(
                "{} already exists; pass --overwrite-key or choose --key-path",
                key_path.display()
            )
            .into());
        }

        write_ed25519_secret_file(&key_path, &secret, cmd.overwrite_key).await?;
        key_path.display().to_string()
    } else {
        let key_ref = trusted_user_identity_key(&daemon.home, &id);
        write_identity_secret_with_store(&key_ref, &secret, cmd.overwrite_key, identity_store)
            .await?;
        key_ref.location()
    };

    tokio::fs::write(daemon.config_file_path(), updated).await?;

    println!("Created user '{id}'.");
    println!("Config: {}", daemon.config_file_path().display());
    println!("Private key: {private_key_location}");
    println!("Public key: {pubkey}");
    println!("Principal: {}", key.id().to_text());
    println!("Use in channel config: user: {id}");
    println!("Run `anda reload` if the daemon is already running.");
    Ok(())
}

async fn import_user(daemon: &Daemon, cmd: UserImportCommand) -> Result<(), BoxError> {
    let id = validate_new_user_id(&cmd.id)?;
    let pubkey = Ed25519PubKey::from_str(cmd.pubkey.trim())?;
    let pubkey_text = encode_ed25519_pubkey(&pubkey);
    let cfg_text = load_config_text(daemon).await?;
    let cfg = Config::from_contents(&cfg_text)?;
    ensure_user_id_available(&cfg, &id)?;
    let updated = add_user_to_config_text(&cfg_text, &id, &pubkey_text)?;

    tokio::fs::write(daemon.config_file_path(), updated).await?;

    println!("Imported user '{id}'.");
    println!("Config: {}", daemon.config_file_path().display());
    println!("Public key: {pubkey_text}");
    println!("Principal: {}", pubkey.id().to_text());
    println!("Use in channel config: user: {id}");
    println!("Run `anda reload` if the daemon is already running.");
    Ok(())
}

async fn print_pubkey(cmd: UserPubkeyCommand) -> Result<(), BoxError> {
    let content = read_text_file(&cmd.key_path).await?;
    let key = Ed25519Key::from_str(content.trim())?;
    let pubkey = key.pubkey();

    println!("Private key: {}", cmd.key_path.display());
    println!("Public key: {}", encode_ed25519_pubkey(&pubkey));
    println!("Principal: {}", pubkey.id().to_text());
    Ok(())
}

async fn load_cli_config(daemon: &Daemon) -> Result<Config, BoxError> {
    let content = load_config_text(daemon).await?;
    Config::from_contents(&content)
}

async fn load_config_text(daemon: &Daemon) -> Result<String, BoxError> {
    daemon.ensure_directories().await?;
    daemon.ensure_config_file_exists().await?;
    Ok(read_text_file(daemon.config_file_path()).await?)
}

#[cfg(test)]
fn default_user_key_path(daemon: &Daemon, id: &str) -> PathBuf {
    daemon
        .keys_dir_path()
        .join("users")
        .join(format!("{id}.key"))
}

fn validate_new_user_id(raw: &str) -> Result<String, BoxError> {
    let id = raw.trim();
    if id.is_empty() {
        return Err("user id cannot be empty".into());
    }
    if id == DEFAULT_USER_ID || id == OWNER_USER_ID {
        return Err(format!("'{id}' is reserved for the local owner").into());
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err("user id may contain only ASCII letters, digits, '.', '_', and '-'".into());
    }
    Ok(id.to_string())
}

fn ensure_user_id_available(cfg: &Config, id: &str) -> Result<(), BoxError> {
    if cfg
        .users
        .iter()
        .filter_map(|user| user.id())
        .any(|existing| existing == id)
    {
        return Err(format!("user '{id}' already exists in config.yaml").into());
    }
    Ok(())
}

fn add_user_to_config_text(content: &str, id: &str, pubkey: &str) -> Result<String, BoxError> {
    let _ = Config::from_contents(content)?;
    let pubkey = Ed25519PubKey::from_str(pubkey)?;
    let entry = user_entry_lines(id, &encode_ed25519_pubkey(&pubkey));
    let mut lines: Vec<String> = content.lines().map(ToOwned::to_owned).collect();

    if let Some(users_index) = find_top_level_key(&lines, "users") {
        insert_user_entry_into_existing_users(&mut lines, users_index, entry)?;
    } else {
        insert_new_users_block(&mut lines, entry);
    }

    let updated = render_lines(lines, content.ends_with('\n'));
    let updated_cfg = Config::from_contents(&updated)?;
    if !updated_cfg
        .users
        .iter()
        .any(|user| user.id().as_deref() == Some(id))
    {
        return Err(format!("failed to add user '{id}' to config.yaml").into());
    }
    Ok(updated)
}

fn insert_user_entry_into_existing_users(
    lines: &mut Vec<String>,
    users_index: usize,
    entry: Vec<String>,
) -> Result<(), BoxError> {
    let value = top_level_value(&lines[users_index]).unwrap_or_default();
    if value == "[]" {
        lines[users_index] = "users:".to_string();
        insert_lines(lines, users_index + 1, entry);
        return Ok(());
    }
    if !value.is_empty() {
        return Err("top-level `users` must be a block list before `anda user` can edit it".into());
    }

    let mut insert_index = next_top_level_key(lines, users_index + 1).unwrap_or(lines.len());
    while insert_index > users_index + 1 && lines[insert_index - 1].trim().is_empty() {
        insert_index -= 1;
    }
    insert_lines(lines, insert_index, entry);
    Ok(())
}

fn insert_new_users_block(lines: &mut Vec<String>, entry: Vec<String>) {
    let insert_index = find_top_level_key(lines, "model").unwrap_or(lines.len());
    let mut block = vec!["users:".to_string()];
    block.extend(entry);

    if insert_index > 0 && !lines[insert_index - 1].trim().is_empty() {
        block.insert(0, String::new());
    }
    if insert_index < lines.len() && !lines[insert_index].trim().is_empty() {
        block.push(String::new());
    }
    insert_lines(lines, insert_index, block);
}

fn user_entry_lines(id: &str, pubkey: &str) -> Vec<String> {
    vec![
        format!("  - id: {}", yaml_quote(id)),
        format!("    pubkey: {}", yaml_quote(pubkey)),
    ]
}

fn insert_lines(lines: &mut Vec<String>, index: usize, items: Vec<String>) {
    lines.splice(index..index, items);
}

fn render_lines(lines: Vec<String>, trailing_newline: bool) -> String {
    let mut output = lines.join("\n");
    if trailing_newline || !output.is_empty() {
        output.push('\n');
    }
    output
}

fn find_top_level_key(lines: &[String], key: &str) -> Option<usize> {
    lines
        .iter()
        .position(|line| top_level_key(line).as_deref() == Some(key))
}

fn next_top_level_key(lines: &[String], start: usize) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, line)| top_level_key(line).map(|_| index))
}

fn top_level_key(line: &str) -> Option<String> {
    if line.starts_with(' ') || line.starts_with('\t') {
        return None;
    }
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let (key, _) = trimmed.split_once(':')?;
    key.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
        .then(|| key.to_string())
}

fn top_level_value(line: &str) -> Option<String> {
    let (_, value) = line.split_once(':')?;
    Some(
        value
            .split('#')
            .next()
            .unwrap_or_default()
            .trim()
            .to_string(),
    )
}

fn yaml_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::key::encode_ed25519_pubkey;

    fn test_pubkey() -> String {
        encode_ed25519_pubkey(&Ed25519Key::new([7; 32]).pubkey())
    }

    #[test]
    fn add_user_inserts_block_before_model_when_missing() {
        let updated = add_user_to_config_text(
            r#"addr: 127.0.0.1:8042
# users:
#   - id: alice
#     pubkey: ""
model:
  active: test
"#,
            "alice",
            &test_pubkey(),
        )
        .unwrap();

        assert!(updated.contains("users:\n  - id: \"alice\"\n    pubkey: \""));
        assert!(updated.find("users:").unwrap() < updated.find("model:").unwrap());
        assert_eq!(
            Config::from_contents(&updated).unwrap().users[0]
                .id()
                .as_deref(),
            Some("alice")
        );
    }

    #[test]
    fn add_user_appends_to_existing_block() {
        let updated = add_user_to_config_text(
            r#"users:
  - id: "alice"
    pubkey: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"

channels: {}
"#,
            "bob",
            &test_pubkey(),
        )
        .unwrap();

        assert!(updated.contains("  - id: \"bob\"\n    pubkey: \""));
        assert!(updated.contains("\n\nchannels: {}"));
    }

    #[test]
    fn add_user_expands_empty_inline_users() {
        let updated =
            add_user_to_config_text("users: []\nmodel: {}\n", "alice", &test_pubkey()).unwrap();

        assert!(updated.starts_with("users:\n  - id: \"alice\"\n"));
        assert!(
            Config::from_contents(&updated).unwrap().users[0]
                .pubkey()
                .is_ok()
        );
    }

    #[test]
    fn add_user_rejects_inline_users_with_values() {
        let err = add_user_to_config_text(
            r#"users: [{id: alice, pubkey: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}]
"#,
            "bob",
            &test_pubkey(),
        )
        .unwrap_err();

        assert!(err.to_string().contains("block list"));
    }

    #[test]
    fn validate_user_id_rejects_reserved_and_path_like_ids() {
        assert!(validate_new_user_id("alice-1").is_ok());
        assert!(validate_new_user_id("owner").is_err());
        assert!(validate_new_user_id("../alice").is_err());
        assert!(validate_new_user_id("alice/team").is_err());
    }

    fn temp_daemon() -> (tempfile::TempDir, Daemon) {
        let dir = tempfile::tempdir().unwrap();
        let daemon = Daemon::new(dir.path().to_path_buf(), Config::default());
        (dir, daemon)
    }

    #[test]
    fn default_user_key_path_lives_under_keys_users() {
        let (_dir, daemon) = temp_daemon();
        let path = default_user_key_path(&daemon, "alice");
        assert!(path.ends_with("keys/users/alice.key"));
    }

    #[test]
    fn ensure_user_id_available_detects_duplicates() {
        let cfg = Config::from_contents(
            "users:\n  - id: \"alice\"\n    pubkey: \"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"\n",
        )
        .unwrap();
        assert!(ensure_user_id_available(&cfg, "bob").is_ok());
        assert!(ensure_user_id_available(&cfg, "alice").is_err());
    }

    #[tokio::test]
    async fn run_defaults_to_listing_users() {
        let (_dir, daemon) = temp_daemon();
        run_with_store(
            &daemon,
            UserCommand { command: None },
            Arc::new(crate::util::key::MemoryIdentityKeyStore::default()),
        )
        .await
        .expect("list users should succeed");
    }

    #[tokio::test]
    async fn create_then_list_and_reject_duplicate_key() {
        let (_dir, daemon) = temp_daemon();
        let identity_store = Arc::new(crate::util::key::MemoryIdentityKeyStore::default());

        create_user(
            &daemon,
            UserCreateCommand {
                id: "alice".to_string(),
                key_path: None,
                overwrite_key: false,
            },
            identity_store.clone(),
        )
        .await
        .expect("create should succeed");

        // The config now contains the new user and the key file exists.
        let cfg = load_cli_config(&daemon).await.unwrap();
        assert!(cfg.users.iter().any(|u| u.id().as_deref() == Some("alice")));
        let key_ref = trusted_user_identity_key(&daemon.home, "alice");
        assert!(identity_store.get_for_test(key_ref.account()).is_some());
        assert!(!default_user_key_path(&daemon, "alice").exists());

        // Listing now iterates the configured users.
        run_with_store(
            &daemon,
            UserCommand {
                command: Some(UserSubcommand::List),
            },
            identity_store.clone(),
        )
        .await
        .unwrap();

        // Re-creating with the same id is rejected (already in config).
        let err = create_user(
            &daemon,
            UserCreateCommand {
                id: "alice".to_string(),
                key_path: None,
                overwrite_key: false,
            },
            identity_store.clone(),
        )
        .await
        .map(|_| ())
        .unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[tokio::test]
    async fn create_user_rejects_existing_key_without_overwrite() {
        let (_dir, daemon) = temp_daemon();
        let key_path = daemon.home.join("preexisting.key");
        tokio::fs::write(&key_path, "dummy").await.unwrap();
        let identity_store = Arc::new(crate::util::key::MemoryIdentityKeyStore::default());

        let err = create_user(
            &daemon,
            UserCreateCommand {
                id: "carol".to_string(),
                key_path: Some(key_path),
                overwrite_key: false,
            },
            identity_store,
        )
        .await
        .map(|_| ())
        .unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[tokio::test]
    async fn import_user_adds_pubkey_and_validates_input() {
        let (_dir, daemon) = temp_daemon();

        import_user(
            &daemon,
            UserImportCommand {
                id: "dave".to_string(),
                pubkey: test_pubkey(),
            },
        )
        .await
        .expect("import should succeed");

        let cfg = load_cli_config(&daemon).await.unwrap();
        assert!(cfg.users.iter().any(|u| u.id().as_deref() == Some("dave")));

        // An invalid public key is rejected.
        let err = import_user(
            &daemon,
            UserImportCommand {
                id: "eve".to_string(),
                pubkey: "not-a-valid-key".to_string(),
            },
        )
        .await
        .map(|_| ())
        .unwrap_err();
        assert!(!err.to_string().is_empty());
    }

    #[tokio::test]
    async fn print_pubkey_reads_private_key_file() {
        let (_dir, daemon) = temp_daemon();
        let key_path = daemon.home.join("owner.key");
        let secret = random_ed25519_privkey();
        write_ed25519_secret_file(&key_path, &secret, false)
            .await
            .unwrap();

        print_pubkey(UserPubkeyCommand {
            key_path: key_path.clone(),
        })
        .await
        .expect("print pubkey should succeed");

        // A missing file surfaces an error.
        assert!(
            print_pubkey(UserPubkeyCommand {
                key_path: daemon.home.join("missing.key"),
            })
            .await
            .is_err()
        );
    }
}
