use anda_core::{AgentInput, BoxError, Principal, ToolInput};
use anda_db::{database::AndaDB, unix_ms};
use anda_engine::{
    engine::{Engine, EngineRef},
    extension::shell::{ExecArgs, ShellTool},
};
use parking_lot::Mutex;
use serde_json::json;
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;

const DEFAULT_POLL_SECS: u64 = 5;
const MAX_DUE_JOBS_PER_TICK: usize = 64;
const MAX_CONCURRENT_JOBS: usize = 8;
// Last-resort prune threshold for leaked in-memory entries. Agent jobs can
// legitimately run for a long time; pruning a live entry restarts the job
// concurrently, so this must stay well above any expected job duration.
const STALE_RUNNING_MS: u64 = 2 * 60 * 60 * 1000;

use super::{store::*, types::*};
use crate::engine::system_runtime_prompt;

#[derive(Debug, Clone, Copy)]
struct RunningJob {
    run_id: u64,
    started_at: u64,
}

type RunningJobs = Arc<Mutex<BTreeMap<u64, RunningJob>>>;

// Removes the owning run's entry on drop (also on panic or cancellation),
// without touching an entry that a newer run has claimed in the meantime.
struct RunningJobGuard {
    running_jobs: RunningJobs,
    job_id: u64,
    run_id: u64,
}

impl Drop for RunningJobGuard {
    fn drop(&mut self) {
        let mut running_jobs = self.running_jobs.lock();
        if running_jobs
            .get(&self.job_id)
            .is_some_and(|entry| entry.run_id == self.run_id)
        {
            running_jobs.remove(&self.job_id);
        }
    }
}

#[derive(Clone)]
pub struct CronRuntime {
    pub store: CronStore,
    engine: Arc<EngineRef>,
    controller: Principal,
    // job_id -> in-flight run
    running_jobs: RunningJobs,
}

impl CronRuntime {
    pub async fn connect(engine: Arc<EngineRef>, db: Arc<AndaDB>) -> Result<Self, BoxError> {
        let store = CronStore::connect(db).await?;
        Ok(Self {
            store,
            engine,
            controller: Principal::management_canister(),
            running_jobs: Arc::new(Mutex::new(BTreeMap::new())),
        })
    }

    async fn process_due_jobs_once(self, engine: Arc<Engine>) -> Result<usize, BoxError> {
        let now_ms = unix_ms();
        let running_ids = {
            let mut running_jobs = self.running_jobs.lock();
            let before_len = running_jobs.len();
            running_jobs
                .retain(|_, entry| now_ms.saturating_sub(entry.started_at) < STALE_RUNNING_MS);
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
                    self.running_jobs.lock().insert(
                        job._id,
                        RunningJob {
                            run_id: run._id,
                            started_at: started_at_ms,
                        },
                    );
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
        // Each job runs in its own task so a panicking job cannot take down
        // the other in-flight jobs or skip the final store flush.
        let mut in_flight = JoinSet::new();
        for (job, run) in runs {
            let this = self.clone();
            let engine = engine.clone();
            in_flight.spawn(async move {
                this.process_due_job(engine, job, run).await;
            });
        }
        tokio::spawn(async move {
            while let Some(result) = in_flight.join_next().await {
                if let Err(err) = result {
                    log::error!(name = "cron"; "cron job task failed: {err}");
                }
            }
            if let Err(err) = store.flush(unix_ms()).await {
                log::error!(name = "cron"; "failed to flush cron store: {err}");
            }
        });

        Ok(len)
    }

    async fn process_due_job(&self, engine: Arc<Engine>, job: CronJob, run: CronRun) {
        let _running_guard = RunningJobGuard {
            running_jobs: self.running_jobs.clone(),
            job_id: job._id,
            run_id: run._id,
        };
        let request_meta = job.request_meta();
        let caller = job
            .origin
            .as_ref()
            .and_then(CronJobOrigin::caller_principal)
            .unwrap_or(self.controller);
        let result: CronJobResult = match &job.job_kind {
            JobKind::Shell => match engine
                .tool_call(
                    caller,
                    ToolInput {
                        name: ShellTool::NAME.to_string(),
                        args: json!(ExecArgs {
                            command: job.job.clone(),
                            ..Default::default()
                        }),
                        meta: request_meta.clone(),
                        ..Default::default()
                    },
                )
                .await
            {
                Ok(result) => {
                    let mut result: CronJobResult = result.into();
                    if let Some(conversation_id) = self
                        .notify_shell_result(
                            engine.clone(),
                            caller,
                            &job,
                            run._id,
                            &result,
                            request_meta.clone(),
                        )
                        .await
                    {
                        result.conversation_id.get_or_insert(conversation_id);
                    }
                    result
                }
                Err(err) => {
                    log::error!(name = "cron"; "failed to submit cron job {} (run id: {}): {err}", job._id, run._id);
                    let result: CronJobResult = err.into();
                    self.notify_shell_result(
                        engine.clone(),
                        caller,
                        &job,
                        run._id,
                        &result,
                        request_meta.clone(),
                    )
                    .await;
                    result
                }
            },
            JobKind::Agent => {
                let prompt = system_runtime_prompt(
                    "cron agent job",
                    format!(
                        "Scheduled agent job is running. Execute the following instructions and produce a helpful result for the user.\n\nJob id: {}\nJob name: {}\nRun id: {}\nInstructions:\n{}",
                        job._id,
                        job.name.as_deref().unwrap_or("unnamed"),
                        run._id,
                        job.job
                    ),
                );
                let mut input = AgentInput::new(String::new(), prompt);
                input.meta = request_meta;
                match engine.agent_run(caller, input).await {
                    Ok(result) => result.into(),
                    Err(err) => {
                        log::error!(name = "cron"; "failed to submit cron job {} (run id: {}): {err}", job._id, run._id);
                        err.into()
                    }
                }
            }
        };

        let finished_at_ms = unix_ms();
        let run_id = run._id;
        if let Err(err) = self.store.job_finish(run, finished_at_ms, result).await {
            log::error!(name = "cron"; "failed to mark cron job {} (run id: {}) as finished: {err}", job._id, run_id);
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn notify_shell_result(
        &self,
        engine: Arc<Engine>,
        caller: Principal,
        job: &CronJob,
        run_id: u64,
        result: &CronJobResult,
        request_meta: Option<anda_core::RequestMeta>,
    ) -> Option<u64> {
        let request_meta = request_meta?;
        let mut input =
            AgentInput::new(String::new(), cron_shell_result_prompt(job, run_id, result));
        input.meta = Some(request_meta);
        match engine.agent_run(caller, input).await {
            Ok(output) => output.conversation,
            Err(err) => {
                log::error!(name = "cron"; "failed to notify agent about cron job {} (run id: {}) result: {err}", job._id, run_id);
                None
            }
        }
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

fn cron_shell_result_prompt(job: &CronJob, run_id: u64, result: &CronJobResult) -> String {
    let outcome = if let Some(error) = &result.error {
        format!("Shell command failed:\n\n{error}")
    } else if let Some(result) = &result.result {
        format!("Shell command completed:\n\n{result}")
    } else {
        "Shell command completed without a textual result.".to_string()
    };
    let name = job.name.as_deref().unwrap_or("unnamed");

    system_runtime_prompt(
        "cron shell job result",
        format!(
            "Scheduled shell job completed. Incorporate this result into the current conversation and tell the originating user the useful outcome succinctly.\n\nJob id: {}\nJob name: {}\nRun id: {}\nCommand:\n{}\n\n{}",
            job._id, name, run_id, job.job, outcome
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running_jobs_with(job_id: u64, run_id: u64, started_at: u64) -> RunningJobs {
        let running_jobs: RunningJobs = Arc::new(Mutex::new(BTreeMap::new()));
        running_jobs
            .lock()
            .insert(job_id, RunningJob { run_id, started_at });
        running_jobs
    }

    #[test]
    fn running_job_guard_removes_own_entry_on_drop() {
        let running_jobs = running_jobs_with(1, 10, 0);

        drop(RunningJobGuard {
            running_jobs: running_jobs.clone(),
            job_id: 1,
            run_id: 10,
        });

        assert!(running_jobs.lock().is_empty());
    }

    #[test]
    fn running_job_guard_keeps_entry_claimed_by_newer_run() {
        // Simulates a stale-pruned run finishing after the job was restarted:
        // the old run's guard must not remove the new run's entry.
        let running_jobs = running_jobs_with(1, 11, 5);

        drop(RunningJobGuard {
            running_jobs: running_jobs.clone(),
            job_id: 1,
            run_id: 10,
        });

        let entries = running_jobs.lock();
        assert_eq!(entries.get(&1).map(|entry| entry.run_id), Some(11));
    }

    #[test]
    fn running_job_guard_runs_on_panic_unwind() {
        let running_jobs = running_jobs_with(1, 10, 0);
        let guard_jobs = running_jobs.clone();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _guard = RunningJobGuard {
                running_jobs: guard_jobs,
                job_id: 1,
                run_id: 10,
            };
            panic!("job panicked");
        }));

        assert!(result.is_err());
        assert!(running_jobs.lock().is_empty());
    }
}
