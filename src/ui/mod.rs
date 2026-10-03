//! GTK4 / libadwaita front-end.
pub mod board;
pub mod controller;
mod dialogs;

use crate::chess::*;
use crate::config::{Config, SearchMode};
use crate::engine::analysis::{format_count, format_score, format_time, numbered_pv};
use crate::engine::log::Logger;
use crate::engine::EngineError;
use crate::platform;
use adw::prelude::*;
use board::{BoardView, Tool};
use controller::{Controller, PendingSearch};
use gtk::glib;
use gtk4 as gtk;
use libadwaita as adw;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

pub struct App {
    pub win: adw::ApplicationWindow,
    pub cfg: RefCell<Config>,
    pub board: BoardView,
    pub ctl: RefCell<Controller>,
    pub updating: Cell<bool>,
    pub last_error: RefCell<Option<EngineError>>,
    engine_ids: RefCell<Vec<String>>,
    // engine + analysis
    engine_row: adw::ComboRow,
    engine_model: gtk::StringList,
    mode_row: adw::ComboRow,
    limit_row: adw::SpinRow,
    multipv_row: adw::SpinRow,
    resources: adw::ExpanderRow,
    threads_row: adw::SpinRow,
    hash_row: adw::SpinRow,
    go_btn: gtk::Button,
    stop_btn: gtk::Button,
    status: gtk::Label,
    // results
    best_label: gtk::Label,
    uci_label: gtk::Label,
    eval_label: gtk::Label,
    stats_label: gtk::Label,
    lines: gtk::ListBox,
    play_btn: gtk::Button,
    // position editor
    side_row: adw::ComboRow,
    castle: [gtk::CheckButton; 4],
    ep_row: adw::EntryRow,
    half_row: adw::SpinRow,
    full_row: adw::SpinRow,
    fen_entry: gtk::Entry,
    fen_status: gtk::Label,
    turn_label: gtk::Label,
    banner: adw::Banner,
}

pub fn run() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(platform::APP_ID).build();
    app.connect_activate(build_ui);
    app.run()
}

fn spin_row(title: &str, min: f64, max: f64, step: f64, digits: u32) -> adw::SpinRow {
    let adj = gtk::Adjustment::new(min, min, max, step, step * 10.0, 0.0);
    let row = adw::SpinRow::new(Some(&adj), step, digits);
    row.set_title(title);
    row
}

fn build_ui(application: &adw::Application) {
    let cfg = Config::load();
    let logger = Logger::new(cfg.debug_logging);
    let board = BoardView::new();
    board.set_flipped(cfg.flip_board);
    board.set_show_legal(cfg.show_legal_moves);

    // ---- sidebar widgets
    let engine_model = gtk::StringList::new(&[]);
    let engine_row = adw::ComboRow::builder().title("Engine").model(&engine_model).build();
    let mode_row = adw::ComboRow::builder().title("Analysis limit").model(&gtk::StringList::new(&["Time", "Depth", "Nodes", "Infinite"])).build();
    let limit_row = spin_row("Seconds", 0.1, 3600.0, 0.5, 1);
    let multipv_row = spin_row("Lines (MultiPV)", 1.0, 500.0, 1.0, 0);
    let resources = adw::ExpanderRow::builder().title("Engine options").subtitle("Lines, threads, memory — only what this engine supports").build();
    let threads_row = spin_row("Threads", 1.0, 1024.0, 1.0, 0);
    let hash_row = spin_row("Hash (MB)", 1.0, 1048576.0, 16.0, 0);
    resources.add_row(&multipv_row);
    resources.add_row(&threads_row);
    resources.add_row(&hash_row);

    let go_btn = gtk::Button::with_label("SEE NEXT MOVE");
    go_btn.add_css_class("suggested-action");
    go_btn.add_css_class("pill");
    go_btn.set_hexpand(true);
    go_btn.set_height_request(52);
    let stop_btn = gtk::Button::with_label("Stop");
    stop_btn.add_css_class("destructive-action");
    stop_btn.add_css_class("pill");
    stop_btn.set_sensitive(false);
    let status = gtk::Label::builder().xalign(0.0).css_classes(["dim-label"]).wrap(true).label("Ready").build();

    let best_label = gtk::Label::builder().xalign(0.0).label("—").css_classes(["title-1"]).selectable(true).build();
    let uci_label = gtk::Label::builder().xalign(0.0).css_classes(["dim-label", "monospace"]).selectable(true).build();
    let eval_label = gtk::Label::builder().xalign(0.0).label("").build();
    let stats_label = gtk::Label::builder().xalign(0.0).wrap(true).css_classes(["dim-label"]).build();
    let lines = gtk::ListBox::new();
    lines.add_css_class("boxed-list");
    lines.set_selection_mode(gtk::SelectionMode::None);
    let play_btn = gtk::Button::with_label("Play this move");
    play_btn.set_sensitive(false);

    let side_row = adw::ComboRow::builder().title("Side to move").model(&gtk::StringList::new(&["White", "Black"])).build();
    let castle = [
        gtk::CheckButton::with_label("K"),
        gtk::CheckButton::with_label("Q"),
        gtk::CheckButton::with_label("k"),
        gtk::CheckButton::with_label("q"),
    ];
    let ep_row = adw::EntryRow::builder().title("En-passant square (e.g. e3, or empty)").build();
    let half_row = spin_row("Halfmove clock", 0.0, 999.0, 1.0, 0);
    let full_row = spin_row("Fullmove number", 1.0, 9999.0, 1.0, 0);
    let fen_entry = gtk::Entry::builder().hexpand(true).css_classes(["monospace"]).placeholder_text("FEN").build();
    let fen_status = gtk::Label::builder().xalign(0.0).wrap(true).build();
    let turn_label = gtk::Label::builder().xalign(0.0).build();
    let banner = adw::Banner::builder().button_label("Details").revealed(false).build();

    let win = adw::ApplicationWindow::builder()
        .application(application)
        .title("Prince's Linux-Chess")
        .default_width(cfg.window_width)
        .default_height(cfg.window_height)
        .build();

    let app = Rc::new(App {
        win: win.clone(),
        cfg: RefCell::new(cfg),
        board: board.clone(),
        ctl: RefCell::new(Controller::new(logger)),
        updating: Cell::new(false),
        last_error: RefCell::new(None),
        engine_ids: RefCell::new(Vec::new()),
        engine_row,
        engine_model,
        mode_row,
        limit_row,
        multipv_row,
        resources,
        threads_row,
        hash_row,
        go_btn,
        stop_btn,
        status,
        best_label,
        uci_label,
        eval_label,
        stats_label,
        lines,
        play_btn,
        side_row,
        castle,
        ep_row,
        half_row,
        full_row,
        fen_entry,
        fen_status,
        turn_label,
        banner,
    });

    layout(&app, application);
    wire(&app);

    // Restore state.
    let fen = app.cfg.borrow().last_fen.clone();
    let pos = Position::from_fen(&fen).unwrap_or_else(|_| Position::starting());
    app.board.set_position(pos, None);
    app.refresh_engine_list();
    app.sync_limit_widgets();
    app.sync_position_widgets();
    app.start_selected_engine();

    let tick_app = app.clone();
    glib::timeout_add_local(Duration::from_millis(40), move || {
        tick_app.tick();
        glib::ControlFlow::Continue
    });
    win.present();
}

fn heading(text: &str) -> gtk::Label {
    gtk::Label::builder().label(text).xalign(0.0).css_classes(["heading"]).margin_top(6).build()
}

fn layout(app: &Rc<App>, application: &adw::Application) {
    // ---- header with menu
    let header = adw::HeaderBar::new();
    let menu = gtk::gio::Menu::new();
    menu.append(Some("Engines…"), Some("win.engines"));
    menu.append(Some("Engine settings…"), Some("win.advanced"));
    let view = gtk::gio::Menu::new();
    view.append(Some("Flip board"), Some("win.flip"));
    view.append(Some("Show legal moves"), Some("win.legal"));
    view.append(Some("Dark appearance"), Some("win.dark"));
    menu.append_section(Some("View"), &view);
    let dbg = gtk::gio::Menu::new();
    dbg.append(Some("Debug logging"), Some("win.debug"));
    dbg.append(Some("Show protocol log…"), Some("win.log"));
    dbg.append(Some("About"), Some("win.about"));
    menu.append_section(None, &dbg);
    let menu_btn = gtk::MenuButton::builder().icon_name("open-menu-symbolic").menu_model(&menu).tooltip_text("Settings").build();
    header.pack_end(&menu_btn);

    // ---- board column
    let aspect = gtk::AspectFrame::new(0.5, 0.5, 1.0, false);
    aspect.set_child(Some(app.board.widget()));
    aspect.set_hexpand(true);
    aspect.set_vexpand(true);
    let palette = build_palette(app);
    let board_col = gtk::Box::new(gtk::Orientation::Vertical, 8);
    board_col.set_margin_start(12);
    board_col.set_margin_end(6);
    board_col.set_margin_top(6);
    board_col.set_margin_bottom(6);
    board_col.append(&app.turn_label);
    board_col.append(&aspect);
    board_col.append(&palette);
    board_col.set_hexpand(true);

    // ---- sidebar
    let side = gtk::Box::new(gtk::Orientation::Vertical, 10);
    side.set_margin_end(12);
    side.set_margin_start(6);
    side.set_margin_top(6);
    side.set_margin_bottom(6);

    let engine_group = adw::PreferencesGroup::new();
    engine_group.set_title("Engine");
    engine_group.add(&app.engine_row);
    let engine_btns = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    for (label, action) in [("Settings…", "win.advanced"), ("Engines…", "win.engines"), ("Restart", "win.restart")] {
        let b = gtk::Button::with_label(label);
        b.set_action_name(Some(action));
        b.set_hexpand(true);
        engine_btns.append(&b);
    }
    engine_group.add(&engine_btns);
    side.append(&engine_group);

    let analysis = adw::PreferencesGroup::new();
    analysis.set_title("Analysis");
    analysis.add(&app.mode_row);
    analysis.add(&app.limit_row);
    analysis.add(&app.resources);
    side.append(&analysis);

    let go_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    go_box.append(&app.go_btn);
    go_box.append(&app.stop_btn);
    side.append(&go_box);
    side.append(&app.status);

    side.append(&heading("Best move"));
    side.append(&app.best_label);
    side.append(&app.uci_label);
    side.append(&app.eval_label);
    side.append(&app.stats_label);
    side.append(&app.play_btn);
    side.append(&heading("Principal variation"));
    side.append(&app.lines);

    let pos_group = adw::PreferencesGroup::new();
    pos_group.set_title("Position");
    let pos_exp = adw::ExpanderRow::builder().title("Edit position").subtitle("Side to move, castling, en passant, move counters").build();
    pos_exp.add_row(&app.side_row);
    let castle_row = adw::ActionRow::builder().title("Castling rights").build();
    let cbox = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    cbox.set_valign(gtk::Align::Center);
    for c in &app.castle {
        cbox.append(c);
    }
    castle_row.add_suffix(&cbox);
    pos_exp.add_row(&castle_row);
    pos_exp.add_row(&app.ep_row);
    pos_exp.add_row(&app.half_row);
    pos_exp.add_row(&app.full_row);
    pos_group.add(&pos_exp);
    side.append(&pos_group);
    side.append(&app.fen_status);

    let scroll = gtk::ScrolledWindow::builder().child(&side).hscrollbar_policy(gtk::PolicyType::Never).width_request(400).build();

    let main = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    main.append(&board_col);
    main.append(&scroll);

    // ---- bottom FEN bar
    let fen_bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    fen_bar.set_margin_start(12);
    fen_bar.set_margin_end(12);
    fen_bar.set_margin_top(6);
    fen_bar.set_margin_bottom(6);
    fen_bar.append(&gtk::Label::new(Some("FEN")));
    fen_bar.append(&app.fen_entry);
    for (label, action) in [("Load FEN", "win.load-fen"), ("Copy FEN", "win.copy-fen"), ("Start position", "win.start"), ("Clear", "win.clear")] {
        let b = gtk::Button::with_label(label);
        b.set_action_name(Some(action));
        fen_bar.append(&b);
    }

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.add_top_bar(&app.banner);
    toolbar.set_content(Some(&main));
    toolbar.add_bottom_bar(&fen_bar);
    app.win.set_content(Some(&toolbar));

    application.set_accels_for_action("win.analyze", &["<primary>Return"]);
    application.set_accels_for_action("win.stop", &["Escape"]);
    application.set_accels_for_action("win.flip", &["<primary>f"]);
}

fn build_palette(app: &Rc<App>) -> gtk::Box {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    bar.set_halign(gtk::Align::Center);
    let move_btn = gtk::ToggleButton::with_label("Move");
    move_btn.set_active(true);
    move_btn.set_tooltip_text(Some("Drag pieces. A legal move is played; Shift+drag edits freely. Drop off the board to remove."));
    {
        let board = app.board.clone();
        move_btn.connect_toggled(move |b| {
            if b.is_active() {
                board.set_tool(Tool::Move)
            }
        });
    }
    bar.append(&move_btn);
    for color in [Color::White, Color::Black] {
        for kind in [PieceKind::King, PieceKind::Queen, PieceKind::Rook, PieceKind::Bishop, PieceKind::Knight, PieceKind::Pawn] {
            let piece = Piece::new(color, kind);
            let btn = gtk::ToggleButton::new();
            btn.set_group(Some(&move_btn));
            let area = gtk::DrawingArea::new();
            area.set_content_width(34);
            area.set_content_height(34);
            area.set_draw_func(move |_, cr, w, h| board::draw_piece(cr, piece, w as f64 / 2.0, h as f64 / 2.0, w.min(h) as f64));
            btn.set_child(Some(&area));
            btn.set_tooltip_text(Some("Click, then click squares to place — or drag onto the board"));
            let source = gtk::DragSource::new();
            source.set_actions(gtk::gdk::DragAction::COPY);
            let ch = piece.fen_char().to_string();
            source.connect_prepare(move |_, _, _| Some(gtk::gdk::ContentProvider::for_value(&ch.to_value())));
            btn.add_controller(source);
            let board = app.board.clone();
            btn.connect_toggled(move |b| {
                if b.is_active() {
                    board.set_tool(Tool::Place(piece))
                }
            });
            bar.append(&btn);
        }
    }
    let erase = gtk::ToggleButton::with_label("Erase");
    erase.set_group(Some(&move_btn));
    {
        let board = app.board.clone();
        erase.connect_toggled(move |b| {
            if b.is_active() {
                board.set_tool(Tool::Erase)
            }
        });
    }
    bar.append(&erase);
    bar
}

fn action(app: &Rc<App>, name: &str, f: impl Fn(&Rc<App>) + 'static) {
    let a = gtk::gio::SimpleAction::new(name, None);
    let app2 = app.clone();
    a.connect_activate(move |_, _| f(&app2));
    app.win.add_action(&a);
}

fn toggle_action(app: &Rc<App>, name: &str, initial: bool, f: impl Fn(&Rc<App>, bool) + 'static) {
    let a = gtk::gio::SimpleAction::new_stateful(name, None, &initial.to_variant());
    let app2 = app.clone();
    a.connect_activate(move |act, _| {
        let new = !act.state().and_then(|s| s.get::<bool>()).unwrap_or(false);
        act.set_state(&new.to_variant());
        f(&app2, new);
    });
    app.win.add_action(&a);
}

fn wire(app: &Rc<App>) {
    // actions
    action(app, "analyze", |a| a.see_next_move());
    action(app, "stop", |a| a.stop());
    action(app, "restart", |a| a.start_selected_engine());
    action(app, "engines", dialogs::show_engines);
    action(app, "advanced", dialogs::show_advanced);
    action(app, "log", dialogs::show_log);
    action(app, "about", dialogs::show_about);
    action(app, "load-fen", |a| {
        let t = a.fen_entry.text().to_string();
        a.load_fen(&t)
    });
    action(app, "copy-fen", |a| a.win.clipboard().set_text(&a.board.position().to_fen()));
    action(app, "start", |a| {
        a.board.set_position(Position::starting(), None);
        a.after_board_change(false);
    });
    action(app, "clear", |a| {
        let mut p = Position::empty();
        p.castling = Default::default();
        a.board.set_position(p, None);
        a.after_board_change(false);
    });
    let (flip, legal, debug, dark) = {
        let c = app.cfg.borrow();
        (c.flip_board, c.show_legal_moves, c.debug_logging, c.color_scheme == "dark")
    };
    toggle_action(app, "flip", flip, |a, on| {
        a.cfg.borrow_mut().flip_board = on;
        a.board.set_flipped(on);
    });
    toggle_action(app, "legal", legal, |a, on| {
        a.cfg.borrow_mut().show_legal_moves = on;
        a.board.set_show_legal(on);
    });
    toggle_action(app, "debug", debug, |a, on| {
        a.cfg.borrow_mut().debug_logging = on;
        a.ctl.borrow().logger.set_enabled(on);
    });
    toggle_action(app, "dark", dark, |a, on| {
        a.cfg.borrow_mut().color_scheme = if on { "dark" } else { "light" }.into();
        adw::StyleManager::default().set_color_scheme(if on { adw::ColorScheme::ForceDark } else { adw::ColorScheme::ForceLight });
    });
    if dark {
        adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
    }

    app.go_btn.connect_clicked({
        let a = app.clone();
        move |_| a.see_next_move()
    });
    app.stop_btn.connect_clicked({
        let a = app.clone();
        move |_| a.stop()
    });
    app.play_btn.connect_clicked({
        let a = app.clone();
        move |_| {
            let best = a.ctl.borrow().analysis.as_ref().and_then(|x| x.best.as_ref()).and_then(|b| b.uci.clone());
            if let Some(u) = best {
                a.board.play_uci(&u);
            }
        }
    });
    app.banner.connect_button_clicked({
        let a = app.clone();
        move |_| dialogs::show_error_details(&a)
    });
    app.fen_entry.connect_activate({
        let a = app.clone();
        move |e| a.load_fen(&e.text())
    });

    // board edits
    app.board.connect_changed({
        let a = app.clone();
        move |played| a.after_board_change(played)
    });

    // engine selection
    app.engine_row.connect_selected_notify({
        let a = app.clone();
        move |row| {
            if a.updating.get() {
                return;
            }
            let id = a.engine_ids.borrow().get(row.selected() as usize).cloned();
            if let Some(id) = id {
                // GTK can report a selection change after the list was rebuilt programmatically.
                // Re-selecting the engine that is already selected and running must not restart it
                // (that used to cause an endless stop/start loop).
                let already_selected = a.cfg.borrow().selected_engine.as_deref() == Some(id.as_str());
                let already_running = a.ctl.borrow().engine_id.as_deref() == Some(id.as_str());
                if already_selected && already_running {
                    return;
                }
                a.cfg.borrow_mut().selected_engine = Some(id);
                a.start_selected_engine();
            }
        }
    });

    // limit widgets
    app.mode_row.connect_selected_notify({
        let a = app.clone();
        move |row| {
            if a.updating.get() {
                return;
            }
            a.cfg.borrow_mut().search_mode = [SearchMode::Time, SearchMode::Depth, SearchMode::Nodes, SearchMode::Infinite][row.selected().min(3) as usize];
            a.sync_limit_widgets();
        }
    });
    app.limit_row.connect_value_notify({
        let a = app.clone();
        move |row| {
            if a.updating.get() {
                return;
            }
            let v = row.value();
            let mut c = a.cfg.borrow_mut();
            match c.search_mode {
                SearchMode::Time => c.search_time_ms = (v * 1000.0).round() as u64,
                SearchMode::Depth => c.search_depth = v as u32,
                SearchMode::Nodes => c.search_nodes = v as u64,
                SearchMode::Infinite => {}
            }
        }
    });
    app.multipv_row.connect_value_notify({
        let a = app.clone();
        move |row| {
            if !a.updating.get() {
                a.cfg.borrow_mut().multipv = row.value() as u32;
            }
        }
    });
    for (row, key) in [(app.threads_row.clone(), "Threads"), (app.hash_row.clone(), "Hash")] {
        let a = app.clone();
        row.connect_value_notify(move |r| {
            if a.updating.get() {
                return;
            }
            let caps = a.ctl.borrow().capabilities();
            let name = if key == "Threads" { caps.threads } else { caps.hash_mb }.map(|c| c.0);
            if let Some(name) = name {
                a.set_engine_option(&name, Some(&(r.value() as i64).to_string()));
            }
        });
    }

    // position editor
    let apply = {
        let a = app.clone();
        move || a.apply_editor_to_position()
    };
    app.side_row.connect_selected_notify({
        let f = apply.clone();
        move |_| f()
    });
    for c in &app.castle {
        let f = apply.clone();
        c.connect_toggled(move |_| f());
    }
    app.ep_row.connect_changed({
        let f = apply.clone();
        move |_| f()
    });
    app.half_row.connect_value_notify({
        let f = apply.clone();
        move |_| f()
    });
    app.full_row.connect_value_notify({
        let f = apply.clone();
        move |_| f()
    });

    app.win.connect_close_request({
        let a = app.clone();
        move |w| {
            {
                let mut c = a.cfg.borrow_mut();
                c.window_width = w.width();
                c.window_height = w.height();
                c.last_fen = a.board.position().to_fen();
                let _ = c.save();
            }
            a.ctl.borrow_mut().shutdown();
            glib::Propagation::Proceed
        }
    });
}

impl App {
    pub fn save_config(&self) {
        let _ = self.cfg.borrow().save();
    }

    pub fn show_error(&self, e: EngineError) {
        self.banner.set_title(&e.user_message());
        self.banner.set_revealed(true);
        *self.last_error.borrow_mut() = Some(e);
    }

    pub fn show_message(&self, text: &str) {
        self.banner.set_title(text);
        self.banner.set_revealed(true);
        *self.last_error.borrow_mut() = None;
    }

    pub fn refresh_engine_list(&self) {
        let prev_updating = self.updating.replace(true);
        {
            let cfg = self.cfg.borrow();
            let enabled: Vec<_> = cfg.engines.iter().filter(|e| e.enabled).collect();
            let ids: Vec<String> = enabled.iter().map(|e| e.id.clone()).collect();
            let names: Vec<String> = enabled.iter().map(|e| e.name.clone()).collect();
            let current: Vec<String> = (0..self.engine_model.n_items()).filter_map(|i| self.engine_model.string(i).map(|g| g.to_string())).collect();
            // Rebuilding the model resets the combo's selection and makes GTK emit change signals,
            // so only touch it when the list really changed.
            if current != names {
                self.engine_model.splice(0, self.engine_model.n_items(), &[]);
                for n in &names {
                    self.engine_model.append(n);
                }
            }
            *self.engine_ids.borrow_mut() = ids.clone();
            let sel = cfg.selected_engine.as_ref().and_then(|s| ids.iter().position(|i| i == s));
            if let Some(i) = sel {
                if self.engine_row.selected() != i as u32 {
                    self.engine_row.set_selected(i as u32);
                }
            }
            match cfg.selected() {
                Some(e) => self.engine_row.set_subtitle(&glib::markup_escape_text(&e.path.display().to_string())),
                None => self.engine_row.set_subtitle("No engine installed — use Engines… to add one"),
            }
        }
        self.updating.set(prev_updating);
    }

    pub fn start_selected_engine(&self) {
        let entry = self.cfg.borrow().selected().cloned();
        self.banner.set_revealed(false);
        match entry {
            Some(e) => {
                self.ctl.borrow_mut().select(&e);
                self.status.set_text(&format!("Starting {}…", e.name));
                self.refresh_engine_list();
                self.sync_resource_widgets();
            }
            None => {
                self.ctl.borrow_mut().deselect();
                self.status.set_text("No engine selected. Open Engines… to add one.");
                self.refresh_engine_list();
                self.sync_resource_widgets();
            }
        }
    }

    /// Persist a changed engine option and apply it to the running engine (queued if busy).
    pub fn set_engine_option(&self, name: &str, value: Option<&str>) {
        let result = self.ctl.borrow_mut().set_option(name, value);
        match result {
            Ok(()) => {
                let mut cfg = self.cfg.borrow_mut();
                if let Some(id) = cfg.selected_engine.clone() {
                    if let (Some(e), Some(v)) = (cfg.engine_mut(&id), value) {
                        e.option_values.insert(name.to_string(), v.to_string());
                    }
                }
                drop(cfg);
                self.save_config();
            }
            Err(e) => self.show_error(e),
        }
    }

    pub fn sync_limit_widgets(&self) {
        let prev_updating = self.updating.replace(true);
        let cfg = self.cfg.borrow();
        self.mode_row.set_selected(match cfg.search_mode {
            SearchMode::Time => 0,
            SearchMode::Depth => 1,
            SearchMode::Nodes => 2,
            SearchMode::Infinite => 3,
        });
        let adj = self.limit_row.adjustment();
        match cfg.search_mode {
            SearchMode::Time => {
                self.limit_row.set_title("Seconds");
                self.limit_row.set_digits(1);
                adj.set_lower(0.1);
                adj.set_upper(3600.0);
                adj.set_step_increment(0.5);
                adj.set_value(cfg.search_time_ms as f64 / 1000.0);
            }
            SearchMode::Depth => {
                self.limit_row.set_title("Depth (plies)");
                self.limit_row.set_digits(0);
                adj.set_lower(1.0);
                adj.set_upper(200.0);
                adj.set_step_increment(1.0);
                adj.set_value(cfg.search_depth as f64);
            }
            SearchMode::Nodes => {
                self.limit_row.set_title("Nodes");
                self.limit_row.set_digits(0);
                adj.set_lower(1000.0);
                adj.set_upper(1e12);
                adj.set_step_increment(100_000.0);
                adj.set_value(cfg.search_nodes as f64);
            }
            SearchMode::Infinite => {}
        }
        self.limit_row.set_visible(cfg.search_mode != SearchMode::Infinite);
        self.updating.set(prev_updating);
    }

    /// Show Threads / Hash / MultiPV controls only for engines that declare those options.
    pub fn sync_resource_widgets(&self) {
        let prev_updating = self.updating.replace(true);
        let (caps, options) = {
            let c = self.ctl.borrow();
            (c.capabilities(), c.options.clone())
        };
        let cfg = self.cfg.borrow();
        let stored = cfg.selected().map(|e| e.option_values.clone()).unwrap_or_default();
        let setup = |row: &adw::SpinRow, cap: &Option<(String, i64, i64)>| {
            match cap {
                Some((name, min, max)) => {
                    let adj = row.adjustment();
                    adj.set_lower(*min as f64);
                    adj.set_upper((*max as f64).min(1e9));
                    let default = options.iter().find(|o| &o.name == name).and_then(|o| o.default_value()).and_then(|v| v.parse::<f64>().ok()).unwrap_or(*min as f64);
                    adj.set_value(stored.get(name).and_then(|v| v.parse::<f64>().ok()).unwrap_or(default));
                    row.set_visible(true);
                }
                None => row.set_visible(false),
            }
        };
        setup(&self.threads_row, &caps.threads);
        setup(&self.hash_row, &caps.hash_mb);
        self.resources.set_visible(caps.threads.is_some() || caps.hash_mb.is_some() || caps.multipv.is_some());
        match &caps.multipv {
            Some((_, min, max)) => {
                let adj = self.multipv_row.adjustment();
                adj.set_lower(*min as f64);
                adj.set_upper((*max as f64).min(500.0));
                adj.set_value((cfg.multipv as f64).clamp(*min as f64, *max as f64));
                self.multipv_row.set_visible(true);
            }
            None => self.multipv_row.set_visible(false),
        }
        self.updating.set(prev_updating);
    }

    // ------------------------------------------------------------- position

    pub fn sync_position_widgets(&self) {
        let prev_updating = self.updating.replace(true);
        let p = self.board.position();
        self.fen_entry.set_text(&p.to_fen());
        self.side_row.set_selected(if p.side_to_move == Color::White { 0 } else { 1 });
        self.castle[0].set_active(p.castling.white_king);
        self.castle[1].set_active(p.castling.white_queen);
        self.castle[2].set_active(p.castling.black_king);
        self.castle[3].set_active(p.castling.black_queen);
        self.ep_row.set_text(&p.en_passant.map(square_name).unwrap_or_default());
        self.half_row.set_value(p.halfmove_clock as f64);
        self.full_row.set_value(p.fullmove_number as f64);
        let issues = p.validate();
        if issues.is_empty() {
            self.fen_status.set_text("");
        } else {
            let text: Vec<String> = issues.iter().map(|i| i.to_string()).collect();
            self.fen_status.set_markup(&format!("<span foreground=\"#c01c28\">{}</span>", glib::markup_escape_text(&text.join("\n"))));
        }
        let side = p.side_to_move.name();
        let marker = if p.side_to_move == Color::White { "⚪" } else { "⚫" };
        let check = if p.is_checkmate() {
            " — checkmate"
        } else if p.is_stalemate() {
            " — stalemate"
        } else if p.is_check() {
            " — check"
        } else {
            ""
        };
        self.turn_label.set_text(&format!("{marker} {side} to move{check}"));
        self.updating.set(prev_updating);
    }

    pub fn apply_editor_to_position(&self) {
        if self.updating.get() {
            return;
        }
        let mut p = self.board.position();
        p.side_to_move = if self.side_row.selected() == 0 { Color::White } else { Color::Black };
        p.castling.white_king = self.castle[0].is_active();
        p.castling.white_queen = self.castle[1].is_active();
        p.castling.black_king = self.castle[2].is_active();
        p.castling.black_queen = self.castle[3].is_active();
        let ep = self.ep_row.text().trim().to_string();
        p.en_passant = if ep.is_empty() || ep == "-" { None } else { parse_square(&ep) };
        p.halfmove_clock = self.half_row.value() as u32;
        p.fullmove_number = self.full_row.value() as u32;
        self.board.set_position(p, None);
        self.after_board_change(false);
    }

    pub fn load_fen(&self, text: &str) {
        match Position::from_fen(text) {
            Ok(p) => {
                self.board.set_position(p, None);
                self.after_board_change(false);
            }
            Err(e) => {
                self.fen_status.set_markup(&format!(
                    "<span foreground=\"#c01c28\">The supplied FEN is invalid.\n{}</span>",
                    glib::markup_escape_text(&e.to_string())
                ));
            }
        }
    }

    pub fn after_board_change(&self, _played: bool) {
        if self.ctl.borrow().is_busy() {
            self.ctl.borrow_mut().stop();
        }
        self.ctl.borrow_mut().analysis = None;
        self.clear_results();
        self.sync_position_widgets();
        self.cfg.borrow_mut().last_fen = self.board.position().to_fen();
    }

    // -------------------------------------------------------------- analysis

    pub fn see_next_move(&self) {
        let pos = self.board.position();
        let issues = pos.validate();
        if !issues.is_empty() {
            let text: Vec<String> = issues.iter().map(|i| i.to_string()).collect();
            self.show_error(EngineError::InvalidPosition(text.join("\n")));
            return;
        }
        if self.cfg.borrow().selected().is_none() {
            self.show_message("No engine selected. Open Engines… to add one.");
            return;
        }
        if self.ctl.borrow().has_exited() {
            self.show_message("The engine is not running. Press Restart.");
            return;
        }
        self.banner.set_revealed(false);
        let (limit, mp) = {
            let cfg = self.cfg.borrow();
            let ctl = self.ctl.borrow();
            (cfg.search_limit(), ctl.capabilities().multipv.map(|(n, _, _)| (n, cfg.multipv.max(1) as i64)))
        };
        let r = self.ctl.borrow_mut().search(PendingSearch { position: pos, limit, multipv: mp });
        match r {
            Ok(()) => {
                self.clear_results();
                self.status.set_text("Thinking…");
                self.go_btn.set_sensitive(false);
                self.stop_btn.set_sensitive(true);
            }
            Err(e) => self.show_error(e),
        }
    }

    pub fn stop(&self) {
        self.ctl.borrow_mut().stop();
        self.status.set_text("Stopping…");
    }

    fn clear_results(&self) {
        self.best_label.set_text("—");
        self.uci_label.set_text("");
        self.eval_label.set_text("");
        self.stats_label.set_text("");
        while let Some(c) = self.lines.first_child() {
            self.lines.remove(&c);
        }
        self.play_btn.set_sensitive(false);
        self.board.set_arrow(None);
    }

    fn tick(&self) {
        let t = self.ctl.borrow_mut().tick();
        if t.launched {
            self.sync_resource_widgets();
            let name = self.ctl.borrow().identity.as_ref().map(|i| i.name.clone()).unwrap_or_default();
            self.status.set_text(&format!("{name} ready"));
            // Fill in details discovered from the handshake (name/author) for the engine entry.
            let (author, id) = (self.ctl.borrow().identity.as_ref().and_then(|i| i.author.clone()), self.cfg.borrow().selected_engine.clone());
            if let (Some(id), Some(author)) = (id, author) {
                if let Some(e) = self.cfg.borrow_mut().engine_mut(&id) {
                    e.author = Some(author);
                }
            }
            if !t.warnings.is_empty() {
                self.show_message(&format!("Some saved settings could not be applied: {}", t.warnings.join("; ")));
            }
        }
        if t.analysis_changed || t.finished {
            self.update_results();
        }
        if let Some(e) = t.error {
            self.status.set_text("Engine problem");
            self.show_error(e);
        }
        if t.finished {
            self.go_btn.set_sensitive(true);
            self.stop_btn.set_sensitive(false);
            let ctl = self.ctl.borrow();
            let msg = match ctl.analysis.as_ref().and_then(|a| a.best.as_ref()) {
                Some(b) if b.illegal => "The engine returned an illegal move.".to_string(),
                Some(_) => "Done".to_string(),
                None => "Stopped".to_string(),
            };
            drop(ctl);
            if self.status.text() != "Engine problem" {
                self.status.set_text(&msg);
            }
        }
    }

    fn update_results(&self) {
        let ctl = self.ctl.borrow();
        let Some(a) = ctl.analysis.as_ref() else { return };
        let root = a.root().clone();
        if let Some(main) = a.main_line() {
            self.eval_label.set_text(&format!(
                "Evaluation: {}    Depth: {}{}",
                main.score.as_ref().map(format_score).unwrap_or_else(|| "—".into()),
                main.depth.map(|d| d.to_string()).unwrap_or_else(|| "—".into()),
                main.seldepth.map(|d| format!("/{d}")).unwrap_or_default()
            ));
            let mut parts = Vec::new();
            if let Some(n) = main.nodes {
                parts.push(format!("Nodes: {}", format_count(n)));
            }
            if let Some(n) = main.nps {
                parts.push(format!("NPS: {}", format_count(n)));
            }
            if let Some(t) = main.time_ms {
                parts.push(format!("Time: {}", format_time(t)));
            }
            if let Some(h) = main.hashfull {
                parts.push(format!("Hash: {:.1}%", h as f64 / 10.0));
            }
            self.stats_label.set_text(&parts.join("   "));
        }
        let (best_text, best_uci, arrow_uci) = match &a.best {
            Some(b) if b.uci.is_some() => (
                b.san.clone().unwrap_or_else(|| b.uci.clone().unwrap_or_default()),
                b.uci.clone().filter(|_| !b.illegal),
                b.uci.clone().filter(|_| !b.illegal),
            ),
            Some(_) => ("no legal move".to_string(), None, None),
            None => match a.main_line().and_then(|l| l.pv_san.first().cloned().zip(l.pv_uci.first().cloned())) {
                Some((san, uci)) => (format!("{san}  (thinking…)"), None, Some(uci)),
                None => ("…".into(), None, None),
            },
        };
        self.best_label.set_text(&format!("{}{}", if a.best.is_some() { "Best move: " } else { "" }, best_text));
        self.uci_label.set_text(&best_uci.clone().unwrap_or_default());
        self.play_btn.set_sensitive(best_uci.is_some());
        self.board.set_arrow(arrow_uci.and_then(|u| Move::from_uci_str(&u)).map(|m| (m.from, m.to)));

        while let Some(c) = self.lines.first_child() {
            self.lines.remove(&c);
        }
        for l in a.lines() {
            let row = adw::ActionRow::new();
            let first = l.pv_san.first().cloned().unwrap_or_else(|| "…".into());
            row.set_title(&glib::markup_escape_text(&format!(
                "{}.  {}    {}    d{}",
                l.multipv,
                first,
                l.score.as_ref().map(format_score).unwrap_or_default(),
                l.depth.unwrap_or(0)
            )));
            row.set_subtitle(&glib::markup_escape_text(&numbered_pv(&root, &l.pv_san)));
            row.set_subtitle_lines(2);
            self.lines.append(&row);
        }
    }
}
