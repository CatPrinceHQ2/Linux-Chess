//! Bridges the GUI and the engine layer. Everything here is non-blocking: engines are launched
//! on a worker thread, searches are driven by polling, and a crashed engine only produces an
//! error value for the GUI to show.
use crate::chess::Position;
use crate::config::EngineEntry;
use crate::engine::analysis::AnalysisState;
use crate::engine::log::Logger;
use crate::engine::options::find_option;
use crate::engine::uci::{UciEngine, DEFAULT_HANDSHAKE_TIMEOUT};
use crate::engine::*;
use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver, TryRecvError};

struct Launching {
    rx: Receiver<Result<UciEngine, EngineError>>,
    values: BTreeMap<String, String>,
}

pub struct PendingSearch {
    pub position: Position,
    pub limit: SearchLimit,
    /// (option name, value) for MultiPV, present only if the engine declares it.
    pub multipv: Option<(String, i64)>,
}

#[derive(Default)]
pub struct Tick {
    pub launched: bool,
    pub analysis_changed: bool,
    pub finished: bool,
    pub error: Option<EngineError>,
    pub warnings: Vec<String>,
}

pub struct Controller {
    pub logger: Logger,
    engine: Option<Box<dyn ChessEngine>>,
    launching: Option<Launching>,
    pending_search: Option<PendingSearch>,
    pub engine_id: Option<String>,
    pub identity: Option<EngineIdentity>,
    pub options: Vec<EngineOption>,
    pub analysis: Option<AnalysisState>,
}

impl Controller {
    pub fn new(logger: Logger) -> Controller {
        Controller { logger, engine: None, launching: None, pending_search: None, engine_id: None, identity: None, options: Vec::new(), analysis: None }
    }

    pub fn capabilities(&self) -> Capabilities {
        Capabilities::from_options(&self.options)
    }

    pub fn is_launching(&self) -> bool {
        self.launching.is_some()
    }

    pub fn is_ready(&self) -> bool {
        self.engine.as_ref().map(|e| e.state() == EngineState::Idle).unwrap_or(false)
    }

    pub fn is_busy(&self) -> bool {
        self.pending_search.is_some()
            || self.engine.as_ref().map(|e| matches!(e.state(), EngineState::Preparing | EngineState::Searching | EngineState::Stopping)).unwrap_or(false)
    }

    pub fn has_exited(&self) -> bool {
        self.engine.as_ref().map(|e| e.state() == EngineState::Exited).unwrap_or(false)
    }

    /// Stops the current engine (in the background, it may take a moment) and starts `entry`.
    pub fn select(&mut self, entry: &EngineEntry) {
        self.retire_engine();
        self.pending_search = None;
        self.analysis = None;
        self.identity = None;
        self.options.clear();
        self.engine_id = Some(entry.id.clone());
        let spec = EngineSpec { path: entry.path.clone(), args: entry.args.clone() };
        let logger = self.logger.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = UciEngine::launch(spec, logger, DEFAULT_HANDSHAKE_TIMEOUT);
            let _ = tx.send(result);
        });
        self.launching = Some(Launching { rx, values: entry.option_values.clone() });
    }

    /// Stops the running engine (in the background) and forgets it, e.g. after the last engine
    /// was removed from the list. Without this the old process kept running unseen.
    pub fn deselect(&mut self) {
        self.retire_engine();
        self.pending_search = None;
        self.analysis = None;
        self.identity = None;
        self.options.clear();
        self.engine_id = None;
    }

    fn retire_engine(&mut self) {
        if let Some(mut old) = self.engine.take() {
            // Shutting down can take up to a second; never do it on the GUI thread.
            std::thread::spawn(move || old.shutdown());
        }
        self.launching = None;
    }

    pub fn shutdown(&mut self) {
        self.pending_search = None;
        if let Some(mut e) = self.engine.take() {
            e.shutdown();
        }
        self.launching = None;
    }

    pub fn set_option(&mut self, name: &str, value: Option<&str>) -> Result<(), EngineError> {
        match self.engine.as_mut() {
            Some(e) => e.set_option(name, value),
            None => Err(EngineError::InvalidOption("The engine is not running.".into())),
        }
    }

    /// Starts a search; if the engine is still launching the search begins as soon as it is ready.
    pub fn search(&mut self, request: PendingSearch) -> Result<(), EngineError> {
        if self.launching.is_some() {
            self.pending_search = Some(request);
            return Ok(());
        }
        let engine = self
            .engine
            .as_mut()
            .ok_or_else(|| EngineError::InvalidOption("No engine is selected.".into()))?;
        if let Some((name, n)) = &request.multipv {
            engine.set_option(name, Some(&n.to_string()))?;
        }
        engine.start_search(SearchRequest { position: request.position.clone(), limit: request.limit })?;
        self.analysis = Some(AnalysisState::new(request.position));
        Ok(())
    }

    pub fn stop(&mut self) {
        self.pending_search = None;
        if let Some(e) = self.engine.as_mut() {
            let _ = e.stop_search();
        }
    }

    pub fn tick(&mut self) -> Tick {
        let mut t = Tick::default();
        if let Some(l) = &self.launching {
            match l.rx.try_recv() {
                Ok(Ok(engine)) => {
                    let values = l.values.clone();
                    self.launching = None;
                    let mut engine: Box<dyn ChessEngine> = Box::new(engine);
                    self.identity = Some(engine.identity().clone());
                    self.options = engine.options().to_vec();
                    for (name, value) in values {
                        if find_option(&self.options, &name).is_some() {
                            if let Err(e) = engine.set_option(&name, Some(&value)) {
                                t.warnings.push(format!("{name}: {}", e.user_message()));
                            }
                        }
                    }
                    self.engine = Some(engine);
                    t.launched = true;
                    if let Some(p) = self.pending_search.take() {
                        if let Err(e) = self.search(p) {
                            t.error = Some(e);
                        }
                    }
                }
                Ok(Err(e)) => {
                    self.launching = None;
                    self.pending_search = None;
                    t.error = Some(e);
                    t.finished = true;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.launching = None;
                    self.pending_search = None;
                    t.error = Some(EngineError::InvalidResponse("engine start-up thread ended unexpectedly".into()));
                    t.finished = true;
                }
            }
        }
        if let Some(engine) = self.engine.as_mut() {
            for ev in engine.poll() {
                match ev {
                    EngineEvent::Info(i) => {
                        if let Some(a) = self.analysis.as_mut() {
                            a.apply_info(&i);
                            t.analysis_changed = true;
                        }
                    }
                    EngineEvent::BestMove { best, ponder } => {
                        if let Some(a) = self.analysis.as_mut() {
                            a.apply_bestmove(best.as_deref(), ponder.as_deref());
                        }
                        t.analysis_changed = true;
                        t.finished = true;
                    }
                    EngineEvent::SearchCancelled => {
                        if let Some(a) = self.analysis.as_mut() {
                            a.cancelled();
                        }
                        t.finished = true;
                    }
                    EngineEvent::Message(_) => {}
                    EngineEvent::Failed(e) => {
                        if let Some(a) = self.analysis.as_mut() {
                            a.cancelled();
                        }
                        t.error = Some(e);
                        t.finished = true;
                        t.analysis_changed = true;
                    }
                }
            }
        }
        t
    }
}
