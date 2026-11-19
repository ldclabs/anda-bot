use super::{Failure, worker::SOURCE_LIMIT};
use anda_core::{FunctionDefinition, validate_function_name};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;

const CAPACITY: usize = 64;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Declaration {
    pub name: String,
    pub description: String,
    pub signature: String,
    pub parameters: Value,
    pub defaults: BTreeMap<String, Value>,
}

impl Declaration {
    /// Treat declarations coming back over the worker pipe like on-disk input.
    fn validate(&self) -> Result<(), Failure> {
        valid_name(&self.name)?;
        let invalid = || Failure::new("invalid_source", "invalid saved Python declaration");
        let p = &self.parameters;
        let props = p["properties"].as_object().ok_or_else(invalid)?;
        let required = p["required"].as_array().ok_or_else(invalid)?;
        if self.description.trim().is_empty()
            || self.description.len() > 6400
            || self.signature.len() > SOURCE_LIMIT
            || p.as_object().is_none_or(|obj| obj.len() != 4)
            || p["type"] != "object"
            || p["additionalProperties"] != false
            || required.len() != props.len()
            || !props
                .iter()
                .all(|(name, s)| required.iter().any(|v| v == name) && parameter_type(s))
            || self.defaults.iter().any(|(k, v)| {
                v.is_array() || v.is_object() || props.get(k).is_none_or(|s| !matches_type(s, v))
            })
        {
            return Err(invalid());
        }
        Ok(())
    }
}

fn parameter_type(schema: &Value) -> bool {
    let Some(obj) = schema.as_object() else {
        return false;
    };
    if obj
        .keys()
        .any(|k| !matches!(k.as_str(), "type" | "description" | "items"))
    {
        return false;
    }
    let kind = match &schema["type"] {
        Value::String(k) => Some(k.as_str()),
        Value::Array(types) if types.len() == 2 && types[1] == "null" => types[0].as_str(),
        _ => None,
    };
    match kind {
        Some("array") => parameter_type(&schema["items"]),
        Some("string" | "boolean" | "integer" | "number") => !obj.contains_key("items"),
        _ => false,
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Script {
    format: u8,
    updated_at: u64,
    pub revision: String,
    pub source: String,
    #[serde(flatten)]
    pub declaration: Declaration,
}

impl Script {
    pub fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.declaration.name.clone(),
            description: self.declaration.description.clone(),
            parameters: self.declaration.parameters.clone(),
            strict: Some(true),
        }
    }
    pub fn arguments(&self, mut args: Value, python: bool) -> Result<Value, Failure> {
        let values = args
            .as_object_mut()
            .ok_or_else(|| Failure::new("invalid_arguments", "expected named arguments"))?;
        if python {
            for (key, value) in &self.declaration.defaults {
                values.entry(key.clone()).or_insert_with(|| value.clone());
            }
        }
        let props = self.declaration.parameters["properties"]
            .as_object()
            .ok_or_else(|| Failure::new("invalid_source", "invalid stored parameters"))?;
        if values.len() != props.len()
            || props
                .iter()
                .any(|(k, schema)| values.get(k).is_none_or(|v| !matches_type(schema, v)))
        {
            return Err(Failure::new(
                "invalid_arguments",
                format!(
                    "expected {}{}",
                    self.declaration.name, self.declaration.signature
                ),
            ));
        }
        Ok(args)
    }
}

fn matches_type(schema: &Value, value: &Value) -> bool {
    fn matches(kind: &str, schema: &Value, v: &Value) -> bool {
        match kind {
            "null" => v.is_null(),
            "boolean" => v.is_boolean(),
            "string" => v.is_string(),
            "integer" => v.is_i64() || v.is_u64(),
            "number" => v.is_number(),
            "array" => v.as_array().is_some_and(|items| {
                items
                    .iter()
                    .all(|item| matches_type(&schema["items"], item))
            }),
            _ => false,
        }
    }
    match &schema["type"] {
        Value::String(k) => matches(k, schema, value),
        Value::Array(types) => types
            .iter()
            .any(|k| k.as_str().is_some_and(|k| matches(k, schema, value))),
        _ => false,
    }
}

pub(super) struct Library {
    root: PathBuf,
    scripts: RwLock<BTreeMap<String, Arc<Script>>>,
    writer: Mutex<()>,
}

impl Library {
    pub async fn open(root: PathBuf) -> Result<Self, Failure> {
        let mut scripts = BTreeMap::new();
        if root.exists() {
            let mut dir = tokio::fs::read_dir(&root).await?;
            while let Some(entry) = dir.next_entry().await? {
                let path = entry.path();
                if path.extension().is_none_or(|x| x != "json")
                    || !entry.file_type().await?.is_file()
                {
                    continue;
                }
                match Self::read(&path).await {
                    Ok(s) => {
                        scripts.insert(s.declaration.name.clone(), Arc::new(s));
                    }
                    Err(e) => log::warn!(
                        "ignoring invalid Code Mode record {}: {}",
                        path.display(),
                        e.message
                    ),
                }
            }
        }
        Ok(Self {
            root,
            scripts: RwLock::new(scripts),
            writer: Mutex::new(()),
        })
    }

    async fn read(path: &Path) -> Result<Script, Failure> {
        if tokio::fs::metadata(path).await?.len() > (SOURCE_LIMIT * 8) as u64 {
            return Err(Failure::new("invalid_source", "oversized script record"));
        }
        let script: Script = serde_json::from_slice(&tokio::fs::read(path).await?)?;
        script.declaration.validate()?;
        if script.format != 1
            || script.source.len() > SOURCE_LIMIT
            || path.file_stem().and_then(|x| x.to_str()) != Some(&script.declaration.name)
        {
            return Err(Failure::new("invalid_source", "invalid script record"));
        }
        Ok(script)
    }

    pub fn get(&self, name: &str) -> Option<Arc<Script>> {
        self.scripts.read().get(name).cloned()
    }
    pub fn definitions(&self, names: Option<&[String]>) -> Vec<FunctionDefinition> {
        self.scripts
            .read()
            .values()
            .filter(|s| {
                names.is_none_or(|ns| {
                    ns.iter()
                        .any(|n| n.eq_ignore_ascii_case(&s.declaration.name))
                })
            })
            .map(|s| s.definition())
            .collect()
    }

    pub fn list(&self, args: ListArgs) -> Result<Value, Failure> {
        let limit = args.limit.unwrap_or(20);
        if !(1..=50).contains(&limit) {
            return Err(Failure::new("invalid_arguments", "limit must be 1..50"));
        }
        let query = args.query.unwrap_or_default().trim().to_lowercase();
        let scripts = self.scripts.read();
        if args.include_source && (valid_name(&query).is_err() || args.cursor.is_some()) {
            return Err(Failure::new(
                "invalid_arguments",
                "reading source requires an exact published name and a null cursor",
            ));
        }
        let state = crate::engine::app_protocol::hash(&serde_json::to_vec(&(
            &self.root,
            &query,
            scripts
                .values()
                .map(|s| (&s.declaration.name, &s.revision))
                .collect::<Vec<_>>(),
        ))?);
        let after = match args.cursor.as_deref() {
            Some(cursor) => {
                cursor
                    .split_once('.')
                    .filter(|(version, _)| *version == state)
                    .ok_or_else(|| {
                        Failure::new("stale_cursor", "restart listing from the first page")
                    })?
                    .1
            }
            None => "",
        };
        let mut matches = scripts.iter().filter(|(name, s)| {
            name.as_str() > after
                && if args.include_source {
                    name.as_str() == query
                } else {
                    name.contains(&query)
                        || s.declaration.description.to_lowercase().contains(&query)
                }
        });
        let page:Vec<_>=matches.by_ref().take(limit).map(|(name,s)|{
            let mut item=json!({"name":name,"description":s.declaration.description,"signature":s.declaration.signature,"revision":s.revision});
            if args.include_source {item["source"]=s.source.clone().into();}
            item
        }).collect();
        let next = matches
            .next()
            .and_then(|_| page.last())
            .and_then(|v| v["name"].as_str())
            .map(|name| format!("{state}.{name}"));
        let mut out = json!({"result":page});
        if let Some(cursor) = next {
            out["next_cursor"] = cursor.into();
        }
        Ok(out)
    }

    /// A terminal answer here never writes. A proposed mutation is checked
    /// again under the writer lock after any approval has finished.
    pub fn check_save(
        &self,
        name: &str,
        source: &str,
        expected: Option<&str>,
    ) -> Result<Option<Value>, Failure> {
        valid_name(name)?;
        let scripts = self.scripts.read();
        let existing = scripts.get(name).map(Arc::as_ref);
        if let Some(s) = existing
            && s.source == source
            && expected.is_none_or(|r| r == s.revision)
        {
            return Ok(Some(
                json!({"result":{"status":"unchanged","name":name,"revision":s.revision}}),
            ));
        }
        if existing.map(|s| s.revision.as_str()) != expected {
            return Err(conflict(existing));
        }
        if existing.is_none() && scripts.len() >= CAPACITY {
            return Err(Failure::new(
                "quota_exceeded",
                "workspace has 64 scripts; remove a named revision first",
            ));
        }
        Ok(None)
    }

    pub async fn save(
        &self,
        source: String,
        decl: Declaration,
        expected: Option<&str>,
    ) -> Result<Value, Failure> {
        decl.validate()?;
        let _guard = self.writer.lock().await;
        if let Some(answer) = self.check_save(&decl.name, &source, expected)? {
            return Ok(answer);
        }
        let name = decl.name.clone();
        if expected.is_none()
            && tokio::fs::try_exists(self.root.join(format!("{name}.json"))).await?
        {
            return Err(Failure::new(
                "storage_error",
                "an unreadable record already exists for this name",
            ));
        }
        let revision = ic_auth_types::Xid::new().to_string();
        let script = Arc::new(Script {
            format: 1,
            updated_at: anda_engine::unix_ms(),
            source,
            revision: revision.clone(),
            declaration: decl,
        });
        tokio::fs::create_dir_all(&self.root).await?;
        crate::engine::write_daemon_config_atomically(
            &self.root.join(format!("{name}.json")),
            &serde_json::to_vec(script.as_ref())?,
        )
        .await
        .map_err(Failure::io)?;
        self.scripts.write().insert(name.clone(), script);
        #[cfg(unix)]
        tokio::fs::File::open(&self.root).await?.sync_all().await?;
        Ok(
            json!({"result":{"name":name,"revision":revision,"status":if expected.is_some(){"updated"}else{"created"}}}),
        )
    }

    pub fn check_remove(&self, name: &str, expected: &str) -> Result<Option<Value>, Failure> {
        valid_name(name)?;
        match self.get(name) {
            None => Ok(Some(
                json!({"result":{"name":name,"status":"absent","revision":null}}),
            )),
            Some(s) if s.revision != expected => Err(conflict(Some(&s))),
            Some(_) => Ok(None),
        }
    }
    pub async fn remove(&self, name: &str, expected: &str) -> Result<Value, Failure> {
        let _guard = self.writer.lock().await;
        if let Some(answer) = self.check_remove(name, expected)? {
            return Ok(answer);
        }
        tokio::fs::remove_file(self.root.join(format!("{name}.json"))).await?;
        self.scripts.write().remove(name);
        #[cfg(unix)]
        tokio::fs::File::open(&self.root).await?.sync_all().await?;
        Ok(json!({"result":{"name":name,"status":"removed","revision":null}}))
    }
}

fn valid_name(name: &str) -> Result<(), Failure> {
    if !name.starts_with("script__") || name.len() == 8 || validate_function_name(name).is_err() {
        return Err(Failure::new(
            "invalid_arguments",
            "expected an exact script__name",
        ));
    }
    Ok(())
}

fn conflict(s: Option<&Script>) -> Failure {
    Failure {
        code: "revision_conflict".into(),
        message: "script revision changed".into(),
        data: json!({"revision":s.map(|s|&s.revision)}),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ListArgs {
    pub query: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
    pub include_source: bool,
}
