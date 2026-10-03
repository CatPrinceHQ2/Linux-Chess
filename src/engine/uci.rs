//! UCI implementation of [`ChessEngine`].
use super::error::EngineError;
use super::log::Logger;
use super::options::{find_option, EngineOption, OptionKind};
use super::parser::{parse_line, UciMessage};
use super::process::{EngineProcess, ProcessEvent};
use super::*;
use std::collections::{BTreeMap, VecDeque};
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::time::{Duration, Instant};

pub const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const READY_TIMEOUT: Duration = Duration::from_secs(15);

pub struct UciEngine {
    spec: EngineSpec,
    process: EngineProcess,
    rx: Receiver<ProcessEvent>,
    identity: EngineIdentity,
    options: Vec<EngineOption>,
    state: EngineState,
    logger: Logger,
    handshake_timeout: Duration,
    pending_search: Option<SearchRequest>,
    pending_options: Vec<(String, Option<String>)>,
    applied_options: BTreeMap<String, String>,
    awaiting_readyok: u32,
    ready_deadline: Option<Instant>,
    shutting_down: bool,
    eof_seen: bool,
    events: VecDeque<EngineEvent>,
}

impl UciEngine {
    /// Launch the executable and perform the UCI handshake (`uci` → `uciok`).
    pub fn launch(spec: EngineSpec, logger: Logger, handshake_timeout: Duration) -> Result<UciEngine, EngineError> {
        logger.engine(format!("Starting {}", spec.path.display()));
        let (mut process, rx) = EngineProcess::spawn(&spec.path, &spec.args)?;
        logger.sent("uci");
        process.send_line("uci").map_err(|e| EngineError::NotUci {
            path: spec.path.clone(),
            detail: format!("could not send 'uci': {}", e.technical_details()),
        })?;

        let mut identity = EngineIdentity::default();
        let mut options = Vec::new();
        let mut noise: Vec<String> = Vec::new();
        let deadline = Instant::now() + handshake_timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(remaining) {
                Ok(ProcessEvent::Line(line)) => {
                    logger.received(&line);
                    match parse_line(&line) {
                        UciMessage::IdName(n) => identity.name = n,
                        UciMessage::IdAuthor(a) => identity.author = Some(a),
                        UciMessage::Option(o) => {
                            if !options.iter().any(|x: &EngineOption| x.name == o.name) {
                                options.push(o)
                            }
                        }
                        UciMessage::UciOk => break,
                        _ => {
                            if noise.len() < 5 {
                                noise.push(line);
                            }
                        }
                    }
                }
                Ok(ProcessEvent::Eof) | Err(RecvTimeoutError::Disconnected) => {
                    let status = process.wait_status(Duration::from_millis(300));
                    let mut detail = format!("the program closed its output (exit status: {status})");
                    append_context(&mut detail, &noise, &process.stderr_tail());
                    return Err(EngineError::NotUci { path: spec.path.clone(), detail });
                }
                Err(RecvTimeoutError::Timeout) => {
                    let mut detail = format!("no 'uciok' received within {} seconds", handshake_timeout.as_secs_f32());
                    append_context(&mut detail, &noise, &process.stderr_tail());
                    return Err(EngineError::NotUci { path: spec.path.clone(), detail });
                }
            }
        }
        if identity.name.trim().is_empty() {
            identity.name = spec.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Unnamed engine".into());
        }
        Ok(UciEngine {
            spec,
            process,
            rx,
            identity,
            options,
            state: EngineState::Idle,
            logger,
            handshake_timeout,
            pending_search: None,
            pending_options: Vec::new(),
            applied_options: BTreeMap::new(),
            awaiting_readyok: 0,
            ready_deadline: None,
            shutting_down: false,
            eof_seen: false,
            events: VecDeque::new(),
        })
    }

    pub fn pid(&self) -> u32 {
        self.process.pid()
    }

    pub fn spec(&self) -> &EngineSpec {
        &self.spec
    }

    pub fn stderr_tail(&self) -> Vec<String> {
        self.process.stderr_tail()
    }

    fn send(&mut self, line: &str) -> Result<(), EngineError> {
        self.logger.sent(line);
        self.process.send_line(line)
    }

    fn send_setoption(&mut self, name: &str, value: Option<&str>) -> Result<(), EngineError> {
        match value {
            Some(v) => {
                let wire = if v.is_empty() { "<empty>" } else { v };
                self.send(&format!("setoption name {name} value {wire}"))?;
                self.applied_options.insert(name.to_string(), v.to_string());
            }
            None => self.send(&format!("setoption name {name}"))?,
        }
        Ok(())
    }

    fn flush_pending_options(&mut self) -> Result<(), EngineError> {
        for (name, value) in std::mem::take(&mut self.pending_options) {
            self.send_setoption(&name, value.as_deref())?;
        }
        Ok(())
    }

    fn handle_message(&mut self, ev: ProcessEvent) {
        match ev {
            ProcessEvent::Line(line) => {
                self.logger.received(&line);
                match parse_line(&line) {
                    UciMessage::ReadyOk => {
                        self.awaiting_readyok = self.awaiting_readyok.saturating_sub(1);
                        if self.awaiting_readyok == 0 && self.state == EngineState::Preparing {
                            self.ready_deadline = None;
                            match self.pending_search.take() {
                                Some(req) => {
                                    if let Err(e) = self.begin_search(&req) {
                                        self.state = EngineState::Idle;
                                        self.events.push_back(EngineEvent::Failed(e));
                                    }
                                }
                                None => self.state = EngineState::Idle,
                            }
                        }
                    }
                    UciMessage::Info(info) => {
                        if matches!(self.state, EngineState::Searching | EngineState::Stopping) {
                            self.events.push_back(EngineEvent::Info(info));
                        }
                    }
                    UciMessage::BestMove { best, ponder } => {
                        if matches!(self.state, EngineState::Searching | EngineState::Stopping) {
                            self.state = EngineState::Idle;
                            if let Err(e) = self.flush_pending_options() {
                                self.events.push_back(EngineEvent::Failed(e));
                            }
                            self.events.push_back(EngineEvent::BestMove { best, ponder });
                        } else {
                            self.logger.engine("ignoring stray bestmove");
                        }
                    }
                    _ => {}
                }
            }
            ProcessEvent::Eof => self.handle_eof(),
        }
    }

    fn begin_search(&mut self, req: &SearchRequest) -> Result<(), EngineError> {
        self.send(&format!("position fen {}", req.position.to_fen()))?;
        let go = match req.limit {
            SearchLimit::Time { ms } => format!("go movetime {ms}"),
            SearchLimit::Depth(d) => format!("go depth {d}"),
            SearchLimit::Nodes(n) => format!("go nodes {n}"),
            SearchLimit::Infinite => "go infinite".to_string(),
        };
        self.send(&go)?;
        self.state = EngineState::Searching;
        Ok(())
    }

    fn handle_eof(&mut self) {
        if self.eof_seen {
            return;
        }
        self.eof_seen = true;
        if self.shutting_down || self.state == EngineState::Exited {
            self.state = EngineState::Exited;
            return;
        }
        let while_searching = matches!(self.state, EngineState::Preparing | EngineState::Searching | EngineState::Stopping);
        let status = self.process.wait_status(Duration::from_millis(500));
        let stderr_tail = self.process.stderr_tail();
        self.logger.engine(format!("process ended ({status})"));
        self.state = EngineState::Exited;
        self.pending_search = None;
        self.events.push_back(EngineEvent::Failed(EngineError::Exited { while_searching, status, stderr_tail }));
    }

    fn check_ready_deadline(&mut self) {
        if let Some(d) = self.ready_deadline {
            if Instant::now() > d && self.state == EngineState::Preparing {
                self.ready_deadline = None;
                self.pending_search = None;
                self.state = EngineState::Idle;
                self.events.push_back(EngineEvent::Failed(EngineError::Timeout { what: "'readyok' after 'isready'" }));
            }
        }
    }

    /// Blocking variant of `poll`, mostly for tests and command-line tools: waits up to `timeout`
    /// for the next event.
    pub fn next_event(&mut self, timeout: Duration) -> Option<EngineEvent> {
        let deadline = Instant::now() + timeout;
        loop {
            self.check_ready_deadline();
            if let Some(e) = self.events.pop_front() {
                return Some(e);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            match self.rx.recv_timeout(remaining.min(Duration::from_millis(50))) {
                Ok(m) => self.handle_message(m),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => self.handle_eof(),
            }
        }
    }
}

fn append_context(detail: &mut String, noise: &[String], stderr: &[String]) {
    if !noise.is_empty() {
        detail.push_str("\nProgram output:\n");
        detail.push_str(&noise.join("\n"));
    }
    if !stderr.is_empty() {
        detail.push_str("\nProgram error output:\n");
        detail.push_str(&stderr.join("\n"));
    }
}

impl ChessEngine for UciEngine {
    fn identity(&self) -> &EngineIdentity {
        &self.identity
    }

    fn options(&self) -> &[EngineOption] {
        &self.options
    }

    fn state(&self) -> EngineState {
        self.state
    }

    fn set_option(&mut self, name: &str, value: Option<&str>) -> Result<(), EngineError> {
        if self.state == EngineState::Exited {
            return Err(EngineError::Exited { while_searching: false, status: "exited".into(), stderr_tail: self.process.stderr_tail() });
        }
        let opt = find_option(&self.options, name)
            .ok_or_else(|| EngineError::InvalidOption(format!("This engine has no option named '{name}'.")))?
            .clone();
        let normalized = match (&opt.kind, value) {
            (OptionKind::Button, _) => None,
            (_, Some(v)) => Some(opt.normalize_value(v).map_err(EngineError::InvalidOption)?),
            (_, None) => return Err(EngineError::InvalidOption(format!("Option '{}' needs a value.", opt.name))),
        };
        if self.state == EngineState::Idle {
            self.send_setoption(&opt.name, normalized.as_deref())
        } else {
            self.pending_options.retain(|(n, _)| n != &opt.name);
            self.pending_options.push((opt.name, normalized));
            Ok(())
        }
    }

    fn new_game(&mut self) -> Result<(), EngineError> {
        if self.state != EngineState::Idle {
            return Err(EngineError::Busy("ucinewgame"));
        }
        self.send("ucinewgame")
    }

    fn start_search(&mut self, request: SearchRequest) -> Result<(), EngineError> {
        match self.state {
            EngineState::Idle => {}
            EngineState::Exited => {
                return Err(EngineError::Exited { while_searching: false, status: "exited".into(), stderr_tail: self.process.stderr_tail() })
            }
            _ => return Err(EngineError::Busy("go")),
        }
        let issues = request.position.validate();
        if !issues.is_empty() {
            let text: Vec<String> = issues.iter().map(|i| i.to_string()).collect();
            return Err(EngineError::InvalidPosition(text.join("\n")));
        }
        self.flush_pending_options()?;
        self.send("isready")?;
        self.awaiting_readyok += 1;
        self.ready_deadline = Some(Instant::now() + READY_TIMEOUT);
        self.pending_search = Some(request);
        self.state = EngineState::Preparing;
        Ok(())
    }

    fn stop_search(&mut self) -> Result<(), EngineError> {
        match self.state {
            EngineState::Searching => {
                self.send("stop")?;
                self.state = EngineState::Stopping;
            }
            EngineState::Preparing => {
                self.pending_search = None;
                self.ready_deadline = None;
                self.state = EngineState::Idle;
                self.events.push_back(EngineEvent::SearchCancelled);
            }
            _ => {}
        }
        Ok(())
    }

    fn poll(&mut self) -> Vec<EngineEvent> {
        loop {
            match self.rx.try_recv() {
                Ok(m) => self.handle_message(m),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.handle_eof();
                    break;
                }
            }
        }
        self.check_ready_deadline();
        self.events.drain(..).collect()
    }

    fn restart(&mut self) -> Result<(), EngineError> {
        self.logger.engine("Restarting engine");
        let applied = std::mem::take(&mut self.applied_options);
        self.shutting_down = true;
        self.process.terminate(Duration::from_millis(500));
        let mut fresh = UciEngine::launch(self.spec.clone(), self.logger.clone(), self.handshake_timeout)?;
        for (name, value) in applied {
            let _ = fresh.set_option(&name, Some(&value));
        }
        *self = fresh;
        Ok(())
    }

    fn shutdown(&mut self) {
        self.shutting_down = true;
        self.logger.engine("Shutting down engine");
        self.process.terminate(Duration::from_secs(1));
        self.state = EngineState::Exited;
    }
}

impl Drop for UciEngine {
    fn drop(&mut self) {
        if self.state != EngineState::Exited {
            self.shutdown();
        }
    }
}

/// Result of probing an executable (used by the Add Engine workflow).
#[derive(Clone, Debug)]
pub struct ProbeResult {
    pub identity: EngineIdentity,
    pub options: Vec<EngineOption>,
}

/// Starts the executable, performs the UCI handshake, reads identity and options, and shuts it
/// down again. Nothing is executed except the single file the user chose.
pub fn probe(spec: EngineSpec, logger: Logger, timeout: Duration) -> Result<ProbeResult, EngineError> {
    let mut engine = UciEngine::launch(spec, logger, timeout)?;
    let result = ProbeResult { identity: engine.identity.clone(), options: engine.options.clone() };
    engine.shutdown();
    Ok(result)
}
