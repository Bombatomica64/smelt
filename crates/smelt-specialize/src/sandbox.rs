//! Fail-closed hard-sandbox backends and bounded guest process execution.

use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fmt::{self, Write as _},
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crate::SandboxPolicyRecord;

/// Thread handle for one bounded process-output reader.
type OutputReader = thread::JoinHandle<Result<Vec<u8>, io::Error>>;

/// Completion state from the wall-time monitor.
struct WaitOutcome {
    /// Final child status.
    status: ExitStatus,
    /// Whether the monitor killed the child for wall time.
    timed_out: bool,
}

/// Availability of a hard-sandbox backend on the current host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendAvailability {
    /// Backend can enforce its policy.
    Available,
    /// Backend is unavailable and specialization must fail closed.
    Unavailable {
        /// Actionable explanation.
        reason: String,
    },
}

/// One host-runtime invocation before sandbox wrapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxRequest {
    /// Guest interpreter or runtime executable.
    pub executable: PathBuf,
    /// Guest arguments, excluding the executable.
    pub args: Vec<OsString>,
    /// Source working directory visible at the same absolute path in the guest.
    pub current_dir: PathBuf,
    /// Existing scratch directory and the only writable guest root.
    pub scratch_dir: PathBuf,
}

/// Fully prepared sandbox launcher command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedCommand {
    /// Sandbox launcher executable.
    pub program: PathBuf,
    /// Launcher arguments.
    pub args: Vec<OsString>,
    /// Explicit launcher environment.
    pub environment: BTreeMap<OsString, OsString>,
    /// Host working directory for the launcher.
    pub current_dir: PathBuf,
}

/// Hard-sandbox backend interface.
pub trait SandboxBackend: fmt::Debug + Send + Sync {
    /// Stable backend implementation name.
    fn name(&self) -> &'static str;

    /// Reports whether this backend can enforce a hard sandbox.
    fn availability(&self) -> BackendAvailability;

    /// Wraps a guest request in backend-specific isolation.
    ///
    /// # Errors
    ///
    /// Returns an error when request paths are invalid or the policy cannot be
    /// represented by this backend.
    fn prepare(
        &self,
        request: &SandboxRequest,
        policy: &SandboxPolicyRecord,
    ) -> Result<PreparedCommand, SandboxError>;
}

impl<T: SandboxBackend + ?Sized> SandboxBackend for Box<T> {
    fn name(&self) -> &'static str {
        (**self).name()
    }

    fn availability(&self) -> BackendAvailability {
        (**self).availability()
    }

    fn prepare(
        &self,
        request: &SandboxRequest,
        policy: &SandboxPolicyRecord,
    ) -> Result<PreparedCommand, SandboxError> {
        (**self).prepare(request, policy)
    }
}

/// Result of one bounded sandbox process.
#[derive(Debug)]
pub struct SandboxOutput {
    /// Guest exit status.
    pub status: ExitStatus,
    /// Captured standard output.
    pub stdout: Vec<u8>,
    /// Captured standard error.
    pub stderr: Vec<u8>,
    /// Host-observed elapsed wall time.
    pub elapsed: Duration,
}

/// Hard-sandbox preparation or execution failure.
#[derive(Debug)]
pub enum SandboxError {
    /// No supported hard sandbox exists.
    Unavailable {
        /// Selected backend.
        backend: String,
        /// Actionable explanation.
        reason: String,
    },
    /// Request or policy cannot be safely represented.
    InvalidPolicy(String),
    /// Host process operation failed.
    Io(io::Error),
    /// Guest exceeded its wall-clock deadline.
    WallTimeExceeded {
        /// Configured deadline.
        limit: Duration,
    },
    /// Guest exceeded the combined output budget.
    OutputLimitExceeded {
        /// Configured byte budget.
        limit: u64,
    },
    /// A bounded output reader thread failed.
    OutputReader(String),
}

impl fmt::Display for SandboxError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable { backend, reason } => {
                write!(
                    formatter,
                    "hard sandbox '{backend}' is unavailable: {reason}"
                )
            }
            Self::InvalidPolicy(message) => write!(formatter, "invalid sandbox policy: {message}"),
            Self::Io(error) => write!(formatter, "sandbox process failed: {error}"),
            Self::WallTimeExceeded { limit } => {
                write!(
                    formatter,
                    "sandbox exceeded wall-time limit of {} ms",
                    limit.as_millis()
                )
            }
            Self::OutputLimitExceeded { limit } => {
                write!(
                    formatter,
                    "sandbox exceeded combined output limit of {limit} bytes"
                )
            }
            Self::OutputReader(message) => {
                write!(formatter, "sandbox output reader failed: {message}")
            }
        }
    }
}

impl std::error::Error for SandboxError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Self::Io(error) = self {
            Some(error)
        } else {
            None
        }
    }
}

impl From<io::Error> for SandboxError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Bounded process runner using one selected hard-sandbox backend.
#[derive(Debug)]
pub struct SandboxRunner<B> {
    /// Backend used for every invocation.
    backend: B,
}

impl<B: SandboxBackend> SandboxRunner<B> {
    /// Creates a runner for a selected hard-sandbox backend.
    pub const fn new(backend: B) -> Self {
        Self { backend }
    }

    /// Returns the stable selected backend name.
    #[must_use]
    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }

    /// Runs one guest with sanitized environment and bounded output.
    ///
    /// # Errors
    ///
    /// Fails closed when the backend is unavailable, policy paths are unsafe,
    /// process creation fails, or a resource deadline is exceeded.
    pub fn run(
        &self,
        request: &SandboxRequest,
        policy: &SandboxPolicyRecord,
    ) -> Result<SandboxOutput, SandboxError> {
        if policy.backend != self.backend.name() {
            return Err(SandboxError::InvalidPolicy(format!(
                "policy backend '{}' does not match selected backend '{}'",
                policy.backend,
                self.backend.name()
            )));
        }
        validate_request(request, policy)?;
        if let BackendAvailability::Unavailable { reason } = self.backend.availability() {
            return Err(SandboxError::Unavailable {
                backend: self.backend.name().to_owned(),
                reason,
            });
        }
        let prepared = self.backend.prepare(request, policy)?;
        run_prepared(&prepared, policy)
    }
}

/// Linux bubblewrap backend with namespace isolation and `prlimit` resource
/// enforcement.
#[derive(Debug, Clone)]
pub struct LinuxBubblewrapBackend {
    /// Resolved bubblewrap launcher.
    bubblewrap: Option<PathBuf>,
    /// Resolved resource-limit launcher.
    prlimit: Option<PathBuf>,
}

impl LinuxBubblewrapBackend {
    /// Discovers `bwrap` and `prlimit` on `PATH`.
    #[must_use]
    pub fn discover() -> Self {
        Self {
            bubblewrap: find_executable("bwrap"),
            prlimit: find_executable("prlimit"),
        }
    }

    /// Creates a backend from explicit launchers, primarily for hermetic tools
    /// and tests.
    #[must_use]
    pub const fn from_paths(bubblewrap: Option<PathBuf>, prlimit: Option<PathBuf>) -> Self {
        Self {
            bubblewrap,
            prlimit,
        }
    }
}

impl SandboxBackend for LinuxBubblewrapBackend {
    fn name(&self) -> &'static str {
        "linux-bubblewrap"
    }

    fn availability(&self) -> BackendAvailability {
        match (&self.bubblewrap, &self.prlimit) {
            (None, _) => BackendAvailability::Unavailable {
                reason: "install bubblewrap or configure the OCI fallback".to_owned(),
            },
            (_, None) => BackendAvailability::Unavailable {
                reason: "prlimit is required for CPU, memory, and process limits".to_owned(),
            },
            (Some(_), Some(_)) => BackendAvailability::Available,
        }
    }

    fn prepare(
        &self,
        request: &SandboxRequest,
        policy: &SandboxPolicyRecord,
    ) -> Result<PreparedCommand, SandboxError> {
        let bubblewrap = required_launcher(self.bubblewrap.as_deref(), self.name())?;
        let prlimit = required_launcher(self.prlimit.as_deref(), self.name())?;
        let mut args = os_args([
            "--die-with-parent",
            "--new-session",
            "--unshare-all",
            "--cap-drop",
            "ALL",
            "--clearenv",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "--tmpfs",
            "/tmp",
        ]);

        for root in &policy.read_only_roots {
            args.extend(os_args(["--ro-bind", root, root]));
        }
        args.extend([
            OsString::from("--bind"),
            request.scratch_dir.as_os_str().to_owned(),
            request.scratch_dir.as_os_str().to_owned(),
            OsString::from("--chdir"),
            request.current_dir.as_os_str().to_owned(),
        ]);
        for (name, value) in &policy.environment {
            args.extend(os_args(["--setenv", name, value]));
        }
        args.push(prlimit.into_os_string());
        args.extend(resource_limit_args(policy));
        args.push(request.executable.as_os_str().to_owned());
        args.extend(request.args.iter().cloned());

        Ok(PreparedCommand {
            program: bubblewrap,
            args,
            environment: BTreeMap::new(),
            current_dir: request.current_dir.clone(),
        })
    }
}

/// macOS `sandbox-exec` backend.
#[derive(Debug, Clone)]
pub struct MacOsSandboxBackend {
    /// Resolved sandbox launcher.
    sandbox_exec: Option<PathBuf>,
}

impl MacOsSandboxBackend {
    /// Discovers the system sandbox launcher.
    #[must_use]
    pub fn discover() -> Self {
        let path = PathBuf::from("/usr/bin/sandbox-exec");
        Self {
            sandbox_exec: path.is_file().then_some(path),
        }
    }

    /// Creates a backend from an explicit launcher.
    #[must_use]
    pub const fn from_path(sandbox_exec: Option<PathBuf>) -> Self {
        Self { sandbox_exec }
    }
}

impl SandboxBackend for MacOsSandboxBackend {
    fn name(&self) -> &'static str {
        "macos-sandbox"
    }

    fn availability(&self) -> BackendAvailability {
        if self.sandbox_exec.is_some() {
            BackendAvailability::Available
        } else {
            BackendAvailability::Unavailable {
                reason: "/usr/bin/sandbox-exec is unavailable; configure the OCI fallback"
                    .to_owned(),
            }
        }
    }

    fn prepare(
        &self,
        request: &SandboxRequest,
        policy: &SandboxPolicyRecord,
    ) -> Result<PreparedCommand, SandboxError> {
        let launcher = required_launcher(self.sandbox_exec.as_deref(), self.name())?;
        let profile = macos_profile(request, policy)?;
        let mut args = vec![
            OsString::from("-p"),
            OsString::from(profile),
            OsString::from("--"),
        ];
        args.push(request.executable.as_os_str().to_owned());
        args.extend(request.args.iter().cloned());
        Ok(PreparedCommand {
            program: launcher,
            args,
            environment: policy_environment(policy),
            current_dir: request.current_dir.clone(),
        })
    }
}

/// Windows `AppContainer` and Job Object backend using Smelt's trusted native
/// launcher helper.
#[derive(Debug, Clone)]
pub struct WindowsAppContainerBackend {
    /// Resolved native launcher.
    launcher: Option<PathBuf>,
}

impl WindowsAppContainerBackend {
    /// Discovers `smelt-appcontainer-runner` on `PATH`.
    #[must_use]
    pub fn discover() -> Self {
        Self {
            launcher: find_executable("smelt-appcontainer-runner"),
        }
    }

    /// Creates a backend from an explicit native launcher.
    #[must_use]
    pub const fn from_path(launcher: Option<PathBuf>) -> Self {
        Self { launcher }
    }
}

impl SandboxBackend for WindowsAppContainerBackend {
    fn name(&self) -> &'static str {
        "windows-appcontainer-job"
    }

    fn availability(&self) -> BackendAvailability {
        if self.launcher.is_some() {
            BackendAvailability::Available
        } else {
            BackendAvailability::Unavailable {
                reason: "smelt-appcontainer-runner is unavailable; configure the OCI fallback"
                    .to_owned(),
            }
        }
    }

    fn prepare(
        &self,
        request: &SandboxRequest,
        policy: &SandboxPolicyRecord,
    ) -> Result<PreparedCommand, SandboxError> {
        let launcher = required_launcher(self.launcher.as_deref(), self.name())?;
        let mut args = vec![
            OsString::from("--deny-network"),
            OsString::from("--memory-bytes"),
            OsString::from(policy.memory_bytes.to_string()),
            OsString::from("--cpu-ms"),
            OsString::from(policy.cpu_time_ms.to_string()),
            OsString::from("--process-limit"),
            OsString::from(policy.process_limit.to_string()),
            OsString::from("--scratch"),
            request.scratch_dir.as_os_str().to_owned(),
        ];
        for root in &policy.read_only_roots {
            args.extend(os_args(["--read-only", root]));
        }
        args.push(OsString::from("--"));
        args.push(request.executable.as_os_str().to_owned());
        args.extend(request.args.iter().cloned());
        Ok(PreparedCommand {
            program: launcher,
            args,
            environment: policy_environment(policy),
            current_dir: request.current_dir.clone(),
        })
    }
}

/// Configured OCI-container fallback.
#[derive(Debug, Clone)]
pub struct OciSandboxBackend {
    /// Docker/Podman-compatible runtime.
    runtime: PathBuf,
    /// Immutable container image reference.
    image: String,
}

impl OciSandboxBackend {
    /// Configures an OCI runtime and pinned image reference.
    #[must_use]
    pub const fn new(runtime: PathBuf, image: String) -> Self {
        Self { runtime, image }
    }
}

impl SandboxBackend for OciSandboxBackend {
    fn name(&self) -> &'static str {
        "oci-container"
    }

    fn availability(&self) -> BackendAvailability {
        if self.runtime.is_file() {
            BackendAvailability::Available
        } else {
            BackendAvailability::Unavailable {
                reason: format!(
                    "configured OCI runtime '{}' does not exist",
                    self.runtime.display()
                ),
            }
        }
    }

    fn prepare(
        &self,
        request: &SandboxRequest,
        policy: &SandboxPolicyRecord,
    ) -> Result<PreparedCommand, SandboxError> {
        if self.image.trim().is_empty() {
            return Err(SandboxError::InvalidPolicy(
                "OCI fallback requires a pinned image reference".to_owned(),
            ));
        }
        let mut args = os_args([
            "run",
            "--rm",
            "--network",
            "none",
            "--read-only",
            "--cap-drop",
            "ALL",
            "--security-opt",
            "no-new-privileges",
            "--pids-limit",
        ]);
        args.push(OsString::from(policy.process_limit.to_string()));
        args.extend(os_args(["--memory"]));
        args.push(OsString::from(policy.memory_bytes.to_string()));
        for root in &policy.read_only_roots {
            args.extend(os_args(["--mount"]));
            args.push(OsString::from(format!(
                "type=bind,src={root},dst={root},readonly"
            )));
        }
        args.extend(os_args(["--mount"]));
        args.push(OsString::from(format!(
            "type=bind,src={},dst={}",
            request.scratch_dir.display(),
            request.scratch_dir.display()
        )));
        args.extend(os_args(["--workdir"]));
        args.push(request.current_dir.as_os_str().to_owned());
        for (name, value) in &policy.environment {
            args.extend(os_args(["--env"]));
            args.push(OsString::from(format!("{name}={value}")));
        }
        args.push(OsString::from(&self.image));
        args.push(request.executable.as_os_str().to_owned());
        args.extend(request.args.iter().cloned());
        Ok(PreparedCommand {
            program: self.runtime.clone(),
            args,
            environment: BTreeMap::new(),
            current_dir: request.current_dir.clone(),
        })
    }
}

/// Selects the native hard-sandbox backend for the current operating system.
#[must_use]
pub fn default_platform_backend() -> Box<dyn SandboxBackend> {
    if cfg!(target_os = "linux") {
        Box::new(LinuxBubblewrapBackend::discover())
    } else if cfg!(target_os = "macos") {
        Box::new(MacOsSandboxBackend::discover())
    } else if cfg!(target_os = "windows") {
        Box::new(WindowsAppContainerBackend::discover())
    } else {
        Box::new(UnavailablePlatformBackend)
    }
}

/// Unsupported host backend used to preserve fail-closed behavior.
#[derive(Debug)]
struct UnavailablePlatformBackend;

impl SandboxBackend for UnavailablePlatformBackend {
    fn name(&self) -> &'static str {
        "unsupported-platform"
    }

    fn availability(&self) -> BackendAvailability {
        BackendAvailability::Unavailable {
            reason: "no native hard-sandbox backend exists for this operating system".to_owned(),
        }
    }

    fn prepare(
        &self,
        _request: &SandboxRequest,
        _policy: &SandboxPolicyRecord,
    ) -> Result<PreparedCommand, SandboxError> {
        Err(SandboxError::Unavailable {
            backend: self.name().to_owned(),
            reason: "no native hard-sandbox backend exists for this operating system".to_owned(),
        })
    }
}

/// Validates common fail-closed policy invariants.
fn validate_request(
    request: &SandboxRequest,
    policy: &SandboxPolicyRecord,
) -> Result<(), SandboxError> {
    if policy.network {
        return Err(SandboxError::InvalidPolicy(
            "specialization cannot enable network access".to_owned(),
        ));
    }
    if policy.process_limit == 0
        || policy.memory_bytes == 0
        || policy.wall_time_ms == 0
        || policy.cpu_time_ms == 0
        || policy.output_bytes == 0
    {
        return Err(SandboxError::InvalidPolicy(
            "resource limits must all be non-zero".to_owned(),
        ));
    }
    if policy.subprocesses && policy.process_limit == 1 {
        return Err(SandboxError::InvalidPolicy(
            "subprocess-enabled policy requires a process limit above one".to_owned(),
        ));
    }
    if !policy.subprocesses && policy.process_limit != 1 {
        return Err(SandboxError::InvalidPolicy(
            "subprocess-denied policy requires a process limit of one".to_owned(),
        ));
    }
    require_directory(&request.current_dir, "working directory")?;
    require_directory(&request.scratch_dir, "scratch directory")?;
    if !request.executable.is_file() {
        return Err(SandboxError::InvalidPolicy(format!(
            "guest executable '{}' is not a file",
            request.executable.display()
        )));
    }
    let scratch = canonical(&request.scratch_dir)?;
    let writable_roots = policy
        .writable_roots
        .iter()
        .map(PathBuf::from)
        .map(|root| canonical(&root))
        .collect::<Result<Vec<_>, _>>()?;
    if writable_roots != [scratch] {
        return Err(SandboxError::InvalidPolicy(
            "the scratch directory must be the only writable root".to_owned(),
        ));
    }
    for root in &policy.read_only_roots {
        require_existing(Path::new(root), "read-only root")?;
    }
    Ok(())
}

/// Runs a prepared launcher while enforcing wall-time and output limits.
fn run_prepared(
    prepared: &PreparedCommand,
    policy: &SandboxPolicyRecord,
) -> Result<SandboxOutput, SandboxError> {
    let started = Instant::now();
    let mut command = Command::new(&prepared.program);
    command
        .args(&prepared.args)
        .current_dir(&prepared.current_dir)
        .env_clear()
        .envs(&prepared.environment)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let stdout_pipe = child.stdout.take().ok_or_else(|| {
        SandboxError::OutputReader("standard output pipe was not created".to_owned())
    })?;
    let stderr_pipe = child.stderr.take().ok_or_else(|| {
        SandboxError::OutputReader("standard error pipe was not created".to_owned())
    })?;
    let consumed = Arc::new(AtomicU64::new(0));
    let exceeded = Arc::new(AtomicBool::new(false));
    let stdout_reader = spawn_bounded_reader(
        stdout_pipe,
        Arc::clone(&consumed),
        Arc::clone(&exceeded),
        policy.output_bytes,
    );
    let stderr_reader = spawn_bounded_reader(
        stderr_pipe,
        Arc::clone(&consumed),
        Arc::clone(&exceeded),
        policy.output_bytes,
    );
    let wall_limit = Duration::from_millis(policy.wall_time_ms);
    let wait = wait_for_child(&mut child, &exceeded, started, wall_limit)?;
    let stdout = join_reader(stdout_reader)?;
    let stderr = join_reader(stderr_reader)?;
    if wait.timed_out {
        return Err(SandboxError::WallTimeExceeded { limit: wall_limit });
    }
    if exceeded.load(Ordering::Acquire) {
        return Err(SandboxError::OutputLimitExceeded {
            limit: policy.output_bytes,
        });
    }
    Ok(SandboxOutput {
        status: wait.status,
        stdout,
        stderr,
        elapsed: started.elapsed(),
    })
}

/// Waits for a child and kills it on output or wall-time exhaustion.
fn wait_for_child(
    child: &mut std::process::Child,
    output_exceeded: &AtomicBool,
    started: Instant,
    wall_limit: Duration,
) -> Result<WaitOutcome, SandboxError> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(WaitOutcome {
                status,
                timed_out: false,
            });
        }
        if output_exceeded.load(Ordering::Acquire) {
            child.kill()?;
            return Ok(WaitOutcome {
                status: child.wait()?,
                timed_out: false,
            });
        }
        if started.elapsed() >= wall_limit {
            child.kill()?;
            return Ok(WaitOutcome {
                status: child.wait()?,
                timed_out: true,
            });
        }
        thread::sleep(Duration::from_millis(5));
    }
}

/// Starts one reader sharing a combined atomic output budget.
fn spawn_bounded_reader<R: Read + Send + 'static>(
    mut reader: R,
    consumed: Arc<AtomicU64>,
    exceeded: Arc<AtomicBool>,
    limit: u64,
) -> OutputReader {
    thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0_u8; 8192];
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                return Ok(output);
            }
            let read_u64 = u64::try_from(read).unwrap_or(u64::MAX);
            let previous = consumed.fetch_add(read_u64, Ordering::AcqRel);
            let remaining = limit.saturating_sub(previous);
            let accepted_u64 = remaining.min(read_u64);
            let accepted = usize::try_from(accepted_u64).unwrap_or(0);
            output.extend_from_slice(buffer.get(..accepted).unwrap_or_default());
            if accepted < read {
                exceeded.store(true, Ordering::Release);
                return Ok(output);
            }
        }
    })
}

/// Joins one bounded output reader.
fn join_reader(reader: OutputReader) -> Result<Vec<u8>, SandboxError> {
    reader
        .join()
        .map_err(|_panic| SandboxError::OutputReader("reader thread panicked".to_owned()))?
        .map_err(SandboxError::Io)
}

/// Converts policy environment pairs for direct launchers.
fn policy_environment(policy: &SandboxPolicyRecord) -> BTreeMap<OsString, OsString> {
    policy
        .environment
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

/// Builds `prlimit` arguments from millisecond and byte policy values.
fn resource_limit_args(policy: &SandboxPolicyRecord) -> Vec<OsString> {
    let cpu_seconds = policy.cpu_time_ms.div_ceil(1000);
    vec![
        OsString::from(format!("--as={}", policy.memory_bytes)),
        OsString::from(format!("--cpu={cpu_seconds}")),
        OsString::from(format!("--nproc={}", policy.process_limit)),
        OsString::from(format!("--fsize={}", policy.output_bytes)),
        OsString::from("--"),
    ]
}

/// Produces a deny-by-default macOS sandbox profile.
fn macos_profile(
    request: &SandboxRequest,
    policy: &SandboxPolicyRecord,
) -> Result<String, SandboxError> {
    let mut profile =
        String::from("(version 1)\n(deny default)\n(deny network*)\n(allow process-exec)\n");
    for root in &policy.read_only_roots {
        writeln!(
            profile,
            "(allow file-read* (subpath \"{}\"))",
            sandbox_quote(root)?
        )
        .map_err(|error| SandboxError::InvalidPolicy(error.to_string()))?;
    }
    writeln!(
        profile,
        "(allow file-write* (subpath \"{}\"))",
        sandbox_quote(&request.scratch_dir.display().to_string())?
    )
    .map_err(|error| SandboxError::InvalidPolicy(error.to_string()))?;
    Ok(profile)
}

/// Escapes a path for the macOS sandbox profile string grammar.
fn sandbox_quote(value: &str) -> Result<String, SandboxError> {
    if value.contains(['\0', '\n', '\r']) {
        return Err(SandboxError::InvalidPolicy(
            "sandbox paths cannot contain control characters".to_owned(),
        ));
    }
    Ok(value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Finds an executable on the sanitized host `PATH`.
fn find_executable(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|directory| directory.join(name))
            .find(|candidate| candidate.is_file())
    })
}

/// Extracts an available backend launcher.
fn required_launcher(launcher: Option<&Path>, backend: &str) -> Result<PathBuf, SandboxError> {
    launcher
        .map(Path::to_path_buf)
        .ok_or_else(|| SandboxError::Unavailable {
            backend: backend.to_owned(),
            reason: "required launcher is unavailable".to_owned(),
        })
}

/// Validates that a path exists.
fn require_existing(path: &Path, label: &str) -> Result<(), SandboxError> {
    if path.exists() {
        Ok(())
    } else {
        Err(SandboxError::InvalidPolicy(format!(
            "{label} '{}' does not exist",
            path.display()
        )))
    }
}

/// Validates that a path is a directory.
fn require_directory(path: &Path, label: &str) -> Result<(), SandboxError> {
    if path.is_dir() {
        Ok(())
    } else {
        Err(SandboxError::InvalidPolicy(format!(
            "{label} '{}' is not a directory",
            path.display()
        )))
    }
}

/// Canonicalizes a policy path.
fn canonical(path: &Path) -> Result<PathBuf, SandboxError> {
    fs::canonicalize(path).map_err(SandboxError::Io)
}

/// Converts string-like arguments into owned OS strings.
fn os_args<I, S>(values: I) -> Vec<OsString>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    values
        .into_iter()
        .map(|value| value.as_ref().to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a restrictive policy against existing test paths.
    fn policy(scratch: &Path) -> SandboxPolicyRecord {
        SandboxPolicyRecord {
            backend: "test".to_owned(),
            network: false,
            read_only_roots: vec!["/usr".to_owned()],
            writable_roots: vec![scratch.display().to_string()],
            environment: BTreeMap::from([("LANG".to_owned(), "C".to_owned())]),
            wall_time_ms: 1_000,
            cpu_time_ms: 1_000,
            memory_bytes: 64 * 1024 * 1024,
            process_limit: 1,
            output_bytes: 1024,
            subprocesses: false,
            native_extensions: Vec::new(),
        }
    }

    /// Returns a request using existing paths without running it.
    fn request(scratch: &Path) -> SandboxRequest {
        SandboxRequest {
            executable: PathBuf::from("/usr/bin/env"),
            args: Vec::new(),
            current_dir: PathBuf::from("/usr"),
            scratch_dir: scratch.to_path_buf(),
        }
    }

    #[test]
    fn linux_plan_denies_network_and_clears_environment() -> Result<(), String> {
        let scratch = std::env::temp_dir();
        let backend = LinuxBubblewrapBackend::from_paths(
            Some(PathBuf::from("/usr/bin/bwrap")),
            Some(PathBuf::from("/usr/bin/prlimit")),
        );
        let prepared = backend
            .prepare(&request(&scratch), &policy(&scratch))
            .map_err(|error| error.to_string())?;
        let rendered = prepared
            .args
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>();
        if !rendered.iter().any(|arg| arg == "--unshare-all")
            || !rendered.iter().any(|arg| arg == "--clearenv")
            || !rendered.iter().any(|arg| arg == "--nproc=1")
        {
            return Err("Linux command omitted a hard-sandbox control".to_owned());
        }
        Ok(())
    }

    #[test]
    fn unavailable_backend_fails_closed() {
        let backend = LinuxBubblewrapBackend::from_paths(None, None);
        assert!(
            matches!(
                backend.availability(),
                BackendAvailability::Unavailable { .. }
            ),
            "missing launchers must never degrade to an unsandboxed process"
        );
    }

    #[test]
    fn macos_profile_rejects_network_and_limits_writes() -> Result<(), String> {
        let scratch = std::env::temp_dir();
        let profile = macos_profile(&request(&scratch), &policy(&scratch))
            .map_err(|error| error.to_string())?;
        if !profile.contains("(deny network*)") || !profile.contains("(allow file-write*") {
            return Err("macOS profile omitted a mandatory restriction".to_owned());
        }
        Ok(())
    }

    #[test]
    fn network_enabled_policy_is_rejected_before_launch() {
        let scratch = std::env::temp_dir();
        let mut unsafe_policy = policy(&scratch);
        unsafe_policy.network = true;
        assert!(
            matches!(
                validate_request(&request(&scratch), &unsafe_policy),
                Err(SandboxError::InvalidPolicy(_))
            ),
            "specialization must not accept a network-enabled policy"
        );
    }
}
