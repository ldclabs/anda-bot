//! Scaffolding shared by the `anda` binary's unit tests.
//!
//! Everything here builds a *disposable* dependency: a database that lives in
//! the test process's memory, an HTTP server that answers on a loopback port
//! for as long as the test process runs. Nothing here touches the user's
//! `~/.anda` state, the network, or the real filesystem.
//!
//! These helpers exist so that upstream changes — a new field on `DBConfig`,
//! a different `axum::serve` signature — are one edit instead of a sweep
//! across every test module in the crate.

use anda_db::{
    database::{AndaDB, DBConfig},
    storage::StorageConfig,
};
use axum::Router;
use object_store::{ObjectStore, memory::InMemory};
use std::sync::Arc;

/// An empty [`AndaDB`] backed by a fresh in-memory object store.
///
/// `name` only labels the database for debugging; each call gets its own
/// store, so two databases with the same name never share state. Panics
/// rather than returning an error: a failure here is a broken test, not a
/// condition under test.
pub async fn memory_db(name: &str) -> Arc<AndaDB> {
    db_on_object_store(Arc::new(InMemory::new()), name).await
}

/// An [`AndaDB`] opened on a caller-supplied object store.
///
/// Use this to reopen the same store twice when a test needs to prove that
/// data survives a reconnect; otherwise prefer [`memory_db`].
pub async fn db_on_object_store(object_store: Arc<dyn ObjectStore>, name: &str) -> Arc<AndaDB> {
    // Tuned small and cheap on purpose: tests hold a handful of documents, so
    // the daemon's production cache sizes (see `Daemon::bot_db_config`) would
    // only cost startup time.
    let config = DBConfig {
        name: name.to_string(),
        description: format!("{name} test db"),
        storage: StorageConfig {
            cache_max_capacity: 1024,
            cache_max_bytes: None,
            compress_level: 1,
            object_chunk_size: 256 * 1024,
            bucket_overload_size: 256 * 1024,
            max_small_object_size: 1024 * 1024,
        },
        lock: None,
    };
    Arc::new(AndaDB::connect(object_store, config).await.unwrap())
}

/// Serves `app` on an ephemeral loopback port and returns its base URL
/// (`http://127.0.0.1:<port>`, no trailing slash).
///
/// The server task runs until the test process exits; there is no shutdown
/// handle, because a mock that outlives its test costs nothing. Append the
/// path a client expects — `format!("{base}/v1/anda_bot")` — when the client
/// under test takes a full endpoint rather than a base URL.
///
/// Panics if the loopback port cannot be bound, which in a sandbox without
/// local networking shows up as `PermissionDenied: Operation not permitted`.
pub async fn spawn_http_mock(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}
