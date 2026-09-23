use crate::util::tool_response::ToolResponse as Response;
use anda_core::{
    BoxError, FunctionDefinition, Json, Principal, Resource, ResourceRef, StateFeatures, Tool,
    ToolOutput, update_resources,
};
use anda_db::{
    collection::{Collection, CollectionConfig},
    database::AndaDB,
    error::DBError,
};
use anda_db_tfs::jieba_tokenizer;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anda_core::{BoxFut, ToolGroup, ToolGroupInfo, ToolInput, ToolProvider};
use anda_engine::{context::BaseCtx, unix_ms};
use parking_lot::RwLock;
use std::collections::BTreeMap;

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
        let saved = self
            .store
            .persist_resources(caller, artifacts.to_vec())
            .await?;
        let mut pending = self.pending.write();
        for (artifact, saved) in artifacts.iter_mut().zip(saved) {
            let blob = artifact.blob.take();
            *artifact = saved.clone();
            artifact.blob = blob;
            pending.insert(saved._id, saved);
        }
        Ok(())
    }

    pub(crate) fn take(&self) -> Vec<Resource> {
        std::mem::take(&mut *self.pending.write())
            .into_values()
            .collect()
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
        if let Some(artifacts) = ctx.get_state::<SessionArtifacts>() {
            artifacts
                .record(ctx.caller(), &mut output.artifacts)
                .await?;
        }
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
            if let Some(artifacts) = ctx.get_state::<SessionArtifacts>() {
                artifacts
                    .record(ctx.caller(), &mut output.artifacts)
                    .await?;
            }
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
        /// Directory to save the file into; defaults to the system temp directory.
        dir: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct ResourceStore {
    resources: Arc<Collection>,
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

        Ok(Self { resources })
    }

    pub async fn get_resource(&self, id: u64) -> Result<Resource, BoxError> {
        Ok(self.resources.get_as(id).await?)
    }

    /// Downloads the blob of a persisted resource into `dir` (the system temp
    /// directory when `None`), returning the resource and the saved file path.
    ///
    /// When `caller` is supplied, ownership is verified before the blob is
    /// written to disk.
    pub async fn download_resource(
        &self,
        id: u64,
        dir: Option<&Path>,
        caller: Option<&Principal>,
    ) -> Result<(Resource, PathBuf), BoxError> {
        let resource = self.get_resource(id).await?;
        if let Some(caller) = caller {
            ensure_resource_access(&resource, caller)?;
        }
        let path = save_resource_blob(&resource, dir).await?;
        Ok((resource, path))
    }

    pub async fn persist_resources(
        &self,
        user: &Principal,
        resources: Vec<Resource>,
    ) -> Result<Vec<Resource>, BoxError> {
        if resources.is_empty() {
            return Ok(Vec::new());
        }

        let resources = update_resources(user, resources);
        let mut refs = Vec::with_capacity(resources.len());
        let mut inserted = 0;

        for resource in resources {
            let resource_ref = ResourceRef::from(&resource);
            let id = if resource._id > 0 {
                resource._id
            } else {
                match self.resources.add_from(&resource_ref).await {
                    Ok(id) => {
                        inserted += 1;
                        id
                    }
                    Err(DBError::AlreadyExists { _id, .. }) => _id,
                    Err(err) => return Err(err.into()),
                }
            };

            refs.push(Resource {
                _id: id,
                blob: None, // remove blob data for message
                ..resource
            });
        }

        if inserted > 0 {
            self.resources.flush(unix_ms()).await?;
        }

        Ok(refs)
    }
}

/// Saves the resource blob to a file in `dir` (the system temp directory when
/// `None`), returning the file path.
async fn save_resource_blob(resource: &Resource, dir: Option<&Path>) -> Result<PathBuf, BoxError> {
    let blob = resource
        .blob
        .as_ref()
        .ok_or_else(|| format!("resource {} has no blob data", resource._id))?;
    let dir = dir
        .map(Path::to_path_buf)
        .unwrap_or_else(std::env::temp_dir);
    tokio::fs::create_dir_all(&dir).await?;
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
                "description": "Resource operation to perform. Use GetResource to load a persisted resource, including its blob, by _id. Use DownloadResource to save the resource blob to a local file for further processing."
            },
            "_id": {
                "type": ["integer", "null"],
                "description": "Resource ID to load. Use the _id from a message attachment resource."
            },
            "dir": {
                "type": ["string", "null"],
                "description": "Only for DownloadResource: directory to save the file into. Defaults to the system temp directory."
            }
        },
        "required": ["type", "_id", "dir"],
        "additionalProperties": false
    })
}

fn resource_owner(resource: &Resource) -> Option<&str> {
    resource
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("user"))
        .and_then(Value::as_str)
}

fn ensure_resource_access(resource: &Resource, caller: &Principal) -> Result<(), BoxError> {
    if let Some(owner) = resource_owner(resource)
        && owner != caller.to_string()
    {
        return Err("permission denied".into());
    }
    Ok(())
}

impl Tool<BaseCtx> for ResourceStore {
    type Args = ResourcesToolArgs;
    type Output = Response;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        "Read persisted resources by ID, including blob content omitted from conversation messages, or download a resource blob to a local file."
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
                if _id == 0 {
                    return Err("_id is required".into());
                }

                let resource = self.get_resource(_id).await?;
                ensure_resource_access(&resource, ctx.caller())?;
                Ok(ToolOutput::new(Response::Ok {
                    result: json!(resource),
                    next_cursor: None,
                }))
            }
            ResourcesToolArgs::DownloadResource { _id, dir } => {
                if _id == 0 {
                    return Err("_id is required".into());
                }

                let (resource, path) = self
                    .download_resource(_id, dir.as_deref().map(Path::new), Some(ctx.caller()))
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

        assert!(ensure_resource_access(&resource, &caller).is_ok());

        let mut metadata = serde_json::Map::new();
        metadata.insert("user".to_string(), "aaaaa-aa".into());
        let resource = Resource {
            metadata: Some(metadata),
            ..Default::default()
        };

        assert!(ensure_resource_access(&resource, &caller).is_err());
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
            .download_resource(id, Some(dir.path()), None)
            .await
            .unwrap();
        assert_eq!(resource.name, "a.txt");
        assert_eq!(path, dir.path().join(format!("{id}_a.txt")));
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
            .download_resource(refs[0]._id, Some(dir.path()), None)
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("no blob data"));
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
                    dir.path().join(format!("{id}_mine.txt"))
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
            Response::Ok { result, .. } => assert_eq!(result["name"], "mine.txt"),
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
