//! Subprocess handling. Engines are started directly (never through a shell) with piped stdio.
//! Two reader threads forward stdout lines / stderr lines so that nothing here ever blocks the
//! caller.
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::error::EngineError;

pub enum ProcessEvent {
    Line(String),
    /// stdout closed: the process has exited (or closed its output).
    Eof,
}

pub struct EngineProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    path: PathBuf,
}

impl EngineProcess {
    pub fn spawn(path: &Path, args: &[String]) -> Result<(EngineProcess, Receiver<ProcessEvent>), EngineError> {
        let mut cmd = Command::new(path);
        cmd.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        // Run the engine from its own directory so relative network/book files resolve.
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty() && d.is_dir()) {
            cmd.current_dir(dir);
        }
        let mut child = cmd
            .spawn()
            .map_err(|source| EngineError::StartFailed { path: path.to_path_buf(), source })?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.split(b'\n') {
                match line {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes).trim_end_matches('\r').to_string();
                        if tx.send(ProcessEvent::Line(text)).is_err() {
                            return;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = tx.send(ProcessEvent::Eof);
        });

        let stderr_tail = Arc::new(Mutex::new(VecDeque::new()));
        let tail = stderr_tail.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).split(b'\n').map_while(Result::ok) {
                if let Ok(mut t) = tail.lock() {
                    if t.len() >= 20 {
                        t.pop_front();
                    }
                    t.push_back(String::from_utf8_lossy(&line).to_string());
                }
            }
        });

        Ok((EngineProcess { child, stdin, stderr_tail, path: path.to_path_buf() }, rx))
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn send_line(&mut self, line: &str) -> Result<(), EngineError> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| EngineError::Io(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "stdin closed")))?;
        // Commands must be single lines: refuse anything that could smuggle a second command.
        if line.contains('\n') || line.contains('\r') {
            return Err(EngineError::InvalidOption("Command contains a line break".into()));
        }
        stdin.write_all(line.as_bytes())?;
        stdin.write_all(b"\n")?;
        stdin.flush()?;
        Ok(())
    }

    pub fn try_wait(&mut self) -> Option<ExitStatus> {
        self.child.try_wait().ok().flatten()
    }

    /// Waits briefly for the process to exit and returns a printable status.
    pub fn wait_status(&mut self, timeout: Duration) -> String {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(s) = self.try_wait() {
                return s.to_string();
            }
            if Instant::now() >= deadline {
                return "still running".into();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn stderr_tail(&self) -> Vec<String> {
        self.stderr_tail.lock().map(|t| t.iter().cloned().collect()).unwrap_or_default()
    }

    /// Ask the engine to quit, then force-kill if it does not comply in time.
    pub fn terminate(&mut self, grace: Duration) {
        if self.try_wait().is_some() {
            return;
        }
        let _ = self.send_line("quit");
        self.stdin = None; // closing stdin also tells well-behaved engines to exit
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline {
            if self.try_wait().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for EngineProcess {
    fn drop(&mut self) {
        self.terminate(Duration::from_millis(300));
    }
}
