use cea::config::*;
use cea::engine::SearchLimit;
use std::path::PathBuf;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cea-cfg-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn roundtrip_preserves_everything() {
    let dir = tmp("roundtrip");
    let file = dir.join("nested/config.json");
    let mut c = Config::default();
    let id = c.add_engine(EngineEntry { name: "My Engine".into(), path: "/home/u/Engines/my-engine".into(), ..Default::default() });
    c.engine_mut(&id).unwrap().option_values.insert("Hash".into(), "4096".into());
    c.engine_mut(&id).unwrap().option_values.insert("Use NNUE".into(), "true".into());
    c.search_mode = SearchMode::Depth;
    c.search_depth = 24;
    c.multipv = 3;
    c.last_fen = "8/8/8/8/8/8/8/K6k b - - 5 40".into();
    c.save_to(&file).unwrap();
    assert_eq!(Config::load_from(&file), c);
    assert!(!file.with_extension("json.tmp").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn engine_ids_are_unique_and_selection_follows_removal() {
    let mut c = Config::default();
    let a = c.add_engine(EngineEntry { name: "Stockfish".into(), ..Default::default() });
    let b = c.add_engine(EngineEntry { name: "Stockfish".into(), ..Default::default() });
    let weird = c.add_engine(EngineEntry { name: "  ***  ".into(), ..Default::default() });
    assert_eq!((a.as_str(), b.as_str(), weird.as_str()), ("stockfish", "stockfish-2", "engine"));
    assert_eq!(c.selected_engine.as_deref(), Some("stockfish"));
    c.remove_engine("stockfish");
    assert_eq!(c.selected_engine.as_deref(), Some("stockfish-2"));
    c.remove_engine("stockfish-2");
    c.remove_engine("engine");
    assert_eq!(c.selected_engine, None);
}

#[test]
fn tolerant_loading_unknown_and_missing_fields() {
    let dir = tmp("tolerant");
    let file = dir.join("config.json");
    std::fs::write(&file, r#"{"search_depth": 33, "future_setting": [1,2,3], "engines":[{"id":"x","name":"X","path":"/bin/x","brand_new_field":true}]}"#).unwrap();
    let c = Config::load_from(&file);
    assert_eq!(c.search_depth, 33);
    assert_eq!(c.search_time_ms, 5000);
    assert_eq!(c.engines[0].protocol, "uci");
    assert!(c.engines[0].enabled);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn corrupt_config_is_backed_up_not_fatal() {
    let dir = tmp("corrupt");
    let file = dir.join("config.json");
    std::fs::write(&file, "{ this is not json").unwrap();
    let c = Config::load_from(&file);
    assert_eq!(c.search_depth, Config::default().search_depth);
    assert!(dir.join("config.json.bak").exists());
    assert_eq!(std::fs::read_to_string(dir.join("config.json.bak")).unwrap(), "{ this is not json");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn search_limit_mapping() {
    let mut c = Config::default();
    c.search_mode = SearchMode::Time;
    c.search_time_ms = 2500;
    assert_eq!(c.search_limit(), SearchLimit::Time { ms: 2500 });
    c.search_mode = SearchMode::Depth;
    c.search_depth = 18;
    assert_eq!(c.search_limit(), SearchLimit::Depth(18));
    c.search_mode = SearchMode::Nodes;
    c.search_nodes = 123;
    assert_eq!(c.search_limit(), SearchLimit::Nodes(123));
    c.search_mode = SearchMode::Infinite;
    assert_eq!(c.search_limit(), SearchLimit::Infinite);
}

#[test]
fn locate_program_finds_executables_without_running_them() {
    assert!(cea::platform::locate_program("sh").is_some());
    assert!(cea::platform::locate_program("definitely-not-installed-xyz").is_none());
    assert_eq!(cea::platform::expand_tilde("/abs/path"), PathBuf::from("/abs/path"));
}
