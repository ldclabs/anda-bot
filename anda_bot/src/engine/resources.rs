use anda_core::{
    BoxError, FunctionDefinition, Principal, Resource, ResourceRef, StateFeatures, Tool,
    ToolOutput, update_resources,
};
use anda_db::{
    collection::{Collection, CollectionConfig},
    database::AndaDB,
    error::DBError,
};
use anda_db_tfs::jieba_tokenizer;
use anda_kip::Response;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

use anda_engine::{context::BaseCtx, unix_ms};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum ResourcesToolArgs {
    GetResource {
        /// The ID of the persisted resource to get.
        _id: u64,
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

fn resources_tool_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "type": {
                "type": "string",
                "enum": ["GetResource"],
                "description": "Resource operation to perform. Use GetResource to load a persisted resource, including its blob, by _id."
            },
            "_id": {
                "type": ["integer", "null"],
                "description": "Resource ID to load. Use the _id from a message attachment resource."
            }
        },
        "required": ["type", "_id"],
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
        "Read persisted resources by ID, including blob content omitted from conversation messages."
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
}
