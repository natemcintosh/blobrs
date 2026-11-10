//! Background work, and the single in-flight operation the UI tracks.
use crate::app::TaskEvent;
use std::fmt::Write as _;
use std::future::Future;
use tokio::{sync::mpsc, task::AbortHandle};
use tokio_util::sync::CancellationToken;

/// Identifies one spawned operation so results that arrive after it was
/// canceled or replaced can be recognized and dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RequestId(u64);

pub(crate) struct TaskMessage {
    request: RequestId,
    body: MessageBody,
}

enum MessageBody {
    Progress { completed: usize, total: usize },
    Finished(TaskEvent),
}

/// Lets a running task report how far it has gotten.
pub(crate) struct Reporter {
    request: RequestId,
    sender: mpsc::UnboundedSender<TaskMessage>,
}

impl Reporter {
    pub(crate) fn progress(&self, completed: usize, total: usize) {
        self.send(MessageBody::Progress { completed, total });
    }

    fn send(&self, body: MessageBody) {
        // The receiver only closes when the application is exiting.
        let _ = self.sender.send(TaskMessage {
            request: self.request,
            body,
        });
    }
}

/// How a running operation is canceled.
enum Cancel {
    /// Read-only work is dropped at its next await point and sends no result.
    Abort(AbortHandle),
    /// Work that changes objects is asked to stop between them, and still
    /// reports what it completed.
    Stop(CancellationToken),
}

enum Activity {
    Idle,
    Running {
        label: String,
        progress: Option<(usize, usize)>,
        cancel: Cancel,
        request: RequestId,
    },
}

/// Owns the one running operation and the channel its result returns on.
pub(crate) struct Tasks {
    activity: Activity,
    next_request: u64,
    sender: mpsc::UnboundedSender<TaskMessage>,
    receiver: mpsc::UnboundedReceiver<TaskMessage>,
    spinner_frame: usize,
}

impl Tasks {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        Self {
            activity: Activity::Idle,
            next_request: 0,
            sender,
            receiver,
            spinner_frame: 0,
        }
    }

    pub(crate) fn is_running(&self) -> bool {
        matches!(self.activity, Activity::Running { .. })
    }

    /// Run read-only `work` in the background; canceling drops it mid-flight.
    ///
    /// Replaces any operation still in flight. Must be called from within a
    /// Tokio runtime.
    pub(crate) fn spawn<F>(&mut self, label: impl Into<String>, work: F)
    where
        F: Future<Output = TaskEvent> + Send + 'static,
    {
        self.start(label.into(), |reporter| {
            let handle = tokio::spawn(async move {
                let event = work.await;
                reporter.send(MessageBody::Finished(event));
            });
            Cancel::Abort(handle.abort_handle())
        });
    }

    /// Run `work` that changes objects in the background. Canceling only
    /// cancels the token it is given, so `work` decides where stopping is safe
    /// and always reports its outcome.
    ///
    /// Replaces any operation still in flight. Must be called from within a
    /// Tokio runtime.
    pub(crate) fn spawn_stoppable<F, Fut>(&mut self, label: impl Into<String>, work: F)
    where
        F: FnOnce(Reporter, CancellationToken) -> Fut,
        Fut: Future<Output = TaskEvent> + Send + 'static,
    {
        self.start(label.into(), |reporter| {
            let token = CancellationToken::new();
            let finished = Reporter {
                request: reporter.request,
                sender: reporter.sender.clone(),
            };
            let work = work(reporter, token.clone());
            tokio::spawn(async move {
                let event = work.await;
                finished.send(MessageBody::Finished(event));
            });
            Cancel::Stop(token)
        });
    }

    fn start(&mut self, label: String, spawn: impl FnOnce(Reporter) -> Cancel) {
        self.cancel();
        let request = RequestId(self.next_request);
        self.next_request += 1;
        let cancel = spawn(Reporter {
            request,
            sender: self.sender.clone(),
        });
        self.activity = Activity::Running {
            label,
            progress: None,
            cancel,
            request,
        };
    }

    /// Cancel the running operation. Returns its label if it ended at once;
    /// an operation that stops between objects keeps running until it reports.
    pub(crate) fn cancel(&mut self) -> Option<String> {
        match &self.activity {
            Activity::Idle => None,
            Activity::Running {
                cancel: Cancel::Stop(token),
                ..
            } => {
                token.cancel();
                None
            }
            Activity::Running {
                cancel: Cancel::Abort(handle),
                ..
            } => {
                handle.abort();
                match std::mem::replace(&mut self.activity, Activity::Idle) {
                    Activity::Running { label, .. } => Some(label),
                    Activity::Idle => None,
                }
            }
        }
    }

    /// Wait for the next message from any task, current or stale.
    pub(crate) async fn recv(&mut self) -> Option<TaskMessage> {
        self.receiver.recv().await
    }

    /// Return the event only if it finishes the running operation. Progress is
    /// recorded here, and messages from canceled or replaced operations are dropped.
    pub(crate) fn accept(&mut self, message: TaskMessage) -> Option<TaskEvent> {
        let Activity::Running {
            request, progress, ..
        } = &mut self.activity
        else {
            return None;
        };
        if *request != message.request {
            return None;
        }
        match message.body {
            MessageBody::Progress { completed, total } => {
                *progress = Some((completed, total));
                None
            }
            MessageBody::Finished(event) => {
                self.activity = Activity::Idle;
                Some(event)
            }
        }
    }

    pub(crate) fn advance_spinner(&mut self) {
        self.spinner_frame = self.spinner_frame.wrapping_add(1);
    }

    /// Footer text for the running operation, if any.
    pub(crate) fn status(&self) -> Option<String> {
        const FRAMES: [char; 4] = ['|', '/', '-', '\\'];
        let Activity::Running {
            label,
            progress,
            cancel,
            ..
        } = &self.activity
        else {
            return None;
        };
        let spinner = FRAMES[self.spinner_frame % FRAMES.len()];
        let mut status = format!("{spinner} {label}");
        if let Some((completed, total)) = progress {
            let _ = write!(status, " ({completed}/{total})");
        }
        if matches!(cancel, Cancel::Stop(token) if token.is_cancelled()) {
            status.push_str(" · stopping after the current object");
        }
        Some(status)
    }
}
