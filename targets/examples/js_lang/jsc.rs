//! In-process and isolated-process `JavaScriptCore` execution.

use anyhow::{Context, Error, anyhow, bail};
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Read, Write};
#[cfg(target_os = "macos")]
use std::io::{Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const MAX_STDERR_BYTES: usize = 64 * 1024;
const MIN_PROBE_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Clone, Debug)]
pub enum Mode {
    InProcess,
    Process {
        executable: PathBuf,
        arguments: Vec<String>,
    },
}

#[derive(Clone, Debug)]
pub struct Config {
    pub mode: Mode,
    pub timeout: Duration,
    pub crash_directory: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Completed,
    Exception,
    Timeout,
    Crash {
        artifact: PathBuf,
        unique: bool,
        detail: String,
    },
}

#[derive(Debug)]
pub struct Execution {
    pub outcome: Outcome,
    pub elapsed: Duration,
}

pub struct Executor {
    backend: Backend,
}

enum Backend {
    #[cfg(target_os = "macos")]
    InProcess(InProcessExecutor),
    Process(ProcessExecutor),
}

struct ProcessConfig {
    executable: PathBuf,
    arguments: Vec<String>,
    timeout: Duration,
    crash_directory: PathBuf,
}

struct ProcessExecutor {
    config: ProcessConfig,
}

struct Captured {
    bytes: Vec<u8>,
    truncated: bool,
}

impl Executor {
    pub fn new(config: Config) -> Result<Self, Error> {
        if config.timeout.is_zero() {
            bail!("JavaScriptCore timeout must be positive");
        }
        let backend = match config.mode {
            Mode::InProcess => {
                #[cfg(target_os = "macos")]
                {
                    Backend::InProcess(InProcessExecutor::new(
                        config.timeout,
                        &config.crash_directory,
                    )?)
                }
                #[cfg(not(target_os = "macos"))]
                {
                    bail!(
                        "in-process JavaScriptCore is currently supported only on macOS; use --jsc PATH"
                    )
                }
            }
            Mode::Process {
                executable,
                arguments,
            } => Backend::Process(ProcessExecutor::new(ProcessConfig {
                executable,
                arguments,
                timeout: config.timeout,
                crash_directory: config.crash_directory,
            })?),
        };
        Ok(Self { backend })
    }

    pub fn execute(&self, source: &str) -> Result<Execution, Error> {
        match &self.backend {
            #[cfg(target_os = "macos")]
            Backend::InProcess(executor) => executor.execute(source),
            Backend::Process(executor) => executor.execute(source),
        }
    }

    pub fn description(&self) -> String {
        match &self.backend {
            #[cfg(target_os = "macos")]
            Backend::InProcess(executor) => format!(
                "in-process timeout_ms={} last_input={}",
                executor.timeout.as_millis(),
                executor.last_input_path.display()
            ),
            Backend::Process(executor) => format!(
                "process={} args={:?} timeout_ms={} crash_dir={}",
                executor.config.executable.display(),
                executor.config.arguments,
                executor.config.timeout.as_millis(),
                executor.config.crash_directory.display()
            ),
        }
    }
}

impl ProcessExecutor {
    fn new(config: ProcessConfig) -> Result<Self, Error> {
        let executor = Self { config };
        let probe_timeout = executor.config.timeout.max(MIN_PROBE_TIMEOUT);
        match executor.execute_inner("0;", false, probe_timeout)?.outcome {
            Outcome::Completed => Ok(executor),
            Outcome::Exception => {
                bail!("JavaScriptCore probe exited with an exception; check --jsc and --jsc-arg")
            }
            Outcome::Timeout => bail!(
                "JavaScriptCore probe exceeded the {} ms timeout",
                probe_timeout.as_millis()
            ),
            Outcome::Crash { detail, .. } => {
                bail!("JavaScriptCore probe crashed ({detail}); check --jsc and --jsc-arg")
            }
        }
    }

    fn execute(&self, source: &str) -> Result<Execution, Error> {
        self.execute_inner(source, true, self.config.timeout)
    }

    fn execute_inner(
        &self,
        source: &str,
        save_crash: bool,
        timeout: Duration,
    ) -> Result<Execution, Error> {
        let start = Instant::now();
        let deadline = start
            .checked_add(timeout)
            .ok_or_else(|| anyhow!("JavaScriptCore timeout is too large"))?;
        let mut child = Command::new(&self.config.executable)
            .args(&self.config.arguments)
            .arg("-e")
            .arg(source)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| {
                format!(
                    "failed to start JavaScriptCore at {}",
                    self.config.executable.display()
                )
            })?;
        let stderr = child.stderr.take().expect("stderr was piped");
        let reader = thread::spawn(move || capture(stderr));
        let mut timed_out = false;
        let status = loop {
            if let Some(status) = child.try_wait().context("failed to poll JavaScriptCore")? {
                break status;
            }
            let now = Instant::now();
            if now >= deadline {
                timed_out = true;
                child
                    .kill()
                    .context("failed to kill timed-out JavaScriptCore")?;
                break child.wait().context("failed to reap JavaScriptCore")?;
            }
            thread::sleep((deadline - now).min(Duration::from_millis(2)));
        };
        let captured = reader
            .join()
            .map_err(|_| anyhow!("JavaScriptCore stderr reader panicked"))?
            .context("failed to read JavaScriptCore stderr")?;
        let elapsed = start.elapsed();
        if timed_out {
            return Ok(Execution {
                outcome: Outcome::Timeout,
                elapsed,
            });
        }
        if status.success() {
            return Ok(Execution {
                outcome: Outcome::Completed,
                elapsed,
            });
        }
        if !is_crash(status, &captured.bytes) {
            return Ok(Execution {
                outcome: Outcome::Exception,
                elapsed,
            });
        }

        let detail = status_detail(status);
        let (artifact, unique) = if save_crash {
            self.save_crash(source, &detail, &captured)?
        } else {
            (PathBuf::new(), false)
        };
        Ok(Execution {
            outcome: Outcome::Crash {
                artifact,
                unique,
                detail,
            },
            elapsed,
        })
    }

    fn save_crash(
        &self,
        source: &str,
        detail: &str,
        captured: &Captured,
    ) -> Result<(PathBuf, bool), Error> {
        fs::create_dir_all(&self.config.crash_directory).with_context(|| {
            format!(
                "failed to create crash directory {}",
                self.config.crash_directory.display()
            )
        })?;
        let id = source_id(source.as_bytes());
        let source_path = self.config.crash_directory.join(format!("jsc-{id}.js"));
        let unique = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&source_path)
        {
            Ok(mut output) => {
                output.write_all(source.as_bytes()).with_context(|| {
                    format!("failed to write crash input {}", source_path.display())
                })?;
                true
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                let existing = fs::read(&source_path).with_context(|| {
                    format!("failed to read crash input {}", source_path.display())
                })?;
                if existing != source.as_bytes() {
                    bail!("crash artifact hash collision at {}", source_path.display());
                }
                false
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to create crash input {}", source_path.display())
                });
            }
        };

        let report_path = self.config.crash_directory.join(format!("jsc-{id}.txt"));
        if unique || !report_path.exists() {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&report_path)
            {
                Ok(mut report) => {
                    writeln!(report, "executable={}", self.config.executable.display())?;
                    writeln!(report, "arguments={:?}", self.config.arguments)?;
                    writeln!(report, "status={detail}")?;
                    writeln!(report, "stderr_truncated={}", captured.truncated)?;
                    writeln!(report, "stderr:")?;
                    report.write_all(&captured.bytes)?;
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("failed to create {}", report_path.display()));
                }
            }
        }
        Ok((source_path, unique))
    }
}

#[cfg(target_os = "macos")]
struct InProcessExecutor {
    group: JSContextGroupRef,
    timeout: Duration,
    last_input: std::fs::File,
    last_input_path: PathBuf,
}

#[cfg(target_os = "macos")]
type JSContextGroupRef = *const std::ffi::c_void;
#[cfg(target_os = "macos")]
type JSContextRef = *const std::ffi::c_void;
#[cfg(target_os = "macos")]
type JSGlobalContextRef = *const std::ffi::c_void;
#[cfg(target_os = "macos")]
type JSStringRef = *const std::ffi::c_void;
#[cfg(target_os = "macos")]
type JSValueRef = *const std::ffi::c_void;
#[cfg(target_os = "macos")]
type JSObjectRef = *const std::ffi::c_void;

#[cfg(target_os = "macos")]
#[link(name = "JavaScriptCore", kind = "framework")]
unsafe extern "C" {
    fn JSContextGroupCreate() -> JSContextGroupRef;
    fn JSContextGroupRelease(group: JSContextGroupRef);
    fn JSContextGroupSetExecutionTimeLimit(
        group: JSContextGroupRef,
        limit: f64,
        callback: Option<unsafe extern "C" fn(JSContextRef, *mut std::ffi::c_void) -> bool>,
        context: *mut std::ffi::c_void,
    );
    fn JSContextGroupClearExecutionTimeLimit(group: JSContextGroupRef);
    fn JSGlobalContextCreateInGroup(
        group: JSContextGroupRef,
        global_object_class: *const std::ffi::c_void,
    ) -> JSGlobalContextRef;
    fn JSGlobalContextRelease(context: JSGlobalContextRef);
    fn JSStringCreateWithCharacters(characters: *const u16, count: usize) -> JSStringRef;
    fn JSStringRelease(string: JSStringRef);
    fn JSEvaluateScript(
        context: JSContextRef,
        script: JSStringRef,
        this_object: JSObjectRef,
        source_url: JSStringRef,
        starting_line_number: i32,
        exception: *mut JSValueRef,
    ) -> JSValueRef;
}

#[cfg(target_os = "macos")]
impl InProcessExecutor {
    fn new(timeout: Duration, crash_directory: &Path) -> Result<Self, Error> {
        fs::create_dir_all(crash_directory).with_context(|| {
            format!(
                "failed to create crash directory {}",
                crash_directory.display()
            )
        })?;
        let last_input_path = crash_directory.join("last-input.js");
        let last_input = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&last_input_path)
            .with_context(|| format!("failed to open {}", last_input_path.display()))?;
        let group = unsafe { JSContextGroupCreate() };
        if group.is_null() {
            bail!("failed to create an in-process JavaScriptCore context group");
        }
        let executor = Self {
            group,
            timeout,
            last_input,
            last_input_path,
        };
        match executor.execute_inner("0;")?.outcome {
            Outcome::Completed => Ok(executor),
            Outcome::Exception => bail!("in-process JavaScriptCore probe threw an exception"),
            Outcome::Timeout => bail!("in-process JavaScriptCore probe timed out"),
            Outcome::Crash { .. } => unreachable!("native crashes terminate the harness"),
        }
    }

    fn execute(&self, source: &str) -> Result<Execution, Error> {
        self.record_last_input(source)?;
        self.execute_inner(source)
    }

    fn record_last_input(&self, source: &str) -> Result<(), Error> {
        let mut file = &self.last_input;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(source.as_bytes())?;
        file.set_len(source.len() as u64)?;
        Ok(())
    }

    fn execute_inner(&self, source: &str) -> Result<Execution, Error> {
        use std::sync::atomic::{AtomicBool, Ordering};

        unsafe extern "C" fn terminate(
            _context: JSContextRef,
            data: *mut std::ffi::c_void,
        ) -> bool {
            let timed_out = data.cast::<AtomicBool>();
            unsafe { (*timed_out).store(true, Ordering::Relaxed) };
            true
        }

        let start = Instant::now();
        let characters = source.encode_utf16().collect::<Vec<_>>();
        let context = unsafe { JSGlobalContextCreateInGroup(self.group, std::ptr::null()) };
        if context.is_null() {
            bail!("failed to create an in-process JavaScriptCore global context");
        }
        let script = unsafe { JSStringCreateWithCharacters(characters.as_ptr(), characters.len()) };
        if script.is_null() {
            unsafe { JSGlobalContextRelease(context) };
            bail!("failed to create an in-process JavaScriptCore source string");
        }
        let timed_out = AtomicBool::new(false);
        unsafe {
            JSContextGroupSetExecutionTimeLimit(
                self.group,
                self.timeout.as_secs_f64(),
                Some(terminate),
                std::ptr::from_ref(&timed_out).cast_mut().cast(),
            );
        }
        let mut exception: JSValueRef = std::ptr::null();
        unsafe {
            JSEvaluateScript(
                context,
                script,
                std::ptr::null(),
                std::ptr::null(),
                1,
                &raw mut exception,
            );
            JSContextGroupClearExecutionTimeLimit(self.group);
            JSStringRelease(script);
            JSGlobalContextRelease(context);
        }
        let outcome = if timed_out.load(Ordering::Relaxed) {
            Outcome::Timeout
        } else if exception.is_null() {
            Outcome::Completed
        } else {
            Outcome::Exception
        };
        Ok(Execution {
            outcome,
            elapsed: start.elapsed(),
        })
    }
}

#[cfg(target_os = "macos")]
impl Drop for InProcessExecutor {
    fn drop(&mut self) {
        unsafe { JSContextGroupRelease(self.group) };
    }
}

fn capture(mut input: impl Read) -> std::io::Result<Captured> {
    let mut bytes = Vec::new();
    let mut truncated = false;
    let mut chunk = [0u8; 4096];
    loop {
        let read = input.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        let keep = read.min(MAX_STDERR_BYTES.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&chunk[..keep]);
        truncated |= keep < read;
    }
    Ok(Captured { bytes, truncated })
}

fn is_crash(status: ExitStatus, stderr: &[u8]) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if status.signal().is_some() {
            return true;
        }
    }
    if matches!(status.code(), Some(132 | 134 | 135 | 136 | 137 | 138 | 139)) {
        return true;
    }
    let stderr = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    [
        "addresssanitizer",
        "undefinedbehaviorsanitizer",
        "memorysanitizer",
        "threadsanitizer",
        "leaksanitizer",
        "runtime error:",
        "assertion failed",
        "release_assert",
        "wtfcrash",
        "segmentation fault",
    ]
    .iter()
    .any(|marker| stderr.contains(marker))
}

fn status_detail(status: ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return format!("signal {signal}");
        }
    }
    status.code().map_or_else(
        || "unknown termination".to_owned(),
        |code| format!("exit {code}"),
    )
}

fn source_id(source: &[u8]) -> String {
    fn fnv(source: &[u8], mut hash: u64) -> u64 {
        for byte in source {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        hash
    }

    format!(
        "{:016x}{:016x}",
        fnv(source, 0xcbf2_9ce4_8422_2325),
        fnv(source, 0x8422_2325_cbf2_9ce4)
    )
}

pub fn default_executable() -> PathBuf {
    if let Some(path) = std::env::var_os("JSC") {
        return path.into();
    }
    let system =
        Path::new("/System/Library/Frameworks/JavaScriptCore.framework/Versions/A/Helpers/jsc");
    if system.is_file() {
        return system.to_owned();
    }
    OsStr::new("jsc").into()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn executor() -> (Executor, PathBuf) {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "fandango-jsc-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let script = r#"
case "$2" in
  ok|"0;") exit 0 ;;
  exception) echo "Exception: test" >&2; exit 3 ;;
  sanitizer) echo "ERROR: AddressSanitizer: test" >&2; exit 1 ;;
  crash) ulimit -c 0; kill -ABRT $$ ;;
  *) while :; do :; done ;;
esac
"#;
        let executor = Executor::new(Config {
            mode: Mode::Process {
                executable: "/bin/sh".into(),
                arguments: vec!["-c".into(), script.into(), "fake-jsc".into()],
            },
            timeout: Duration::from_millis(250),
            crash_directory: directory.clone(),
        })
        .unwrap();
        (executor, directory)
    }

    #[test]
    fn classifies_completion_exception_timeout_and_crash() {
        let (executor, directory) = executor();
        assert_eq!(executor.execute("ok").unwrap().outcome, Outcome::Completed);
        assert_eq!(
            executor.execute("exception").unwrap().outcome,
            Outcome::Exception
        );
        assert_eq!(executor.execute("loop").unwrap().outcome, Outcome::Timeout);
        let first = executor.execute("crash").unwrap().outcome;
        let Outcome::Crash {
            artifact, unique, ..
        } = first
        else {
            panic!("expected crash")
        };
        assert!(unique);
        assert_eq!(fs::read_to_string(&artifact).unwrap(), "crash");
        assert!(artifact.with_extension("txt").is_file());
        let Outcome::Crash { unique, .. } = executor.execute("crash").unwrap().outcome else {
            panic!("expected duplicate crash")
        };
        assert!(!unique);
        let Outcome::Crash { unique, .. } = executor.execute("sanitizer").unwrap().outcome else {
            panic!("expected sanitizer crash")
        };
        assert!(unique);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn caps_diagnostic_output() {
        let captured = capture(&vec![b'x'; MAX_STDERR_BYTES + 1][..]).unwrap();
        assert_eq!(captured.bytes.len(), MAX_STDERR_BYTES);
        assert!(captured.truncated);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn executes_in_process_with_fresh_globals_and_a_watchdog() {
        let directory = std::env::temp_dir().join(format!(
            "fandango-jsc-in-process-test-{}",
            std::process::id()
        ));
        let executor = Executor::new(Config {
            mode: Mode::InProcess,
            timeout: Duration::from_millis(30),
            crash_directory: directory.clone(),
        })
        .unwrap();
        assert_eq!(executor.execute("").unwrap().outcome, Outcome::Completed);
        assert_eq!(
            executor.execute("let onlyHere = 1;").unwrap().outcome,
            Outcome::Completed
        );
        assert_eq!(
            executor.execute("onlyHere;").unwrap().outcome,
            Outcome::Exception
        );
        assert_eq!(
            executor.execute("while (true) {}").unwrap().outcome,
            Outcome::Timeout
        );
        assert_eq!(
            fs::read_to_string(directory.join("last-input.js")).unwrap(),
            "while (true) {}"
        );
        drop(executor);
        fs::remove_dir_all(directory).unwrap();
    }
}
