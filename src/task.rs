//! Background work, and the single in-flight operation the UI tracks.
use crate::app::TaskEvent;
use std::future::Future;
use tokio::{sync::mpsc, task::AbortHandle};

/// Identifies one spawned operation so results that arrive after it was
/// canceled or replaced can be recognized and dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RequestId(u64);

pub(crate) struct TaskMessage {
    request: RequestId,
    event: TaskEvent,
}

enum Activity {
    Idle,
    Running {
        label: String,
        handle: AbortHandle,
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

    /// Run `work` in the background, replacing any operation still in flight.
    ///
    /// Must be called from within a Tokio runtime.
    pub(crate) fn spawn<F>(&mut self, label: impl Into<String>, work: F)
    where
        F: Future<Output = TaskEvent> + Send + 'static,
    {
        self.cancel();
        let request = RequestId(self.next_request);
        self.next_request += 1;
        let sender = self.sender.clone();
        let handle = tokio::spawn(async move {
            let event = work.await;
            // The receiver only closes when the application is exiting.
            let _ = sender.send(TaskMessage { request, event });
        })
        .abort_handle();
        self.activity = Activity::Running {
            label: label.into(),
            handle,
            request,
        };
    }

    /// Stop the running operation, returning its label if there was one.
    pub(crate) fn cancel(&mut self) -> Option<String> {
        match std::mem::replace(&mut self.activity, Activity::Idle) {
            Activity::Idle => None,
            Activity::Running { label, handle, .. } => {
                handle.abort();
                Some(label)
            }
        }
    }

    /// Wait for the next message from any task, current or stale.
    pub(crate) async fn recv(&mut self) -> Option<TaskMessage> {
        self.receiver.recv().await
    }

    /// Return the event only if it belongs to the running operation, which is
    /// then finished. Results of canceled or replaced operations are dropped.
    pub(crate) fn accept(&mut self, message: TaskMessage) -> Option<TaskEvent> {
        match &self.activity {
            Activity::Running { request, .. } if *request == message.request => {
                self.activity = Activity::Idle;
                Some(message.event)
            }
            _ => None,
        }
    }

    pub(crate) fn advance_spinner(&mut self) {
        self.spinner_frame = self.spinner_frame.wrapping_add(1);
    }

    /// Footer text for the running operation, if any.
    pub(crate) fn status(&self) -> Option<String> {
        const FRAMES: [char; 4] = ['|', '/', '-', '\\'];
        let Activity::Running { label, .. } = &self.activity else {
            return None;
        };
        let spinner = FRAMES[self.spinner_frame % FRAMES.len()];
        Some(format!(
            "{spinner} {label} • `Esc` or `Ctrl-C` to cancel • `q` to quit"
        ))
    }
}
