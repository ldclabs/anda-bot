use anda_core::{BoxError, Principal, Resource, ResourceRef, update_resources};
use anda_db::{
    collection::{Collection, CollectionConfig},
    database::AndaDB,
    error::DBError,
};
use anda_db_tfs::jieba_tokenizer;
use std::sync::Arc;

use anda_engine::unix_ms;

#[derive(Debug, Clone)]
pub struct ResourceStore {
    resources: Arc<Collection>,
}

impl ResourceStore {
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
