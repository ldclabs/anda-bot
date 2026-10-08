use crate::util::{
    request_meta::{keys, request_meta_extra_as},
    tool_response::ToolResponse as Response,
};
use anda_core::{
    BoxError, FunctionDefinition, Json, Principal, Resource, ResourceRef, StateFeatures, Tool,
    ToolOutput, update_resources,
};
use anda_db::{
    collection::{Collection, CollectionConfig},
    database::AndaDB,
    error::DBError,
    query::{Filter, Query, RangeQuery},
    schema::Fv,
};
use anda_db_tfs::jieba_tokenizer;
use ic_auth_types::ByteArrayB64;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anda_core::{BoxFut, ToolGroup, ToolGroupInfo, ToolInput, ToolProvider};
use anda_engine::{context::BaseCtx, unix_ms};
use parking_lot::RwLock;
use std::collections::BTreeMap;

use super::conversation::AgentCaller;

/// Metadata key listing the principals a stored resource is shared with
/// besides its owner (`user`).
const SHARED_USERS_KEY: &str = "shared_users";

/// Capture tool/agent artifacts at the execution boundary. Unbound runners only
/// expose their accumulated artifacts when finalized, which is too late for an
/// interactive conversation. Children inherit this collector and returned
/// artifacts keep their blobs while gaining stable resource IDs.
#[derive(Clone)]
pub(crate) struct SessionArtifacts {
    store: Arc<ResourceStore>,
    pending: Arc<RwLock<BTreeMap<u64, Resource>>>,
}

impl SessionArtifacts {
    pub(crate) fn new(store: Arc<ResourceStore>) -> Self {
        Self {
            store,
            pending: Arc::default(),
        }
    }

    pub(crate) async fn record(
        &self,
        caller: &Principal,
        artifacts: &mut [Resource],
    ) -> Result<(), BoxError> {
        if artifacts.is_empty() {
            return Ok(());
        }
        self.store.persist_in_place(caller, artifacts).await?;
        let mut pending = self.pending.write();
        for artifact in artifacts {
            let blob = artifact.blob.take();
            pending.insert(artifact._id, artifact.clone());
            artifact.blob = blob;
        }
        Ok(())
    }

    pub(crate) fn take(&self) -> Vec<Resource> {
        std::mem::take(&mut *self.pending.write())
            .into_values()
            .collect()
    }

    /// Records the artifacts a finished tool call returned. The call already
    /// ran, so a storage failure is logged instead of failing it, which would
    /// invite the model to repeat its side effects. Unrecorded artifacts keep
    /// their blobs and are recorded again when the runner hands them back.
    async fn capture(ctx: &BaseCtx, artifacts: &mut [Resource]) {
        if let Some(collector) = ctx.get_state::<SessionArtifacts>()
            && let Err(err) = collector.record(ctx.caller(), artifacts).await
        {
            log::warn!("failed to record tool artifacts: {err}");
        }
    }
}

/// Engine-level hooks do not wrap model-internal dispatch in anda_engine 0.16.
/// Register artifact capture on the callable itself so both paths behave alike.
pub(crate) fn record_artifacts<T>(tool: Arc<T>) -> Arc<ArtifactTool<T>> {
    Arc::new(ArtifactTool(tool))
}

pub(crate) struct ArtifactTool<T>(Arc<T>);
impl<T: Tool<BaseCtx>> Tool<BaseCtx> for ArtifactTool<T>
where
    T::Output: Send,
{
    type Args = T::Args;
    type Output = T::Output;
    fn name(&self) -> String {
        self.0.name()
    }
    fn description(&self) -> String {
        self.0.description()
    }
    fn definition(&self) -> FunctionDefinition {
        self.0.definition()
    }
    fn group(&self) -> Option<ToolGroupInfo> {
        self.0.group()
    }
    fn supported_resource_tags(&self) -> Vec<String> {
        self.0.supported_resource_tags()
    }
    async fn init(&self, ctx: BaseCtx) -> Result<(), BoxError> {
        self.0.init(ctx).await
    }
    async fn call(
        &self,
        ctx: BaseCtx,
        args: Self::Args,
        resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let mut output = self.0.call(ctx.clone(), args, resources).await?;
        SessionArtifacts::capture(&ctx, &mut output.artifacts).await;
        Ok(output)
    }
}

pub(crate) struct ArtifactProvider<T>(pub Arc<T>);
impl<T: ToolProvider<BaseCtx>> ToolProvider<BaseCtx> for ArtifactProvider<T> {
    fn name(&self) -> String {
        self.0.name()
    }
    fn definitions(&self, names: Option<&[String]>) -> Vec<FunctionDefinition> {
        self.0.definitions(names)
    }
    fn groups(&self) -> Vec<ToolGroup> {
        self.0.groups()
    }
    fn contains_lowercase(&self, name: &str) -> bool {
        self.0.contains_lowercase(name)
    }
    fn supported_resource_tags(&self, name: &str) -> Vec<String> {
        self.0.supported_resource_tags(name)
    }
    fn select_resources(&self, name: &str, resources: &mut Vec<Resource>) -> Vec<Resource> {
        self.0.select_resources(name, resources)
    }
    fn init(&self, ctx: BaseCtx) -> BoxFut<'_, Result<(), BoxError>> {
        self.0.init(ctx)
    }
    fn refresh(&self) -> BoxFut<'_, Result<(), BoxError>> {
        self.0.refresh()
    }
    fn call(
        &self,
        ctx: BaseCtx,
        input: ToolInput<Json>,
    ) -> BoxFut<'_, Result<ToolOutput<Json>, BoxError>> {
        Box::pin(async move {
            let mut output = self.0.call(ctx.clone(), input).await?;
            SessionArtifacts::capture(&ctx, &mut output.artifacts).await;
            Ok(output)
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum ResourcesToolArgs {
    GetResource {
        /// The ID of the persisted resource to get.
        _id: u64,
    },
    DownloadResource {
        /// The ID of the persisted resource to download.
        _id: u64,
        /// Directory to save the file into, inside a workspace or the system temp
        /// directory; defaults to the system temp directory.
        dir: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct ResourceStore {
    resources: Arc<Collection>,
    /// Directories a download may target besides the system temp directory,
    /// matching the roots the filesystem tools may write.
    download_roots: Vec<PathBuf>,
}

impl ResourceStore {
    pub const NAME: &'static str = "resources_api";

    pub async fn connect(db: Arc<AndaDB>) -> Result<Self, BoxError> {
        let schema = Resource::schema()?;
        let resources = db
            .open_or_create_collection(
                schema,
                CollectionConfig {
                    name: "resources".to_string(),
                    description: "Resources collection".to_string(),
                },
                async |collection| {
                    collection.set_tokenizer(jieba_tokenizer());
                    collection.create_btree_index_nx(&["tags"]).await?;
                    collection.create_btree_index_nx(&["hash"]).await?;
                    collection.create_btree_index_nx(&["mime_type"]).await?;
                    collection
                        .create_bm25_index_nx(&["name", "description", "metadata"])
                        .await?;

                    Ok::<(), DBError>(())
                },
            )
            .await?;

        Ok(Self {
            resources,
            download_roots: Vec::new(),
        })
    }

    pub fn with_download_roots(mut self, roots: Vec<PathBuf>) -> Self {
        self.download_roots = roots;
        self
    }

    pub async fn get_resource(&self, id: u64) -> Result<Resource, BoxError> {
        Ok(self.resources.get_as(id).await?)
    }

    /// Loads a persisted resource, including its blob, after verifying that
    /// `caller` may read it.
    pub async fn get_resource_for(
        &self,
        id: u64,
        caller: &Principal,
    ) -> Result<Resource, BoxError> {
        if id == 0 {
            return Err("_id is required".into());
        }
        let resource = self.get_resource(id).await?;
        if !may_read(&resource, &caller.to_string()) {
            return Err("permission denied".into());
        }
        Ok(resource)
    }

    /// Downloads the blob of a persisted resource `caller` may read into `dir`
    /// (the system temp directory when `None`), returning the resource and the
    /// saved file path. A relative `dir` starts at `workspace` when that lies
    /// in a download root. `dir` must stay inside a download root or the temp
    /// directory: this call is not approval-gated, so it must not write where
    /// the filesystem tools cannot.
    pub async fn download_resource(
        &self,
        id: u64,
        dir: Option<&Path>,
        workspace: Option<&Path>,
        caller: &Principal,
    ) -> Result<(Resource, PathBuf), BoxError> {
        let resource = self.get_resource_for(id, caller).await?;
        let dir = resolve_download_dir(dir, workspace, &self.download_roots).await?;
        let path = save_resource_blob(&resource, &dir).await?;
        Ok((resource, path))
    }

    /// Persists the resources for a message, returning references without
    /// their blobs.
    pub async fn persist_resources(
        &self,
        user: &Principal,
        mut resources: Vec<Resource>,
    ) -> Result<Vec<Resource>, BoxError> {
        self.persist_in_place(user, &mut resources).await?;
        for resource in &mut resources {
            resource.blob = None; // remove blob data for message
        }
        Ok(resources)
    }

    /// Stores each resource without an id and gives it its stored id. Blobs
    /// stay in place; a resource that already has an id is left alone.
    async fn persist_in_place(
        &self,
        user: &Principal,
        resources: &mut [Resource],
    ) -> Result<(), BoxError> {
        let fresh: Vec<usize> = (0..resources.len())
            .filter(|&index| resources[index]._id == 0)
            .collect();
        if fresh.is_empty() {
            return Ok(());
        }
        let taken = fresh
            .iter()
            .map(|&index| std::mem::take(&mut resources[index]))
            .collect();
        for (&index, resource) in fresh.iter().zip(update_resources(user, taken)) {
            resources[index] = resource;
        }

        let user = user.to_string();
        let mut changed = false;
        for &index in &fresh {
            let resource = &resources[index];
            let id = match self.resources.add_from(&ResourceRef::from(resource)).await {
                Ok(id) => {
                    changed = true;
                    id
                }
                // The same content is already stored. The conflict reports
                // the rejected new id, so find the stored one by its hash.
                Err(err) if err.unique_index_conflict().is_some() => {
                    let id = self.id_by_hash(resource.hash.as_ref()).await?;
                    // The hash was computed from the bytes this user sent, so
                    // they hold the content: share the stored copy with them.
                    if resource.blob.is_some() {
                        changed |= self.share_with(id, &user).await?;
                    }
                    id
                }
                Err(err) => return Err(err.into()),
            };
            resources[index]._id = id;
        }

        if changed {
            self.resources.flush(unix_ms()).await?;
        }
        Ok(())
    }

    async fn id_by_hash(&self, hash: Option<&ByteArrayB64<32>>) -> Result<u64, BoxError> {
        let hash = hash.ok_or("resource conflicts with a stored one but has no content hash")?;
        self.resources
            .search_ids(Query {
                search: None,
                filter: Some(Filter::Field((
                    "hash".to_string(),
                    RangeQuery::Eq(Fv::Bytes(hash.to_vec())),
                ))),
                limit: Some(1),
            })
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| "stored resource with the same content hash not found".into())
    }

    /// Lets `user` read the stored resource `id`, returning whether that
    /// changed anything.
    async fn share_with(&self, id: u64, user: &str) -> Result<bool, BoxError> {
        let stored = self.get_resource(id).await?;
        if may_read(&stored, user) {
            return Ok(false);
        }
        let mut metadata = stored.metadata.unwrap_or_default();
        let shared = metadata
            .entry(SHARED_USERS_KEY)
            .or_insert_with(|| Value::Array(Vec::new()));
        match shared.as_array_mut() {
            Some(users) => users.push(user.into()),
            None => *shared = json!([user]),
        }
        self.resources
            .update(
                id,
                BTreeMap::from([("metadata".to_string(), Fv::from(metadata))]),
            )
            .await?;
        Ok(true)
    }
}

/// Resolves a requested download directory to a canonical path inside one of
/// `roots` or the system temp directory. A relative directory starts at
/// `workspace` when that lies in a root, as with the filesystem tools, and
/// otherwise at the first root (the temp directory when there is none).
async fn resolve_download_dir(
    dir: Option<&Path>,
    workspace: Option<&Path>,
    roots: &[PathBuf],
) -> Result<PathBuf, BoxError> {
    let temp_dir = std::env::temp_dir();
    let Some(dir) = dir else {
        return Ok(temp_dir);
    };
    if dir
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("download directory must not contain '..'".into());
    }
    let mut allowed = Vec::with_capacity(roots.len() + 1);
    for root in roots {
        if let Ok(root) = tokio::fs::canonicalize(root).await {
            allowed.push(root);
        }
    }
    let dir = if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        let mut base = roots.first().unwrap_or(&temp_dir).clone();
        if let Some(workspace) = workspace
            && let Ok(workspace) = tokio::fs::canonicalize(workspace).await
            && allowed.iter().any(|root| workspace.starts_with(root))
        {
            base = workspace;
        }
        base.join(dir)
    };
    if let Ok(temp_dir) = tokio::fs::canonicalize(&temp_dir).await {
        allowed.push(temp_dir);
    }
    // Canonicalize the deepest existing ancestor so a link cannot lead out.
    let mut existing = dir.as_path();
    while tokio::fs::metadata(existing).await.is_err() {
        existing = existing
            .parent()
            .ok_or_else(|| format!("invalid download directory: {}", dir.display()))?;
    }
    let resolved = tokio::fs::canonicalize(existing)
        .await?
        .join(dir.strip_prefix(existing)?);
    if allowed.iter().any(|root| resolved.starts_with(root)) {
        return Ok(resolved);
    }
    Err(format!(
        "download directory must be inside a workspace or the temp directory: {}",
        dir.display()
    )
    .into())
}

/// Saves the resource blob to a file in `dir`, returning the file path.
async fn save_resource_blob(resource: &Resource, dir: &Path) -> Result<PathBuf, BoxError> {
    let blob = resource
        .blob
        .as_ref()
        .ok_or_else(|| format!("resource {} has no blob data", resource._id))?;
    tokio::fs::create_dir_all(dir).await?;
    let path = dir.join(download_file_name(resource));
    tokio::fs::write(&path, blob.as_slice()).await?;
    Ok(path)
}

/// Derives a safe, collision-free file name from the resource id and name.
fn download_file_name(resource: &Resource) -> String {
    let name = sanitize_file_name(&resource.name);
    if name.is_empty() {
        format!("resource_{}", resource._id)
    } else {
        format!("{}_{}", resource._id, name)
    }
}

/// Strips path separators and control characters so a resource name cannot
/// escape the target directory.
fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    cleaned
        .trim_matches(|c: char| c.is_whitespace() || c == '.')
        .to_string()
}

fn resources_tool_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "type": {
                "type": "string",
                "enum": ["GetResource", "DownloadResource"],
                "description": "Resource operation to perform. Use GetResource to load a persisted resource's details by _id; an agent gets them without the blob. Use DownloadResource to save the resource blob to a local file for further processing."
            },
            "_id": {
                "type": "integer",
                "description": "Resource ID to load. Use the _id from a message attachment resource."
            },
            "dir": {
                "type": ["string", "null"],
                "description": "Only for DownloadResource: directory to save the file into, inside a workspace or the system temp directory. A relative path starts at the current workspace. Defaults to the system temp directory."
            }
        },
        "required": ["type", "_id", "dir"],
        "additionalProperties": false
    })
}

/// Whether `user` may read `resource`: its owner, a user it is shared with,
/// or anyone when it records no owner.
fn may_read(resource: &Resource, user: &str) -> bool {
    let Some(metadata) = &resource.metadata else {
        return true;
    };
    let Some(owner) = metadata.get("user").and_then(Value::as_str) else {
        return true;
    };
    owner == user || shared_users(metadata).any(|shared| shared == user)
}

fn shared_users(metadata: &Map<String, Value>) -> impl Iterator<Item = &str> {
    metadata
        .get(SHARED_USERS_KEY)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

impl Tool<BaseCtx> for ResourceStore {
    type Args = ResourcesToolArgs;
    type Output = Response;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        "Read persisted resources by ID or download a resource blob to a local file. An agent gets a resource without its blob: download it, or inspect it with the matching understanding tool."
            .to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: resources_tool_parameters(),
            strict: Some(true),
        }
    }

    async fn call(
        &self,
        ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        match args {
            ResourcesToolArgs::GetResource { _id } => {
                let mut resource = self.get_resource_for(_id, ctx.caller()).await?;
                // Clients render the blob; an agent would get it as base64
                // text that floods its context.
                if ctx.get_state::<AgentCaller>().is_some()
                    && let Some(blob) = resource.blob.take()
                {
                    resource.size.get_or_insert(blob.len() as u64);
                }
                Ok(ToolOutput::new(Response::Ok {
                    result: json!(resource),
                    next_cursor: None,
                }))
            }
            ResourcesToolArgs::DownloadResource { _id, dir } => {
                let workspace = request_meta_extra_as::<PathBuf>(ctx.meta(), keys::WORKSPACE);
                let (resource, path) = self
                    .download_resource(
                        _id,
                        dir.as_deref().map(Path::new),
                        workspace.as_deref(),
                        ctx.caller(),
                    )
                    .await?;
                Ok(ToolOutput::new(Response::Ok {
                    result: json!({
                        "_id": resource._id,
                        "name": resource.name,
                        "mime_type": resource.mime_type,
                        "size": resource.blob.as_ref().map(|blob| blob.len()),
                        "path": path,
                    }),
                    next_cursor: None,
                }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::json_schema::assert_openai_strict_parameters;

    #[test]
    fn resources_api_schema_is_openai_strict() {
        assert_openai_strict_parameters(&resources_tool_parameters());
    }

    #[test]
    fn resources_tool_args_parse_tagged_variants() {
        let args: ResourcesToolArgs = serde_json::from_value(json!({
            "type": "GetResource",
            "_id": 42,
        }))
        .expect("get resource variant should parse");

        assert_eq!(args, ResourcesToolArgs::GetResource { _id: 42 });

        let args: ResourcesToolArgs = serde_json::from_value(json!({
            "type": "DownloadResource",
            "_id": 42,
            "dir": null,
        }))
        .expect("download variant with null dir should parse");

        assert_eq!(
            args,
            ResourcesToolArgs::DownloadResource { _id: 42, dir: None }
        );

        let args: ResourcesToolArgs = serde_json::from_value(json!({
            "type": "DownloadResource",
            "_id": 42,
            "dir": "/tmp/anda",
        }))
        .expect("download variant with dir should parse");

        assert_eq!(
            args,
            ResourcesToolArgs::DownloadResource {
                _id: 42,
                dir: Some("/tmp/anda".to_string())
            }
        );
    }

    #[test]
    fn sanitize_file_name_strips_path_components() {
        assert_eq!(sanitize_file_name("a.txt"), "a.txt");
        assert_eq!(sanitize_file_name("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(sanitize_file_name("dir\\evil.exe"), "dir_evil.exe");
        assert_eq!(sanitize_file_name("..."), "");
        assert_eq!(sanitize_file_name(" spaced \n"), "spaced");
    }

    #[test]
    fn resource_access_checks_owner_metadata_when_present() {
        let caller = Principal::anonymous();
        let mut metadata = serde_json::Map::new();
        metadata.insert("user".to_string(), caller.to_string().into());
        let resource = Resource {
            metadata: Some(metadata),
            ..Default::default()
        };

        assert!(may_read(&resource, &caller.to_string()));

        let mut metadata = serde_json::Map::new();
        metadata.insert("user".to_string(), "aaaaa-aa".into());
        let resource = Resource {
            metadata: Some(metadata.clone()),
            ..Default::default()
        };
        assert!(!may_read(&resource, &caller.to_string()));

        metadata.insert(SHARED_USERS_KEY.to_string(), json!([caller.to_string()]));
        let resource = Resource {
            metadata: Some(metadata),
            ..Default::default()
        };
        assert!(may_read(&resource, &caller.to_string()));
        assert!(may_read(&Resource::default(), &caller.to_string()));
    }

    use anda_core::ByteBufB64;
    use anda_engine::engine::EngineBuilder;

    async fn test_resource_store() -> ResourceStore {
        let db = crate::test_support::memory_db("resources").await;
        ResourceStore::connect(db).await.unwrap()
    }

    fn sample_resource(name: &str) -> Resource {
        Resource {
            name: name.to_string(),
            tags: vec!["text".to_string()],
            mime_type: Some("text/plain".to_string()),
            blob: Some(ByteBufB64(format!("contents of {name}").into_bytes())),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn persist_resources_assigns_ids_and_strips_blobs() {
        let store = test_resource_store().await;
        let user = Principal::anonymous();

        assert!(
            store
                .persist_resources(&user, Vec::new())
                .await
                .unwrap()
                .is_empty()
        );

        let refs = store
            .persist_resources(
                &user,
                vec![sample_resource("a.txt"), sample_resource("b.txt")],
            )
            .await
            .unwrap();

        assert_eq!(refs.len(), 2);
        for resource_ref in &refs {
            assert!(resource_ref._id > 0);
            assert!(resource_ref.blob.is_none());
        }

        // Stored resources keep their blob and remain loadable by id.
        let stored = store.get_resource(refs[0]._id).await.unwrap();
        assert_eq!(stored.name, "a.txt");
        assert!(stored.blob.is_some());
    }

    #[tokio::test]
    async fn persist_resources_keeps_existing_ids() {
        let store = test_resource_store().await;
        let user = Principal::anonymous();

        let refs = store
            .persist_resources(&user, vec![sample_resource("a.txt")])
            .await
            .unwrap();
        let id = refs[0]._id;

        // Re-persisting an already-persisted ref keeps its id without inserting.
        let again = store.persist_resources(&user, refs).await.unwrap();
        assert_eq!(again[0]._id, id);
    }

    #[tokio::test]
    async fn persist_resources_reuses_the_stored_id_for_repeated_content() {
        let store = test_resource_store().await;
        let user = Principal::anonymous();

        let first = store
            .persist_resources(&user, vec![sample_resource("a.txt")])
            .await
            .unwrap()[0]
            ._id;

        // Sending the same content again, alone or twice in one message, must
        // point at the stored resource rather than an id that was never saved.
        let again = store
            .persist_resources(
                &user,
                vec![sample_resource("a.txt"), sample_resource("a.txt")],
            )
            .await
            .unwrap();
        assert_eq!(again[0]._id, first);
        assert_eq!(again[1]._id, first);
        let stored = store.get_resource_for(first, &user).await.unwrap();
        assert_eq!(stored.name, "a.txt");

        let fresh = store
            .persist_resources(
                &user,
                vec![sample_resource("b.txt"), sample_resource("b.txt")],
            )
            .await
            .unwrap();
        assert_ne!(fresh[0]._id, first);
        assert_eq!(fresh[1]._id, fresh[0]._id);
        assert!(store.get_resource_for(fresh[1]._id, &user).await.is_ok());
    }

    #[tokio::test]
    async fn repeated_content_is_shared_only_with_senders_of_the_bytes() {
        let store = test_resource_store().await;
        let owner = Principal::anonymous();
        let other = Principal::management_canister();

        let first = store
            .persist_resources(&owner, vec![sample_resource("a.txt")])
            .await
            .unwrap();
        let id = first[0]._id;
        assert!(store.get_resource_for(id, &other).await.is_err());

        // A reference carrying only the hash proves nothing and gains nothing.
        let forged = store
            .persist_resources(
                &other,
                vec![Resource {
                    name: "forged.txt".to_string(),
                    hash: store.get_resource(id).await.unwrap().hash,
                    ..Default::default()
                }],
            )
            .await
            .unwrap();
        assert_eq!(forged[0]._id, id);
        assert!(store.get_resource_for(id, &other).await.is_err());

        // Sending the same bytes shares the stored copy, once.
        for _ in 0..2 {
            let copy = Resource {
                name: "copy.txt".to_string(),
                ..sample_resource("a.txt")
            };
            let again = store.persist_resources(&other, vec![copy]).await.unwrap();
            assert_eq!(again[0]._id, id);
        }
        let stored = store.get_resource_for(id, &other).await.unwrap();
        assert_eq!(stored.name, "a.txt");
        assert_eq!(
            shared_users(stored.metadata.as_ref().unwrap()).collect::<Vec<_>>(),
            vec![other.to_string()]
        );
        assert!(store.get_resource_for(id, &owner).await.is_ok());
    }

    #[tokio::test]
    async fn download_resource_writes_blob_to_dir() {
        let store = test_resource_store().await;
        let user = Principal::anonymous();
        let dir = tempfile::tempdir().unwrap();

        let refs = store
            .persist_resources(&user, vec![sample_resource("a.txt")])
            .await
            .unwrap();
        let id = refs[0]._id;

        let (resource, path) = store
            .download_resource(id, Some(dir.path()), None, &user)
            .await
            .unwrap();
        assert_eq!(resource.name, "a.txt");
        assert_eq!(
            path,
            dir.path()
                .canonicalize()
                .unwrap()
                .join(format!("{id}_a.txt"))
        );
        let contents = tokio::fs::read(&path).await.unwrap();
        assert_eq!(contents, b"contents of a.txt");

        // A resource without blob data cannot be downloaded.
        let refs = store
            .persist_resources(
                &user,
                vec![Resource {
                    name: "no-blob.txt".to_string(),
                    ..Default::default()
                }],
            )
            .await
            .unwrap();
        let err = store
            .download_resource(refs[0]._id, Some(dir.path()), None, &user)
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("no blob data"));
    }

    #[tokio::test]
    async fn download_directory_stays_inside_workspaces_or_temp() {
        let home = tempfile::tempdir().unwrap();
        let workspace = home.path().join("workspace");
        tokio::fs::create_dir_all(&workspace).await.unwrap();
        let roots = vec![workspace.clone()];
        // The temp directory itself is always allowed, so probe outside it.
        let outside = PathBuf::from(if cfg!(windows) {
            "C:\\anda-download-outside"
        } else {
            "/anda-download-outside"
        });

        // Missing subdirectories of a root resolve; relative ones use the first root.
        let nested = resolve_download_dir(Some(Path::new("out/files")), None, &roots)
            .await
            .unwrap();
        assert_eq!(
            nested,
            workspace.canonicalize().unwrap().join("out").join("files")
        );
        assert_eq!(
            resolve_download_dir(None, None, &roots).await.unwrap(),
            std::env::temp_dir()
        );

        // A relative directory starts at the request's workspace when that
        // lies in a root, and ignores one outside every root.
        let project = workspace.join("project");
        tokio::fs::create_dir_all(&project).await.unwrap();
        assert_eq!(
            resolve_download_dir(Some(Path::new("out")), Some(&project), &roots)
                .await
                .unwrap(),
            project.canonicalize().unwrap().join("out")
        );
        let elsewhere = home.path().join("elsewhere");
        tokio::fs::create_dir_all(&elsewhere).await.unwrap();
        assert_eq!(
            resolve_download_dir(Some(Path::new("out")), Some(&elsewhere), &roots)
                .await
                .unwrap(),
            workspace.canonicalize().unwrap().join("out")
        );

        // Anything outside the roots and the temp directory is refused.
        for dir in [outside.join("sub"), workspace.join("../escape")] {
            assert!(
                resolve_download_dir(Some(&dir), None, &roots)
                    .await
                    .is_err()
            );
        }
        #[cfg(unix)]
        {
            let link = workspace.join("link");
            std::os::unix::fs::symlink("/", &link).unwrap();
            assert!(
                resolve_download_dir(Some(&link.join("sub")), None, &roots)
                    .await
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn tool_call_downloads_resource_and_enforces_ownership() {
        let store = test_resource_store().await;
        let ctx = EngineBuilder::new().mock_ctx().base;
        let dir = tempfile::tempdir().unwrap();

        let err = store
            .call(
                ctx.clone(),
                ResourcesToolArgs::DownloadResource { _id: 0, dir: None },
                Vec::new(),
            )
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("_id is required"));

        let refs = store
            .persist_resources(&Principal::anonymous(), vec![sample_resource("mine.txt")])
            .await
            .unwrap();
        let id = refs[0]._id;
        let output = store
            .call(
                ctx.clone(),
                ResourcesToolArgs::DownloadResource {
                    _id: id,
                    dir: Some(dir.path().to_string_lossy().into_owned()),
                },
                Vec::new(),
            )
            .await
            .unwrap();
        match output.output {
            Response::Ok { result, .. } => {
                assert_eq!(result["name"], "mine.txt");
                let path = result["path"].as_str().unwrap();
                assert_eq!(
                    PathBuf::from(path),
                    dir.path()
                        .canonicalize()
                        .unwrap()
                        .join(format!("{id}_mine.txt"))
                );
                let contents = tokio::fs::read(path).await.unwrap();
                assert_eq!(contents, b"contents of mine.txt");
            }
            other => panic!("expected ok response, got {other:?}"),
        }

        // A resource owned by someone else cannot be downloaded.
        let foreign = store
            .persist_resources(
                &Principal::management_canister(),
                vec![sample_resource("theirs.txt")],
            )
            .await
            .unwrap();
        let err = store
            .call(
                ctx,
                ResourcesToolArgs::DownloadResource {
                    _id: foreign[0]._id,
                    dir: None,
                },
                Vec::new(),
            )
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("permission denied"));
    }

    #[tokio::test]
    async fn tool_call_enforces_id_and_ownership() {
        let store = test_resource_store().await;
        let ctx = EngineBuilder::new().mock_ctx().base;

        let err = store
            .call(
                ctx.clone(),
                ResourcesToolArgs::GetResource { _id: 0 },
                Vec::new(),
            )
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("_id is required"));

        // The mock context's caller is anonymous, matching the persisting user.
        let refs = store
            .persist_resources(&Principal::anonymous(), vec![sample_resource("mine.txt")])
            .await
            .unwrap();
        let output = store
            .call(
                ctx.clone(),
                ResourcesToolArgs::GetResource { _id: refs[0]._id },
                Vec::new(),
            )
            .await
            .unwrap();
        match output.output {
            Response::Ok { result, .. } => {
                assert_eq!(result["name"], "mine.txt");
                assert!(result["blob"].is_string());
            }
            other => panic!("expected ok response, got {other:?}"),
        }

        // An agent gets the resource without its blob.
        let agent_ctx = EngineBuilder::new().mock_ctx().base;
        agent_ctx.set_state(AgentCaller);
        let output = store
            .call(
                agent_ctx,
                ResourcesToolArgs::GetResource { _id: refs[0]._id },
                Vec::new(),
            )
            .await
            .unwrap();
        match output.output {
            Response::Ok { result, .. } => {
                assert_eq!(result["name"], "mine.txt");
                assert!(result.get("blob").is_none());
                assert_eq!(result["size"], "contents of mine.txt".len());
            }
            other => panic!("expected ok response, got {other:?}"),
        }

        // A resource owned by someone else is rejected.
        let foreign = store
            .persist_resources(
                &Principal::management_canister(),
                vec![sample_resource("theirs.txt")],
            )
            .await
            .unwrap();
        let err = store
            .call(
                ctx,
                ResourcesToolArgs::GetResource {
                    _id: foreign[0]._id,
                },
                Vec::new(),
            )
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("permission denied"));
    }

    #[tokio::test]
    async fn session_artifacts_preserve_blobs_and_deduplicate_repeated_outputs() {
        let db = crate::test_support::memory_db("artifact_capture").await;
        let store = Arc::new(ResourceStore::connect(db).await.unwrap());
        let collector = SessionArtifacts::new(store.clone());
        let owner = Principal::from_slice(&[8]);
        let mut artifacts = vec![Resource {
            name: "result.txt".into(),
            blob: Some(anda_core::ByteBufB64(b"result".to_vec())),
            ..Default::default()
        }];
        collector.record(&owner, &mut artifacts).await.unwrap();
        let id = artifacts[0]._id;
        // A finished runner hands recorded artifacts back; they keep their id.
        collector.record(&owner, &mut artifacts).await.unwrap();
        assert_ne!(id, 0);
        assert_eq!(artifacts[0]._id, id);
        assert!(artifacts[0].blob.is_some());
        let pending = collector.take();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].blob.is_none());
        assert!(collector.take().is_empty());
        assert_eq!(
            store.get_resource(id).await.unwrap().blob,
            artifacts[0].blob
        );
    }
}
