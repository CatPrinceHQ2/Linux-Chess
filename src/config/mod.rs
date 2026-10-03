//! Persistent configuration (JSON in `$XDG_CONFIG_HOME/linux-chess/config.json`).
//! Loading is forgiving: unknown fields are ignored, missing fields take defaults, and a corrupt
//! file is moved aside rather than crashing the application.
use crate::chess::Position;
use crate::engine::SearchLimit;
use crate::platform;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode {
    #[default]
    Time,
    Depth,
    Nodes,
    Infinite,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineEntry {
    /// Stable identifier used for selection; never shown to the user.
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub args: Vec<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub protocol: String,
    pub enabled: bool,
    pub icon: Option<String>,
    pub description: Option<String>,
    /// True for engines that ship with the application package.
    pub bundled: bool,
    /// User-chosen values of the engine's own options, keyed by the option name the engine declared.
    pub option_values: BTreeMap<String, String>,
}

impl Default for EngineEntry {
    fn default() -> Self {
        EngineEntry {
            id: String::new(),
            name: String::new(),
            path: PathBuf::new(),
            args: Vec::new(),
            version: None,
            author: None,
            protocol: "uci".into(),
            enabled: true,
            icon: None,
            description: None,
            bundled: false,
            option_values: BTreeMap::new(),
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub engines: Vec<EngineEntry>,
    pub selected_engine: Option<String>,
    pub search_mode: SearchMode,
    pub search_time_ms: u64,
    pub search_depth: u32,
    pub search_nodes: u64,
    pub multipv: u32,
    pub last_fen: String,
    pub show_legal_moves: bool,
    pub flip_board: bool,
    pub debug_logging: bool,
    pub window_width: i32,
    pub window_height: i32,
    /// "system" | "light" | "dark"
    pub color_scheme: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            version: 1,
            engines: Vec::new(),
            selected_engine: None,
            search_mode: SearchMode::Time,
            search_time_ms: 5000,
            search_depth: 20,
            search_nodes: 5_000_000,
            multipv: 1,
            last_fen: Position::START_FEN.to_string(),
            show_legal_moves: true,
            flip_board: false,
            debug_logging: false,
            window_width: 1100,
            window_height: 780,
            color_scheme: "system".into(),
        }
    }
}

impl Config {
    pub fn search_limit(&self) -> SearchLimit {
        match self.search_mode {
            SearchMode::Time => SearchLimit::Time { ms: self.search_time_ms.max(1) },
            SearchMode::Depth => SearchLimit::Depth(self.search_depth.max(1)),
            SearchMode::Nodes => SearchLimit::Nodes(self.search_nodes.max(1)),
            SearchMode::Infinite => SearchLimit::Infinite,
        }
    }

    pub fn engine(&self, id: &str) -> Option<&EngineEntry> {
        self.engines.iter().find(|e| e.id == id)
    }

    pub fn engine_mut(&mut self, id: &str) -> Option<&mut EngineEntry> {
        self.engines.iter_mut().find(|e| e.id == id)
    }

    pub fn selected(&self) -> Option<&EngineEntry> {
        self.selected_engine.as_deref().and_then(|id| self.engine(id))
    }

    /// Adds an engine (assigning a unique id) and returns that id.
    pub fn add_engine(&mut self, mut entry: EngineEntry) -> String {
        let base: String = entry
            .name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .trim_matches('-')
            .to_string();
        let base = if base.is_empty() { "engine".to_string() } else { base };
        let mut id = base.clone();
        let mut n = 2;
        while self.engines.iter().any(|e| e.id == id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        entry.id = id.clone();
        self.engines.push(entry);
        if self.selected_engine.is_none() {
            self.selected_engine = Some(id.clone());
        }
        id
    }

    pub fn remove_engine(&mut self, id: &str) {
        self.engines.retain(|e| e.id != id);
        if self.selected_engine.as_deref() == Some(id) {
            self.selected_engine = self.engines.iter().find(|e| e.enabled).map(|e| e.id.clone());
        }
    }

    pub fn has_path(&self, path: &Path) -> bool {
        self.engines.iter().any(|e| e.path == path)
    }

    /// Registers engines that ship with the package or are installed system-wide, *without*
    /// running them. Currently Stockfish is the only default; the list is data, not architecture.
    pub fn add_default_engines(&mut self) {
        const DEFAULTS: [(&str, &str, &str); 1] = [("Stockfish", "stockfish", "Open-source UCI engine (GPLv3).")];
        for (name, program, desc) in DEFAULTS {
            if let Some(path) = platform::locate_program(program) {
                if !self.has_path(&path) {
                    let bundled = platform::bundled_engine_dirs().iter().any(|d| path.starts_with(d.canonicalize().unwrap_or_else(|_| d.clone())));
                    self.add_engine(EngineEntry {
                        name: name.into(),
                        path,
                        description: Some(desc.into()),
                        bundled,
                        ..Default::default()
                    });
                }
            }
        }
    }

    // ------------------------------------------------------------ persistence

    /// Loads from the default location; first run (no file) yields defaults plus any detected
    /// default engines.
    pub fn load() -> Config {
        Self::load_from(&platform::config_file())
    }

    pub fn load_from(path: &Path) -> Config {
        match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<Config>(&text) {
                Ok(c) => c,
                Err(_) => {
                    let backup = path.with_extension("json.bak");
                    let _ = std::fs::rename(path, backup);
                    Config::first_run()
                }
            },
            Err(_) => Config::first_run(),
        }
    }

    fn first_run() -> Config {
        let mut c = Config::default();
        c.add_default_engines();
        c
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&platform::config_file())
    }

    /// Atomic save: write a temporary file next to the target, then rename it into place.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(json.as_bytes())?;
            f.sync_all()?;
        }
        std::fs::rename(tmp, path)
    }
}
