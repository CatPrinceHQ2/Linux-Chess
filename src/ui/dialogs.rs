//! Secondary windows: engine manager, add-engine flow, dynamic engine settings, log, about.
use super::App;
use crate::config::EngineEntry;
use crate::engine::options::{EngineOption, OptionKind};
use crate::engine::uci::{probe, ProbeResult};
use crate::engine::{EngineError, EngineSpec};
use crate::platform;
use adw::prelude::*;
use gtk::glib;
use gtk4 as gtk;
use libadwaita as adw;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

/// Run `work` on a thread and deliver the result to `done` on the GUI thread.
fn run_bg<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static, done: impl FnOnce(T) + 'static) {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    let mut done = Some(done);
    glib::timeout_add_local(Duration::from_millis(50), move || match rx.try_recv() {
        Ok(v) => {
            if let Some(d) = done.take() {
                d(v);
            }
            glib::ControlFlow::Break
        }
        Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(_) => glib::ControlFlow::Break,
    });
}

fn message(app: &App, heading: &str, body: &str) {
    let d = adw::MessageDialog::new(Some(&app.win), Some(heading), Some(body));
    d.add_response("ok", "Close");
    d.present();
}

pub fn show_error_details(app: &Rc<App>) {
    let text = match app.last_error.borrow().as_ref() {
        Some(e) => format!("{}\n\nTechnical details:\n{}", e.user_message(), e.technical_details()),
        None => return,
    };
    let win = adw::Window::builder().transient_for(&app.win).modal(true).title("Technical details").default_width(560).default_height(320).build();
    let tv = gtk::TextView::builder().editable(false).monospace(true).wrap_mode(gtk::WrapMode::WordChar).left_margin(12).right_margin(12).top_margin(12).build();
    tv.buffer().set_text(&text);
    let tb = adw::ToolbarView::new();
    tb.add_top_bar(&adw::HeaderBar::new());
    tb.set_content(Some(&gtk::ScrolledWindow::builder().child(&tv).build()));
    win.set_content(Some(&tb));
    win.present();
}

pub fn show_about(app: &Rc<App>) {
    adw::AboutWindow::builder()
        .transient_for(&app.win)
        .application_name("Prince's Linux-Chess")
        .developer_name("CatPrinceHQ")
        .application_icon(crate::platform::APP_ID)
        .version(env!("CARGO_PKG_VERSION"))
        .comments("Play chess against a bot locally on your Linux machine, with performance in mind. Works offline, using any UCI engine you have installed.\n\nEngines are separate programs with their own licenses; they are never part of this application's license.")
        .license_type(gtk::License::Gpl30)
        .build()
        .present();
}

pub fn show_log(app: &Rc<App>) {
    let logger = app.ctl.borrow().logger.clone();
    let win = adw::Window::builder().transient_for(&app.win).title("Protocol log").default_width(760).default_height(460).build();
    let tv = gtk::TextView::builder().editable(false).monospace(true).left_margin(8).build();
    let header = adw::HeaderBar::new();
    let clear = gtk::Button::with_label("Clear");
    {
        let l = logger.clone();
        clear.connect_clicked(move |_| l.clear());
    }
    header.pack_start(&clear);
    if !logger.is_enabled() {
        header.set_title_widget(Some(&gtk::Label::new(Some("Logging is off — enable “Debug logging” in the menu"))));
    }
    let tb = adw::ToolbarView::new();
    tb.add_top_bar(&header);
    tb.set_content(Some(&gtk::ScrolledWindow::builder().child(&tv).build()));
    win.set_content(Some(&tb));
    let (tv2, win2) = (tv.clone(), win.clone());
    glib::timeout_add_local(Duration::from_millis(500), move || {
        if !win2.is_visible() {
            return glib::ControlFlow::Break;
        }
        let text = logger.snapshot().join("\n");
        if tv2.buffer().char_count() as usize != text.chars().count() {
            tv2.buffer().set_text(&text);
        }
        glib::ControlFlow::Continue
    });
    win.present();
}

// --------------------------------------------------------------- engine settings

pub fn show_advanced(app: &Rc<App>) {
    let (options, identity) = {
        let c = app.ctl.borrow();
        (c.options.clone(), c.identity.clone())
    };
    let stored = app.cfg.borrow().selected().map(|e| e.option_values.clone()).unwrap_or_default();
    let path = app.cfg.borrow().selected().map(|e| e.path.display().to_string()).unwrap_or_default();
    let win = adw::Window::builder().transient_for(&app.win).modal(true).title("Engine settings").default_width(520).default_height(640).build();
    let page = adw::PreferencesPage::new();

    let info = adw::PreferencesGroup::new();
    match &identity {
        Some(id) => {
            info.set_title(&glib::markup_escape_text(&id.name));
            info.set_description(Some(&glib::markup_escape_text(&format!("{}\nExecutable: {path}", id.author.clone().unwrap_or_default()))));
        }
        None => info.set_title("The engine is not running yet — select or restart an engine first."),
    }
    page.add(&info);

    let group = adw::PreferencesGroup::new();
    group.set_title("Options reported by the engine");
    for o in &options {
        group.add(&option_row(app, o, stored.get(&o.name)));
    }
    page.add(&group);

    let header = adw::HeaderBar::new();
    let reset = gtk::Button::with_label("Restore defaults");
    {
        let (app2, win2, options) = (app.clone(), win.clone(), options.clone());
        reset.connect_clicked(move |_| {
            for o in &options {
                if let Some(d) = o.default_value() {
                    app2.set_engine_option(&o.name, Some(&d));
                }
            }
            if let Some(id) = app2.cfg.borrow().selected_engine.clone() {
                let mut cfg = app2.cfg.borrow_mut();
                if let Some(e) = cfg.engine_mut(&id) {
                    e.option_values.clear();
                }
            }
            app2.save_config();
            win2.close();
            show_advanced(&app2);
        });
    }
    header.pack_start(&reset);
    let tb = adw::ToolbarView::new();
    tb.add_top_bar(&header);
    tb.set_content(Some(&page));
    win.set_content(Some(&tb));
    {
        let app2 = app.clone();
        win.connect_close_request(move |_| {
            app2.sync_resource_widgets();
            glib::Propagation::Proceed
        });
    }
    win.present();
}

fn option_row(app: &Rc<App>, o: &EngineOption, stored: Option<&String>) -> gtk::Widget {
    let name = o.name.clone();
    let esc = glib::markup_escape_text(&o.name);
    match &o.kind {
        OptionKind::Check { default } => {
            let row = adw::SwitchRow::builder().title(esc.as_str()).subtitle(format!("Default: {default}")).build();
            row.set_active(stored.map(|v| v == "true").unwrap_or(*default));
            let a = app.clone();
            row.connect_active_notify(move |r| a.set_engine_option(&name, Some(if r.is_active() { "true" } else { "false" })));
            row.upcast()
        }
        OptionKind::Spin { default, min, max } => {
            let adj = gtk::Adjustment::new(stored.and_then(|v| v.parse().ok()).unwrap_or(*default as f64), *min as f64, *max as f64, 1.0, 10.0, 0.0);
            let row = adw::SpinRow::new(Some(&adj), 1.0, 0);
            row.set_title(esc.as_str());
            row.set_subtitle(&format!("Default {default} · range {min} – {max}"));
            let a = app.clone();
            row.connect_value_notify(move |r| a.set_engine_option(&name, Some(&(r.value() as i64).to_string())));
            row.upcast()
        }
        OptionKind::Combo { default, choices } => {
            let strs: Vec<&str> = choices.iter().map(|s| s.as_str()).collect();
            let row = adw::ComboRow::builder().title(esc.as_str()).subtitle(format!("Default: {default}")).model(&gtk::StringList::new(&strs)).build();
            let cur = stored.cloned().unwrap_or_else(|| default.clone());
            row.set_selected(choices.iter().position(|c| *c == cur).unwrap_or(0) as u32);
            let (a, choices) = (app.clone(), choices.clone());
            row.connect_selected_notify(move |r| {
                if let Some(c) = choices.get(r.selected() as usize) {
                    a.set_engine_option(&name, Some(c));
                }
            });
            row.upcast()
        }
        OptionKind::Str { default } => {
            let row = adw::EntryRow::builder().title(esc.as_str()).show_apply_button(true).build();
            let cur = stored.cloned().unwrap_or_else(|| default.clone());
            row.set_text(if cur == "<empty>" { "" } else { &cur });
            let a = app.clone();
            row.connect_apply(move |r| a.set_engine_option(&name, Some(&r.text())));
            row.upcast()
        }
        OptionKind::Button => {
            let row = adw::ActionRow::builder().title(esc.as_str()).build();
            let b = gtk::Button::with_label("Run");
            b.set_valign(gtk::Align::Center);
            let a = app.clone();
            b.connect_clicked(move |_| a.set_engine_option(&name, None));
            row.add_suffix(&b);
            row.upcast()
        }
    }
}

// --------------------------------------------------------------- engine manager

pub fn show_engines(app: &Rc<App>) {
    let win = adw::Window::builder().transient_for(&app.win).modal(true).title("Engines").default_width(560).default_height(560).build();
    let page = adw::PreferencesPage::new();

    let list = adw::PreferencesGroup::new();
    list.set_title("Installed engines");
    list.set_description(Some("Any program that speaks the UCI protocol can be added. Nothing is run until you select it."));
    let engines = app.cfg.borrow().engines.clone();
    if engines.is_empty() {
        list.add(&adw::ActionRow::builder().title("No engines yet").subtitle("Use “Add engine” below").build());
    }
    for e in engines {
        let row = adw::ActionRow::builder().title(glib::markup_escape_text(&e.name).as_str()).build();
        let mut sub = glib::markup_escape_text(&e.path.display().to_string()).to_string();
        if let Some(a) = &e.author {
            sub.push_str(&format!("\n{}", glib::markup_escape_text(a)));
        }
        if e.bundled {
            sub.push_str("\nBundled with the application");
        }
        row.set_subtitle(&sub);
        let sw = gtk::Switch::builder().active(e.enabled).valign(gtk::Align::Center).tooltip_text("Enabled").build();
        let del = gtk::Button::builder().icon_name("user-trash-symbolic").valign(gtk::Align::Center).tooltip_text("Remove from list (the file is not deleted)").css_classes(["flat"]).build();
        row.add_suffix(&sw);
        row.add_suffix(&del);
        let id = e.id.clone();
        {
            let (a, win2, id) = (app.clone(), win.clone(), id.clone());
            sw.connect_active_notify(move |s| {
                let was_selected = a.cfg.borrow().selected_engine.as_deref() == Some(&id);
                if let Some(en) = a.cfg.borrow_mut().engine_mut(&id) {
                    en.enabled = s.is_active();
                }
                if was_selected && !s.is_active() {
                    let next = a.cfg.borrow().engines.iter().find(|x| x.enabled).map(|x| x.id.clone());
                    a.cfg.borrow_mut().selected_engine = next;
                    a.start_selected_engine();
                }
                a.refresh_engine_list();
                a.save_config();
                let _ = &win2;
            });
        }
        {
            let (a, win2) = (app.clone(), win.clone());
            del.connect_clicked(move |_| {
                let was_selected = a.cfg.borrow().selected_engine.as_deref() == Some(&id);
                a.cfg.borrow_mut().remove_engine(&id);
                if was_selected {
                    a.start_selected_engine();
                }
                a.refresh_engine_list();
                a.save_config();
                win2.close();
                show_engines(&a);
            });
        }
        list.add(&row);
    }
    page.add(&list);

    let add = adw::PreferencesGroup::new();
    add.set_title("Add engine");
    let by_path = adw::EntryRow::builder().title("Executable path, e.g. ~/Engines/my-engine").show_apply_button(true).build();
    {
        let (a, win2) = (app.clone(), win.clone());
        by_path.connect_apply(move |r| {
            let p = platform::expand_tilde(r.text().trim());
            probe_and_confirm(&a, &win2, p);
        });
    }
    add.add(&by_path);
    let pick = adw::ActionRow::builder().title("Select executable…").subtitle("Choose the engine program with a file chooser").activatable(true).build();
    pick.add_suffix(&gtk::Image::from_icon_name("document-open-symbolic"));
    {
        let (a, win2) = (app.clone(), win.clone());
        pick.connect_activated(move |_| choose_executable(&a, &win2, None));
    }
    add.add(&pick);
    let folder = adw::ActionRow::builder().title("Add from a folder…").subtitle("Browse a folder (for example an unpacked download) and pick the executable").activatable(true).build();
    folder.add_suffix(&gtk::Image::from_icon_name("folder-open-symbolic"));
    {
        let (a, win2) = (app.clone(), win.clone());
        folder.connect_activated(move |_| {
            let dlg = gtk::FileDialog::builder().title("Choose the engine's folder").build();
            let (a, win3) = (a.clone(), win2.clone());
            dlg.select_folder(Some(&win2), gtk::gio::Cancellable::NONE, move |res| {
                if let Ok(f) = res {
                    choose_executable(&a, &win3, Some(f));
                }
            });
        });
    }
    add.add(&folder);
    page.add(&add);

    let tb = adw::ToolbarView::new();
    tb.add_top_bar(&adw::HeaderBar::new());
    tb.set_content(Some(&page));
    win.set_content(Some(&tb));
    win.present();
}

fn choose_executable(app: &Rc<App>, parent: &adw::Window, folder: Option<gtk::gio::File>) {
    let dlg = gtk::FileDialog::builder().title("Select the engine executable").build();
    if let Some(f) = folder {
        dlg.set_initial_folder(Some(&f));
    }
    let (app, parent2) = (app.clone(), parent.clone());
    dlg.open(Some(parent), gtk::gio::Cancellable::NONE, move |res| {
        if let Ok(file) = res {
            if let Some(path) = file.path() {
                probe_and_confirm(&app, &parent2, path);
            }
        }
    });
}

/// Launch the chosen executable once, run the UCI handshake, then ask the user to confirm.
fn probe_and_confirm(app: &Rc<App>, parent: &adw::Window, path: PathBuf) {
    app.banner.set_revealed(false);
    let logger = app.ctl.borrow().logger.clone();
    let p2 = path.clone();
    let (app2, parent2) = (app.clone(), parent.clone());
    run_bg(
        move || probe(EngineSpec::new(p2), logger, Duration::from_secs(8)),
        move |res: Result<ProbeResult, EngineError>| match res {
            Ok(pr) => confirm_add(&app2, &parent2, path, pr),
            Err(e) => {
                let body = format!("{}\n\nTechnical details:\n{}", e.user_message(), e.technical_details());
                *app2.last_error.borrow_mut() = Some(e);
                message(&app2, "Could not add this program", &body);
            }
        },
    );
}

fn confirm_add(app: &Rc<App>, parent: &adw::Window, path: PathBuf, pr: ProbeResult) {
    let body = format!(
        "Detected a UCI engine.\n\nName: {}\nAuthor: {}\nOptions: {}\nExecutable: {}",
        pr.identity.name,
        pr.identity.author.clone().unwrap_or_else(|| "unknown".into()),
        pr.options.len(),
        path.display()
    );
    let d = adw::MessageDialog::new(Some(parent), Some("Add this engine?"), Some(&body));
    let extra = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let name_entry = gtk::Entry::builder().text(&pr.identity.name).build();
    let copy = gtk::CheckButton::with_label("Copy the program into the application's engine folder");
    extra.append(&gtk::Label::builder().label("Display name").xalign(0.0).build());
    extra.append(&name_entry);
    extra.append(&copy);
    d.set_extra_child(Some(&extra));
    d.add_response("cancel", "Cancel");
    d.add_response("add", "Add engine");
    d.set_response_appearance("add", adw::ResponseAppearance::Suggested);
    let (app2, parent2) = (app.clone(), parent.clone());
    d.connect_response(None, move |_, resp| {
        if resp != "add" {
            return;
        }
        let mut final_path = path.clone();
        if copy.is_active() {
            let dir = platform::user_engines_dir();
            let target = dir.join(path.file_name().unwrap_or_default());
            if std::fs::create_dir_all(&dir).and_then(|_| std::fs::copy(&path, &target).map(|_| ())).is_ok() {
                final_path = target;
            } else {
                message(&app2, "Copy failed", "The engine stays where it is and was added from its original location.");
            }
        }
        let name = {
            let t = name_entry.text().trim().to_string();
            if t.is_empty() { pr.identity.name.clone() } else { t }
        };
        let id = app2.cfg.borrow_mut().add_engine(EngineEntry {
            name,
            path: final_path,
            author: pr.identity.author.clone(),
            ..Default::default()
        });
        app2.cfg.borrow_mut().selected_engine = Some(id);
        app2.save_config();
        app2.refresh_engine_list();
        app2.start_selected_engine();
        parent2.close();
        show_engines(&app2);
    });
    d.present();
}
