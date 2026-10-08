use anda_core::BoxError;
use anda_db::{
    collection::{Collection, CollectionConfig},
    database::AndaDB,
    error::DBError,
    index::BTree,
    query::{Filter, RangeQuery},
    schema::Fv,
    unix_ms,
};
use serde::de::DeserializeOwned;
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

use super::types::*;

/// Run history kept per job; older runs are pruned as new ones finish. Runs of
/// a removed job stay for audit.
const MAX_RUNS_PER_JOB: usize = 200;
/// Older runs removed per finished run, so the first pass over a long history
/// does not hold the scheduler.
const RUN_PRUNE_BATCH: usize = 100;

#[derive(Clone)]
pub struct CronStore {
    jobs: Arc<Collection>,
    runs: Arc<Collection>,
    mutations: Arc<tokio::sync::Mutex<()>>,
}

impl CronStore {
    pub async fn connect(db: Arc<AndaDB>) -> Result<Self, BoxError> {
        let mut jobs_schema = CronJob::schema()?;
        jobs_schema.with_version(2);
        let jobs = db
            .open_or_create_collection(
                jobs_schema,
                CollectionConfig {
                    name: "cron_jobs".to_string(),
                    description: "Scheduled prompt jobs".to_string(),
                },
                async |collection| {
                    collection.create_btree_index_nx(&["next_run"]).await?;
                    collection.remove_btree_index(&["created_at"]).await?;
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
                    collection.remove_btree_index(&["started_at"]).await?;
                    Ok::<(), DBError>(())
                },
            )
            .await?;

        Ok(Self {
            jobs,
            runs,
            mutations: Arc::new(tokio::sync::Mutex::new(())),
        })
    }

    pub async fn insert_job(
        &self,
        args: CreateCronJobArgs,
        origin: Option<CronJobOrigin>,
    ) -> Result<CronJob, BoxError> {
        let now_ms = unix_ms();
        let mut job = args.into_cron_job_with_origin(now_ms, origin)?;

        let id = self.jobs.add_from(&job).await?;
        job._id = id;
        self.jobs.flush(now_ms).await?;
        Ok(job)
    }

    #[cfg(test)]
    pub async fn update_job(&self, args: UpdateCronJobArgs) -> Result<CronJob, BoxError> {
        self.update_job_with_origin(args, None).await
    }

    pub async fn update_job_with_origin(
        &self,
        args: UpdateCronJobArgs,
        origin: Option<CronJobOrigin>,
    ) -> Result<CronJob, BoxError> {
        let _guard = self.mutations.lock().await;
        let now_ms = unix_ms();
        let job: CronJob = self.jobs.get_as(args.id).await?;
        let before = cron_job_update_patch(&job)?;
        let updated = args.into_update_with_origin(origin).apply_to(job, now_ms)?;
        let mut patch = cron_job_update_patch(&updated)?;
        patch.retain(|key, value| before.get(key) != Some(value));
        // AndaDB rejects an empty patch; a no-op edit within the same
        // millisecond leaves nothing to write.
        if patch.is_empty() {
            return Ok(updated);
        }
        let job = self.jobs.update(updated._id, patch).await?;
        self.jobs.flush(now_ms).await?;
        Ok(job.try_into()?)
    }

    pub async fn list_jobs(
        &self,
        cursor: Option<String>,
        limit: Option<usize>,
    ) -> Result<(Vec<CronJob>, Option<String>), BoxError> {
        newest_page(&self.jobs, None, cursor, limit).await
    }

    pub async fn get_job(&self, id: u64) -> Result<CronJob, BoxError> {
        let job = self.jobs.get_as(id).await?;
        Ok(job)
    }

    pub async fn pause_job(&self, id: u64) -> Result<CronJob, BoxError> {
        let _guard = self.mutations.lock().await;
        let now_ms = unix_ms();
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
        let _guard = self.mutations.lock().await;
        let job: CronJob = self.jobs.get_as(id).await?;
        let now_ms = unix_ms();
        let next_run = job.schedule()?.next_run(now_ms);
        if next_run >= DISABLED_JOB_NEXT_RUN {
            return Err(
                "the job has no future run time (a one-time job that already ran); update its schedule instead"
                    .into(),
            );
        }

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
        let _guard = self.mutations.lock().await;
        let now_ms = unix_ms();
        if self.jobs.remove(id).await?.is_none() {
            return Ok(());
        }

        // Run history is intentionally retained for audit after the job
        // definition is removed; only the `cron_jobs` entry goes away.
        self.jobs.flush(now_ms).await?;
        Ok(())
    }

    pub async fn list_runs(
        &self,
        cursor: Option<String>,
        limit: Option<usize>,
        job_id: Option<u64>,
    ) -> Result<(Vec<CronRun>, Option<String>), BoxError> {
        newest_page(&self.runs, job_id.map(runs_of_job), cursor, limit).await
    }

    /// Ids of due jobs, earliest `next_run` first; [`Self::claim_job`]
    /// rereads and rechecks each one.
    pub fn due_job_ids(
        &self,
        now_ms: u64,
        limit: usize,
        exclude: &HashSet<u64>,
    ) -> Result<Vec<u64>, BoxError> {
        let mut ids = Vec::with_capacity(limit);
        if limit == 0 {
            return Ok(ids);
        }

        // Bounded collection queries select by document id, so walk the
        // next_run index directly: the limit keeps the earliest due jobs and
        // in-flight ids are skipped without using up the page.
        self.jobs
            .get_btree_index(&["next_run"])?
            .try_range_query_ids(RangeQuery::Le(Fv::U64(now_ms / 1000)), false, |matches| {
                for &id in matches {
                    if !exclude.contains(&id) {
                        ids.push(id);
                        if ids.len() == limit {
                            return false;
                        }
                    }
                }
                true
            })?;
        Ok(ids)
    }

    /// Recheck the definition under the same lock used by management and completion.
    pub async fn claim_job(
        &self,
        id: u64,
        now_ms: u64,
    ) -> Result<Option<(CronJob, CronRun)>, BoxError> {
        let _guard = self.mutations.lock().await;
        // A job removed after the index walk is simply not claimed.
        let Some(job) = get_existing::<CronJob>(&self.jobs, id).await? else {
            return Ok(None);
        };
        if job.is_paused() || job.next_run > now_ms / 1000 {
            return Ok(None);
        }
        let run = self.job_start(id, now_ms).await?;
        Ok(Some((job, run)))
    }

    pub async fn job_start(&self, job_id: u64, started_at: u64) -> Result<CronRun, BoxError> {
        let mut run = CronRun {
            job_id,
            started_at,
            ..Default::default()
        };
        let id = self.runs.add_from(&run).await?;
        run._id = id;
        Ok(run)
    }

    pub async fn job_finish(
        &self,
        started_job: &CronJob,
        run: CronRun,
        finished_at: u64,
        result: CronJobResult,
    ) -> Result<(), BoxError> {
        let _guard = self.mutations.lock().await;
        let mut run_patch: BTreeMap<String, Fv> =
            BTreeMap::from([("finished_at".to_string(), Fv::U64(finished_at))]);
        let mut job_patch: BTreeMap<String, Fv> = BTreeMap::from([
            ("last_finished_at".to_string(), Fv::U64(finished_at)),
            ("updated_at".to_string(), Fv::U64(finished_at)),
        ]);

        if let Some(conversation_id) = result.conversation_id {
            run_patch.insert("conversation_id".to_string(), Fv::U64(conversation_id));
        }

        for (run_field, job_field, value) in [
            ("error", "last_error", &result.error),
            ("result", "last_result", &result.result),
        ] {
            job_patch.insert(job_field.into(), optional_text(value));
            if let Some(value) = value {
                run_patch.insert(run_field.into(), Fv::Text(value.clone()));
            }
        }

        self.runs.update(run._id, run_patch).await?;
        let Some(job) = get_existing::<CronJob>(&self.jobs, run.job_id).await? else {
            return Ok(());
        };
        if job.origin == started_job.origin
            && let Some(conversation_id) = result.conversation_id
        {
            job_patch.insert("last_conversation_id".to_string(), Fv::U64(conversation_id));
        }

        // only update next_run if the job is not already paused
        if !job.is_paused()
            && job.next_run == started_job.next_run
            && job.schedule_kind == started_job.schedule_kind
            && job.schedule == started_job.schedule
            && job.tz == started_job.tz
        {
            // A schedule that no longer parses must not leave the job due, or
            // it would be claimed again as soon as this run is released.
            let next_run = match job.schedule() {
                Ok(schedule) => schedule.next_run(finished_at),
                Err(err) => {
                    log::warn!(name = "cron"; "disabling cron job {} with an invalid schedule: {err}", job._id);
                    DISABLED_JOB_NEXT_RUN
                }
            };
            job_patch.insert("next_run".to_string(), Fv::U64(next_run));
        }

        self.jobs.update(job._id, job_patch).await?;
        if let Err(err) = self.prune_runs(job._id, run._id).await {
            log::warn!(name = "cron"; "failed to prune run history of cron job {}: {err}", job._id);
        }
        Ok(())
    }

    async fn prune_runs(&self, job_id: u64, latest_run: u64) -> Result<(), BoxError> {
        let runs_before = |range: RangeQuery<Fv>| {
            Filter::And(vec![
                Box::new(runs_of_job(job_id)),
                Box::new(Filter::Field(("_id".to_string(), range))),
            ])
        };
        let kept = self
            .runs
            .query_last_ids(
                runs_before(RangeQuery::Le(Fv::U64(latest_run))),
                Some(MAX_RUNS_PER_JOB),
            )
            .await?;
        let Some(&oldest_kept) = kept.first().filter(|_| kept.len() >= MAX_RUNS_PER_JOB) else {
            return Ok(());
        };
        let stale = self
            .runs
            .query_last_ids(
                runs_before(RangeQuery::Lt(Fv::U64(oldest_kept))),
                Some(RUN_PRUNE_BATCH),
            )
            .await?;
        for id in stale {
            self.runs.remove(id).await?;
        }
        Ok(())
    }

    pub async fn flush(&self, now_ms: u64) -> Result<(), BoxError> {
        self.jobs.flush(now_ms).await?;
        self.runs.flush(now_ms).await?;
        Ok(())
    }
}

/// Newest-first cursor page of `scope` matches, rows in ascending id order.
async fn newest_page<T: DeserializeOwned>(
    collection: &Collection,
    scope: Option<Filter>,
    cursor: Option<String>,
    limit: Option<usize>,
) -> Result<(Vec<T>, Option<String>), BoxError> {
    let limit = limit.unwrap_or(10).clamp(1, 100);
    let cursor = match BTree::from_cursor::<u64>(&cursor)? {
        Some(cursor) => cursor,
        None => collection.max_document_id() + 1,
    };
    let below = Filter::Field(("_id".to_string(), RangeQuery::Lt(Fv::U64(cursor))));
    let filter = match scope {
        Some(scope) => Filter::And(vec![Box::new(scope), Box::new(below)]),
        None => below,
    };
    let ids = collection.query_last_ids(filter, Some(limit)).await?;
    // Derive the next cursor from the queried ids, not from the materialized
    // rows: a concurrent removal between the id query and the reads below
    // would otherwise shorten the page and stop pagination early.
    let next_cursor = if ids.len() >= limit {
        ids.first().and_then(BTree::to_cursor)
    } else {
        None
    };
    let mut rows = Vec::with_capacity(ids.len());
    for id in ids {
        rows.extend(get_existing(collection, id).await?);
    }
    Ok((rows, next_cursor))
}

/// Reads a document that may have been removed concurrently.
async fn get_existing<T: DeserializeOwned>(
    collection: &Collection,
    id: u64,
) -> Result<Option<T>, BoxError> {
    match collection.get_as(id).await {
        Ok(doc) => Ok(Some(doc)),
        Err(DBError::NotFound { .. }) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn runs_of_job(job_id: u64) -> Filter {
    Filter::Field(("job_id".to_string(), RangeQuery::Eq(Fv::U64(job_id))))
}

fn cron_job_update_patch(job: &CronJob) -> Result<BTreeMap<String, Fv>, BoxError> {
    Ok(BTreeMap::from([
        ("origin".to_string(), Fv::serialized(&job.origin, None)?),
        ("job_kind".to_string(), Fv::Text(job.job_kind.to_string())),
        ("job".to_string(), Fv::Text(job.job.clone())),
        (
            "schedule_kind".to_string(),
            Fv::Text(job.schedule_kind.to_string()),
        ),
        ("schedule".to_string(), Fv::Text(job.schedule.clone())),
        ("tz".to_string(), optional_text(&job.tz)),
        ("name".to_string(), optional_text(&job.name)),
        ("updated_at".to_string(), Fv::U64(job.updated_at)),
        ("next_run".to_string(), Fv::U64(job.next_run)),
        (
            "last_conversation_id".to_string(),
            job.last_conversation_id.map(Fv::U64).unwrap_or(Fv::Null),
        ),
    ]))
}

fn optional_text(value: &Option<String>) -> Fv {
    value
        .as_ref()
        .map(|value| Fv::Text(value.clone()))
        .unwrap_or(Fv::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    async fn test_store() -> CronStore {
        let db = crate::test_support::memory_db("cron").await;
        CronStore::connect(db).await.unwrap()
    }

    async fn insert_test_job(store: &CronStore, name: &str) -> CronJob {
        store
            .insert_job(
                CreateCronJobArgs {
                    job_kind: JobKind::Agent,
                    job: format!("run {name}"),
                    schedule_kind: ScheduleKind::Every,
                    schedule: "60".to_string(),
                    name: Some(name.to_string()),
                    tz: None,
                },
                None,
            )
            .await
            .unwrap()
    }

    async fn insert_at_job(store: &CronStore, name: &str, at_ms: u64) -> CronJob {
        let at = chrono::DateTime::from_timestamp_millis(at_ms as i64)
            .unwrap()
            .to_rfc3339();
        store
            .insert_job(
                CreateCronJobArgs {
                    job_kind: JobKind::Agent,
                    job: format!("run {name}"),
                    schedule_kind: ScheduleKind::At,
                    schedule: at,
                    name: Some(name.to_string()),
                    tz: None,
                },
                None,
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn finished_runs_prune_history_beyond_the_per_job_limit() {
        let store = test_store().await;
        let job = insert_test_job(&store, "busy").await;
        let other = insert_test_job(&store, "quiet").await;
        let other_run = store.job_start(other._id, 1).await.unwrap();
        let mut last = None;
        for at in 0..(MAX_RUNS_PER_JOB + 3) as u64 {
            last = Some(store.job_start(job._id, at).await.unwrap());
        }
        store
            .job_finish(&job, last.unwrap(), 1, CronJobResult::default())
            .await
            .unwrap();

        let ids = store
            .runs
            .query_last_ids(
                Filter::Field(("job_id".to_string(), RangeQuery::Eq(Fv::U64(job._id)))),
                Some(MAX_RUNS_PER_JOB + 10),
            )
            .await
            .unwrap();
        assert_eq!(ids.len(), MAX_RUNS_PER_JOB);
        // Other jobs keep their history.
        assert!(store.runs.get_as::<CronRun>(other_run._id).await.is_ok());
    }

    #[tokio::test]
    async fn list_jobs_cursor_pages_without_overlap() {
        let store = test_store().await;
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
    async fn list_jobs_and_runs_survive_zero_limit() {
        let store = test_store().await;
        let job = insert_test_job(&store, "job-1").await;
        let _run = store.job_start(job._id, unix_ms()).await.unwrap();
        store.flush(unix_ms()).await.unwrap();

        // limit=0 (e.g. an LLM passing {"limit":0} for "no limit") must not
        // panic; it clamps to at least 1.
        let (jobs, _cursor) = store.list_jobs(None, Some(0)).await.unwrap();
        assert_eq!(jobs.len(), 1);
        let (runs, _cursor) = store.list_runs(None, Some(0), Some(job._id)).await.unwrap();
        assert_eq!(runs.len(), 1);
    }

    #[tokio::test]
    async fn list_runs_cursor_pages_without_overlap() {
        let store = test_store().await;
        let job = insert_test_job(&store, "job-1").await;

        let _run1 = store.job_start(job._id, unix_ms()).await.unwrap();
        let _run2 = store.job_start(job._id, unix_ms()).await.unwrap();
        let _run3 = store.job_start(job._id, unix_ms()).await.unwrap();
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
        let store = test_store().await;
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

        let due_ids = store
            .due_job_ids(
                (base + Duration::seconds(60)).timestamp_millis() as u64,
                2,
                &HashSet::new(),
            )
            .unwrap();

        assert_eq!(due_ids, vec![job_earliest._id, job_middle._id]);
        assert!(!due_ids.contains(&job_late._id));
    }

    #[tokio::test]
    async fn due_jobs_does_not_starve_when_excluding_in_flight_jobs() {
        let store = test_store().await;
        let base = Utc::now();
        // Insert in next_run order so the highest document ids are also the
        // ones the range query keeps first; excluding them previously emptied
        // the result even though earlier jobs were still due.
        let mut jobs = Vec::new();
        for secs in [10, 20, 30, 40, 50] {
            jobs.push(
                insert_at_job(
                    &store,
                    &format!("job-{secs}"),
                    (base + Duration::seconds(secs)).timestamp_millis() as u64,
                )
                .await,
            );
        }
        let exclude: HashSet<u64> = [jobs[3]._id, jobs[4]._id].into_iter().collect();

        let due_ids = store
            .due_job_ids(
                (base + Duration::seconds(60)).timestamp_millis() as u64,
                2,
                &exclude,
            )
            .unwrap();

        // In-flight jobs must not use up the free slots, and the earliest
        // remaining jobs come first.
        assert_eq!(due_ids, vec![jobs[0]._id, jobs[1]._id]);
    }

    #[tokio::test]
    async fn update_job_changes_fields_preserves_origin_and_history() {
        let store = test_store().await;
        let origin = CronJobOrigin {
            user: Some("alice".to_string()),
            source: Some("wechat:daily".to_string()),
            ..Default::default()
        };
        let job = store
            .insert_job(
                CreateCronJobArgs {
                    job_kind: JobKind::Agent,
                    job: "old prompt".to_string(),
                    schedule_kind: ScheduleKind::Every,
                    schedule: "60".to_string(),
                    name: Some("old-name".to_string()),
                    tz: None,
                },
                Some(origin.clone()),
            )
            .await
            .unwrap();
        let run = store.job_start(job._id, unix_ms()).await.unwrap();
        store
            .job_finish(
                &job,
                run,
                unix_ms(),
                CronJobResult {
                    conversation_id: Some(7),
                    result: Some("done".to_string()),
                    error: None,
                },
            )
            .await
            .unwrap();
        let before_update = store.get_job(job._id).await.unwrap();

        let updated = store
            .update_job(UpdateCronJobArgs {
                id: job._id,
                job_kind: Some(JobKind::Shell),
                job: Some("echo updated".to_string()),
                schedule_kind: Some(ScheduleKind::Every),
                schedule: Some("2m".to_string()),
                name: Some("".to_string()),
                tz: None,
                origin: None,
            })
            .await
            .unwrap();

        assert_eq!(updated._id, job._id);
        assert_eq!(updated.origin, Some(origin));
        assert_eq!(updated.created_at, job.created_at);
        assert_eq!(updated.job_kind, JobKind::Shell);
        assert_eq!(updated.job, "echo updated");
        assert_eq!(updated.schedule_kind, ScheduleKind::Every);
        assert_eq!(updated.schedule, "2m");
        assert_eq!(updated.name, None);
        assert_eq!(updated.last_conversation_id, Some(7));
        assert_eq!(updated.last_result, Some("done".to_string()));
        assert!(updated.updated_at >= before_update.updated_at);
        assert!(updated.next_run > before_update.next_run);
    }

    #[tokio::test]
    async fn update_job_can_replace_origin() {
        let store = test_store().await;
        let job = store
            .insert_job(
                CreateCronJobArgs {
                    job_kind: JobKind::Agent,
                    job: "old prompt".to_string(),
                    schedule_kind: ScheduleKind::Every,
                    schedule: "60".to_string(),
                    name: Some("daily".to_string()),
                    tz: None,
                },
                Some(CronJobOrigin {
                    user: Some("alice".to_string()),
                    source: Some("wechat:old".to_string()),
                    reply_target: Some("alice".to_string()),
                    conversation_id: Some(7),
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
        let new_origin = CronJobOrigin {
            user: Some("bob".to_string()),
            source: Some("wechat:new".to_string()),
            reply_target: Some("bob".to_string()),
            thread: Some("session-2".to_string()),
            conversation_id: Some(42),
            external_user: Some(true),
            ..Default::default()
        };

        let updated = store
            .update_job_with_origin(
                UpdateCronJobArgs {
                    id: job._id,
                    job_kind: None,
                    job: None,
                    schedule_kind: None,
                    schedule: None,
                    name: None,
                    tz: None,
                    origin: Some(true),
                },
                Some(new_origin.clone()),
            )
            .await
            .unwrap();

        assert_eq!(updated.origin, Some(new_origin));
        assert_eq!(updated.job, job.job);
        assert_eq!(updated.next_run, job.next_run);
    }

    #[tokio::test]
    async fn update_job_without_schedule_change_keeps_next_run() {
        let store = test_store().await;
        let job = insert_test_job(&store, "job-1").await;

        let updated = store
            .update_job(UpdateCronJobArgs {
                id: job._id,
                job_kind: None,
                job: Some("updated prompt".to_string()),
                schedule_kind: None,
                schedule: None,
                name: None,
                tz: None,
                origin: None,
            })
            .await
            .unwrap();

        assert_eq!(updated.job, "updated prompt");
        assert_eq!(updated.next_run, job.next_run);
    }
    #[tokio::test]
    async fn one_shot_is_never_due_early_or_again_after_completion() {
        let store = test_store().await;
        let at = (unix_ms() / 1000 + 60) * 1000 + 900;
        let job = insert_at_job(&store, "once", at).await;
        assert!(
            store
                .due_job_ids(at - 700, 8, &HashSet::new())
                .unwrap()
                .is_empty()
        );
        let (snapshot, run) = store.claim_job(job._id, at + 100).await.unwrap().unwrap();
        store
            .job_finish(&snapshot, run, at + 101, CronJobResult::default())
            .await
            .unwrap();
        assert!(
            store
                .due_job_ids(at + 5100, 8, &HashSet::new())
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn completed_one_shot_cannot_resume_and_a_bad_schedule_stops_the_job() {
        let store = test_store().await;
        let at = (unix_ms() / 1000 + 1) * 1000;
        let job = insert_at_job(&store, "once", at).await;
        tokio::time::sleep(std::time::Duration::from_millis(
            at.saturating_sub(unix_ms()) + 5,
        ))
        .await;
        let (snapshot, run) = store.claim_job(job._id, unix_ms()).await.unwrap().unwrap();
        store
            .job_finish(&snapshot, run, unix_ms(), CronJobResult::default())
            .await
            .unwrap();
        assert!(store.get_job(job._id).await.unwrap().is_completed());
        let err = store.resume_job(job._id).await.unwrap_err();
        assert!(err.to_string().contains("no future run"));

        // A persisted schedule that no longer parses disables the job instead
        // of leaving it due for an immediate rerun.
        let job = insert_test_job(&store, "broken").await;
        store
            .jobs
            .update(
                job._id,
                BTreeMap::from([("schedule".to_string(), Fv::Text("soon".into()))]),
            )
            .await
            .unwrap();
        let (snapshot, run) = store
            .claim_job(job._id, unix_ms() + 3_600_000)
            .await
            .unwrap()
            .unwrap();
        store
            .job_finish(&snapshot, run, unix_ms(), CronJobResult::default())
            .await
            .unwrap();
        assert!(store.get_job(job._id).await.unwrap().is_paused());
    }

    #[tokio::test]
    async fn origin_replacement_clears_old_conversation_and_ignores_old_completion() {
        let store = test_store().await;
        let job = insert_test_job(&store, "origin").await;
        let first = store.job_start(job._id, unix_ms()).await.unwrap();
        store
            .job_finish(
                &job,
                first,
                unix_ms(),
                CronJobResult {
                    conversation_id: Some(7),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let old = store.get_job(job._id).await.unwrap();
        let running = store.job_start(job._id, unix_ms()).await.unwrap();
        let args = serde_json::from_value(serde_json::json!({"id":job._id,"origin":true})).unwrap();
        let updated = store
            .update_job_with_origin(
                args,
                Some(CronJobOrigin {
                    conversation_id: Some(42),
                    reply_target: Some("new".into()),
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
        assert_eq!(updated.last_conversation_id, None);
        assert_eq!(
            updated.request_meta(0).get_extra_as::<u64>("conversation"),
            Some(42)
        );
        store
            .job_finish(
                &old,
                running,
                unix_ms(),
                CronJobResult {
                    conversation_id: Some(7),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            store
                .get_job(job._id)
                .await
                .unwrap()
                .request_meta(0)
                .get_extra_as::<u64>("conversation"),
            Some(42)
        );
    }

    #[tokio::test]
    async fn management_and_completion_preserve_explicit_schedule_changes() {
        let store = test_store().await;
        let job = insert_test_job(&store, "managed").await;
        let run = store.job_start(job._id, unix_ms()).await.unwrap();
        let rename =
            serde_json::from_value(serde_json::json!({"id":job._id,"name":"renamed"})).unwrap();
        let (paused, renamed) = tokio::join!(store.pause_job(job._id), store.update_job(rename));
        paused.unwrap();
        renamed.unwrap();
        let args =
            serde_json::from_value(serde_json::json!({"id":job._id,"schedule":"120"})).unwrap();
        assert!(store.update_job(args).await.unwrap().is_paused());
        assert!(
            store
                .claim_job(job._id, unix_ms() + 3600000)
                .await
                .unwrap()
                .is_none()
        );
        store
            .job_finish(&job, run, unix_ms(), CronJobResult::default())
            .await
            .unwrap();
        assert!(store.get_job(job._id).await.unwrap().is_paused());
        let resumed = store.resume_job(job._id).await.unwrap();
        assert!(!resumed.is_paused());
        let run = store.job_start(job._id, unix_ms()).await.unwrap();
        let args =
            serde_json::from_value(serde_json::json!({"id":job._id,"schedule":"600"})).unwrap();
        let changed = store.update_job(args).await.unwrap();
        store
            .job_finish(&resumed, run, unix_ms() + 10000, CronJobResult::default())
            .await
            .unwrap();
        assert_eq!(
            store.get_job(job._id).await.unwrap().next_run,
            changed.next_run
        );
    }

    #[tokio::test]
    async fn remove_surfaces_storage_error_and_retains_history() {
        let store = test_store().await;
        let job = insert_test_job(&store, "remove").await;
        let run = store.job_start(job._id, unix_ms()).await.unwrap();
        store.jobs.set_read_only(true);
        assert!(store.remove_job(job._id).await.is_err());
        assert!(store.get_job(job._id).await.is_ok());
        store.jobs.set_read_only(false);
        store.remove_job(job._id).await.unwrap();
        store.remove_job(job._id).await.unwrap();
        store
            .job_finish(
                &job,
                run,
                unix_ms(),
                CronJobResult {
                    result: Some("done".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let (runs, _) = store.list_runs(None, None, Some(job._id)).await.unwrap();
        assert_eq!(runs[0].result.as_deref(), Some("done"));
    }
}
