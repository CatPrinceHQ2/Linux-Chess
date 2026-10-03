//! Optional protocol logging. Disabled by default; when enabled every line is forwarded to a sink
//! (the GUI keeps them in a buffer; setting CEA_DEBUG=1 also mirrors them to stderr).
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const MAX_LINES: usize = 2000;

#[derive(Clone)]
pub struct Logger {
    enabled: Arc<AtomicBool>,
    buffer: Arc<Mutex<VecDeque<String>>>,
    mirror_stderr: bool,
}

impl Logger {
    pub fn new(enabled: bool) -> Logger {
        let mirror = std::env::var_os("CEA_DEBUG").is_some();
        Logger {
            enabled: Arc::new(AtomicBool::new(enabled || mirror)),
            buffer: Arc::new(Mutex::new(VecDeque::new())),
            mirror_stderr: mirror,
        }
    }

    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn log(&self, line: impl AsRef<str>) {
        if !self.is_enabled() {
            return;
        }
        let line = line.as_ref();
        if self.mirror_stderr {
            eprintln!("{line}");
        }
        if let Ok(mut b) = self.buffer.lock() {
            if b.len() >= MAX_LINES {
                b.pop_front();
            }
            b.push_back(line.to_string());
        }
    }

    pub fn sent(&self, line: &str) {
        self.log(format!("[UCI →] {line}"));
    }
    pub fn received(&self, line: &str) {
        self.log(format!("[UCI ←] {line}"));
    }
    pub fn engine(&self, line: impl AsRef<str>) {
        self.log(format!("[Engine] {}", line.as_ref()));
    }

    pub fn snapshot(&self) -> Vec<String> {
        self.buffer.lock().map(|b| b.iter().cloned().collect()).unwrap_or_default()
    }

    pub fn clear(&self) {
        if let Ok(mut b) = self.buffer.lock() {
            b.clear();
        }
    }
}

impl Default for Logger {
    fn default() -> Self {
        Logger::new(false)
    }
}
