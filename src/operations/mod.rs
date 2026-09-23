//! Background jobs, cancellation, progress and mutation serialization.
//!
//! The UI thread only submits work and reads snapshots. Every subprocess, file
//! scan and download happens on a worker thread.
//!
//! Two invariants are load-bearing:
//!
//! - **Cancellation never waits behind the job it cancels.** [`OperationCoordinator::cancel`]
//!   flips a flag under a lock that the worker does not hold while it runs, so a
//!   job stuck in a subprocess cannot block the cancel path.
//! - **Mutations are serialized per resource.** Submitting a second job for a
//!   resource that is already being mutated fails immediately with a reason
//!   instead of queueing behind it.

pub mod jobs;

use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use crate::process::{
    CancellationToken, CommandSpec, ProcessEvent, ProcessOutput, RunOptions, Stream,
};

/// Identifies one job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(u64);

impl std::fmt::Display for JobId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "#{}", self.0)
    }
}

/// Which channel a log line came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogStream {
    /// Avdium's own narration.
    Info,
    /// The child's standard output.
    Stdout,
    /// The child's standard error.
    Stderr,
    /// A failure Avdium is reporting.
    Error,
}

impl LogStream {
    /// Label for the log viewer.
    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Stdout => "out",
            Self::Stderr => "err",
            Self::Error => "error",
        }
    }
}

/// One retained log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    /// Where it came from.
    pub stream: LogStream,
    /// The text, without a terminator.
    pub text: String,
}

/// Bounded log retention.
///
/// A tool that prints gigabytes must not exhaust memory, so the oldest lines
/// are dropped and the drop is counted rather than hidden.
#[derive(Debug, Clone)]
pub struct LogBuffer {
    lines: VecDeque<LogLine>,
    bytes: usize,
    limit_bytes: usize,
    dropped_lines: usize,
}

impl LogBuffer {
    /// A buffer retaining at most `limit_bytes` of text.
    pub fn new(limit_bytes: usize) -> Self {
        Self {
            lines: VecDeque::new(),
            bytes: 0,
            limit_bytes,
            dropped_lines: 0,
        }
    }

    /// Append a line, dropping the oldest lines if needed.
    pub fn push(&mut self, stream: LogStream, text: impl Into<String>) {
        let text = text.into();
        self.bytes += text.len() + 1;
        self.lines.push_back(LogLine { stream, text });
        while self.bytes > self.limit_bytes && self.lines.len() > 1 {
            if let Some(dropped) = self.lines.pop_front() {
                self.bytes = self.bytes.saturating_sub(dropped.text.len() + 1);
                self.dropped_lines += 1;
            }
        }
    }

    /// The retained lines, oldest first.
    pub fn lines(&self) -> impl Iterator<Item = &LogLine> {
        self.lines.iter()
    }

    /// How many lines were dropped to stay within the limit.
    pub fn dropped_lines(&self) -> usize {
        self.dropped_lines
    }

    /// Retained text, for copying.
    pub fn text(&self) -> String {
        let mut output = String::new();
        if self.dropped_lines > 0 {
            output.push_str(&format!(
                "[...{} earlier lines dropped...]\n",
                self.dropped_lines
            ));
        }
        for line in &self.lines {
            output.push_str(&line.text);
            output.push('\n');
        }
        output
    }

    /// Number of retained lines.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether nothing has been logged.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// The state of a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobState {
    /// Submitted but not started.
    Queued,
    /// Running.
    Running,
    /// Cancellation requested.
    Cancelling,
    /// Finished successfully.
    Succeeded,
    /// Finished with a failure.
    Failed(String),
    /// Finished because it was cancelled.
    Cancelled,
}

impl JobState {
    /// Whether the job has stopped.
    pub fn is_finished(&self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed(_) | Self::Cancelled)
    }

    /// Label for the UI.
    pub fn label(&self) -> &str {
        match self {
            Self::Queued => "Queued",
            Self::Running => "Running",
            Self::Cancelling => "Cancelling",
            Self::Succeeded => "Done",
            Self::Failed(_) => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }
}

/// Optional progress for a job.
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    /// What is happening.
    pub label: String,
    /// Completion between 0 and 1, when it can be estimated.
    pub fraction: Option<f32>,
}

/// A job as the UI sees it.
#[derive(Debug, Clone)]
pub struct Job {
    /// The identifier.
    pub id: JobId,
    /// What the job is doing.
    pub title: String,
    /// The command that is running, for inspection.
    pub command_preview: Option<String>,
    /// The current state.
    pub state: JobState,
    /// Progress, when known.
    pub progress: Option<Progress>,
    /// Retained output.
    pub log: LogBuffer,
    /// The resource being mutated, when this is a mutation.
    pub mutation_key: Option<String>,
    /// When the job was submitted.
    pub submitted_at: Instant,
    /// When the job finished.
    pub finished_at: Option<Instant>,
}

impl Job {
    /// How long the job has been running, or ran for.
    pub fn duration(&self) -> Duration {
        match self.finished_at {
            Some(finished) => finished.saturating_duration_since(self.submitted_at),
            None => self.submitted_at.elapsed(),
        }
    }

    /// Whether the job can be cancelled right now.
    pub fn is_cancellable(&self) -> bool {
        matches!(self.state, JobState::Queued | JobState::Running)
    }

    /// A one-line status, including the failure reason when there is one.
    pub fn status_line(&self) -> String {
        match &self.state {
            JobState::Failed(reason) => format!("Failed: {reason}"),
            other => other.label().to_owned(),
        }
    }
}

/// How a job finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobOutcome {
    /// The work completed.
    Succeeded,
    /// The work failed, with a reason for the user.
    Failed(String),
    /// The work noticed cancellation and stopped.
    Cancelled,
}

/// Why a job could not be submitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitError {
    /// Another job is already mutating this resource.
    MutationBusy {
        /// The resource.
        key: String,
    },
}

impl std::fmt::Display for SubmitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MutationBusy { key } => write!(
                formatter,
                "another operation is already changing {key}; wait for it to finish or cancel it"
            ),
        }
    }
}

impl std::error::Error for SubmitError {}

/// The handle a worker uses to report progress.
#[derive(Debug, Clone)]
pub struct JobContext {
    id: JobId,
    token: CancellationToken,
    jobs: Arc<Mutex<Vec<Job>>>,
    notify: mpsc::SyncSender<()>,
}

impl JobContext {
    /// The job identifier.
    pub fn id(&self) -> JobId {
        self.id
    }

    /// A copy of the cancellation token.
    pub fn token(&self) -> CancellationToken {
        self.token.clone()
    }

    /// Whether cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    /// Return `Err` when cancellation was requested.
    pub fn check_cancelled(&self) -> Result<(), JobOutcome> {
        if self.is_cancelled() {
            Err(JobOutcome::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Append a log line.
    pub fn log(&self, stream: LogStream, text: impl Into<String>) {
        if let Ok(mut jobs) = self.jobs.lock()
            && let Some(job) = jobs.iter_mut().find(|job| job.id == self.id)
        {
            job.log.push(stream, text);
        }
        self.request_repaint();
    }

    /// Log an informational line.
    pub fn info(&self, text: impl Into<String>) {
        self.log(LogStream::Info, text);
    }

    /// Log a failure line.
    pub fn error(&self, text: impl Into<String>) {
        self.log(LogStream::Error, text);
    }

    /// Record the command that is running, so the UI can show it.
    pub fn set_command(&self, preview: impl Into<String>) {
        let preview = preview.into();
        if let Ok(mut jobs) = self.jobs.lock()
            && let Some(job) = jobs.iter_mut().find(|job| job.id == self.id)
        {
            job.command_preview = Some(preview);
        }
        self.request_repaint();
    }

    /// Record progress.
    pub fn progress(&self, label: impl Into<String>, fraction: Option<f32>) {
        let label = label.into();
        if let Ok(mut jobs) = self.jobs.lock()
            && let Some(job) = jobs.iter_mut().find(|job| job.id == self.id)
        {
            job.progress = Some(Progress { label, fraction });
        }
        self.request_repaint();
    }

    /// Run a command, streaming its output into this job's log.
    ///
    /// This is the only way jobs should execute a subprocess: it applies the
    /// cancellation token, drains both streams, and keeps the output bounded.
    pub fn run(&self, spec: &CommandSpec, options: &RunOptions) -> io::Result<ProcessOutput> {
        self.set_command(spec.preview());
        let jobs = Arc::clone(&self.jobs);
        let id = self.id;
        let notify = self.notify.clone();
        crate::process::run(spec, options, &self.token, |event| {
            let (stream, text) = match event {
                ProcessEvent::Line { stream, text } => (stream, text),
                ProcessEvent::Partial { stream, text } => (stream, format!("{text}  [partial]")),
                ProcessEvent::Progress { stream, text } => (stream, format!("\r{text}")),
            };
            let stream = match stream {
                Stream::Stdout => LogStream::Stdout,
                Stream::Stderr => LogStream::Stderr,
            };
            if let Ok(mut jobs) = jobs.lock()
                && let Some(job) = jobs.iter_mut().find(|job| job.id == id)
            {
                job.log.push(stream, text);
            }
            let _ = notify.try_send(());
        })
    }

    /// Run a command with the default options.
    pub fn run_default(&self, spec: &CommandSpec) -> io::Result<ProcessOutput> {
        self.run(spec, &RunOptions::default())
    }

    fn request_repaint(&self) {
        let _ = self.notify.try_send(());
    }
}

struct CoordinatorInner {
    next_id: AtomicU64,
    jobs: Arc<Mutex<Vec<Job>>>,
    tokens: Mutex<HashMap<JobId, CancellationToken>>,
    mutations: Mutex<HashSet<String>>,
    notify: mpsc::SyncSender<()>,
}

/// Submits and tracks background work.
pub struct OperationCoordinator {
    inner: Arc<CoordinatorInner>,
    repaint: mpsc::Receiver<()>,
}

impl Default for OperationCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl OperationCoordinator {
    /// Create a coordinator.
    pub fn new() -> Self {
        let (notify, repaint) = mpsc::sync_channel(1);
        Self {
            inner: Arc::new(CoordinatorInner {
                next_id: AtomicU64::new(1),
                jobs: Arc::new(Mutex::new(Vec::new())),
                tokens: Mutex::new(HashMap::new()),
                mutations: Mutex::new(HashSet::new()),
                notify,
            }),
            repaint,
        }
    }

    /// Submit work.
    ///
    /// `mutation_key` names the resource being changed, for example
    /// `avd:Pixel_9`. Two jobs with the same key cannot run at the same time.
    pub fn submit<F>(
        &self,
        title: impl Into<String>,
        mutation_key: Option<String>,
        work: F,
    ) -> Result<JobId, SubmitError>
    where
        F: FnOnce(JobContext) -> JobOutcome + Send + 'static,
    {
        let title = title.into();

        if let Some(key) = &mutation_key {
            let mut mutations = self.inner.mutations.lock().expect("mutations lock");
            if mutations.contains(key) {
                return Err(SubmitError::MutationBusy { key: key.clone() });
            }
            mutations.insert(key.clone());
        }

        let id = JobId(self.inner.next_id.fetch_add(1, Ordering::SeqCst));
        let token = CancellationToken::new();

        {
            let mut jobs = self.inner.jobs.lock().expect("jobs lock");
            jobs.push(Job {
                id,
                title,
                command_preview: None,
                state: JobState::Queued,
                progress: None,
                log: LogBuffer::new(DEFAULT_LOG_BYTES),
                mutation_key: mutation_key.clone(),
                submitted_at: Instant::now(),
                finished_at: None,
            });
        }
        self.inner
            .tokens
            .lock()
            .expect("tokens lock")
            .insert(id, token.clone());
        self.request_repaint();

        let context = JobContext {
            id,
            token,
            jobs: Arc::clone(&self.inner.jobs),
            notify: self.inner.notify.clone(),
        };
        let inner = Arc::clone(&self.inner);
        // The worker owns its own copy so the failure path below can still
        // release the mutation key.
        let worker_mutation_key = mutation_key.clone();

        let spawn = std::thread::Builder::new()
            .name(format!("avdium-job-{}", id.0))
            .spawn(move || {
                set_state(&inner.jobs, id, JobState::Running, None);
                let outcome = work(context);
                let final_state = match outcome {
                    JobOutcome::Succeeded => JobState::Succeeded,
                    JobOutcome::Failed(reason) => JobState::Failed(reason),
                    JobOutcome::Cancelled => JobState::Cancelled,
                };
                set_state(&inner.jobs, id, final_state, Some(Instant::now()));
                inner.tokens.lock().expect("tokens lock").remove(&id);
                if let Some(key) = &worker_mutation_key {
                    inner.mutations.lock().expect("mutations lock").remove(key);
                }
                let _ = inner.notify.try_send(());
            });

        if spawn.is_err() {
            // Thread creation failed: do not leave the resource locked.
            set_state(
                &self.inner.jobs,
                id,
                JobState::Failed("worker thread could not start".to_owned()),
                Some(Instant::now()),
            );
            self.inner.tokens.lock().expect("tokens lock").remove(&id);
            if let Some(key) = &mutation_key {
                self.inner
                    .mutations
                    .lock()
                    .expect("mutations lock")
                    .remove(key);
            }
        }

        Ok(id)
    }

    /// Request cancellation.
    ///
    /// Returns immediately. It does not wait for the job to notice, so a job
    /// blocked in a subprocess cannot delay the caller.
    pub fn cancel(&self, id: JobId) -> bool {
        let token = self
            .inner
            .tokens
            .lock()
            .expect("tokens lock")
            .get(&id)
            .cloned();
        let Some(token) = token else {
            return false;
        };
        token.cancel();
        if let Ok(mut jobs) = self.inner.jobs.lock()
            && let Some(job) = jobs.iter_mut().find(|job| job.id == id)
            && !job.state.is_finished()
        {
            job.state = JobState::Cancelling;
        }
        self.request_repaint();
        true
    }

    /// Request cancellation of every running job.
    pub fn cancel_all(&self) -> usize {
        let ids: Vec<JobId> = self
            .inner
            .tokens
            .lock()
            .expect("tokens lock")
            .keys()
            .copied()
            .collect();
        ids.into_iter().filter(|id| self.cancel(*id)).count()
    }

    /// A snapshot of every job, newest first.
    pub fn jobs(&self) -> Vec<Job> {
        let mut jobs = self.inner.jobs.lock().expect("jobs lock").clone();
        jobs.sort_by_key(|job| std::cmp::Reverse(job.id));
        jobs
    }

    /// Look up one job.
    pub fn job(&self, id: JobId) -> Option<Job> {
        self.inner
            .jobs
            .lock()
            .expect("jobs lock")
            .iter()
            .find(|job| job.id == id)
            .cloned()
    }

    /// The jobs that have not finished.
    pub fn running(&self) -> Vec<Job> {
        self.jobs()
            .into_iter()
            .filter(|job| !job.state.is_finished())
            .collect()
    }

    /// Remove finished jobs, returning how many were removed.
    pub fn clear_finished(&self) -> usize {
        let mut jobs = self.inner.jobs.lock().expect("jobs lock");
        let before = jobs.len();
        jobs.retain(|job| !job.state.is_finished());
        let removed = before - jobs.len();
        drop(jobs);
        if removed > 0 {
            self.request_repaint();
        }
        removed
    }

    /// The resources currently being mutated.
    pub fn active_mutations(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .inner
            .mutations
            .lock()
            .expect("mutations lock")
            .iter()
            .cloned()
            .collect();
        keys.sort();
        keys
    }

    /// Whether a repaint was requested since the last call.
    pub fn take_repaint(&self) -> bool {
        let mut any = false;
        while self.repaint.try_recv().is_ok() {
            any = true;
        }
        any
    }

    /// Ask the UI to repaint without waiting for the next scheduled frame.
    fn request_repaint(&self) {
        let _ = self.inner.notify.try_send(());
    }
}

/// Default log retention per job.
pub const DEFAULT_LOG_BYTES: usize = 256 * 1024;

fn set_state(
    jobs: &Arc<Mutex<Vec<Job>>>,
    id: JobId,
    state: JobState,
    finished_at: Option<Instant>,
) {
    if let Ok(mut jobs) = jobs.lock()
        && let Some(job) = jobs.iter_mut().find(|job| job.id == id)
    {
        job.state = state;
        if finished_at.is_some() {
            job.finished_at = finished_at;
        }
    }
}

/// Wait for a job to finish. Intended for tests.
pub fn wait_for(coordinator: &OperationCoordinator, id: JobId, timeout: Duration) -> Option<Job> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(job) = coordinator.job(id)
            && job.state.is_finished()
        {
            return Some(job);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    coordinator.job(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fake_tool() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin/fake-tool")
    }

    #[test]
    fn a_job_reports_success_and_its_log() {
        let coordinator = OperationCoordinator::new();
        let id = coordinator
            .submit("discovery", None, |context| {
                context.info("scanning");
                JobOutcome::Succeeded
            })
            .expect("submit");
        let job = wait_for(&coordinator, id, Duration::from_secs(5)).expect("job");
        assert_eq!(job.state, JobState::Succeeded);
        assert!(job.state.is_finished());
        assert!(job.finished_at.is_some());
        assert!(job.log.text().contains("scanning"));
        assert_eq!(job.status_line(), "Done");
    }

    #[test]
    fn a_job_can_report_a_failure_with_a_reason() {
        let coordinator = OperationCoordinator::new();
        let id = coordinator
            .submit("start", None, |_| {
                JobOutcome::Failed("emulator exited with code 1".to_owned())
            })
            .expect("submit");
        let job = wait_for(&coordinator, id, Duration::from_secs(5)).expect("job");
        assert_eq!(
            job.state,
            JobState::Failed("emulator exited with code 1".to_owned())
        );
        assert!(job.status_line().contains("emulator exited with code 1"));
    }

    #[test]
    fn cancellation_is_delivered_and_reported() {
        let coordinator = OperationCoordinator::new();
        let id = coordinator
            .submit("long work", None, |context| {
                for _ in 0..500 {
                    if context.is_cancelled() {
                        return JobOutcome::Cancelled;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                JobOutcome::Succeeded
            })
            .expect("submit");

        std::thread::sleep(Duration::from_millis(50));
        assert!(coordinator.cancel(id));
        let job = wait_for(&coordinator, id, Duration::from_secs(5)).expect("job");
        assert_eq!(job.state, JobState::Cancelled);
    }

    #[test]
    fn cancellation_does_not_wait_behind_the_job_it_cancels() {
        let coordinator = OperationCoordinator::new();
        let id = coordinator
            .submit("hanging subprocess", None, |context| {
                let script = std::env::temp_dir().join("avdium-cancel-test-scenario");
                std::fs::write(&script, "hang\n").expect("write scenario");
                let spec = CommandSpec::new(fake_tool())
                    .env("FAKE_TOOL_NAME", "hanging")
                    .env("FAKE_SCENARIO", &script)
                    .env("FAKE_RECORD_DIR", std::env::temp_dir());
                let output = context.run_default(&spec).expect("run");
                if output.cancelled {
                    JobOutcome::Cancelled
                } else {
                    JobOutcome::Succeeded
                }
            })
            .expect("submit");

        std::thread::sleep(Duration::from_millis(150));
        let started = Instant::now();
        assert!(coordinator.cancel(id));
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(100),
            "cancel took {elapsed:?}"
        );
    }

    #[test]
    fn a_mutation_key_serializes_jobs_for_the_same_resource() {
        let coordinator = OperationCoordinator::new();
        let first = coordinator
            .submit("edit", Some("avd:Pixel_9".to_owned()), |context| {
                for _ in 0..200 {
                    if context.is_cancelled() {
                        return JobOutcome::Cancelled;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                JobOutcome::Succeeded
            })
            .expect("first submit");

        let second = coordinator.submit("wipe", Some("avd:Pixel_9".to_owned()), |_| {
            JobOutcome::Succeeded
        });
        assert_eq!(
            second,
            Err(SubmitError::MutationBusy {
                key: "avd:Pixel_9".to_owned()
            })
        );
        assert!(
            second
                .expect_err("busy")
                .to_string()
                .contains("already changing")
        );

        // A different resource is unaffected.
        assert!(
            coordinator
                .submit("other", Some("avd:Pixel_Tablet".to_owned()), |_| {
                    JobOutcome::Succeeded
                })
                .is_ok()
        );

        coordinator.cancel(first);
        wait_for(&coordinator, first, Duration::from_secs(5));
        // Once the first job is gone, the key is free again.
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut submitted = false;
        while Instant::now() < deadline && !submitted {
            submitted = coordinator
                .submit("wipe again", Some("avd:Pixel_9".to_owned()), |_| {
                    JobOutcome::Succeeded
                })
                .is_ok();
            if !submitted {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        assert!(submitted, "mutation key was never released");
    }

    #[test]
    fn jobs_without_a_mutation_key_do_not_block_each_other() {
        let coordinator = OperationCoordinator::new();
        let a = coordinator
            .submit("a", None, |_| JobOutcome::Succeeded)
            .expect("a");
        let b = coordinator
            .submit("b", None, |_| JobOutcome::Succeeded)
            .expect("b");
        assert_ne!(a, b);
        wait_for(&coordinator, a, Duration::from_secs(5));
        wait_for(&coordinator, b, Duration::from_secs(5));
    }

    #[test]
    fn subprocess_output_is_streamed_into_the_job_log() {
        let coordinator = OperationCoordinator::new();
        let id = coordinator
            .submit("probe", None, |context| {
                let script = std::env::temp_dir().join("avdium-log-test-scenario");
                std::fs::write(
                    &script,
                    "out hello from stdout\nerr hello from stderr\nexit 0\n",
                )
                .expect("write");
                let spec = CommandSpec::new(fake_tool())
                    .env("FAKE_TOOL_NAME", "logger")
                    .env("FAKE_SCENARIO", &script)
                    .env("FAKE_RECORD_DIR", std::env::temp_dir());
                let output = context.run_default(&spec).expect("run");
                if output.success {
                    JobOutcome::Succeeded
                } else {
                    JobOutcome::Failed(output.describe_failure())
                }
            })
            .expect("submit");

        let job = wait_for(&coordinator, id, Duration::from_secs(10)).expect("job");
        assert_eq!(job.state, JobState::Succeeded);
        let text = job.log.text();
        assert!(text.contains("hello from stdout"), "{text}");
        assert!(text.contains("hello from stderr"), "{text}");
        assert!(
            job.command_preview.is_some(),
            "the command must be inspectable"
        );
        assert!(job.log.lines().any(|line| line.stream == LogStream::Stderr));
    }

    #[test]
    fn a_cancelled_subprocess_finishes_the_job_as_cancelled() {
        let coordinator = OperationCoordinator::new();
        let id = coordinator
            .submit("hanging probe", None, |context| {
                let script = std::env::temp_dir().join("avdium-cancel-subprocess-scenario");
                std::fs::write(&script, "out started\nhang\n").expect("write");
                let spec = CommandSpec::new(fake_tool())
                    .env("FAKE_TOOL_NAME", "hanging2")
                    .env("FAKE_SCENARIO", &script)
                    .env("FAKE_RECORD_DIR", std::env::temp_dir());
                let output = context.run_default(&spec).expect("run");
                if output.cancelled {
                    JobOutcome::Cancelled
                } else {
                    JobOutcome::Succeeded
                }
            })
            .expect("submit");

        std::thread::sleep(Duration::from_millis(200));
        coordinator.cancel(id);
        let job = wait_for(&coordinator, id, Duration::from_secs(10)).expect("job");
        assert_eq!(job.state, JobState::Cancelled);
        assert!(job.log.text().contains("started"));
    }

    #[test]
    fn progress_is_recorded() {
        let coordinator = OperationCoordinator::new();
        let id = coordinator
            .submit("download", None, |context| {
                context.progress("downloading", Some(0.25));
                JobOutcome::Succeeded
            })
            .expect("submit");
        let job = wait_for(&coordinator, id, Duration::from_secs(5)).expect("job");
        let progress = job.progress.expect("progress");
        assert_eq!(progress.label, "downloading");
        assert_eq!(progress.fraction, Some(0.25));
    }

    #[test]
    fn the_log_buffer_is_bounded_and_reports_drops() {
        let mut buffer = LogBuffer::new(200);
        for index in 0..100 {
            buffer.push(
                LogStream::Stdout,
                format!("line {index} with some padding text"),
            );
        }
        assert!(buffer.len() < 100, "the buffer kept everything");
        assert!(buffer.dropped_lines() > 0);
        assert!(buffer.text().contains("earlier lines dropped"));
        assert!(
            buffer.text().len() < 400,
            "retained {} bytes",
            buffer.text().len()
        );
    }

    #[test]
    fn clearing_finished_jobs_leaves_running_ones_alone() {
        let coordinator = OperationCoordinator::new();
        let finished = coordinator
            .submit("quick", None, |_| JobOutcome::Succeeded)
            .expect("submit");
        wait_for(&coordinator, finished, Duration::from_secs(5));
        let running = coordinator
            .submit("slow", None, |context| {
                while !context.is_cancelled() {
                    std::thread::sleep(Duration::from_millis(5));
                }
                JobOutcome::Cancelled
            })
            .expect("submit");

        assert_eq!(coordinator.clear_finished(), 1);
        let ids: Vec<JobId> = coordinator.jobs().iter().map(|job| job.id).collect();
        assert!(!ids.contains(&finished));
        assert!(ids.contains(&running));
        coordinator.cancel(running);
        wait_for(&coordinator, running, Duration::from_secs(5));
    }

    #[test]
    fn repaint_is_requested_when_state_changes() {
        let coordinator = OperationCoordinator::new();
        let _ = coordinator.take_repaint();
        let id = coordinator
            .submit("work", None, |_| JobOutcome::Succeeded)
            .expect("submit");
        wait_for(&coordinator, id, Duration::from_secs(5));
        assert!(
            coordinator.take_repaint(),
            "a repaint should have been requested"
        );
    }

    #[test]
    fn cancelling_an_unknown_job_is_not_an_error() {
        let coordinator = OperationCoordinator::new();
        let id = coordinator
            .submit("quick", None, |_| JobOutcome::Succeeded)
            .expect("submit");
        wait_for(&coordinator, id, Duration::from_secs(5));
        assert!(!coordinator.cancel(id));
    }

    #[test]
    fn active_mutations_are_listed() {
        let coordinator = OperationCoordinator::new();
        let id = coordinator
            .submit("edit", Some("avd:X".to_owned()), |context| {
                while !context.is_cancelled() {
                    std::thread::sleep(Duration::from_millis(5));
                }
                JobOutcome::Cancelled
            })
            .expect("submit");
        assert_eq!(coordinator.active_mutations(), vec!["avd:X".to_owned()]);
        coordinator.cancel(id);
        wait_for(&coordinator, id, Duration::from_secs(5));
    }

    #[test]
    fn a_job_is_cancellable_only_while_it_can_still_stop() {
        let coordinator = OperationCoordinator::new();
        let id = coordinator
            .submit("quick", None, |_| JobOutcome::Succeeded)
            .expect("submit");
        let job = wait_for(&coordinator, id, Duration::from_secs(5)).expect("job");
        assert!(!job.is_cancellable());
    }
}
