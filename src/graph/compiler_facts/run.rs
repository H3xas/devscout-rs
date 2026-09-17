// The local-acquisition entry point: spawns the engine located by
// `SCOUT_COMPILER_ENGINE`, captures its stdout under a byte cap and a
// wall-clock timeout, and hands the captured bytes to `admit`. No code path
// here writes to the artifact path directly -- only the publish step does,
// and only once `admit` returns `Ok`.
//
// Process-spawn safety: the command line is built entirely from an
// already-validated local path plus caller-supplied flags, never passed
// through a shell (`std::process::Command` invoked directly), matching the
// crate's one other subprocess site, `manifest::git_head`'s own `git`
// shell-out, which avoids a shell too. Bounded output capture keeps an
// unbounded or adversarial engine (or a corrupted build) from exhausting
// memory -- this is itself one of the refusal classes below, not merely a
// hardening afterthought.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::reasons::RefusalReason;

/// The default wall-clock budget a local engine run is given before it is
/// killed and refused as `engine-timeout`. Generous because a Roslyn/MSBuild
/// load is the dominant cost this crate has never measured before; tighten
/// once real cold/warm figures exist.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

/// The default stdout byte cap a local engine run is held to before it is
/// killed and refused as `output-bounded`. 256 MiB comfortably exceeds any
/// fixture this crate ships while still bounding an adversarial or
/// corrupted engine's memory cost.
pub const DEFAULT_OUTPUT_CAP: usize = 256 * 1024 * 1024;

enum ReaderMsg {
    Done(Vec<u8>),
    CapExceeded,
    ReadError,
}

fn spawn_reader(mut stdout: impl Read + Send + 'static, cap: usize) -> mpsc::Receiver<ReaderMsg> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 64 * 1024];
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) => {
                    let _ = tx.send(ReaderMsg::Done(buf));
                    return;
                }
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    if buf.len() > cap {
                        let _ = tx.send(ReaderMsg::CapExceeded);
                        return;
                    }
                }
                Err(_) => {
                    let _ = tx.send(ReaderMsg::ReadError);
                    return;
                }
            }
        }
    });
    rx
}

fn kill_and_wait(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Runs `command` to completion, capturing stdout under `output_cap` bytes
/// and `timeout` wall-clock time. `command`'s stdin is closed and stderr is
/// discarded -- only stdout carries the candidate artifact bytes.
///
/// Returns the captured bytes on a clean, in-budget exit; otherwise one of
/// the three broken-run reasons this function alone can produce
/// (`engine-killed`, `engine-timeout`, `output-bounded`). Never touches the
/// filesystem.
pub fn run_engine(
    mut command: Command,
    timeout: Duration,
    output_cap: usize,
) -> Result<Vec<u8>, RefusalReason> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().map_err(|_| RefusalReason::EngineKilled)?;
    let stdout = child.stdout.take().expect("stdout was piped");
    let rx = spawn_reader(stdout, output_cap);
    let deadline = Instant::now() + timeout;

    loop {
        match rx.recv_timeout(Duration::from_millis(25)) {
            Ok(ReaderMsg::Done(bytes)) => {
                let status = match child.wait() {
                    Ok(s) => s,
                    Err(_) => return Err(RefusalReason::EngineKilled),
                };
                return if status.success() {
                    Ok(bytes)
                } else {
                    Err(RefusalReason::EngineKilled)
                };
            }
            Ok(ReaderMsg::CapExceeded) => {
                kill_and_wait(&mut child);
                return Err(RefusalReason::OutputBounded);
            }
            Ok(ReaderMsg::ReadError) => {
                kill_and_wait(&mut child);
                return Err(RefusalReason::EngineKilled);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if Instant::now() >= deadline {
                    kill_and_wait(&mut child);
                    return Err(RefusalReason::EngineTimeout);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(RefusalReason::EngineKilled);
            }
        }
    }
}
