use anda_core::{AgentInput, BoxError, Principal, ToolInput};
use anda_db::{database::AndaDB, unix_ms};
use anda_engine::{
    engine::{Engine, EngineRef},
    extension::shell::{ExecArgs, ShellTool},
};
use futures_util::{StreamExt, stream};
use parking_lot::Mutex;
use serde_json::json;
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

const DEFAULT_POLL_SECS: u64 = 5;
const MAX_DUE_JOBS_PER_TICK: usize = 64;
const MAX_CONCURRENT_JOBS: usize = 8;
const STALE_RUNNING_MS: u64 = 10 * 60 * 1000;

use super::{store::*, types::*};

#[derive(Clone)]
pub struct CronRuntime {
    pub store: CronStore,
    engine: Arc<EngineRef>,
    controller: Principal,
    // job_id -> started_at
    running_jobs: Arc<Mutex<BTreeMap<u64, u64>>>,
}

impl CronRuntime {
    pub async fn connect(
        engine: Arc<EngineRef>,
        db: Arc<AndaDB>,
        controller: Principal,
    ) -> Result<Self, BoxError> {
        let store = CronStore::connect(db).await?;
        Ok(Self {
            store,
            engine,
            controller,
            running_jobs: Arc::new(Mutex::new(BTreeMap::new())),
        })
    }

    async fn process_due_jobs_once(self, engine: Arc<Engine>) -> Result<usize, BoxError> {
        let now_ms = unix_ms();
        let running_ids = {
            let mut running_jobs = self.running_jobs.lock();
            let before_len = running_jobs.len();
            running_jobs
                .retain(|_, started_at| now_ms.saturating_sub(*started_at) < STALE_RUNNING_MS);
            let pruned = before_len.saturating_sub(running_jobs.len());
            if pruned > 0 {
                log::warn!(name = "cron"; "pruned {pruned} stale in-memory cron jobs");
            }
            running_jobs.keys().copied().collect::<HashSet<u64>>()
        };

        let available_slots = MAX_CONCURRENT_JOBS.saturating_sub(running_ids.len());
        if available_slots == 0 {
            return Ok(0);
        }

        let due_jobs = self
            .store
            .due_jobs(
                now_ms,
                MAX_DUE_JOBS_PER_TICK.min(available_slots),
                &running_ids,
            )
            .await?;

        let mut runs: Vec<(CronJob, CronRun)> = Vec::new();
        for job in due_jobs {
            let started_at_ms = unix_ms();
            let run = match self.store.job_start(job._id, started_at_ms).await {
                Ok(run) => {
                    self.running_jobs.lock().insert(job._id, started_at_ms);
                    run
                }
                Err(err) => {
                    log::warn!(name = "cron"; "failed to mark cron job {} as running: {err}", job._id);
                    continue;
                }
            };
            runs.push((job, run));
        }

        let len = runs.len();
        if len == 0 {
            return Ok(0);
        }

        let store = self.store.clone();
        let mut in_flight = stream::iter(runs.into_iter().map(move |(job, run)| {
            let this = self.clone();
            let engine = engine.clone();
            async move {
                this.process_due_job(engine, job, run).await;
            }
        }))
        .buffer_unordered(available_slots);
        tokio::spawn(async move {
            while let Some(()) = in_flight.next().await {
                // nothing to do here, just drive the stream
            }
            if let Err(err) = store.flush(unix_ms()).await {
                log::error!(name = "cron"; "failed to flush cron store: {err}");
            }
        });

        Ok(len)
    }

    async fn process_due_job(&self, engine: Arc<Engine>, job: CronJob, run: CronRun) {
        let result: CronJobResult = match job.job_kind {
            JobKind::Shell => match engine
                .tool_call(
                    self.controller,
                    ToolInput {
                        name: ShellTool::NAME.to_string(),
                        args: json!(ExecArgs {
                            command: job.job,
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                )
                .await
            {
                Ok(result) => result.into(),
                Err(err) => {
                    log::error!(name = "cron"; "failed to submit cron job {}: {err}", job._id);
                    err.into()
                }
            },
            JobKind::Agent => {
                match engine
                    .agent_run(self.controller, AgentInput::new(String::new(), job.job))
                    .await
                {
                    Ok(result) => result.into(),
                    Err(err) => {
                        log::error!(name = "cron"; "failed to submit cron job {}: {err}", job._id);
                        err.into()
                    }
                }
            }
        };

        let finished_at_ms = unix_ms();
        if let Err(err) = self.store.job_finish(run, finished_at_ms, result).await {
            log::error!(name = "cron"; "failed to mark cron job {} as finished: {err}", job._id);
        }

        self.running_jobs.lock().remove(&job._id);
    }

    pub async fn serve(
        self,
        cancel_token: CancellationToken,
    ) -> Result<JoinHandle<Result<(), BoxError>>, BoxError> {
        Ok(tokio::spawn(async move {
            log::warn!(name = "cron"; "cron scheduler started");
            let mut interval = tokio::time::interval(Duration::from_secs(DEFAULT_POLL_SECS));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        log::warn!(name = "cron"; "cron scheduler stopped");
                        return Ok(());
                    }
                    _ = interval.tick() => {
                        let Some(engine) = self.engine.get() else {
                             log::warn!(name = "cron"; "engine is not available, skipping cron tick");
                             continue;
                        };
                        if let Err(err) = self.clone().process_due_jobs_once(engine).await {
                            log::error!(name = "cron"; "cron tick failed: {err}");
                        }
                    }
                }
            }
        }))
    }
}
