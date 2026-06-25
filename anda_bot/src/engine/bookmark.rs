use anda_core::{
    BoxError, CompletionRequest, ContentPart, FunctionDefinition, ModelEffort, Resource,
    StateFeatures, Tool, ToolOutput,
};
use anda_db::{
    collection::{Collection, CollectionConfig},
    database::AndaDB,
    error::DBError,
    index::{BTree, jieba_tokenizer},
    query::{Filter, Query, RangeQuery},
    schema::{AndaDBSchema, FieldEntry, FieldKey, FieldType, FieldTyped, Fv, Schema, SchemaError},
    unix_ms,
};
use anda_engine::{context::BaseCtx, model::Models, truncate_utf8_to_max_bytes};
use anda_kip::Response;
use cbor2::Cbor;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Default page size for `ListBookmarks`.
const DEFAULT_LIST_LIMIT: usize = 20;
/// Hard cap for a single `ListBookmarks` page.
const MAX_LIST_LIMIT: usize = 100;
const BOOKMARK_FOLDERS_EXTENSION_KEY: &str = "bookmark_folders";
const BOOKMARK_PREVIEW_MAX_BYTES: usize = 280;

/// A bookmarked conversation and the messages marked inside it.
///
/// `_id` is the collection's auto-incrementing primary key. `conversation` is
/// the stable business key scoped by `user`; individual marked messages live in
/// `messages`, with each message carrying the client-visible message index from
/// `m-<conversation>-<index>`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, FieldTyped, AndaDBSchema)]
pub struct Bookmark {
    pub _id: u64,
    /// Owner principal (text form) — scopes every read/write to the caller.
    pub user: String,
    /// Conversation the message belongs to.
    pub conversation: u64,
    /// Channel source, so the panel can switch channels when jumping back.
    pub source: String,
    /// Folder ids this bookmark belongs to. Empty means unfiled.
    pub folder_ids: Vec<u64>,
    /// Marked message previews.
    pub messages: Vec<MessageInfo>,
    /// Bookmark creation time (unix ms).
    pub created_at: u64,
}

/// A bookmarked message inside a conversation bookmark.
#[derive(Debug, Clone, Default, PartialEq, Eq, Cbor, FieldTyped)]
pub struct MessageInfo {
    /// Message index in `m-<conversation>-<index>`.
    #[cbor(key = 1)]
    pub index: usize,
    /// Message role (currently always `assistant`).
    #[cbor(key = 2)]
    pub role: String,
    /// Generated text snapshot for previewing without loading the conversation.
    #[cbor(key = 3)]
    pub text: String,
}

/// Folder metadata for one caller.
#[derive(Debug, Clone, PartialEq, Eq, Cbor)]
pub struct BookmarkFolders {
    #[cbor(key = 1)]
    pub version: u32,
    #[cbor(key = 2)]
    pub next_folder_id: u64,
    #[cbor(key = 3)]
    pub folders: BTreeMap<u64, BookmarkFolder>,
    #[cbor(key = 4)]
    pub updated_at: u64,
}

impl Default for BookmarkFolders {
    fn default() -> Self {
        Self {
            version: 1,
            next_folder_id: 1,
            folders: BTreeMap::new(),
            updated_at: 0,
        }
    }
}

/// One user-defined bookmark folder.
#[derive(Debug, Clone, PartialEq, Eq, Cbor)]
pub struct BookmarkFolder {
    #[cbor(key = 1)]
    pub _id: u64,
    #[cbor(key = 2)]
    pub name: String,
    #[cbor(key = 3)]
    pub parent_id: Option<u64>,
    #[cbor(key = 4)]
    pub order: i64,
    #[cbor(key = 5)]
    pub created_at: u64,
    #[cbor(key = 6)]
    pub updated_at: u64,
}

type BookmarkFoldersByUser = BTreeMap<String, BookmarkFolders>;

/// A dedicated AndaDB collection for bookmarks, mirroring `CronStore`:
/// single-row inserts/deletes, BTree-indexed field filters, and `_id` cursor
/// pagination instead of a single serialized blob.
#[derive(Clone)]
pub struct BookmarkStore {
    bookmarks: Arc<Collection>,
    extension_save_lock: Arc<tokio::sync::Mutex<()>>,
}

impl BookmarkStore {
    pub async fn connect(db: Arc<AndaDB>) -> Result<Self, BoxError> {
        let bookmarks = db
            .open_or_create_collection(
                Bookmark::schema()?,
                CollectionConfig {
                    name: "bookmarks".to_string(),
                    description: "User conversation bookmarks".to_string(),
                },
                async |collection| {
                    collection.set_tokenizer(jieba_tokenizer());
                    collection.create_btree_index_nx(&["user"]).await?;
                    collection.create_btree_index_nx(&["conversation"]).await?;
                    Ok::<(), DBError>(())
                },
            )
            .await?;

        Ok(Self {
            bookmarks,
            extension_save_lock: Arc::new(tokio::sync::Mutex::new(())),
        })
    }

    /// Looks up a bookmark by its `(user, conversation)` business key.
    async fn find(&self, user: &str, conversation: u64) -> Result<Option<Bookmark>, BoxError> {
        let rt: Vec<Bookmark> = self
            .bookmarks
            .search_as(Query {
                search: None,
                filter: Some(Filter::Field((
                    "conversation".to_string(),
                    RangeQuery::Eq(Fv::U64(conversation)),
                ))),
                limit: Some(1),
            })
            .await?;
        match rt.into_iter().next() {
            Some(bookmark) if bookmark.user == user => Ok(Some(bookmark)),
            _ => Ok(None),
        }
    }

    /// Adds a marked message, idempotent on `(user, conversation, message index)`.
    pub async fn add(
        &self,
        user: String,
        conversation: u64,
        source: String,
        message: MessageInfo,
        folder_ids: Vec<u64>,
    ) -> Result<Bookmark, BoxError> {
        if let Some(mut existing) = self.find(&user, conversation).await? {
            let mut changed = false;
            if !existing
                .messages
                .iter()
                .any(|item| item.index == message.index)
            {
                existing.messages.push(message);
                normalize_messages(&mut existing.messages);
                changed = true;
            }
            if !source.is_empty() && existing.source != source {
                existing.source = source;
                changed = true;
            }
            let next_folder_ids = normalize_folder_ids(
                existing
                    .folder_ids
                    .iter()
                    .copied()
                    .chain(folder_ids)
                    .collect(),
            );
            if existing.folder_ids != next_folder_ids {
                existing.folder_ids = next_folder_ids;
                changed = true;
            }
            if changed {
                return self.update_bookmark(existing).await;
            }
            return Ok(existing);
        }

        let now = unix_ms();
        let mut bookmark = Bookmark {
            _id: 0,
            user,
            conversation,
            source,
            folder_ids: normalize_folder_ids(folder_ids),
            messages: vec![message],
            created_at: now,
        };
        normalize_messages(&mut bookmark.messages);
        let id = self.bookmarks.add_from(&bookmark).await?;
        bookmark._id = id;
        self.bookmarks.flush(now).await?;
        Ok(bookmark)
    }

    /// Removes a marked message. If it was the last mark in the conversation,
    /// the whole conversation bookmark row is deleted.
    pub async fn remove(
        &self,
        user: &str,
        message_ref: MessageRef,
    ) -> Result<(bool, Option<Bookmark>), BoxError> {
        let Some(mut found) = self.find(user, message_ref.conversation).await? else {
            return Ok((false, None));
        };
        let Some(pos) = found
            .messages
            .iter()
            .position(|message| message.index == message_ref.index)
        else {
            return Ok((false, Some(found)));
        };
        found.messages.remove(pos);
        if found.messages.is_empty() {
            let removed = matches!(self.bookmarks.remove(found._id).await, Ok(Some(_)));
            if removed {
                self.bookmarks.flush(unix_ms()).await?;
            }
            return Ok((removed, None));
        }
        let updated = self.update_bookmark(found).await?;
        Ok((true, Some(updated)))
    }

    /// Lists the caller's bookmarks newest-first, paginated by `_id` cursor.
    pub async fn list(
        &self,
        user: &str,
        cursor: Option<String>,
        limit: Option<usize>,
    ) -> Result<(Vec<Bookmark>, Option<String>), BoxError> {
        self.list_filtered(user, cursor, limit, |_| true).await
    }

    /// Lists bookmarks in a folder. `folder_id = 0` means unfiled bookmarks.
    pub async fn list_in_folder(
        &self,
        user: &str,
        folder_id: u64,
        cursor: Option<String>,
        limit: Option<usize>,
    ) -> Result<(Vec<Bookmark>, Option<String>), BoxError> {
        if folder_id != 0 {
            let folders = self.folders(user)?;
            if !folders.folders.contains_key(&folder_id) {
                return Err(format!("bookmark folder {folder_id} does not exist").into());
            }
        }
        self.list_filtered(user, cursor, limit, |bookmark| {
            if folder_id == 0 {
                bookmark.folder_ids.is_empty()
            } else {
                bookmark.folder_ids.contains(&folder_id)
            }
        })
        .await
    }

    async fn list_filtered(
        &self,
        user: &str,
        cursor: Option<String>,
        limit: Option<usize>,
        mut keep: impl FnMut(&Bookmark) -> bool,
    ) -> Result<(Vec<Bookmark>, Option<String>), BoxError> {
        let limit = limit.unwrap_or(DEFAULT_LIST_LIMIT).clamp(1, MAX_LIST_LIMIT);
        let mut cursor = match BTree::from_cursor::<u64>(&cursor)? {
            Some(cursor) => cursor,
            None => self.bookmarks.max_document_id() + 1,
        };
        let mut items = Vec::with_capacity(limit);
        let batch_limit = MAX_LIST_LIMIT;

        loop {
            // `_id < cursor` keeps the largest-id block below the cursor,
            // returned ascending; reverse each block so callers see
            // newest-first. Filtered folder pages may need multiple blocks.
            let mut rt: Vec<Bookmark> = self
                .bookmarks
                .search_as(Query {
                    search: None,
                    filter: Some(Filter::And(vec![
                        Box::new(Filter::Field((
                            "user".to_string(),
                            RangeQuery::Eq(Fv::Text(user.to_string())),
                        ))),
                        Box::new(Filter::Field((
                            "_id".to_string(),
                            RangeQuery::Lt(Fv::U64(cursor)),
                        ))),
                    ])),
                    limit: Some(batch_limit),
                })
                .await?;
            let has_more = rt.len() >= batch_limit;
            if let Some(first) = rt.first() {
                cursor = first._id;
            }
            rt.reverse();

            for bookmark in rt {
                if keep(&bookmark) {
                    let next_cursor = if items.len() + 1 >= limit {
                        BTree::to_cursor(&bookmark._id)
                    } else {
                        None
                    };
                    items.push(bookmark);
                    if items.len() >= limit {
                        return Ok((items, next_cursor));
                    }
                }
            }

            if !has_more {
                return Ok((items, None));
            }
        }
    }

    pub fn folders(&self, user: &str) -> Result<BookmarkFolders, BoxError> {
        let all = self.load_all_folders();
        Ok(all.get(user).cloned().unwrap_or_default().normalized())
    }

    pub async fn create_folder(
        &self,
        user: &str,
        name: String,
        parent_id: Option<u64>,
    ) -> Result<BookmarkFolders, BoxError> {
        let _guard = self.extension_save_lock.lock().await;
        let mut all = self.load_all_folders();
        let folders = all.entry(user.to_string()).or_default();
        folders.normalize_in_place();
        let name = normalize_folder_name(name)?;
        validate_parent_exists(folders, parent_id)?;
        ensure_unique_folder_name(folders, None, parent_id, &name)?;

        let now = unix_ms();
        let id = folders.next_folder_id.max(1);
        folders.next_folder_id = id + 1;
        folders.folders.insert(
            id,
            BookmarkFolder {
                _id: id,
                name,
                parent_id,
                order: next_folder_order(folders, parent_id),
                created_at: now,
                updated_at: now,
            },
        );
        folders.updated_at = now;
        let updated = folders.clone();
        self.save_all_folders(&all).await?;
        Ok(updated)
    }

    pub async fn rename_folder(
        &self,
        user: &str,
        folder_id: u64,
        name: String,
    ) -> Result<BookmarkFolders, BoxError> {
        let _guard = self.extension_save_lock.lock().await;
        let mut all = self.load_all_folders();
        let folders = all.entry(user.to_string()).or_default();
        folders.normalize_in_place();
        let name = normalize_folder_name(name)?;
        let parent_id = folders
            .folders
            .get(&folder_id)
            .ok_or_else(|| format!("bookmark folder {folder_id} does not exist"))?
            .parent_id;
        ensure_unique_folder_name(folders, Some(folder_id), parent_id, &name)?;

        let now = unix_ms();
        let folder = folders
            .folders
            .get_mut(&folder_id)
            .expect("folder existence checked above");
        folder.name = name;
        folder.updated_at = now;
        folders.updated_at = now;
        let updated = folders.clone();
        self.save_all_folders(&all).await?;
        Ok(updated)
    }

    pub async fn move_folder(
        &self,
        user: &str,
        folder_id: u64,
        parent_id: Option<u64>,
        order: Option<i64>,
    ) -> Result<BookmarkFolders, BoxError> {
        let _guard = self.extension_save_lock.lock().await;
        let mut all = self.load_all_folders();
        let folders = all.entry(user.to_string()).or_default();
        folders.normalize_in_place();
        if !folders.folders.contains_key(&folder_id) {
            return Err(format!("bookmark folder {folder_id} does not exist").into());
        }
        validate_parent_exists(folders, parent_id)?;
        if parent_id == Some(folder_id) || is_descendant_folder(folders, parent_id, folder_id) {
            return Err("cannot move a folder under itself or its descendant".into());
        }
        let name = folders
            .folders
            .get(&folder_id)
            .map(|folder| folder.name.clone())
            .unwrap_or_default();
        ensure_unique_folder_name(folders, Some(folder_id), parent_id, &name)?;

        let now = unix_ms();
        let next_order = order.unwrap_or_else(|| next_folder_order(folders, parent_id));
        let folder = folders
            .folders
            .get_mut(&folder_id)
            .expect("folder existence checked above");
        folder.parent_id = parent_id;
        folder.order = next_order;
        folder.updated_at = now;
        folders.updated_at = now;
        let updated = folders.clone();
        self.save_all_folders(&all).await?;
        Ok(updated)
    }

    pub async fn delete_folder(
        &self,
        user: &str,
        folder_id: u64,
    ) -> Result<BookmarkFolders, BoxError> {
        let deleted_ids = {
            let _guard = self.extension_save_lock.lock().await;
            let mut all = self.load_all_folders();
            let folders = all.entry(user.to_string()).or_default();
            folders.normalize_in_place();
            if !folders.folders.contains_key(&folder_id) {
                return Err(format!("bookmark folder {folder_id} does not exist").into());
            }

            let now = unix_ms();
            let deleted_ids = folder_subtree_ids(folders, folder_id);
            for id in &deleted_ids {
                folders.folders.remove(id);
            }
            folders.updated_at = now;
            self.save_all_folders(&all).await?;
            deleted_ids
        };

        self.remove_folder_ids_from_bookmarks(user, &deleted_ids)
            .await?;
        self.folders(user)
    }

    pub async fn set_bookmark_folders(
        &self,
        user: &str,
        message_ref: MessageRef,
        folder_ids: Vec<u64>,
    ) -> Result<Bookmark, BoxError> {
        let folders = self.folders(user)?;
        let folder_ids = validate_folder_ids(&folders, folder_ids)?;
        let mut bookmark = self
            .find(user, message_ref.conversation)
            .await?
            .ok_or_else(|| format!("bookmark {} does not exist", message_ref.message_id()))?;
        if !bookmark
            .messages
            .iter()
            .any(|message| message.index == message_ref.index)
        {
            return Err(format!("bookmark {} does not exist", message_ref.message_id()).into());
        }
        if bookmark.folder_ids == folder_ids {
            return Ok(bookmark);
        }
        bookmark.folder_ids = folder_ids;
        self.update_bookmark(bookmark).await
    }

    pub async fn add_bookmark_to_folder(
        &self,
        user: &str,
        message_ref: MessageRef,
        folder_id: u64,
    ) -> Result<Bookmark, BoxError> {
        let folders = self.folders(user)?;
        validate_folder_ids(&folders, vec![folder_id])?;
        let mut bookmark = self
            .find(user, message_ref.conversation)
            .await?
            .ok_or_else(|| format!("bookmark {} does not exist", message_ref.message_id()))?;
        if !bookmark
            .messages
            .iter()
            .any(|message| message.index == message_ref.index)
        {
            return Err(format!("bookmark {} does not exist", message_ref.message_id()).into());
        }
        if !bookmark.folder_ids.contains(&folder_id) {
            bookmark.folder_ids.push(folder_id);
            bookmark.folder_ids = normalize_folder_ids(bookmark.folder_ids);
            bookmark = self.update_bookmark(bookmark).await?;
        }
        Ok(bookmark)
    }

    pub async fn remove_bookmark_from_folder(
        &self,
        user: &str,
        message_ref: MessageRef,
        folder_id: u64,
    ) -> Result<Bookmark, BoxError> {
        let folders = self.folders(user)?;
        validate_folder_ids(&folders, vec![folder_id])?;
        let mut bookmark = self
            .find(user, message_ref.conversation)
            .await?
            .ok_or_else(|| format!("bookmark {} does not exist", message_ref.message_id()))?;
        if !bookmark
            .messages
            .iter()
            .any(|message| message.index == message_ref.index)
        {
            return Err(format!("bookmark {} does not exist", message_ref.message_id()).into());
        }
        let before = bookmark.folder_ids.len();
        bookmark.folder_ids.retain(|id| *id != folder_id);
        if bookmark.folder_ids.len() != before {
            bookmark = self.update_bookmark(bookmark).await?;
        }
        Ok(bookmark)
    }

    async fn update_bookmark(&self, bookmark: Bookmark) -> Result<Bookmark, BoxError> {
        let doc = self
            .bookmarks
            .update(
                bookmark._id,
                BTreeMap::from([
                    ("source".to_string(), Fv::Text(bookmark.source.clone())),
                    (
                        "folder_ids".to_string(),
                        Fv::serialized(&bookmark.folder_ids, None)?,
                    ),
                    (
                        "messages".to_string(),
                        Fv::serialized(&bookmark.messages, None)?,
                    ),
                ]),
            )
            .await?;
        self.bookmarks.flush(unix_ms()).await?;
        Ok(doc.try_into()?)
    }

    async fn remove_folder_ids_from_bookmarks(
        &self,
        user: &str,
        folder_ids: &BTreeSet<u64>,
    ) -> Result<(), BoxError> {
        let mut cursor = self.bookmarks.max_document_id() + 1;
        loop {
            let rt: Vec<Bookmark> = self
                .bookmarks
                .search_as(Query {
                    search: None,
                    filter: Some(Filter::And(vec![
                        Box::new(Filter::Field((
                            "user".to_string(),
                            RangeQuery::Eq(Fv::Text(user.to_string())),
                        ))),
                        Box::new(Filter::Field((
                            "_id".to_string(),
                            RangeQuery::Lt(Fv::U64(cursor)),
                        ))),
                    ])),
                    limit: Some(MAX_LIST_LIMIT),
                })
                .await?;
            let has_more = rt.len() >= MAX_LIST_LIMIT;
            if let Some(first) = rt.first() {
                cursor = first._id;
            }
            for mut bookmark in rt {
                let before = bookmark.folder_ids.len();
                bookmark.folder_ids.retain(|id| !folder_ids.contains(id));
                if bookmark.folder_ids.len() != before {
                    self.update_bookmark(bookmark).await?;
                }
            }
            if !has_more {
                return Ok(());
            }
        }
    }

    fn load_all_folders(&self) -> BookmarkFoldersByUser {
        self.bookmarks
            .get_extension_as::<BookmarkFoldersByUser>(BOOKMARK_FOLDERS_EXTENSION_KEY)
            .unwrap_or_default()
    }

    async fn save_all_folders(&self, all: &BookmarkFoldersByUser) -> Result<(), BoxError> {
        self.bookmarks
            .save_extension_from(BOOKMARK_FOLDERS_EXTENSION_KEY.to_string(), all)
            .await?;
        Ok(())
    }
}

impl BookmarkFolders {
    fn normalized(mut self) -> Self {
        self.normalize_in_place();
        self
    }

    fn normalize_in_place(&mut self) {
        self.version = 1;
        self.next_folder_id = self.next_folder_id.max(1);
        let max_id = self.folders.keys().copied().max().unwrap_or(0);
        self.next_folder_id = self.next_folder_id.max(max_id.saturating_add(1));
    }
}

fn normalize_folder_name(name: String) -> Result<String, BoxError> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("folder name is required".into());
    }
    if name.chars().count() > 80 {
        return Err("folder name must be 80 characters or fewer".into());
    }
    Ok(name)
}

fn normalize_folder_ids(folder_ids: Vec<u64>) -> Vec<u64> {
    let mut seen = BTreeSet::new();
    folder_ids
        .into_iter()
        .filter(|id| *id != 0 && seen.insert(*id))
        .collect()
}

fn validate_folder_ids(
    folders: &BookmarkFolders,
    folder_ids: Vec<u64>,
) -> Result<Vec<u64>, BoxError> {
    let folder_ids = normalize_folder_ids(folder_ids);
    for id in &folder_ids {
        if !folders.folders.contains_key(id) {
            return Err(format!("bookmark folder {id} does not exist").into());
        }
    }
    Ok(folder_ids)
}

fn normalize_messages(messages: &mut Vec<MessageInfo>) {
    messages.sort_by_key(|message| message.index);
    messages.dedup_by_key(|message| message.index);
}

fn validate_parent_exists(
    folders: &BookmarkFolders,
    parent_id: Option<u64>,
) -> Result<(), BoxError> {
    if let Some(parent_id) = parent_id
        && !folders.folders.contains_key(&parent_id)
    {
        return Err(format!("parent bookmark folder {parent_id} does not exist").into());
    }
    Ok(())
}

fn ensure_unique_folder_name(
    folders: &BookmarkFolders,
    except_id: Option<u64>,
    parent_id: Option<u64>,
    name: &str,
) -> Result<(), BoxError> {
    let normalized = name.to_lowercase();
    let duplicate = folders.folders.values().any(|folder| {
        Some(folder._id) != except_id
            && folder.parent_id == parent_id
            && folder.name.to_lowercase() == normalized
    });
    if duplicate {
        Err("a bookmark folder with this name already exists".into())
    } else {
        Ok(())
    }
}

fn next_folder_order(folders: &BookmarkFolders, parent_id: Option<u64>) -> i64 {
    folders
        .folders
        .values()
        .filter(|folder| folder.parent_id == parent_id)
        .map(|folder| folder.order)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

fn is_descendant_folder(
    folders: &BookmarkFolders,
    maybe_descendant: Option<u64>,
    ancestor: u64,
) -> bool {
    let mut current = maybe_descendant;
    let mut seen = BTreeSet::new();
    while let Some(folder_id) = current {
        if folder_id == ancestor {
            return true;
        }
        if !seen.insert(folder_id) {
            return false;
        }
        current = folders
            .folders
            .get(&folder_id)
            .and_then(|folder| folder.parent_id);
    }
    false
}

fn folder_subtree_ids(folders: &BookmarkFolders, root_id: u64) -> BTreeSet<u64> {
    let mut ids = BTreeSet::from([root_id]);
    loop {
        let before = ids.len();
        for folder in folders.folders.values() {
            if folder
                .parent_id
                .is_some_and(|parent_id| ids.contains(&parent_id))
            {
                ids.insert(folder._id);
            }
        }
        if ids.len() == before {
            return ids;
        }
    }
}

/// Arguments for the `bookmarks_api` tool.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum BookmarksToolArgs {
    /// Bookmark an assistant message.
    AddBookmark {
        /// Stable client message id `m-<conversation>-<index>`.
        message_id: String,
        /// Conversation the message belongs to.
        conversation: u64,
        /// Channel source key.
        source: String,
        /// Message role (usually `assistant`).
        role: String,
        /// Original message text used to generate the preview.
        text: String,
        /// Optional folder ids to assign immediately.
        #[serde(default)]
        folder_ids: Option<Vec<u64>>,
    },
    /// Remove a bookmark by message id.
    RemoveBookmark {
        /// The bookmarked message id to remove.
        message_id: String,
    },
    /// List bookmarks newest-first, paginated.
    ListBookmarks {
        /// Pagination cursor from a previous response. Omit for the first page.
        #[serde(default)]
        cursor: Option<String>,
        /// Page size, default 20, max 100.
        #[serde(default)]
        limit: Option<usize>,
    },
    /// Fetch one conversation bookmark for rendering message star state.
    GetConversationBookmark {
        /// Conversation id to inspect.
        conversation: u64,
    },
    /// List bookmark folder metadata.
    ListBookmarkFolders {},
    /// Create a bookmark folder.
    CreateBookmarkFolder {
        name: String,
        #[serde(default)]
        parent_id: Option<u64>,
    },
    /// Rename a bookmark folder.
    RenameBookmarkFolder { folder_id: u64, name: String },
    /// Delete a bookmark folder. Bookmarks remain; this only removes folder membership.
    DeleteBookmarkFolder { folder_id: u64 },
    /// Move/reorder a bookmark folder.
    MoveBookmarkFolder {
        folder_id: u64,
        #[serde(default)]
        parent_id: Option<u64>,
        #[serde(default)]
        order: Option<i64>,
    },
    /// Replace all folders assigned to one bookmark.
    SetBookmarkFolders {
        message_id: String,
        folder_ids: Vec<u64>,
    },
    /// Add one bookmark to a folder.
    AddBookmarkToFolder { message_id: String, folder_id: u64 },
    /// Remove one bookmark from a folder.
    RemoveBookmarkFromFolder { message_id: String, folder_id: u64 },
    /// List bookmarks in a folder. folder_id = 0 means unfiled.
    ListBookmarksInFolder {
        folder_id: u64,
        #[serde(default)]
        cursor: Option<String>,
        #[serde(default)]
        limit: Option<usize>,
    },
}

fn bookmarks_tool_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "type": {
                "type": "string",
                "enum": [
                    "AddBookmark",
                    "RemoveBookmark",
                    "ListBookmarks",
                    "GetConversationBookmark",
                    "ListBookmarkFolders",
                    "CreateBookmarkFolder",
                    "RenameBookmarkFolder",
                    "DeleteBookmarkFolder",
                    "MoveBookmarkFolder",
                    "SetBookmarkFolders",
                    "AddBookmarkToFolder",
                    "RemoveBookmarkFromFolder",
                    "ListBookmarksInFolder"
                ],
                "description": "Bookmark operation to perform."
            },
            "message_id": {
                "type": ["string", "null"],
                "description": "Stable client message id `m-<conversation>-<index>`. Required for AddBookmark and RemoveBookmark."
            },
            "conversation": {
                "type": ["integer", "null"],
                "description": "Conversation the message belongs to. Only for AddBookmark."
            },
            "source": {
                "type": ["string", "null"],
                "description": "Channel source key. Only for AddBookmark."
            },
            "role": {
                "type": ["string", "null"],
                "description": "Message role, usually `assistant`. Only for AddBookmark."
            },
            "text": {
                "type": ["string", "null"],
                "description": "Original message text used to generate the preview. Only for AddBookmark."
            },
            "folder_ids": {
                "type": ["array", "null"],
                "items": { "type": "integer" },
                "description": "Bookmark folder ids. Optional for AddBookmark; required for SetBookmarkFolders."
            },
            "folder_id": {
                "type": ["integer", "null"],
                "description": "Bookmark folder id. Use 0 for unfiled in ListBookmarksInFolder."
            },
            "name": {
                "type": ["string", "null"],
                "description": "Bookmark folder name."
            },
            "parent_id": {
                "type": ["integer", "null"],
                "description": "Optional parent bookmark folder id."
            },
            "order": {
                "type": ["integer", "null"],
                "description": "Optional manual ordering value for folders."
            },
            "cursor": {
                "type": ["string", "null"],
                "description": "Pagination cursor from a previous ListBookmarks response. Omit for the first page."
            },
            "limit": {
                "type": ["integer", "null"],
                "description": "Optional page size for ListBookmarks. Defaults to 20, max 100."
            }
        },
        "required": [
            "type",
            "message_id",
            "conversation",
            "source",
            "role",
            "text",
            "folder_ids",
            "folder_id",
            "name",
            "parent_id",
            "order",
            "cursor",
            "limit"
        ],
        "additionalProperties": false
    })
}

/// A tool for the bookmarks API.
#[derive(Clone)]
pub struct BookmarksTool {
    store: BookmarkStore,
    models: Option<Arc<Models>>,
}

impl BookmarksTool {
    pub const NAME: &'static str = "bookmarks_api";

    #[cfg(test)]
    pub fn new(store: BookmarkStore) -> Self {
        Self {
            store,
            models: None,
        }
    }

    pub fn with_models(store: BookmarkStore, models: Arc<Models>) -> Self {
        Self {
            store,
            models: Some(models),
        }
    }

    async fn bookmark_text(&self, source_text: &str) -> String {
        let fallback = fallback_bookmark_text(source_text);
        let Some(model) = self.models.as_ref().and_then(|models| {
            models
                .get("lite")
                .or_else(|| models.get("flash"))
                .or_else(|| models.get_model())
        }) else {
            return fallback;
        };

        match model
            .completion(CompletionRequest {
                instructions: concat!(
                    "Generate a concise bookmark preview for one assistant chat message. ",
                    "Return only the preview text, no markdown list, no quotes, no preamble. ",
                    "Preserve concrete names, commands, files, decisions, and outcomes. ",
                    "Keep it roughly under 80 CJK characters, 120 Latin-script words, ",
                    "or a comparable short length for other languages. ",
                    "Detect the source message's natural language and write the preview in that same language, ",
                    "including languages other than Chinese or English. ",
                    "Do not translate it to another language. ",
                    "For mixed-language messages, follow the dominant natural language. ",
                    "If the message is mostly code, commands, logs, or file paths, keep the original technical tokens."
                )
                .to_string(),
                content: vec![ContentPart::Text {
                    text: source_text.to_string(),
                }],
                effort: Some(ModelEffort::Low),
                ..Default::default()
            })
            .await
        {
            Ok(output) => {
                let text = output.content.trim();
                if text.is_empty() {
                    fallback
                } else {
                    truncate_bookmark_preview(text, BOOKMARK_PREVIEW_MAX_BYTES)
                }
            }
            Err(err) => {
                log::warn!("failed to generate bookmark preview; using fallback text: {err}");
                fallback
            }
        }
    }
}

fn fallback_bookmark_text(text: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_bookmark_preview(&normalized, BOOKMARK_PREVIEW_MAX_BYTES)
}

fn truncate_bookmark_preview(text: &str, max_bytes: usize) -> String {
    let mut out = text.to_string();
    if truncate_utf8_to_max_bytes(&mut out, max_bytes).is_some() {
        out.push('…');
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageRef {
    conversation: u64,
    index: usize,
}

impl MessageRef {
    fn message_id(self) -> String {
        format!("m-{}-{}", self.conversation, self.index)
    }
}

fn parse_message_id(message_id: &str) -> Result<MessageRef, BoxError> {
    let mut parts = message_id.split('-');
    if !matches!(parts.next(), Some("m")) {
        return Err("message_id must be a stable chat message id".into());
    }
    let Some(conversation) = parts.next().and_then(|part| {
        (!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse::<u64>().ok())
            .flatten()
    }) else {
        return Err("message_id must be a stable chat message id".into());
    };
    let Some(index) = parts.next().and_then(|part| {
        (!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse::<usize>().ok())
            .flatten()
    }) else {
        return Err("message_id must be a stable chat message id".into());
    };
    if parts.next().is_some() {
        return Err("message_id must be a stable chat message id".into());
    }
    Ok(MessageRef {
        conversation,
        index,
    })
}

fn normalize_message_id(message_id: String) -> Result<MessageRef, BoxError> {
    let message_id = message_id.trim().to_string();
    if message_id.is_empty() {
        return Err("message_id is required".into());
    }
    parse_message_id(&message_id)
}

impl Tool<BaseCtx> for BookmarksTool {
    type Args = BookmarksToolArgs;
    type Output = Response;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        concat!(
            "Manage the caller's saved (bookmarked) chat messages grouped by conversation. ",
            "Use AddBookmark to save an assistant message, RemoveBookmark to unsave it, ",
            "ListBookmarks to page through saved conversations newest-first, and ",
            "GetConversationBookmark to fetch marked messages for one conversation. ",
            "Use ListBookmarkFolders, CreateBookmarkFolder, RenameBookmarkFolder, ",
            "DeleteBookmarkFolder, MoveBookmarkFolder, SetBookmarkFolders, ",
            "AddBookmarkToFolder, RemoveBookmarkFromFolder, and ListBookmarksInFolder ",
            "to organize bookmarks into user folders."
        )
        .to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: bookmarks_tool_parameters(),
            strict: Some(true),
        }
    }

    async fn call(
        &self,
        ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let user = ctx.caller().to_text();
        match args {
            BookmarksToolArgs::AddBookmark {
                message_id,
                conversation,
                source,
                role,
                text,
                folder_ids,
            } => {
                let message_ref = normalize_message_id(message_id)?;
                let source = source.trim().to_string();
                if source.is_empty() {
                    return Err("source is required".into());
                }
                let role = role.trim().to_string();
                if role != "assistant" {
                    return Err("only assistant messages can be bookmarked".into());
                }
                if conversation == 0 {
                    return Err("conversation is required".into());
                }
                if conversation != message_ref.conversation {
                    return Err("message_id conversation does not match conversation".into());
                }
                if text.trim().is_empty() {
                    return Err("text is required".into());
                }
                let folder_ids = validate_folder_ids(
                    &self.store.folders(&user)?,
                    folder_ids.unwrap_or_default(),
                )?;
                let preview_text = self.bookmark_text(&text).await;

                let bookmark = self
                    .store
                    .add(
                        user,
                        conversation,
                        source,
                        MessageInfo {
                            index: message_ref.index,
                            role,
                            text: preview_text,
                        },
                        folder_ids,
                    )
                    .await?;

                Ok(ToolOutput::new(Response::Ok {
                    result: json!(bookmark),
                    next_cursor: None,
                }))
            }
            BookmarksToolArgs::RemoveBookmark { message_id } => {
                let message_ref = normalize_message_id(message_id)?;
                let message_id = message_ref.message_id();

                let (removed, bookmark) = self.store.remove(&user, message_ref).await?;
                Ok(ToolOutput::new(Response::Ok {
                    result: json!({
                        "message_id": message_id,
                        "conversation": message_ref.conversation,
                        "removed": removed,
                        "bookmark": bookmark
                    }),
                    next_cursor: None,
                }))
            }
            BookmarksToolArgs::ListBookmarks { cursor, limit } => {
                let (items, next_cursor) = self.store.list(&user, cursor, limit).await?;
                Ok(ToolOutput::new(Response::Ok {
                    result: json!(items),
                    next_cursor,
                }))
            }
            BookmarksToolArgs::GetConversationBookmark { conversation } => {
                if conversation == 0 {
                    return Err("conversation is required".into());
                }
                let bookmark = self.store.find(&user, conversation).await?;
                Ok(ToolOutput::new(Response::Ok {
                    result: json!(bookmark),
                    next_cursor: None,
                }))
            }
            BookmarksToolArgs::ListBookmarkFolders {} => {
                let folders = self.store.folders(&user)?;
                Ok(ToolOutput::new(Response::Ok {
                    result: json!(folders),
                    next_cursor: None,
                }))
            }
            BookmarksToolArgs::CreateBookmarkFolder { name, parent_id } => {
                let folders = self.store.create_folder(&user, name, parent_id).await?;
                Ok(ToolOutput::new(Response::Ok {
                    result: json!(folders),
                    next_cursor: None,
                }))
            }
            BookmarksToolArgs::RenameBookmarkFolder { folder_id, name } => {
                let folders = self.store.rename_folder(&user, folder_id, name).await?;
                Ok(ToolOutput::new(Response::Ok {
                    result: json!(folders),
                    next_cursor: None,
                }))
            }
            BookmarksToolArgs::DeleteBookmarkFolder { folder_id } => {
                let folders = self.store.delete_folder(&user, folder_id).await?;
                Ok(ToolOutput::new(Response::Ok {
                    result: json!(folders),
                    next_cursor: None,
                }))
            }
            BookmarksToolArgs::MoveBookmarkFolder {
                folder_id,
                parent_id,
                order,
            } => {
                let folders = self
                    .store
                    .move_folder(&user, folder_id, parent_id, order)
                    .await?;
                Ok(ToolOutput::new(Response::Ok {
                    result: json!(folders),
                    next_cursor: None,
                }))
            }
            BookmarksToolArgs::SetBookmarkFolders {
                message_id,
                folder_ids,
            } => {
                let message_ref = normalize_message_id(message_id)?;
                let bookmark = self
                    .store
                    .set_bookmark_folders(&user, message_ref, folder_ids)
                    .await?;
                Ok(ToolOutput::new(Response::Ok {
                    result: json!(bookmark),
                    next_cursor: None,
                }))
            }
            BookmarksToolArgs::AddBookmarkToFolder {
                message_id,
                folder_id,
            } => {
                let message_ref = normalize_message_id(message_id)?;
                let bookmark = self
                    .store
                    .add_bookmark_to_folder(&user, message_ref, folder_id)
                    .await?;
                Ok(ToolOutput::new(Response::Ok {
                    result: json!(bookmark),
                    next_cursor: None,
                }))
            }
            BookmarksToolArgs::RemoveBookmarkFromFolder {
                message_id,
                folder_id,
            } => {
                let message_ref = normalize_message_id(message_id)?;
                let bookmark = self
                    .store
                    .remove_bookmark_from_folder(&user, message_ref, folder_id)
                    .await?;
                Ok(ToolOutput::new(Response::Ok {
                    result: json!(bookmark),
                    next_cursor: None,
                }))
            }
            BookmarksToolArgs::ListBookmarksInFolder {
                folder_id,
                cursor,
                limit,
            } => {
                let (items, next_cursor) = self
                    .store
                    .list_in_folder(&user, folder_id, cursor, limit)
                    .await?;
                Ok(ToolOutput::new(Response::Ok {
                    result: json!(items),
                    next_cursor,
                }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::json_schema::assert_openai_strict_parameters;
    use anda_core::{AgentOutput, BoxPinFut};
    use anda_db::{
        database::{AndaDB, DBConfig},
        storage::StorageConfig,
    };
    use anda_engine::{
        engine::EngineBuilder,
        model::{CompletionFeaturesDyn, Model},
    };
    use object_store::memory::InMemory;
    use std::sync::Mutex;

    async fn test_store() -> BookmarkStore {
        let object_store: Arc<dyn object_store::ObjectStore> = Arc::new(InMemory::new());
        test_store_with_object_store(object_store).await
    }

    async fn test_store_with_object_store(
        object_store: Arc<dyn object_store::ObjectStore>,
    ) -> BookmarkStore {
        let db = AndaDB::connect(
            object_store,
            DBConfig {
                name: "bookmarks_test_db".to_string(),
                description: "bookmarks test db".to_string(),
                storage: StorageConfig {
                    cache_max_capacity: 1024,
                    compress_level: 1,
                    object_chunk_size: 256 * 1024,
                    bucket_overload_size: 256 * 1024,
                    max_small_object_size: 1024 * 1024,
                },
                lock: None,
            },
        )
        .await
        .unwrap();
        BookmarkStore::connect(Arc::new(db)).await.unwrap()
    }

    async fn add_sample(store: &BookmarkStore, user: &str, message_id: &str) -> Bookmark {
        let message_ref = parse_message_id(message_id).unwrap();
        store
            .add(
                user.to_string(),
                message_ref.conversation,
                "cli:/tmp/ws".to_string(),
                MessageInfo {
                    index: message_ref.index,
                    role: "assistant".to_string(),
                    text: format!("snapshot for {message_id}"),
                },
                Vec::new(),
            )
            .await
            .unwrap()
    }

    fn message_indexes(bookmark: &Bookmark) -> Vec<usize> {
        bookmark
            .messages
            .iter()
            .map(|message| message.index)
            .collect()
    }

    struct RecordingCompleter {
        requests: Arc<Mutex<Vec<CompletionRequest>>>,
        response: String,
    }

    impl CompletionFeaturesDyn for RecordingCompleter {
        fn completion(&self, req: CompletionRequest) -> BoxPinFut<Result<AgentOutput, BoxError>> {
            self.requests.lock().unwrap().push(req);
            let content = self.response.clone();
            Box::pin(async move {
                Ok(AgentOutput {
                    content,
                    ..Default::default()
                })
            })
        }

        fn model_name(&self) -> String {
            "recording".to_string()
        }
    }

    #[test]
    fn bookmark_preview_truncation_respects_utf8_boundaries() {
        assert_eq!(truncate_bookmark_preview("你好世界", 7), "你好…");
        assert_eq!(truncate_bookmark_preview("👩‍💻 report", 5), "…");
    }

    #[tokio::test]
    async fn bookmark_text_sends_original_text_to_summary_model() {
        let store = test_store().await;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let models = Arc::new(Models::default());
        models.set(
            "flash".to_string(),
            Model::with_completer(Arc::new(RecordingCompleter {
                requests: requests.clone(),
                response: "保持原文语言的摘要".to_string(),
            })),
        );
        let tool = BookmarksTool::with_models(store, models);
        let source = format!(
            "原始消息开头 {} 原始消息结尾",
            "需要完整参与摘要的内容".repeat(BOOKMARK_PREVIEW_MAX_BYTES)
        );

        let preview = tool.bookmark_text(&source).await;

        assert_eq!(preview, "保持原文语言的摘要");
        let requests = requests.lock().unwrap();
        let request = requests.first().expect("expected one summary request");
        assert_eq!(requests.len(), 1);
        assert!(request.instructions.contains("same language"));
        assert!(
            request
                .instructions
                .contains("languages other than Chinese or English")
        );
        assert!(request.instructions.contains("comparable short length"));
        assert!(request.instructions.contains("Do not translate"));
        assert!(request.prompt.is_empty());
        assert!(source.len() > BOOKMARK_PREVIEW_MAX_BYTES);
        assert_eq!(
            request.content,
            vec![ContentPart::Text {
                text: source.clone()
            }]
        );
    }

    #[tokio::test]
    async fn add_is_idempotent_on_conversation_message_index() {
        let store = test_store().await;

        let first = add_sample(&store, "alice", "m-1-0").await;
        assert!(first._id > 0);
        assert!(first.created_at > 0);
        assert_eq!(message_indexes(&first), vec![0]);

        let again = add_sample(&store, "alice", "m-1-0").await;
        assert_eq!(again._id, first._id, "repeat add returns the same row");
        assert_eq!(again.messages.len(), 1);

        let second = add_sample(&store, "alice", "m-1-1").await;
        assert_eq!(second._id, first._id, "same conversation stays one row");
        assert_eq!(message_indexes(&second), vec![0, 1]);
    }

    #[tokio::test]
    async fn remove_reports_hit_and_miss() {
        let store = test_store().await;
        add_sample(&store, "alice", "m-1-0").await;
        add_sample(&store, "alice", "m-1-1").await;

        let (removed, remaining) = store
            .remove("alice", parse_message_id("m-1-0").unwrap())
            .await
            .unwrap();
        assert!(removed);
        assert_eq!(message_indexes(&remaining.unwrap()), vec![1]);

        let (removed, remaining) = store
            .remove("alice", parse_message_id("m-1-0").unwrap())
            .await
            .unwrap();
        assert!(!removed);
        assert_eq!(message_indexes(&remaining.unwrap()), vec![1]);

        let (removed, remaining) = store
            .remove("alice", parse_message_id("m-1-1").unwrap())
            .await
            .unwrap();
        assert!(removed);
        assert!(remaining.is_none());
        assert!(store.find("alice", 1).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn list_is_scoped_per_user_and_newest_first() {
        let store = test_store().await;
        add_sample(&store, "alice", "m-1-0").await;
        add_sample(&store, "alice", "m-2-0").await;
        add_sample(&store, "bob", "m-3-0").await;

        let (items, next) = store.list("alice", None, Some(10)).await.unwrap();
        assert!(next.is_none());
        let ids: Vec<u64> = items.iter().map(|b| b.conversation).collect();
        // Newest first: m-2-0 was added after m-1-0.
        assert_eq!(ids, vec![2, 1]);

        let bob = store.find("bob", 3).await.unwrap().unwrap();
        assert_eq!(message_indexes(&bob), vec![0]);
    }

    #[tokio::test]
    async fn list_cursor_pages_without_overlap() {
        let store = test_store().await;
        for i in 0..3u64 {
            add_sample(&store, "alice", &format!("m-{i}-0")).await;
        }

        let (page1, cursor1) = store.list("alice", None, Some(2)).await.unwrap();
        let cursor1 = cursor1.expect("expected a next cursor");
        let (page2, cursor2) = store.list("alice", Some(cursor1), Some(2)).await.unwrap();

        assert_eq!(page1.len(), 2);
        assert_eq!(page2.len(), 1);
        assert!(cursor2.is_none());

        let mut seen: Vec<u64> = page1.iter().map(|b| b.conversation).collect();
        seen.extend(page2.iter().map(|b| b.conversation));
        seen.sort();
        assert_eq!(seen, vec![0, 1, 2]);
    }

    #[tokio::test]
    async fn folders_are_scoped_per_user() {
        let store = test_store().await;

        let folders = store
            .create_folder("alice", "Project".to_string(), None)
            .await
            .unwrap();
        assert_eq!(folders.next_folder_id, 2);
        assert_eq!(folders.folders.get(&1).unwrap().name, "Project");
        assert!(store.folders("bob").unwrap().folders.is_empty());

        let restored = store.folders("alice").unwrap();
        assert_eq!(restored.next_folder_id, 2);
        assert_eq!(restored.folders.get(&1).unwrap().name, "Project");
    }

    #[tokio::test]
    async fn folder_names_are_unique_within_parent() {
        let store = test_store().await;
        store
            .create_folder("alice", "Project".to_string(), None)
            .await
            .unwrap();
        let err = store
            .create_folder("alice", " project ".to_string(), None)
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("a bookmark folder with this name already exists")
        );

        let child = store
            .create_folder("alice", "Child".to_string(), Some(1))
            .await
            .unwrap();
        assert_eq!(child.folders.get(&2).unwrap().parent_id, Some(1));

        let renamed = store
            .rename_folder("alice", 2, "Child Renamed".to_string())
            .await
            .unwrap();
        assert_eq!(renamed.folders.get(&2).unwrap().name, "Child Renamed");

        let moved = store.move_folder("alice", 2, None, Some(10)).await.unwrap();
        assert_eq!(moved.folders.get(&2).unwrap().parent_id, None);
        assert_eq!(moved.folders.get(&2).unwrap().order, 10);
    }

    #[tokio::test]
    async fn bookmark_folder_membership_round_trips_and_filters() {
        let store = test_store().await;
        store
            .create_folder("alice", "Work".to_string(), None)
            .await
            .unwrap();
        store
            .create_folder("alice", "Read".to_string(), None)
            .await
            .unwrap();
        add_sample(&store, "alice", "m-1-0").await;
        add_sample(&store, "alice", "m-2-0").await;
        add_sample(&store, "alice", "m-3-0").await;

        let updated = store
            .set_bookmark_folders("alice", parse_message_id("m-1-0").unwrap(), vec![1, 2, 1])
            .await
            .unwrap();
        assert_eq!(updated.folder_ids, vec![1, 2]);
        store
            .add_bookmark_to_folder("alice", parse_message_id("m-2-0").unwrap(), 2)
            .await
            .unwrap();

        let (work, _) = store
            .list_in_folder("alice", 1, None, Some(10))
            .await
            .unwrap();
        assert_eq!(
            work.iter().map(|b| b.conversation).collect::<Vec<_>>(),
            vec![1]
        );
        let (read, _) = store
            .list_in_folder("alice", 2, None, Some(10))
            .await
            .unwrap();
        assert_eq!(
            read.iter().map(|b| b.conversation).collect::<Vec<_>>(),
            vec![2, 1]
        );
        let (unfiled, _) = store
            .list_in_folder("alice", 0, None, Some(10))
            .await
            .unwrap();
        assert_eq!(
            unfiled.iter().map(|b| b.conversation).collect::<Vec<_>>(),
            vec![3]
        );

        let updated = store
            .remove_bookmark_from_folder("alice", parse_message_id("m-1-0").unwrap(), 2)
            .await
            .unwrap();
        assert_eq!(updated.folder_ids, vec![1]);

        let err = store
            .remove_bookmark_from_folder("alice", parse_message_id("m-1-0").unwrap(), 999)
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("bookmark folder 999 does not exist")
        );
    }

    #[tokio::test]
    async fn deleting_folder_removes_membership_without_deleting_bookmarks() {
        let store = test_store().await;
        store
            .create_folder("alice", "Work".to_string(), None)
            .await
            .unwrap();
        add_sample(&store, "alice", "m-1-0").await;
        store
            .set_bookmark_folders("alice", parse_message_id("m-1-0").unwrap(), vec![1])
            .await
            .unwrap();

        let folders = store.delete_folder("alice", 1).await.unwrap();
        assert!(folders.folders.is_empty());

        let (items, _) = store.list("alice", None, Some(10)).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].conversation, 1);
        assert_eq!(message_indexes(&items[0]), vec![0]);
        assert!(items[0].folder_ids.is_empty());
    }

    #[test]
    fn bookmarks_api_schema_is_openai_strict() {
        assert_openai_strict_parameters(&bookmarks_tool_parameters());
    }

    #[test]
    fn bookmarks_tool_args_parse_tagged_variants() {
        let args: BookmarksToolArgs = serde_json::from_value(json!({
            "type": "AddBookmark",
            "message_id": "m-1-0",
            "conversation": 1,
            "source": "cli:/tmp/ws",
            "role": "assistant",
            "text": "hello",
        }))
        .expect("add variant should parse");
        assert!(matches!(args, BookmarksToolArgs::AddBookmark { .. }));

        let args: BookmarksToolArgs = serde_json::from_value(json!({
            "type": "ListBookmarks",
        }))
        .expect("missing optional list fields should parse");
        assert_eq!(
            args,
            BookmarksToolArgs::ListBookmarks {
                cursor: None,
                limit: None,
            }
        );

        let args: BookmarksToolArgs = serde_json::from_value(json!({
            "type": "GetConversationBookmark",
            "conversation": 1,
        }))
        .expect("conversation bookmark variant should parse");
        assert_eq!(
            args,
            BookmarksToolArgs::GetConversationBookmark { conversation: 1 }
        );
    }

    #[tokio::test]
    async fn tool_call_round_trips_and_scopes_by_caller() {
        let store = test_store().await;
        let tool = BookmarksTool::new(store);
        let ctx = EngineBuilder::new().mock_ctx().base;
        let caller = ctx.caller().to_text();

        let output = tool
            .call(
                ctx.clone(),
                BookmarksToolArgs::AddBookmark {
                    message_id: " m-1-0 ".to_string(),
                    conversation: 1,
                    source: "cli:/tmp/ws".to_string(),
                    role: "assistant".to_string(),
                    text: "hello world".to_string(),
                    folder_ids: None,
                },
                Vec::new(),
            )
            .await
            .unwrap();
        match output.output {
            Response::Ok { result, .. } => {
                assert_eq!(result["user"], caller);
                assert_eq!(result["conversation"], 1);
                assert_eq!(result["messages"][0]["index"], 0);
                assert_eq!(result["messages"][0]["role"], "assistant");
                assert_eq!(result["messages"][0]["text"], "hello world");
            }
            other => panic!("expected ok response, got {other:?}"),
        }

        let output = tool
            .call(
                ctx.clone(),
                BookmarksToolArgs::GetConversationBookmark { conversation: 1 },
                Vec::new(),
            )
            .await
            .unwrap();
        match output.output {
            Response::Ok { result, .. } => {
                assert_eq!(result["conversation"], 1);
                assert_eq!(result["messages"][0]["index"], 0);
                assert_eq!(result["messages"][0]["text"], "hello world");
            }
            other => panic!("expected ok response, got {other:?}"),
        }

        let output = tool
            .call(
                ctx.clone(),
                BookmarksToolArgs::RemoveBookmark {
                    message_id: "m-1-0".to_string(),
                },
                Vec::new(),
            )
            .await
            .unwrap();
        match output.output {
            Response::Ok { result, .. } => {
                assert_eq!(result["removed"], true);
                assert_eq!(result["bookmark"], Value::Null);
            }
            other => panic!("expected ok response, got {other:?}"),
        }

        let err = tool
            .call(
                ctx,
                BookmarksToolArgs::AddBookmark {
                    message_id: "   ".to_string(),
                    conversation: 1,
                    source: "cli:/tmp/ws".to_string(),
                    role: "assistant".to_string(),
                    text: "x".to_string(),
                    folder_ids: None,
                },
                Vec::new(),
            )
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("message_id is required"));
    }

    #[tokio::test]
    async fn tool_rejects_non_assistant_or_unstable_bookmarks() {
        let store = test_store().await;
        let tool = BookmarksTool::new(store);
        let ctx = EngineBuilder::new().mock_ctx().base;

        let err = tool
            .call(
                ctx.clone(),
                BookmarksToolArgs::AddBookmark {
                    message_id: "m-local-1".to_string(),
                    conversation: 1,
                    source: "cli:/tmp/ws".to_string(),
                    role: "assistant".to_string(),
                    text: "hello".to_string(),
                    folder_ids: None,
                },
                Vec::new(),
            )
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("message_id must be a stable chat message id")
        );

        let err = tool
            .call(
                ctx,
                BookmarksToolArgs::AddBookmark {
                    message_id: "m-1-0".to_string(),
                    conversation: 1,
                    source: "cli:/tmp/ws".to_string(),
                    role: "user".to_string(),
                    text: "hello".to_string(),
                    folder_ids: None,
                },
                Vec::new(),
            )
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("only assistant messages can be bookmarked")
        );
    }

    #[tokio::test]
    async fn tool_folder_calls_round_trip() {
        let store = test_store().await;
        let tool = BookmarksTool::new(store);
        let ctx = EngineBuilder::new().mock_ctx().base;

        let output = tool
            .call(
                ctx.clone(),
                BookmarksToolArgs::CreateBookmarkFolder {
                    name: "Work".to_string(),
                    parent_id: None,
                },
                Vec::new(),
            )
            .await
            .unwrap();
        match output.output {
            Response::Ok { result, .. } => {
                let folders: BookmarkFolders = serde_json::from_value(result).unwrap();
                assert_eq!(folders.next_folder_id, 2);
                assert_eq!(folders.folders.get(&1).unwrap().name, "Work");
            }
            other => panic!("expected ok response, got {other:?}"),
        }

        let output = tool
            .call(
                ctx.clone(),
                BookmarksToolArgs::AddBookmark {
                    message_id: "m-1-0".to_string(),
                    conversation: 1,
                    source: "cli:/tmp/ws".to_string(),
                    role: "assistant".to_string(),
                    text: "hello work".to_string(),
                    folder_ids: Some(vec![1]),
                },
                Vec::new(),
            )
            .await
            .unwrap();
        match output.output {
            Response::Ok { result, .. } => {
                let bookmark: Bookmark = serde_json::from_value(result).unwrap();
                assert_eq!(bookmark.folder_ids, vec![1]);
                assert_eq!(bookmark.messages[0].index, 0);
                assert_eq!(bookmark.messages[0].text, "hello work");
            }
            other => panic!("expected ok response, got {other:?}"),
        }

        let output = tool
            .call(
                ctx.clone(),
                BookmarksToolArgs::ListBookmarksInFolder {
                    folder_id: 1,
                    cursor: None,
                    limit: Some(10),
                },
                Vec::new(),
            )
            .await
            .unwrap();
        match output.output {
            Response::Ok { result, .. } => {
                let bookmarks: Vec<Bookmark> = serde_json::from_value(result).unwrap();
                assert_eq!(bookmarks.len(), 1);
                assert_eq!(bookmarks[0].conversation, 1);
                assert_eq!(message_indexes(&bookmarks[0]), vec![0]);
            }
            other => panic!("expected ok response, got {other:?}"),
        }

        tool.call(
            ctx.clone(),
            BookmarksToolArgs::DeleteBookmarkFolder { folder_id: 1 },
            Vec::new(),
        )
        .await
        .unwrap();

        let output = tool
            .call(
                ctx,
                BookmarksToolArgs::ListBookmarks {
                    cursor: None,
                    limit: Some(10),
                },
                Vec::new(),
            )
            .await
            .unwrap();
        match output.output {
            Response::Ok { result, .. } => {
                let bookmarks: Vec<Bookmark> = serde_json::from_value(result).unwrap();
                assert_eq!(bookmarks.len(), 1);
                assert!(bookmarks[0].folder_ids.is_empty());
            }
            other => panic!("expected ok response, got {other:?}"),
        }
    }
}
