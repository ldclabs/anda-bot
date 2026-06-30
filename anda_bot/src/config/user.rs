use anda_core::{BoxError, Principal};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr,
};

use super::{Config, normalize_optional, normalize_string};
use crate::identity::Ed25519PubKey;

pub const DEFAULT_USER_ID: &str = "default";
pub const OWNER_USER_ID: &str = "owner";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct UserSettings {
    #[serde(default)]
    pub id: Option<String>,

    #[serde(
        default,
        alias = "public_key",
        alias = "pub_key",
        alias = "user_pubkey"
    )]
    pub pubkey: String,
}

impl UserSettings {
    pub fn id(&self) -> Option<String> {
        normalize_optional(&self.id)
    }

    pub fn is_empty(&self) -> bool {
        self.id().is_none() && self.pubkey.trim().is_empty()
    }

    pub fn pubkey(&self) -> Result<Ed25519PubKey, BoxError> {
        Self::pubkey_from_str(&self.pubkey)
    }

    pub fn pubkey_from_str(value: &str) -> Result<Ed25519PubKey, BoxError> {
        Ed25519PubKey::from_str(value.trim())
    }
}

#[derive(Clone)]
pub struct UserRegistry {
    default_user: Principal,
    aliases: BTreeMap<String, Principal>,
    pubkeys: Vec<Ed25519PubKey>,
}

impl UserRegistry {
    pub fn from_config(default_user_pubkey: Ed25519PubKey, cfg: &Config) -> Result<Self, BoxError> {
        let default_user = default_user_pubkey.id();
        let mut registry = Self {
            default_user,
            aliases: BTreeMap::from([
                (DEFAULT_USER_ID.to_string(), default_user),
                (OWNER_USER_ID.to_string(), default_user),
                (default_user.to_text(), default_user),
            ]),
            pubkeys: Vec::new(),
        };
        let mut seen_pubkeys = BTreeSet::new();
        registry.push_pubkey(default_user_pubkey, &mut seen_pubkeys);

        for (index, user) in cfg.users.iter().enumerate() {
            if user.is_empty() {
                continue;
            }

            let pubkey = user
                .pubkey()
                .map_err(|err| format!("users[{index}].pubkey: {err}"))?;
            let principal = pubkey.id();
            registry.push_pubkey(pubkey, &mut seen_pubkeys);
            registry.aliases.insert(principal.to_text(), principal);
            if let Some(id) = user.id()
                && registry.aliases.insert(id.clone(), principal).is_some()
            {
                return Err(format!("duplicate user id '{id}'").into());
            }
        }

        for user_ref in cfg.channels.user_refs() {
            registry.resolve_or_register(user_ref.as_str(), &mut seen_pubkeys)?;
        }

        Ok(registry)
    }

    pub fn default_user(&self) -> Principal {
        self.default_user
    }

    pub fn pubkeys(&self) -> Vec<Ed25519PubKey> {
        self.pubkeys.clone()
    }

    pub fn resolve(&self, user_ref: Option<&str>) -> Result<Principal, BoxError> {
        let Some(user_ref) = user_ref.and_then(normalize_string) else {
            return Ok(self.default_user);
        };

        if let Some(user) = self.aliases.get(&user_ref) {
            return Ok(*user);
        }

        if let Ok(user) = Principal::from_text(&user_ref) {
            return Ok(user);
        }

        if let Ok(pubkey) = UserSettings::pubkey_from_str(&user_ref) {
            return Ok(pubkey.id());
        }

        Err(format!("unknown user '{user_ref}'").into())
    }

    fn resolve_or_register(
        &mut self,
        user_ref: &str,
        seen_pubkeys: &mut BTreeSet<Principal>,
    ) -> Result<Principal, BoxError> {
        let Some(user_ref) = normalize_string(user_ref) else {
            return Ok(self.default_user);
        };

        if let Some(user) = self.aliases.get(&user_ref) {
            return Ok(*user);
        }

        if let Ok(user) = Principal::from_text(&user_ref) {
            self.aliases.insert(user_ref, user);
            return Ok(user);
        }

        if let Ok(pubkey) = UserSettings::pubkey_from_str(&user_ref) {
            let user = pubkey.id();
            self.push_pubkey(pubkey, seen_pubkeys);
            self.aliases.insert(user_ref, user);
            self.aliases.insert(user.to_text(), user);
            return Ok(user);
        }

        Err(format!("unknown user '{user_ref}'").into())
    }

    fn push_pubkey(&mut self, pubkey: Ed25519PubKey, seen_pubkeys: &mut BTreeSet<Principal>) {
        if seen_pubkeys.insert(pubkey.id()) {
            self.pubkeys.push(pubkey);
        }
    }
}

impl Config {
    pub fn user_registry(
        &self,
        default_user_pubkey: Ed25519PubKey,
    ) -> Result<UserRegistry, BoxError> {
        UserRegistry::from_config(default_user_pubkey, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{ChannelSettings, WechatChannelSettings},
        identity::Ed25519Key,
    };
    use ic_auth_types::ByteBufB64;

    fn pubkey_string(key: &Ed25519Key) -> String {
        ByteBufB64(key.pubkey().as_bytes().to_vec()).to_string()
    }

    #[test]
    fn user_registry_resolves_channel_user_aliases() {
        let default_key = Ed25519Key::new([1; 32]);
        let teammate_key = Ed25519Key::new([2; 32]);
        let cfg = Config {
            users: vec![UserSettings {
                id: Some("alice".to_string()),
                pubkey: pubkey_string(&teammate_key),
            }],
            channels: ChannelSettings {
                wechat: vec![WechatChannelSettings {
                    id: Some("alice-wechat".to_string()),
                    user: Some("alice".to_string()),
                    bot_token: "token".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            ..Default::default()
        };

        let registry = cfg.user_registry(default_key.pubkey()).unwrap();

        assert_eq!(
            registry.resolve(Some("default")).unwrap(),
            default_key.pubkey().id()
        );
        assert_eq!(
            registry.resolve(Some("alice")).unwrap(),
            teammate_key.pubkey().id()
        );
        assert_eq!(registry.pubkeys().len(), 2);
        assert_eq!(
            cfg.channels
                .user_bindings(&registry)
                .unwrap()
                .get("wechat:alice-wechat"),
            Some(&teammate_key.pubkey().id())
        );
    }

    #[test]
    fn user_registry_rejects_unknown_channel_user_aliases() {
        let default_key = Ed25519Key::new([1; 32]);
        let cfg = Config {
            channels: ChannelSettings {
                wechat: vec![WechatChannelSettings {
                    id: Some("unknown-wechat".to_string()),
                    user: Some("missing".to_string()),
                    bot_token: "token".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            ..Default::default()
        };

        let err = match cfg.user_registry(default_key.pubkey()) {
            Ok(_) => panic!("unknown channel user should fail"),
            Err(err) => err,
        };

        assert!(err.to_string().contains("unknown user 'missing'"));
    }

    #[test]
    fn user_registry_rejects_duplicate_user_ids() {
        let default_key = Ed25519Key::new([1; 32]);
        let cfg = Config {
            users: vec![
                UserSettings::default(), // empty entries are skipped
                UserSettings {
                    id: Some("alice".to_string()),
                    pubkey: pubkey_string(&Ed25519Key::new([2; 32])),
                },
                UserSettings {
                    id: Some("alice".to_string()),
                    pubkey: pubkey_string(&Ed25519Key::new([3; 32])),
                },
            ],
            ..Default::default()
        };

        let err = match cfg.user_registry(default_key.pubkey()) {
            Ok(_) => panic!("duplicate user id should fail"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("duplicate user id 'alice'"));
    }

    #[test]
    fn user_registry_resolves_principals_and_raw_pubkeys() {
        let default_key = Ed25519Key::new([1; 32]);
        let registry = Config::default()
            .user_registry(default_key.pubkey())
            .unwrap();

        assert_eq!(registry.default_user(), default_key.pubkey().id());
        assert_eq!(registry.resolve(None).unwrap(), default_key.pubkey().id());
        assert_eq!(
            registry.resolve(Some(" ")).unwrap(),
            default_key.pubkey().id()
        );

        // Principal text and raw pubkey strings resolve without registration.
        let other = Ed25519Key::new([4; 32]);
        assert_eq!(
            registry
                .resolve(Some(&other.pubkey().id().to_text()))
                .unwrap(),
            other.pubkey().id()
        );
        assert_eq!(
            registry.resolve(Some(&pubkey_string(&other))).unwrap(),
            other.pubkey().id()
        );

        let err = registry.resolve(Some("nobody")).map(|_| ()).unwrap_err();
        assert!(err.to_string().contains("unknown user 'nobody'"));
    }

    #[test]
    fn channel_user_refs_register_principals_and_pubkeys() {
        let default_key = Ed25519Key::new([1; 32]);
        let by_principal = Ed25519Key::new([5; 32]);
        let by_pubkey = Ed25519Key::new([6; 32]);
        let cfg = Config {
            channels: ChannelSettings {
                wechat: vec![
                    WechatChannelSettings {
                        id: Some("via-principal".to_string()),
                        user: Some(by_principal.pubkey().id().to_text()),
                        bot_token: "token".to_string(),
                        ..Default::default()
                    },
                    WechatChannelSettings {
                        id: Some("via-pubkey".to_string()),
                        user: Some(pubkey_string(&by_pubkey)),
                        bot_token: "token".to_string(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            ..Default::default()
        };

        let registry = cfg.user_registry(default_key.pubkey()).unwrap();
        let bindings = cfg.channels.user_bindings(&registry).unwrap();

        assert_eq!(
            bindings.get("wechat:via-principal"),
            Some(&by_principal.pubkey().id())
        );
        assert_eq!(
            bindings.get("wechat:via-pubkey"),
            Some(&by_pubkey.pubkey().id())
        );
        // The pubkey-referenced user is registered into the key list.
        assert_eq!(registry.pubkeys().len(), 2);
    }
}
