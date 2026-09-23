//! Subprocess execution with cancellation, bounded output and no shell.
//!
//! Invariants this module exists to protect:
//!
//! - Arguments are passed as an array. Nothing is ever handed to a shell, and
//!   the shell-escaped [`CommandSpec::preview`] is display-only.
//! - stdout and stderr are drained concurrently, so a chatty child cannot
//!   deadlock on a full pipe.
//! - Cancellation signals the whole process group, waits briefly, then forces
//!   termination. The child is always reaped.
//! - Retained output is bounded. A tool that prints gigabytes cannot exhaust
//!   memory, and the truncation is reported rather than hidden.
//! - Prompts that do not end with a newline are surfaced as partial events
//!   instead of being buffered forever.

use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Which output stream an event came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stream {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
}

impl Stream {
    /// Every stream, for iteration.
    pub const ALL: [Self; 2] = [Self::Stdout, Self::Stderr];

    fn index(self) -> usize {
        match self {
            Self::Stdout => 0,
            Self::Stderr => 1,
        }
    }

    /// Label for the log viewer.
    pub fn label(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

/// Something the child produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessEvent {
    /// A complete line, without its terminator.
    Line {
        /// Which stream produced it.
        stream: Stream,
        /// The line contents.
        text: String,
    },
    /// Output that paused without a newline. This is how a prompt appears.
    Partial {
        /// Which stream produced it.
        stream: Stream,
        /// The pending text.
        text: String,
    },
    /// A carriage-return progress update, which replaces the current line.
    Progress {
        /// Which stream produced it.
        stream: Stream,
        /// The updated text.
        text: String,
    },
}

/// How the child's standard input is set up.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StdinMode {
    /// Inherit nothing: `/dev/null`.
    #[default]
    Null,
    /// An empty, immediately closed pipe.
    Closed,
    /// Write this text, then close. Used for enumerated license answers.
    Text(String),
}

/// A fully specified child process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    /// Absolute path to the executable.
    pub program: PathBuf,
    /// Arguments, passed as an array.
    pub args: Vec<OsString>,
    /// Environment overrides applied on top of the inherited environment.
    pub env: Vec<(OsString, OsString)>,
    /// Working directory, when it matters.
    pub cwd: Option<PathBuf>,
    /// Standard input handling.
    pub stdin: StdinMode,
}

impl CommandSpec {
    /// A command with no arguments.
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
            cwd: None,
            stdin: StdinMode::Null,
        }
    }

    /// Append one argument.
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Append several arguments.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Add an environment override.
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Add several environment overrides.
    pub fn envs<I, K, V>(mut self, vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<OsString>,
        V: Into<OsString>,
    {
        self.env.extend(
            vars.into_iter()
                .map(|(key, value)| (key.into(), value.into())),
        );
        self
    }

    /// Set the working directory.
    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Set the standard input mode.
    pub fn stdin(mut self, stdin: StdinMode) -> Self {
        self.stdin = stdin;
        self
    }

    /// A copyable command line for the UI.
    ///
    /// This is for display only. It is never parsed or executed.
    pub fn preview(&self) -> String {
        let mut parts = vec![quote(&self.program.to_string_lossy())];
        parts.extend(self.args.iter().map(|arg| quote(&arg.to_string_lossy())));
        parts.join(" ")
    }
}

/// Quote one argument for display.
fn quote(value: &str) -> String {
    let plain = !value.is_empty()
        && value.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '_' | '-' | '.' | '/' | ':' | '@' | '+' | '=' | ',')
        });
    if plain {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// A cooperative cancellation flag shared with the caller.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    flag: Arc<AtomicBool>,
}

impl CancellationToken {
    /// A fresh, uncancelled token.
    pub fn new() -> Self {
        Self::default()
    }

    /// Request cancellation. Safe to call from any thread, any number of times.
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    /// Whether cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

/// Limits for one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOptions {
    /// Give up after this long. `None` means no deadline.
    pub timeout: Option<Duration>,
    /// Total bytes retained across both streams.
    pub max_output_bytes: usize,
    /// How long output must pause before a partial line is reported.
    pub partial_flush_after: Duration,
    /// How long a signalled child may take to exit before it is killed.
    pub termination_grace: Duration,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            timeout: None,
            max_output_bytes: 1024 * 1024,
            partial_flush_after: Duration::from_millis(250),
            termination_grace: Duration::from_secs(3),
        }
    }
}

/// The result of one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    /// The executable that ran.
    pub program: PathBuf,
    /// The argument array.
    pub args: Vec<OsString>,
    /// Display-only command line.
    pub preview: String,
    /// Exit code, when the process exited normally.
    pub exit_code: Option<i32>,
    /// Whether the process exited successfully.
    pub success: bool,
    /// Whether cancellation ended the process.
    pub cancelled: bool,
    /// Whether the deadline ended the process.
    pub timed_out: bool,
    /// Whether retained output was cut short.
    pub output_truncated: bool,
    /// Retained standard output.
    pub stdout: String,
    /// Retained standard error.
    pub stderr: String,
    /// Wall-clock duration.
    pub duration: Duration,
}

impl ProcessOutput {
    /// stdout followed by stderr, for display.
    pub fn combined(&self) -> String {
        match (self.stdout.is_empty(), self.stderr.is_empty()) {
            (false, false) => format!("{}\n{}", self.stdout.trim_end(), self.stderr.trim_end()),
            (false, true) => self.stdout.clone(),
            (true, false) => self.stderr.clone(),
            (true, true) => String::new(),
        }
    }

    /// A one-line explanation suitable for the UI.
    pub fn describe_failure(&self) -> String {
        if self.cancelled {
            return format!("{} was cancelled.", self.program.display());
        }
        if self.timed_out {
            return format!("{} did not finish in time.", self.program.display());
        }
        match self.exit_code {
            Some(code) => format!("{} exited with code {code}.", self.program.display()),
            None => format!("{} was terminated by a signal.", self.program.display()),
        }
    }
}

/// A message from one of the child's pipe readers.
///
/// Completion travels in band rather than being inferred from the channel
/// being empty. An empty channel and a finished reader look identical to
/// `try_recv`, and treating the first as the second discards output.
enum PipeMessage {
    /// Bytes read from one of the child's streams.
    Data(Stream, Vec<u8>),
    /// The reader reached end of file and has stopped.
    Finished,
}

/// Assemble one chunk and record everything it produced.
fn apply_chunk<F>(
    assemblers: &mut [LineAssembler; 2],
    output: &mut OutputBuffer,
    stream: Stream,
    chunk: &[u8],
    on_event: &mut F,
) where
    F: FnMut(ProcessEvent),
{
    let assembler = &mut assemblers[stream.index()];
    for event in assembler.push(chunk) {
        output.record(&event);
        on_event(event);
    }
}

/// Run a command, streaming events to a callback.
///
/// The callback runs on the calling thread while the child's pipes are drained
/// by helper threads, so it must not block for long.
pub fn run<F>(
    spec: &CommandSpec,
    options: &RunOptions,
    cancel: &CancellationToken,
    mut on_event: F,
) -> io::Result<ProcessOutput>
where
    F: FnMut(ProcessEvent),
{
    let started = Instant::now();
    let mut command = Command::new(&spec.program);
    command.args(&spec.args);
    for (key, value) in &spec.env {
        command.env(key, value);
    }
    if let Some(cwd) = &spec.cwd {
        command.current_dir(cwd);
    }
    command.stdin(match spec.stdin {
        StdinMode::Null => Stdio::null(),
        StdinMode::Closed | StdinMode::Text(_) => Stdio::piped(),
    });
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    // SAFETY: `setpgid` is async-signal-safe and only touches this child's own
    // process group. It is what makes `killpg` able to reach the child's
    // descendants during cancellation.
    unsafe {
        use std::os::unix::process::CommandExt;
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let mut child = command.spawn()?;
    let pid = child.id();

    // Feed standard input, then close it. A separate thread avoids blocking on
    // a full pipe if the child reads late.
    match &spec.stdin {
        StdinMode::Text(text) => {
            if let Some(mut stdin) = child.stdin.take() {
                let text = text.clone();
                std::thread::spawn(move || {
                    let _ = stdin.write_all(text.as_bytes());
                    let _ = stdin.flush();
                });
            }
        }
        StdinMode::Closed => {
            drop(child.stdin.take());
        }
        StdinMode::Null => {}
    }

    let (sender, receiver) = mpsc::channel::<PipeMessage>();
    let mut readers = Vec::new();
    if let Some(pipe) = child.stdout.take() {
        readers.push(spawn_reader(pipe, Stream::Stdout, sender.clone()));
    }
    if let Some(pipe) = child.stderr.take() {
        readers.push(spawn_reader(pipe, Stream::Stderr, sender.clone()));
    }
    drop(sender);
    let reader_count = readers.len();

    let mut assemblers = [
        LineAssembler::new(Stream::Stdout),
        LineAssembler::new(Stream::Stderr),
    ];
    let mut output = OutputBuffer::new(options.max_output_bytes);
    let mut status: Option<ExitStatus> = None;
    let mut cancelled = false;
    let mut timed_out = false;
    let mut signalled_at: Option<Instant> = None;
    let mut force_killed = false;
    let mut last_data = Instant::now();
    let mut drain_deadline: Option<Instant> = None;
    let mut finished_readers = 0usize;

    let poll = Duration::from_millis(20);
    let post_exit_drain = Duration::from_millis(500);

    loop {
        // Drain everything queued. Nothing is discarded: data is assembled and
        // completion is counted, so a momentarily empty channel is never
        // mistaken for the end of output.
        let mut received = false;
        while let Ok(message) = receiver.try_recv() {
            match message {
                PipeMessage::Data(stream, chunk) => {
                    received = true;
                    apply_chunk(&mut assemblers, &mut output, stream, &chunk, &mut on_event);
                }
                PipeMessage::Finished => finished_readers += 1,
            }
        }
        if received {
            last_data = Instant::now();
        }

        if status.is_none() {
            match child.try_wait() {
                Ok(Some(exit)) => {
                    status = Some(exit);
                    drain_deadline = Some(Instant::now() + post_exit_drain);
                }
                Ok(None) => {}
                Err(_) => {}
            }
        }

        // Report prompts and progress that never end with a newline. A partial
        // is display-only: it reaches the callback but is not retained, so the
        // final `Line` that `finish` emits for the same text is the only copy
        // in the retained output.
        if last_data.elapsed() >= options.partial_flush_after {
            for assembler in &mut assemblers {
                for event in assembler.flush_partial() {
                    on_event(event);
                }
            }
            last_data = Instant::now();
        }

        let deadline_hit = options
            .timeout
            .is_some_and(|timeout| started.elapsed() >= timeout);
        if status.is_none() && (cancel.is_cancelled() || deadline_hit) && signalled_at.is_none() {
            cancelled = cancel.is_cancelled();
            timed_out = !cancelled && deadline_hit;
            signal_group(pid, libc::SIGTERM);
            signalled_at = Some(Instant::now());
        }
        if status.is_none()
            && let Some(signalled) = signalled_at
            && !force_killed
            && signalled.elapsed() >= options.termination_grace
        {
            signal_group(pid, libc::SIGKILL);
            force_killed = true;
        }

        let readers_done = finished_readers >= reader_count;
        let drained = drain_deadline.is_some_and(|deadline| Instant::now() >= deadline);
        if status.is_some() && (drained || readers_done) {
            break;
        }

        // A cancelled child that refuses to exit must not hang the caller.
        if signalled_at
            .is_some_and(|signalled| signalled.elapsed() >= options.termination_grace * 4)
            && status.is_none()
        {
            let _ = child.kill();
            if let Ok(exit) = child.wait() {
                status = Some(exit);
            }
            drain_deadline = Some(Instant::now());
        }

        if status.is_none() || !drained {
            match receiver.recv_timeout(poll) {
                Ok(PipeMessage::Data(stream, chunk)) => {
                    apply_chunk(&mut assemblers, &mut output, stream, &chunk, &mut on_event);
                    last_data = Instant::now();
                }
                Ok(PipeMessage::Finished) => finished_readers += 1,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {}
            }
        }
    }

    for assembler in &mut assemblers {
        for event in assembler.finish() {
            output.record(&event);
            on_event(event);
        }
    }

    if status.is_none() {
        status = child.wait().ok();
    }
    for reader in readers {
        let _ = reader.join();
    }

    let exit_code = status.and_then(|status| status.code());
    Ok(ProcessOutput {
        program: spec.program.clone(),
        args: spec.args.clone(),
        preview: spec.preview(),
        exit_code,
        success: status.is_some_and(|status| status.success()) && !cancelled && !timed_out,
        cancelled,
        timed_out,
        output_truncated: output.truncated,
        stdout: output.stdout,
        stderr: output.stderr,
        duration: started.elapsed(),
    })
}

/// Run a command and discard its events.
pub fn run_collect(
    spec: &CommandSpec,
    options: &RunOptions,
    cancel: &CancellationToken,
) -> io::Result<ProcessOutput> {
    run(spec, options, cancel, |_| {})
}

/// Signal a process group, falling back to the single process.
fn signal_group(pid: u32, signal: libc::c_int) {
    // SAFETY: `killpg` takes a process group id and a signal number. The group
    // was created by `setpgid` in `pre_exec`, so it contains only this child
    // and its descendants.
    unsafe {
        if libc::killpg(pid as libc::pid_t, signal) == -1 {
            libc::kill(pid as libc::pid_t, signal);
        }
    }
}

/// Drain one of the child's pipes on its own thread.
///
/// A dedicated thread per stream is what stops a chatty child from blocking on
/// a full pipe while the other stream is being read.
fn spawn_reader<R>(
    mut pipe: R,
    stream: Stream,
    sender: mpsc::Sender<PipeMessage>,
) -> std::thread::JoinHandle<()>
where
    R: Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut buffer = [0u8; 8192];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    if sender
                        .send(PipeMessage::Data(stream, buffer[..read].to_vec()))
                        .is_err()
                    {
                        break;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        // Signal completion explicitly, so the receiver never has to guess.
        let _ = sender.send(PipeMessage::Finished);
    })
}

/// Assembles bytes into lines, partial lines and carriage-return progress.
#[derive(Debug)]
struct LineAssembler {
    stream: Stream,
    pending: Vec<u8>,
    partial_reported: bool,
    line_truncated: bool,
}

/// Longest line reported intact. Longer lines are cut and marked.
const MAX_LINE_BYTES: usize = 64 * 1024;

impl LineAssembler {
    fn new(stream: Stream) -> Self {
        Self {
            stream,
            pending: Vec::new(),
            partial_reported: false,
            line_truncated: false,
        }
    }

    fn push(&mut self, chunk: &[u8]) -> Vec<ProcessEvent> {
        let stream = self.stream;
        let mut events = Vec::new();
        let mut start = 0usize;
        for (index, byte) in chunk.iter().enumerate() {
            match byte {
                b'\n' => {
                    self.pending.extend_from_slice(&chunk[start..index]);
                    let text = self.take_line();
                    events.push(ProcessEvent::Line { stream, text });
                    start = index + 1;
                }
                b'\r' => {
                    self.pending.extend_from_slice(&chunk[start..index]);
                    start = index + 1;
                    // A `\r` immediately followed by `\n` is just CRLF: the
                    // `\n` terminates the line, so this must not also be
                    // reported as a progress update.
                    if chunk.get(index + 1) == Some(&b'\n') {
                        continue;
                    }
                    let text = self.take_partial_text();
                    if !text.is_empty() {
                        events.push(ProcessEvent::Progress { stream, text });
                    }
                }
                _ => {}
            }
        }
        self.pending.extend_from_slice(&chunk[start..]);
        if self.pending.len() > MAX_LINE_BYTES {
            let excess = self.pending.len() - MAX_LINE_BYTES;
            self.pending.drain(..excess);
            self.line_truncated = true;
        }
        events
    }

    fn flush_partial(&mut self) -> Vec<ProcessEvent> {
        if self.pending.is_empty() || self.partial_reported {
            return Vec::new();
        }
        self.partial_reported = true;
        let text = String::from_utf8_lossy(&self.pending).into_owned();
        vec![ProcessEvent::Partial {
            stream: self.stream,
            text,
        }]
    }

    /// Emit whatever is left when a stream ends without a newline.
    ///
    /// This is the only point at which the trailing text enters retained
    /// output, because a `Partial` is display-only, so it cannot duplicate.
    fn finish(&mut self) -> Vec<ProcessEvent> {
        if self.pending.is_empty() {
            return Vec::new();
        }
        let text = self.take_line();
        vec![ProcessEvent::Line {
            stream: self.stream,
            text,
        }]
    }

    fn take_line(&mut self) -> String {
        let mut bytes = std::mem::take(&mut self.pending);
        let truncated = self.line_truncated;
        self.partial_reported = false;
        self.line_truncated = false;
        if truncated {
            bytes.splice(0..0, b"[...truncated...] ".iter().copied());
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn take_partial_text(&mut self) -> String {
        let bytes = std::mem::take(&mut self.pending);
        self.partial_reported = false;
        self.line_truncated = false;
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

/// Bounded retention of everything the child printed.
#[derive(Debug)]
struct OutputBuffer {
    stdout: String,
    stderr: String,
    retained: usize,
    limit: usize,
    truncated: bool,
}

impl OutputBuffer {
    fn new(limit: usize) -> Self {
        Self {
            stdout: String::new(),
            stderr: String::new(),
            retained: 0,
            limit,
            truncated: false,
        }
    }

    fn record(&mut self, event: &ProcessEvent) {
        let (stream, text, terminator) = match event {
            ProcessEvent::Line { stream, text } => (*stream, text, "\n"),
            ProcessEvent::Partial { stream, text } | ProcessEvent::Progress { stream, text } => {
                (*stream, text, "")
            }
        };
        if self.retained >= self.limit {
            self.truncated = true;
            return;
        }
        let remaining = self.limit - self.retained;
        let mut piece = text.as_str();
        if piece.len() + terminator.len() > remaining {
            // Cut on a character boundary so the retained text stays valid.
            let mut cut = remaining.saturating_sub(terminator.len());
            while cut > 0 && !piece.is_char_boundary(cut) {
                cut -= 1;
            }
            piece = &piece[..cut];
            self.truncated = true;
        }
        let target = match stream {
            Stream::Stdout => &mut self.stdout,
            Stream::Stderr => &mut self.stderr,
        };
        target.push_str(piece);
        target.push_str(terminator);
        self.retained += piece.len() + terminator.len();
    }
}

/// Whether an executable exists and can be run by this user.
pub fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(metadata) => metadata.is_file() && metadata.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

/// Convenience for a one-shot probe with a short timeout.
pub fn probe(
    program: impl Into<PathBuf>,
    args: &[&str],
    timeout: Duration,
) -> io::Result<ProcessOutput> {
    let spec = CommandSpec::new(program).args(args.iter().copied());
    let options = RunOptions {
        timeout: Some(timeout),
        ..RunOptions::default()
    };
    run_collect(&spec, &options, &CancellationToken::new())
}

/// Re-export so callers do not need `std::ffi`.
pub type OsStrLike = OsStr;

#[cfg(test)]
mod tests {
    use super::*;

    /// Fake tools live in `tests/fixtures/bin` and never touch a real SDK.
    fn fake_tool() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin/fake-tool")
    }

    struct Scenario {
        _dir: tempfile::TempDir,
        spec: CommandSpec,
        record_dir: PathBuf,
    }

    fn scenario(name: &str, script: &str) -> Scenario {
        let dir = tempfile::tempdir().expect("tempdir");
        let record_dir = dir.path().join("records");
        std::fs::create_dir_all(&record_dir).expect("create record dir");
        let script_path = dir.path().join("scenario");
        std::fs::write(&script_path, script).expect("write scenario");
        let spec = CommandSpec::new(fake_tool())
            .env("FAKE_TOOL_NAME", name)
            .env("FAKE_SCENARIO", &script_path)
            .env("FAKE_RECORD_DIR", &record_dir);
        Scenario {
            _dir: dir,
            spec,
            record_dir,
        }
    }

    fn record(scenario: &Scenario, name: &str) -> String {
        std::fs::read_to_string(scenario.record_dir.join(format!("{name}.log")))
            .expect("fake tool recorded a log")
    }

    #[test]
    fn stdout_and_stderr_are_captured_separately() {
        let scenario = scenario("probe", "out hello\nerr trouble\nexit 0\n");
        let output = run_collect(
            &scenario.spec,
            &RunOptions::default(),
            &CancellationToken::new(),
        )
        .expect("run");
        assert!(output.success);
        assert_eq!(output.exit_code, Some(0));
        assert_eq!(output.stdout.trim_end(), "hello");
        assert_eq!(output.stderr.trim_end(), "trouble");
        assert!(!output.output_truncated);
    }

    #[test]
    fn arguments_and_environment_reach_the_child_unchanged() {
        let dir = tempfile::tempdir().expect("tempdir");
        let script = dir.path().join("scenario");
        std::fs::write(&script, "out ok\nexit 0\n").expect("write");
        let spec = CommandSpec::new(fake_tool())
            .args(["list", "--long", "name with spaces", "--flag=value"])
            .env("FAKE_TOOL_NAME", "probe")
            .env("FAKE_SCENARIO", &script)
            .env("FAKE_RECORD_DIR", dir.path())
            .env("ANDROID_HOME", "/tmp/example-sdk");

        let output =
            run_collect(&spec, &RunOptions::default(), &CancellationToken::new()).expect("run");
        assert!(output.success);

        let log = std::fs::read_to_string(dir.path().join("probe.log")).expect("log");
        assert!(log.contains("arg1=list"));
        assert!(log.contains("arg2=--long"));
        assert!(log.contains("arg3=name with spaces"));
        assert!(log.contains("arg4=--flag=value"));
        assert!(log.contains("ANDROID_HOME=/tmp/example-sdk"));
    }

    #[test]
    fn a_non_zero_exit_is_reported_with_its_code() {
        let scenario = scenario("probe", "err boom\nexit 7\n");
        let output = run_collect(
            &scenario.spec,
            &RunOptions::default(),
            &CancellationToken::new(),
        )
        .expect("run");
        assert!(!output.success);
        assert_eq!(output.exit_code, Some(7));
        assert!(output.describe_failure().contains("code 7"));
    }

    #[test]
    fn a_missing_program_is_an_error_not_a_panic() {
        let spec = CommandSpec::new("/definitely/not/a/real/program");
        let error = run_collect(&spec, &RunOptions::default(), &CancellationToken::new())
            .expect_err("spawn must fail");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn a_prompt_without_a_newline_is_reported_as_a_partial() {
        let scenario = scenario(
            "probe",
            "prompt Do you accept the license?\nsleep 0.4\nout done\nexit 0\n",
        );
        let mut partials = Vec::new();
        let output = run(
            &scenario.spec,
            &RunOptions {
                partial_flush_after: Duration::from_millis(80),
                ..RunOptions::default()
            },
            &CancellationToken::new(),
            |event| {
                if let ProcessEvent::Partial { text, .. } = event {
                    partials.push(text);
                }
            },
        )
        .expect("run");

        assert!(output.success);
        assert!(
            partials.iter().any(|text| text.contains("Do you accept")),
            "expected a partial prompt, got {partials:?}"
        );
    }

    #[test]
    fn carriage_return_progress_is_reported_as_progress() {
        let scenario = scenario("probe", "cr 10%\ncr 50%\ncr 100%\nout done\nexit 0\n");
        let mut progress = Vec::new();
        let output = run(
            &scenario.spec,
            &RunOptions::default(),
            &CancellationToken::new(),
            |event| {
                if let ProcessEvent::Progress { text, .. } = event {
                    progress.push(text);
                }
            },
        )
        .expect("run");
        assert!(output.success);
        assert!(
            progress.iter().any(|text| text.contains("50%")),
            "got {progress:?}"
        );
    }

    #[test]
    fn crlf_does_not_produce_phantom_progress() {
        let scenario = scenario("probe", "crlf hello\nexit 0\n");
        let mut progress = 0usize;
        let output = run(
            &scenario.spec,
            &RunOptions::default(),
            &CancellationToken::new(),
            |event| {
                if matches!(event, ProcessEvent::Progress { .. }) {
                    progress += 1;
                }
            },
        )
        .expect("run");
        assert!(output.success);
        assert_eq!(progress, 0);
        assert_eq!(output.stdout.trim_end(), "hello");
    }

    #[test]
    fn invalid_utf8_is_replaced_rather_than_failing() {
        let scenario = scenario("probe", "invalid\nexit 0\n");
        let output = run_collect(
            &scenario.spec,
            &RunOptions::default(),
            &CancellationToken::new(),
        )
        .expect("run");
        assert!(output.success);
        assert!(
            output.stdout.contains('\u{fffd}'),
            "got {:?}",
            output.stdout
        );
    }

    #[test]
    fn large_output_is_bounded_and_the_truncation_is_reported() {
        let scenario = scenario("probe", "huge 40000\nexit 0\n");
        let options = RunOptions {
            max_output_bytes: 64 * 1024,
            ..RunOptions::default()
        };
        let output = run_collect(&scenario.spec, &options, &CancellationToken::new()).expect("run");
        assert!(output.success);
        assert!(
            output.output_truncated,
            "expected truncation to be reported"
        );
        assert!(
            output.stdout.len() <= 64 * 1024,
            "retained {} bytes",
            output.stdout.len()
        );
    }

    #[test]
    fn cancellation_terminates_a_hanging_child() {
        let scenario = scenario("probe", "out starting\nhang\n");
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            trigger.cancel();
        });

        let started = Instant::now();
        let output = run_collect(&scenario.spec, &RunOptions::default(), &cancel).expect("run");
        assert!(output.cancelled, "cancellation must be reported");
        assert!(!output.success);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "cancellation was too slow"
        );
        assert!(output.describe_failure().contains("cancelled"));
    }

    #[test]
    fn cancellation_reaches_the_whole_process_group() {
        // The fake tool spawns a long-lived grandchild. Killing only the direct
        // child would leave the grandchild running.
        let dir = tempfile::tempdir().expect("tempdir");
        let pid_file = dir.path().join("grandchild.pid");
        let script = dir.path().join("scenario");
        std::fs::write(
            &script,
            format!("grandchild {}\nexit 0\n", pid_file.display()),
        )
        .expect("write");
        let spec = CommandSpec::new(fake_tool())
            .env("FAKE_TOOL_NAME", "probe")
            .env("FAKE_SCENARIO", &script)
            .env("FAKE_RECORD_DIR", dir.path());

        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            trigger.cancel();
        });

        let output = run_collect(&spec, &RunOptions::default(), &cancel).expect("run");
        assert!(output.cancelled);

        let pid_text = std::fs::read_to_string(&pid_file).expect("grandchild pid recorded");
        let pid: i32 = pid_text.trim().parse().expect("pid");
        // Give the signal a moment to propagate.
        std::thread::sleep(Duration::from_millis(300));
        // SAFETY: signal 0 only checks for existence.
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        if alive {
            // Do not leave a stray process behind while failing.
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
        assert!(!alive, "grandchild {pid} survived cancellation");
    }

    #[test]
    fn a_timeout_terminates_the_child_and_is_distinguished_from_cancellation() {
        let scenario = scenario("probe", "hang\n");
        let options = RunOptions {
            timeout: Some(Duration::from_millis(200)),
            ..RunOptions::default()
        };
        let output = run_collect(&scenario.spec, &options, &CancellationToken::new()).expect("run");
        assert!(output.timed_out);
        assert!(!output.cancelled);
        assert!(output.describe_failure().contains("did not finish in time"));
    }

    #[test]
    fn standard_input_text_is_written_and_then_closed() {
        let scenario = scenario("probe", "stdin echo\nexit 0\n");
        let spec = scenario
            .spec
            .clone()
            .stdin(StdinMode::Text("y\nn\n".to_owned()));
        let output =
            run_collect(&spec, &RunOptions::default(), &CancellationToken::new()).expect("run");
        assert!(output.success);
        assert!(output.stdout.contains("echo=y"), "got {:?}", output.stdout);
        assert!(output.stdout.contains('n'), "got {:?}", output.stdout);
    }

    #[test]
    fn a_command_with_no_output_still_exits_cleanly() {
        let scenario = scenario("probe", "exit 0\n");
        let output = run_collect(
            &scenario.spec,
            &RunOptions::default(),
            &CancellationToken::new(),
        )
        .expect("run");
        assert!(output.success);
        assert!(output.stdout.is_empty());
        assert!(output.combined().is_empty());
    }

    #[test]
    fn preview_quotes_only_when_needed() {
        let spec =
            CommandSpec::new("/usr/bin/emulator").args(["-avd", "Pixel 9", "--flag=value", "it's"]);
        assert_eq!(
            spec.preview(),
            "/usr/bin/emulator -avd 'Pixel 9' --flag=value 'it'\\''s'"
        );
    }

    #[test]
    fn preview_is_never_used_as_an_actual_command() {
        // A name full of shell metacharacters must reach the child verbatim.
        let dir = tempfile::tempdir().expect("tempdir");
        let script = dir.path().join("scenario");
        std::fs::write(&script, "out ok\nexit 0\n").expect("write");
        let spec = CommandSpec::new(fake_tool())
            .arg("; rm -rf /")
            .env("FAKE_TOOL_NAME", "probe")
            .env("FAKE_SCENARIO", &script)
            .env("FAKE_RECORD_DIR", dir.path());

        let output =
            run_collect(&spec, &RunOptions::default(), &CancellationToken::new()).expect("run");
        assert!(output.success);
        let log = std::fs::read_to_string(dir.path().join("probe.log")).expect("log");
        assert!(log.contains("arg1=; rm -rf /"));
    }

    #[test]
    fn executable_detection_rejects_non_executable_files() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().expect("tempdir");
        let file = temp.path().join("plain");
        std::fs::write(&file, "x").expect("write");
        assert!(!is_executable(&file));
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        assert!(is_executable(&file));
        assert!(!is_executable(&temp.path().join("missing")));
    }

    #[test]
    fn the_fake_tool_records_the_cwd_so_tests_cannot_escape_their_tempdir() {
        let scenario = scenario("probe", "out ok\nexit 0\n");
        let cwd = tempfile::tempdir().expect("tempdir");
        let spec = scenario.spec.clone().cwd(cwd.path());
        run_collect(&spec, &RunOptions::default(), &CancellationToken::new()).expect("run");
        let log = record(&scenario, "probe");
        assert!(log.contains(&format!("cwd={}", cwd.path().display())));
    }

    // --------------------------------------------------------- output loss
    //
    // The completion check used to call `receiver.try_recv().is_err()`, which
    // both consumed and discarded a queued chunk and treated a momentarily
    // empty channel as end of output. These tests fail against that code.

    #[test]
    fn output_from_a_child_that_exits_immediately_is_never_lost() {
        // Repeated because the defect was a race: it needed the child to exit
        // between two polls with output still queued.
        let scenario = scenario("probe", "out first\nout second\nexit 0\n");
        for attempt in 0..50 {
            let output = run_collect(
                &scenario.spec,
                &RunOptions::default(),
                &CancellationToken::new(),
            )
            .expect("run");
            assert!(output.success);
            assert_eq!(
                output.stdout, "first\nsecond\n",
                "attempt {attempt} lost output"
            );
        }
    }

    #[test]
    fn output_larger_than_one_read_is_retained_completely() {
        // 200 lines of 70 characters is about 14 KiB, so it crosses the 8 KiB
        // read buffer several times.
        let scenario = scenario("probe", "huge 200\nexit 0\n");
        let output = run_collect(
            &scenario.spec,
            &RunOptions::default(),
            &CancellationToken::new(),
        )
        .expect("run");
        assert!(output.success);
        assert!(!output.output_truncated, "14 KiB should not be truncated");
        assert_eq!(output.stdout.lines().count(), 200);
        assert_eq!(output.stdout.lines().last().map(str::len), Some(70));
    }

    #[test]
    fn output_written_just_before_exit_is_drained_from_the_pipe() {
        // A pipe holds 64 KiB. Writing more than that and exiting immediately
        // leaves output in the pipe that the reader must still collect.
        let scenario = scenario("probe", "huge 1500\nexit 0\n");
        let options = RunOptions {
            max_output_bytes: 4 * 1024 * 1024,
            ..RunOptions::default()
        };
        let output = run_collect(&scenario.spec, &options, &CancellationToken::new()).expect("run");
        assert!(output.success);
        assert!(!output.output_truncated);
        assert_eq!(output.stdout.lines().count(), 1500);
    }

    #[test]
    fn repeated_runs_produce_identical_output() {
        let scenario = scenario("probe", "out alpha\nout beta\nerr gamma\nexit 0\n");
        let first = run_collect(
            &scenario.spec,
            &RunOptions::default(),
            &CancellationToken::new(),
        )
        .expect("run");
        for attempt in 0..20 {
            let next = run_collect(
                &scenario.spec,
                &RunOptions::default(),
                &CancellationToken::new(),
            )
            .expect("run");
            assert_eq!(next.stdout, first.stdout, "attempt {attempt}");
            assert_eq!(next.stderr, first.stderr, "attempt {attempt}");
        }
    }

    // ------------------------------------------------------- trailing output

    #[test]
    fn a_final_line_without_a_newline_is_retained_on_stdout() {
        // The partial is never flushed here, so `finish` is the only thing that
        // can retain the text. The old `finish` cleared it and returned nothing.
        let scenario = scenario("probe", "prompt trailing text\nexit 0\n");
        let options = RunOptions {
            partial_flush_after: Duration::from_secs(30),
            ..RunOptions::default()
        };
        let output = run_collect(&scenario.spec, &options, &CancellationToken::new()).expect("run");
        assert!(output.success);
        assert_eq!(output.stdout, "trailing text\n");
    }

    #[test]
    fn a_final_line_without_a_newline_is_retained_on_stderr() {
        let scenario = scenario("probe", "prompt-err trailing error\nexit 0\n");
        let options = RunOptions {
            partial_flush_after: Duration::from_secs(30),
            ..RunOptions::default()
        };
        let output = run_collect(&scenario.spec, &options, &CancellationToken::new()).expect("run");
        assert!(output.success);
        assert_eq!(output.stderr, "trailing error\n");
        assert!(output.stdout.is_empty());
    }

    #[test]
    fn a_prompt_that_is_later_terminated_is_retained_exactly_once() {
        // A partial is display-only. When the newline finally arrives the text
        // must appear in retained output once, not twice.
        let scenario = scenario("probe", "prompt Do you accept?\nout yes\nexit 0\n");
        let mut partials = 0usize;
        let output = run(
            &scenario.spec,
            &RunOptions {
                partial_flush_after: Duration::from_millis(80),
                ..RunOptions::default()
            },
            &CancellationToken::new(),
            |event| {
                if matches!(event, ProcessEvent::Partial { .. }) {
                    partials += 1;
                }
            },
        )
        .expect("run");
        assert!(output.success);
        assert!(partials >= 1, "the prompt should still reach the callback");
        assert_eq!(
            output.stdout.matches("Do you accept?").count(),
            1,
            "retained output should contain the prompt once: {:?}",
            output.stdout
        );
    }
}
