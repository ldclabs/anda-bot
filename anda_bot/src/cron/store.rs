use anda_core::BoxError;
use anda_db::{
    collection::{Collection, CollectionConfig},
    database::AndaDB,
    error::DBError,
    index::BTree,
    query::{Filter, Query, RangeQuery},
    schema::Fv,
    unix_ms,
};
use chrono::Utc;
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

use super::types::*;

#[derive(Clone)]
pub struct CronStore {
    jobs: Arc<Collection>,
    runs: Arc<Collection>,
}

impl CronStore {
    pub async fn connect(db: Arc<AndaDB>) -> Result<Self, BoxError> {
        let jobs = db
            .open_or_create_collection(
                CronJob::schema()?,
                CollectionConfig {
                    name: "cron_jobs".to_string(),
                    description: "Scheduled prompt jobs".to_string(),
                },
                async |collection| {
                    collection.create_btree_index_nx(&["next_run"]).await?;
                    collection.create_btree_index_nx(&["created_at"]).await?;
                    Ok::<(), DBError>(())
                },
            )
            .await?;

        let runs = db
            .open_or_create_collection(
                CronRun::schema()?,
                CollectionConfig {
                    name: "cron_runs".to_string(),
                    description: "Prompt cron run history".to_string(),
                },
                async |collection| {
                    collection.create_btree_index_nx(&["job_id"]).await?;
                    collection.create_btree_index_nx(&["started_at"]).await?;
                    Ok::<(), DBError>(())
                },
            )
            .await?;

        Ok(Self { jobs, runs })
    }

    pub async fn insert_job(&self, args: CreateCronJobArgs) -> Result<CronJob, BoxError> {
        let now_ms = unix_ms();
        let mut job = args.into_cron_job(now_ms)?;

        let id = self.jobs.add_from(&job).await?;
        job._id = id;
        self.jobs.flush(now_ms).await?;
        Ok(job)
    }

    pub async fn list_jobs(
        &self,
        cursor: Option<String>,
        limit: Option<usize>,
    ) -> Result<(Vec<CronJob>, Option<String>), BoxError> {
        let limit = limit.unwrap_or(10).min(100);
        let cursor = match BTree::from_cursor::<u64>(&cursor)? {
            Some(cursor) => cursor,
            None => self.jobs.max_document_id() + 1,
        };
        let rt: Vec<CronJob> = self
            .jobs
            .search_as(Query {
                search: None,
                filter: Some(Filter::Field((
                    "_id".to_string(),
                    RangeQuery::Lt(Fv::U64(cursor)),
                ))),
                limit: Some(limit),
            })
            .await?;
        let cursor = if rt.len() >= limit {
            BTree::to_cursor(&rt.first().unwrap()._id)
        } else {
            None
        };
        Ok((rt, cursor))
    }

    pub async fn get_job(&self, id: u64) -> Result<CronJob, BoxError> {
        let job = self.jobs.get_as(id).await?;
        Ok(job)
    }

    pub async fn pause_job(&self, id: u64) -> Result<CronJob, BoxError> {
        let now = Utc::now();
        let now_ms = now.timestamp_millis() as u64;
        let job = self
            .jobs
            .update(
                id,
                BTreeMap::from([
                    ("next_run".to_string(), Fv::U64(DISABLED_JOB_NEXT_RUN)),
                    ("updated_at".to_string(), Fv::U64(now_ms)),
                ]),
            )
            .await?;
        self.jobs.flush(now_ms).await?;
        Ok(job.try_into()?)
    }

    pub async fn resume_job(&self, id: u64) -> Result<CronJob, BoxError> {
        let job: CronJob = self.jobs.get_as(id).await?;
        let now = Utc::now();
        let now_ms = now.timestamp_millis() as u64;
        let schedule = job.schedule()?;
        let next_run = schedule.next_run(now_ms);

        let job = self
            .jobs
            .update(
                id,
                BTreeMap::from([
                    ("next_run".to_string(), Fv::U64(next_run)),
                    ("updated_at".to_string(), Fv::U64(now_ms)),
                ]),
            )
            .await?;
        self.jobs.flush(now_ms).await?;
        Ok(job.try_into()?)
    }

    pub async fn remove_job(&self, id: u64) -> Result<(), BoxError> {
        let now = Utc::now();
        let now_ms = now.timestamp_millis() as u64;
        let _: CronJob = if let Ok(Some(job)) = self.jobs.remove(id).await {
            job.try_into()?
        } else {
            return Ok(());
        };

        // keep the run history for removed jobs, but mark them as disabled
        // let runs = self.list_runs_for_job(job._id).await?;
        // for run in runs {
        //     let _ = self.runs.remove(run._id).await;
        // }
        // self.runs.flush(now_ms).await?;

        self.jobs.flush(now_ms).await?;
        Ok(())
    }

    pub async fn list_runs(
        &self,
        cursor: Option<String>,
        limit: Option<usize>,
        job_id: Option<u64>,
    ) -> Result<(Vec<CronRun>, Option<String>), BoxError> {
        let limit = limit.unwrap_or(10).min(100);
        let cursor = match BTree::from_cursor::<u64>(&cursor)? {
            Some(cursor) => cursor,
            None => self.runs.max_document_id() + 1,
        };
        let filter = if let Some(job_id) = job_id {
            Some(Filter::And(vec![
                Box::new(Filter::Field((
                    "job_id".to_string(),
                    RangeQuery::Eq(Fv::U64(job_id)),
                ))),
                Box::new(Filter::Field((
                    "_id".to_string(),
                    RangeQuery::Lt(Fv::U64(cursor)),
                ))),
            ]))
        } else {
            Some(Filter::Field((
                "_id".to_string(),
                RangeQuery::Lt(Fv::U64(cursor)),
            )))
        };

        let rt: Vec<CronRun> = self
            .runs
            .search_as(Query {
                search: None,
                filter,
                limit: Some(limit),
            })
            .await?;
        let cursor = if rt.len() >= limit {
            BTree::to_cursor(&rt.first().unwrap()._id)
        } else {
            None
        };
        Ok((rt, cursor))
    }

    pub async fn due_jobs(
        &self,
        now_ms: u64,
        limit: usize,
        exclude: &HashSet<u64>,
    ) -> Result<Vec<CronJob>, BoxError> {
        let mut jobs: Vec<CronJob> = self
            .jobs
            .search_as(Query {
                filter: Some(Filter::Field((
                    "next_run".to_string(),
                    RangeQuery::Le(Fv::U64(now_ms / 1000)),
                ))),
                limit: Some(limit),
                ..Default::default()
            })
            .await?;
        jobs.retain(|job| !exclude.contains(&job._id));
        jobs.sort_by_key(|job| (job.next_run, job._id));
        jobs.truncate(limit);
        Ok(jobs)
    }

    pub async fn json_start(&self, job_id: u64, started_at: u64) -> Result<CronRun, BoxError> {
        let mut run = CronRun {
            job_id,
            started_at,
            ..Default::default()
        };
        let id = self.runs.add_from(&run).await?;
        run._id = id;
        Ok(run)
    }

    #[allow(unused)]
    pub async fn job_abort(
        &self,
        run: CronRun,
        finished_at: u64,
        error: String,
    ) -> Result<(), BoxError> {
        let run_patch: BTreeMap<String, Fv> = BTreeMap::from([
            ("finished_at".to_string(), Fv::U64(finished_at)),
            ("error".to_string(), Fv::Text(error.clone())),
        ]);
        let job_patch: BTreeMap<String, Fv> = BTreeMap::from([
            ("last_finished_at".to_string(), Fv::U64(finished_at)),
            ("updated_at".to_string(), Fv::U64(finished_at)),
            ("last_error".to_string(), Fv::Text(error)),
            ("last_result".to_string(), Fv::Null),
        ]);

        self.runs.update(run._id, run_patch).await?;
        let Ok(job) = self.jobs.get_as::<CronJob>(run.job_id).await else {
            return Ok(());
        };

        self.jobs.update(job._id, job_patch).await?;
        Ok(())
    }

    pub async fn job_finish(
        &self,
        run: CronRun,
        finished_at: u64,
        result: CronJobResult,
    ) -> Result<(), BoxError> {
        let mut run_patch: BTreeMap<String, Fv> =
            BTreeMap::from([("finished_at".to_string(), Fv::U64(finished_at))]);
        let mut job_patch: BTreeMap<String, Fv> = BTreeMap::from([
            ("last_finished_at".to_string(), Fv::U64(finished_at)),
            ("updated_at".to_string(), Fv::U64(finished_at)),
        ]);

        if let Some(conversation_id) = result.conversation_id {
            run_patch.insert("conversation_id".to_string(), Fv::U64(conversation_id));
            job_patch.insert("last_conversation_id".to_string(), Fv::U64(conversation_id));
        }

        if let Some(error) = result.error {
            run_patch.insert("error".to_string(), Fv::Text(error.clone()));
            job_patch.insert("last_error".to_string(), Fv::Text(error));
            job_patch.insert("last_result".to_string(), Fv::Null);
        } else if let Some(result) = result.result {
            run_patch.insert("result".to_string(), Fv::Text(result.clone()));
            job_patch.insert("last_result".to_string(), Fv::Text(result));
            job_patch.insert("last_error".to_string(), Fv::Null);
        }

        self.runs.update(run._id, run_patch).await?;
        let Ok(job) = self.jobs.get_as::<CronJob>(run.job_id).await else {
            return Ok(());
        };

        // only update next_run if the job is not already disabled
        if job.next_run < DISABLED_JOB_NEXT_RUN {
            let schedule = job.schedule()?;
            let next_run = schedule.next_run(finished_at);
            job_patch.insert("next_run".to_string(), Fv::U64(next_run));
        }

        self.jobs.update(job._id, job_patch).await?;
        Ok(())
    }

    pub async fn flush(&self, now_ms: u64) -> Result<(), BoxError> {
        self.jobs.flush(now_ms).await?;
        self.runs.flush(now_ms).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anda_db::{
        database::{AndaDB, DBConfig},
        storage::StorageConfig,
    };
    use chrono::{Duration, Utc};
    use object_store::{ObjectStore, memory::InMemory};
    use tempfile::tempdir;

    async fn test_store() -> (tempfile::TempDir, CronStore) {
        let dir = tempdir().unwrap();
        let object_store: Arc<dyn ObjectStore> = { Arc::new(InMemory::new()) };
        let db = AndaDB::connect(
            object_store,
            DBConfig {
                name: "cron_test_db".to_string(),
                description: "cron test db".to_string(),
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
        let store = CronStore::connect(Arc::new(db)).await.unwrap();
        (dir, store)
    }

    async fn insert_test_job(store: &CronStore, name: &str) -> CronJob {
        store
            .insert_job(CreateCronJobArgs {
                job_kind: JobKind::Agent,
                job: format!("run {name}"),
                schedule_kind: ScheduleKind::Every,
                schedule: "60".to_string(),
                name: Some(name.to_string()),
                tz: None,
            })
            .await
            .unwrap()
    }

    async fn insert_at_job(store: &CronStore, name: &str, at_ms: u64) -> CronJob {
        let at = chrono::DateTime::from_timestamp_millis(at_ms as i64)
            .unwrap()
            .to_rfc3339();
        store
            .insert_job(CreateCronJobArgs {
                job_kind: JobKind::Agent,
                job: format!("run {name}"),
                schedule_kind: ScheduleKind::At,
                schedule: at,
                name: Some(name.to_string()),
                tz: None,
            })
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn list_jobs_cursor_pages_without_overlap() {
        let (_dir, store) = test_store().await;
        let inserted: Vec<CronJob> = vec![
            insert_test_job(&store, "job-1").await,
            insert_test_job(&store, "job-2").await,
            insert_test_job(&store, "job-3").await,
        ];

        let (page1, cursor1) = store.list_jobs(None, Some(2)).await.unwrap();
        let cursor1 = cursor1.expect("expected next cursor for first page");
        let (page2, cursor2) = store.list_jobs(Some(cursor1), Some(2)).await.unwrap();

        assert_eq!(page1.len(), 2);
        assert_eq!(page2.len(), 1);
        assert!(cursor2.is_none());

        let page1_ids: HashSet<u64> = page1.iter().map(|job| job._id).collect();
        let page2_ids: HashSet<u64> = page2.iter().map(|job| job._id).collect();
        let inserted_ids: HashSet<u64> = inserted.iter().map(|job| job._id).collect();

        assert!(page1_ids.is_disjoint(&page2_ids));
        assert_eq!(
            page1_ids
                .union(&page2_ids)
                .copied()
                .collect::<HashSet<u64>>(),
            inserted_ids
        );
    }

    #[tokio::test]
    async fn list_runs_cursor_pages_without_overlap() {
        let (_dir, store) = test_store().await;
        let job = insert_test_job(&store, "job-1").await;

        let _run1 = store.json_start(job._id, unix_ms()).await.unwrap();
        let _run2 = store.json_start(job._id, unix_ms()).await.unwrap();
        let _run3 = store.json_start(job._id, unix_ms()).await.unwrap();
        store.flush(unix_ms()).await.unwrap();

        let (page1, cursor1) = store.list_runs(None, Some(2), Some(job._id)).await.unwrap();
        let cursor1 = cursor1.expect("expected next cursor for first page");
        let (page2, cursor2) = store
            .list_runs(Some(cursor1), Some(2), Some(job._id))
            .await
            .unwrap();

        assert_eq!(page1.len(), 2);
        assert_eq!(page2.len(), 1);
        assert!(cursor2.is_none());

        let page1_ids: HashSet<u64> = page1.iter().map(|run| run._id).collect();
        let page2_ids: HashSet<u64> = page2.iter().map(|run| run._id).collect();

        assert!(page1_ids.is_disjoint(&page2_ids));
        assert_eq!(page1_ids.union(&page2_ids).count(), 3);
    }

    #[tokio::test]
    async fn due_jobs_prefers_earliest_next_run() {
        let (_dir, store) = test_store().await;
        let base = Utc::now();
        let job_late = insert_at_job(
            &store,
            "late",
            (base + Duration::seconds(30)).timestamp_millis() as u64,
        )
        .await;
        let job_earliest = insert_at_job(
            &store,
            "earliest",
            (base + Duration::seconds(10)).timestamp_millis() as u64,
        )
        .await;
        let job_middle = insert_at_job(
            &store,
            "middle",
            (base + Duration::seconds(20)).timestamp_millis() as u64,
        )
        .await;

        let due_jobs = store
            .due_jobs(
                (base + Duration::seconds(60)).timestamp_millis() as u64,
                2,
                &HashSet::new(),
            )
            .await
            .unwrap();

        let due_ids: Vec<u64> = due_jobs.iter().map(|job| job._id).collect();
        assert_eq!(due_ids, vec![job_earliest._id, job_middle._id]);
        assert!(!due_ids.contains(&job_late._id));
    }
}
