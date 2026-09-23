//! In-process execution receipts. These cannot be forged through request metadata.

use anda_core::{AgentOutput, Principal};
use parking_lot::Mutex;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::types::CronJobResult;

#[derive(Clone)]
pub(crate) struct AgentSubmission {
    receipt: Arc<Mutex<Option<AgentReceipt>>>,
    claimed: Arc<AtomicBool>,
    cancel: CancellationToken,
}

impl AgentSubmission {
    pub fn new() -> (Self, oneshot::Receiver<CronJobResult>) {
        let (sender, receiver) = oneshot::channel();
        let cancel = CancellationToken::new();
        (
            Self {
                receipt: Arc::new(Mutex::new(Some(AgentReceipt {
                    sender: Some(sender),
                    cancel: cancel.clone(),
                    cancellation_task: None,
                }))),
                claimed: Arc::new(AtomicBool::new(false)),
                cancel,
            },
            receiver,
        )
    }

    pub fn take(&self) -> Option<AgentReceipt> {
        let receipt = self.receipt.lock().take();
        if receipt.is_some() {
            self.claimed.store(true, Ordering::SeqCst);
        }
        receipt
    }

    pub fn was_claimed(&self) -> bool {
        self.claimed.load(Ordering::SeqCst)
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancel.clone()
    }
}

pub(crate) struct AgentReceipt {
    sender: Option<oneshot::Sender<CronJobResult>>,
    cancel: CancellationToken,
    cancellation_task: Option<tokio::task::JoinHandle<()>>,
}

impl AgentReceipt {
    pub fn on_cancel(&mut self, stop: impl Future<Output = ()> + Send + 'static) {
        if let Some(task) = self.cancellation_task.take() {
            task.abort();
        }
        let cancel = self.cancel.clone();
        self.cancellation_task = Some(tokio::spawn(async move {
            cancel.cancelled().await;
            stop.await;
        }));
    }

    pub fn finish(mut self, result: CronJobResult) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(result);
        }
    }
}

impl Drop for AgentReceipt {
    fn drop(&mut self) {
        if let Some(task) = self.cancellation_task.take() {
            task.abort();
        }
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(CronJobResult {
                error: Some("Scheduled agent execution stopped before completion".into()),
                ..Default::default()
            });
        }
    }
}

/// Receipts move with inputs into the runner; dropping queued or active work
/// resolves its waiter even on cancellation, an early return, or unwinding.
#[derive(Default)]
pub(crate) struct AgentReceipts {
    pending: Vec<AgentReceipt>,
    last_result: CronJobResult,
}

impl AgentReceipts {
    pub fn push(&mut self, receipt: Option<AgentReceipt>) {
        self.pending.extend(receipt);
    }

    pub fn record(&mut self, output: &AgentOutput) {
        if !self.pending.is_empty() {
            self.last_result = CronJobResult {
                conversation_id: output.conversation,
                result: Some(output.content.clone()),
                error: output.failed_reason.clone(),
            };
        }
    }

    pub fn finish(&mut self) {
        for receipt in self.pending.drain(..) {
            receipt.finish(self.last_result.clone());
        }
        self.last_result = CronJobResult::default();
    }

    pub fn fail(&mut self, reason: &str) {
        self.last_result.error = Some(reason.into());
        self.finish();
    }
}

/// Only the scheduler installs this state, from a directory validated and
/// persisted when the owner created or moved the job. It is never decoded from
/// API request metadata.
#[derive(Clone, Debug)]
pub(crate) struct CronWorkspaceGrant {
    pub caller: Principal,
    pub path: PathBuf,
}
