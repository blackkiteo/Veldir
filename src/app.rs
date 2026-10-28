use crate::fs_ops::{self, Entry, Job};
use chrono::{DateTime, Local};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

const ACCENT: Color32 = Color32::from_rgb(112, 175, 250);
const TEXT_MUTED: Color32 = Color32::from_rgb(144, 155, 171);

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    favorites: Vec<PathBuf>,
    tabs: Vec<PathBuf>,
    active: usize,
    secondary_path: Option<PathBuf>,
    dual_pane: bool,
}

struct Tab {
    id: u64,
    path: PathBuf,
    entries: Vec<Entry>,
    back: Vec<PathBuf>,
    forward: Vec<PathBuf>,
    loading: bool,
    generation: u64,
    error: Option<String>,
    selected: BTreeSet<PathBuf>,
    anchor: Option<PathBuf>,
    filter: String,
}

impl Tab {
    fn new(id: u64, path: PathBuf) -> Self {
        Self {
            id, path, entries: Vec::new(), back: Vec::new(), forward: Vec::new(),
            loading: false, generation: 0, error: None,
            selected: BTreeSet::new(), anchor: None, filter: String::new(),
        }
    }
}

enum Message {
    FolderLoaded { tab_id: u64, generation: u64, result: Result<Vec<Entry>, String> },
    JobFinished { result: Result<String, String>, is_move: bool },
}

enum Dialog {
    None,
    NewFolder(String),
    Rename { source: PathBuf, name: String },
    Delete(Vec<PathBuf>),
    ExternalDrop { sources: Vec<PathBuf>, destination: PathBuf },
    ConfirmMove { sources: Vec<PathBuf>, destination_folder: PathBuf },
}

#[derive(Clone)]
struct InternalClipboard {
    sources: Vec<PathBuf>,
    cut: bool,
}

pub struct Veldir {
    tabs: Vec<Tab>,
    active: usize,
    secondary: Option<Tab>,
    focused_right: bool,
    next_id: u64,
    tx: Sender<Message>,
    rx: Receiver<Message>,
    context: egui::Context,
    favorites: Vec<PathBuf>,
    shortcuts: Vec<(String, PathBuf)>,
    drives: Vec<PathBuf>,
    clipboard: Option<InternalClipboard>,
    busy: bool,
    dialog: Dialog,
    status: String,
    editing_address: bool,
    address_input: String,
}

impl Veldir {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visual = egui::Visuals::dark();
        visual.panel_fill = Color32::from_rgb(19, 24, 32);
        visual.window_fill = Color32::from_rgb(28, 34, 44);
        visual.extreme_bg_color = Color32::from_rgb(13, 18, 26);
        visual.selection.bg_fill = Color32::from_rgb(50, 86, 127);
        visual.hyperlink_color = ACCENT;
        cc.egui_ctx.set_visuals(visual);

        let saved: Saved = cc.storage.and_then(|s| eframe::get_value(s, "veldir_state"))
            .unwrap_or_default();
        let (tx, rx) = mpsc::channel();
        let home = dirs_next::home_dir()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("C:\\"));
        let mut shortcuts = Vec::new();
        shortcuts.push(("Home".into(), home.clone()));
        if let Some(p) = dirs_next::desktop_dir() { shortcuts.push(("Desktop".into(), p)); }
        if let Some(p) = dirs_next::download_dir() { shortcuts.push(("Download".into(), p)); }
        if let Some(p) = dirs_next::document_dir() { shortcuts.push(("Documenti".into(), p)); }
        if let Some(p) = dirs_next::picture_dir() { shortcuts.push(("Immagini".into(), p)); }

        let drives = if cfg!(windows) {
            ('A'..='Z')
                .map(|letter| PathBuf::from(format!("{letter}:\\")))
                .filter(|p| p.is_dir())
                .collect()
        } else { vec![PathBuf::from("/")] };

        let mut app = Self {
            tabs: Vec::new(), active: 0, secondary: None, focused_right: false,
            next_id: 1, tx, rx, context: cc.egui_ctx.clone(),
            favorites: saved.favorites, shortcuts, drives,
            clipboard: None, busy: false, dialog: Dialog::None,
            status: "Pronto".into(), editing_address: false, address_input: String::new(),
        };
        let restored: Vec<PathBuf> = saved.tabs.into_iter().filter(|p| p.is_dir()).take(12).collect();
        if restored.is_empty() {
            app.add_tab(home);
        } else {
            for path in restored { app.add_tab(path); }
            app.active = saved.active.min(app.tabs.len().saturating_sub(1));
        }
        if saved.dual_pane {
            let right = saved.secondary_path.filter(|p| p.is_dir())
                .unwrap_or_else(|| app.tabs[app.active].path.clone());
            app.open_secondary(right);
        }
        app.address_input = app.current().path.display().to_string();
        app
    }

    fn panel(&self, right: bool) -> &Tab {
        if right { self.secondary.as_ref().expect("secondo pannello non attivo") }
        else { &self.tabs[self.active] }
    }
    fn panel_mut(&mut self, right: bool) -> &mut Tab {
        if right { self.secondary.as_mut().expect("secondo pannello non attivo") }
        else { &mut self.tabs[self.active] }
    }
    fn current(&self) -> &Tab { self.panel(self.focused_right) }
    fn current_mut(&mut self) -> &mut Tab { self.panel_mut(self.focused_right) }

    fn open_secondary(&mut self, path: PathBuf) {
        if self.secondary.is_some() { return; }
        let id = self.next_id;
        self.next_id += 1;
        self.secondary = Some(Tab::new(id, path));
        self.load_secondary();
    }

    fn toggle_dual(&mut self) {
        if self.secondary.is_some() {
            self.secondary = None;
            self.focused_right = false;
        } else {
            let path = self.tabs[self.active].path.clone();
            self.open_secondary(path);
        }
        self.address_input = self.current().path.display().to_string();
        self.editing_address = false;
    }

    fn focus(&mut self, right: bool) {
        if right && self.secondary.is_none() { return; }
        if self.focused_right != right {
            self.focused_right = right;
            self.address_input = self.current().path.display().to_string();
            self.editing_address = false;
        }
    }

    fn load_secondary(&mut self) {
        let Some(tab) = self.secondary.as_mut() else { return; };
        tab.generation += 1;
        tab.loading = true;
        tab.error = None;
        tab.entries.clear();
        tab.selected.clear();
        tab.anchor = None;
        let path = tab.path.clone();
        let id = tab.id;
        let generation = tab.generation;
        let sender = self.tx.clone();
        let ctx = self.context.clone();
        std::thread::spawn(move || {
            let result = fs_ops::list(&path);
            let _ = sender.send(Message::FolderLoaded { tab_id: id, generation, result });
            ctx.request_repaint();
        });
    }

    fn reload_current(&mut self) {
        if self.focused_right { self.load_secondary(); }
        else { self.load(self.active); }
    }

    fn load(&mut self, index: usize) {
        let tab = &mut self.tabs[index];
        tab.generation += 1;
        tab.loading = true;
        tab.error = None;
        tab.entries.clear();
        tab.selected.clear();
        tab.anchor = None;
        let path = tab.path.clone();
        let id = tab.id;
        let generation = tab.generation;
        let sender = self.tx.clone();
        let ctx = self.context.clone();
        std::thread::spawn(move || {
            let result = fs_ops::list(&path);
            let _ = sender.send(Message::FolderLoaded { tab_id: id, generation, result });
            ctx.request_repaint();
        });
    }

    fn add_tab(&mut self, path: PathBuf) {
        self.focused_right = false;
        let path = if path.is_dir() { path } else {
            dirs_next::home_dir().unwrap_or_else(|| PathBuf::from("C:\\"))
        };
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(Tab::new(id, path));
        self.active = self.tabs.len() - 1;
        self.address_input = self.current().path.display().to_string();
        self.editing_address = false;
        self.load(self.active);
    }

    fn close_tab(&mut self, id: u64) {
        if let Some(index) = self.tabs.iter().position(|t| t.id == id) {
            if self.tabs.len() == 1 {
                self.add_tab(dirs_next::home_dir().unwrap_or_else(|| PathBuf::from("C:\\")));
            }
            self.tabs.remove(index);
            if self.active >= index && self.active > 0 { self.active -= 1; }
            self.active = self.active.min(self.tabs.len() - 1);
            self.focused_right = false;
            self.address_input = self.current().path.display().to_string();
        }
    }

    fn navigate(&mut self, path: PathBuf) {
        let path = match path.canonicalize() {
            Ok(p) if p.is_dir() => p,
            _ => { self.status = format!("Cartella non accessibile: {}", path.display()); return; }
        };
        if self.current().path == path { return; }
        let tab = self.current_mut();
        tab.back.push(tab.path.clone());
        tab.forward.clear();
        tab.path = path;
        tab.filter.clear();
        self.address_input = self.current().path.display().to_string();
        self.editing_address = false;
        self.reload_current();
    }

    fn history_back(&mut self) {
        if let Some(previous) = self.current_mut().back.pop() {
            let tab = self.current_mut();
            tab.forward.push(tab.path.clone());
            tab.path = previous;
            tab.filter.clear();
            self.address_input = self.current().path.display().to_string();
            self.reload_current();
        }
    }

    fn history_forward(&mut self) {
        if let Some(next) = self.current_mut().forward.pop() {
            let tab = self.current_mut();
            tab.back.push(tab.path.clone());
            tab.path = next;
            tab.filter.clear();
            self.address_input = self.current().path.display().to_string();
            self.reload_current();
        }
    }

    fn up(&mut self) {
        if let Some(parent) = self.current().path.parent().map(Path::to_path_buf) {
            self.navigate(parent);
        }
    }

    fn refresh(&mut self) {
        for i in 0..self.tabs.len() { self.load(i); }
        if self.secondary.is_some() { self.load_secondary(); }
        self.status = "Aggiornamento richiesto".into();
    }

    fn pump(&mut self) {
        while let Ok(message) = self.rx.try_recv() {
            match message {
                Message::FolderLoaded { tab_id, generation, result } => {
                    let tab = self.tabs.iter_mut().find(|t| t.id == tab_id && t.generation == generation)
                        .or_else(|| self.secondary.as_mut()
                            .filter(|t| t.id == tab_id && t.generation == generation));
                    if let Some(tab) = tab {
                        tab.loading = false;
                        match result {
                            Ok(entries) => { tab.entries = entries; tab.error = None; }
                            Err(e) => { tab.error = Some(e); }
                        }
                    }
                }
                Message::JobFinished { result, is_move } => {
                    self.busy = false;
                    match result {
                        Ok(message) => {
                            if is_move { self.clipboard = None; }
                            self.refresh();
                            self.status = message;
                        }
                        Err(error) => {
                            // Some operations may have changed files before reporting an error.
                            self.refresh();
                            self.status = format!("Errore: {error}");
                        }
                    }
                }
            }
        }
    }

    fn begin_job(&mut self, job: Job) {
        if self.busy { return; }
        self.busy = true;
        self.status = "Operazione in corso...".into();
        let sender = self.tx.clone();
        let ctx = self.context.clone();
        let is_move = matches!(&job, Job::Move { .. } | Job::MoveMany { .. });
        std::thread::spawn(move || {
            let result = fs_ops::execute(job);
            let _ = sender.send(Message::JobFinished { result, is_move });
            ctx.request_repaint();
        });
    }

    fn selected(&self) -> Vec<PathBuf> { self.current().selected.iter().cloned().collect() }

    fn selected_one(&self) -> Option<PathBuf> {
        if self.current().selected.len() == 1 { self.current().selected.iter().next().cloned() }
        else { None }
    }

    fn open_entry(&mut self, path: PathBuf) {
        if path.is_dir() { self.navigate(path); }
        else if let Err(error) = opener::open(&path) {
            self.status = format!("Impossibile aprire {}: {error}", path.display());
        }
    }

    fn open_selected(&mut self) {
        if let Some(path) = self.selected_one() { self.open_entry(path); }
    }

    fn prepare_rename(&mut self) {
        if let Some(source) = self.selected_one() {
            let name = source.file_name().unwrap_or_default().to_string_lossy().to_string();
            self.dialog = Dialog::Rename { source, name };
        }
    }

    fn prepare_trash(&mut self) {
        let sources = self.selected();
        if !sources.is_empty() { self.dialog = Dialog::Delete(sources); }
    }

    fn clipboard_set(&mut self, cut: bool) {
        let sources = self.selected();
        if !sources.is_empty() {
            self.clipboard = Some(InternalClipboard { sources, cut });
            self.status = if cut { "Elemento tagliato (clipboard interna)".into() }
                else { "Elemento copiato (clipboard interna)".into() };
        }
    }

    fn paste(&mut self) {
        if let Some(clip) = self.clipboard.clone() {
            let destination_folder = self.current().path.clone();
            let job = if clip.cut {
                Job::MoveMany { sources: clip.sources, destination_folder }
            } else {
                Job::CopyMany { sources: clip.sources, destination_folder }
            };
            self.begin_job(job);
        }
    }

    fn transfer_to_other(&mut self, cut: bool) {
        let sources = self.selected();
        if sources.is_empty() { return; }
        let Some(right) = &self.secondary else { return; };
        let destination_folder = if self.focused_right {
            self.tabs[self.active].path.clone()
        } else { right.path.clone() };
        if cut {
            self.dialog = Dialog::ConfirmMove { sources, destination_folder };
        } else {
            self.begin_job(Job::CopyMany { sources, destination_folder });
        }
    }

    fn toggle_favorite(&mut self) {
        let path = self.current().path.clone();
        if let Some(i) = self.favorites.iter().position(|p| p == &path) { self.favorites.remove(i); }
        else { self.favorites.push(path); }
    }

    fn keyboard(&mut self, ctx: &egui::Context) {
        // Dialogs are modal: navigation and file shortcuts must not change their context.
        if !matches!(self.dialog, Dialog::None) { return; }
        let shortcut = |key: egui::Key| ctx.input(|i| i.modifiers.command && i.key_pressed(key));
        if shortcut(egui::Key::T) { self.add_tab(self.current().path.clone()); }
        if shortcut(egui::Key::W) { let id = self.tabs[self.active].id; self.close_tab(id); }
        if shortcut(egui::Key::L) {
            self.editing_address = true;
            self.address_input = self.current().path.display().to_string();
            ctx.memory_mut(|m| m.request_focus(egui::Id::new("address_bar")));
        }
        if shortcut(egui::Key::F) {
            let filter_id = if self.focused_right { "folder_filter_right" } else { "folder_filter" };
            ctx.memory_mut(|m| m.request_focus(egui::Id::new(filter_id)));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F5)) { self.refresh(); }
        if ctx.input(|i| i.key_pressed(egui::Key::F6)) {
            if self.secondary.is_some() { self.focus(!self.focused_right); }
            else { self.toggle_dual(); }
        }
        if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft)) { self.history_back(); }
        if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight)) { self.history_forward(); }
        if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowUp)) { self.up(); }

        if !ctx.egui_wants_keyboard_input() && !self.busy && matches!(self.dialog, Dialog::None) {
            if shortcut(egui::Key::C) { self.clipboard_set(false); }
            if shortcut(egui::Key::X) { self.clipboard_set(true); }
            if shortcut(egui::Key::V) { self.paste(); }
            if ctx.input(|i| i.key_pressed(egui::Key::F2)) { self.prepare_rename(); }
            if ctx.input(|i| i.key_pressed(egui::Key::Delete)) { self.prepare_trash(); }
            if ctx.input(|i| i.key_pressed(egui::Key::Enter)) { self.open_selected(); }
        }
    }

    fn tabs_ui(&mut self, ui: &mut egui::Ui) {
        let mut switch = None;
        let mut close = None;
        ui.horizontal_wrapped(|ui| {
            for (i, tab) in self.tabs.iter().enumerate() {
                let title = tab.path.file_name().and_then(|s| s.to_str())
                    .map(str::to_owned).unwrap_or_else(|| tab.path.display().to_string());
                let title = if title.chars().count() > 22 { format!("{}…", title.chars().take(21).collect::<String>()) } else { title };
                if ui.add(egui::Button::new(format!("  {title}  ")).selected(self.active == i))
                    .on_hover_text(tab.path.display().to_string()).clicked() { switch = Some(i); }
                if ui.small_button("×").on_hover_text("Chiudi scheda").clicked() { close = Some(tab.id); }
            }
            if ui.button("＋").on_hover_text("Nuova scheda · Ctrl+T").clicked() {
                switch = Some(usize::MAX);
            }
        });
        if let Some(id) = close { self.close_tab(id); }
        if let Some(index) = switch {
            if index == usize::MAX { self.add_tab(self.current().path.clone()); }
            else if index < self.tabs.len() {
                self.focused_right = false;
                self.active = index;
                self.address_input = self.current().path.display().to_string();
                self.editing_address = false;
            }
        }
    }

    fn toolbar_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.add_enabled(!self.current().back.is_empty(), egui::Button::new("←"))
                .on_hover_text("Indietro · Alt+←").clicked() { self.history_back(); }
            if ui.add_enabled(!self.current().forward.is_empty(), egui::Button::new("→"))
                .on_hover_text("Avanti · Alt+→").clicked() { self.history_forward(); }
            if ui.button("↑").on_hover_text("Cartella superiore · Alt+↑").clicked() { self.up(); }
            if ui.button("⟳").on_hover_text("Aggiorna · F5").clicked() { self.refresh(); }
            ui.separator();
            if self.editing_address {
                let r = ui.add(egui::TextEdit::singleline(&mut self.address_input)
                    .desired_width(ui.available_width().max(270.0).min(600.0) * 0.52)
                    .id(egui::Id::new("address_bar")));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let target = PathBuf::from(self.address_input.trim());
                    self.navigate(target);
                }
                if ui.button("Vai").clicked() { self.navigate(PathBuf::from(self.address_input.trim())); }
            } else {
                let text = self.current().path.display().to_string();
                if ui.add(egui::Button::new(RichText::new(format!("⌁   {text}")).monospace())
                    .frame(false)).on_hover_text("Modifica percorso · Ctrl+L").clicked() {
                    self.editing_address = true;
                    self.address_input = text;
                }
            }
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!self.busy, egui::Button::new("＋ Cartella"))
                .on_hover_text("Crea cartella").clicked() {
                self.dialog = Dialog::NewFolder("Nuova cartella".into());
            }
            let selected = !self.selected().is_empty();
            let single = self.selected_one().is_some();
            if ui.add_enabled(!self.busy && single, egui::Button::new("Rinomina"))
                .on_hover_text("F2").clicked() { self.prepare_rename(); }
            if ui.add_enabled(!self.busy && selected, egui::Button::new("Copia"))
                .on_hover_text("Ctrl+C").clicked() { self.clipboard_set(false); }
            if ui.add_enabled(!self.busy && selected, egui::Button::new("Taglia"))
                .on_hover_text("Ctrl+X").clicked() { self.clipboard_set(true); }
            if ui.add_enabled(!self.busy && self.clipboard.is_some(), egui::Button::new("Incolla"))
                .on_hover_text("Ctrl+V · clipboard interna").clicked() { self.paste(); }
            if ui.add_enabled(!self.busy && selected, egui::Button::new("Cestino"))
                .on_hover_text("Canc · con conferma").clicked() { self.prepare_trash(); }
            ui.separator();
            let dual = self.secondary.is_some();
            if ui.button(if dual { "▣ Pannello singolo" } else { "▥ Due pannelli" })
                .on_hover_text("Mostra/nascondi il secondo pannello").clicked() { self.toggle_dual(); }
            if dual {
                if ui.add_enabled(!self.busy && selected, egui::Button::new("⇢ Copia"))
                    .on_hover_text("Copia nel pannello opposto").clicked() { self.transfer_to_other(false); }
                if ui.add_enabled(!self.busy && selected, egui::Button::new("⇢ Sposta"))
                    .on_hover_text("Sposta nel pannello opposto").clicked() { self.transfer_to_other(true); }
            }
            ui.separator();
            let is_fav = self.favorites.contains(&self.current().path);
            if ui.button(if is_fav { "★" } else { "☆" })
                .on_hover_text("Aggiungi/rimuovi preferiti").clicked() { self.toggle_favorite(); }
        });
    }

    fn sidebar_ui(&mut self, ui: &mut egui::Ui) {
        ui.set_width(198.0);
        ui.add_space(4.0);
        ui.label(RichText::new("ACCESSO RAPIDO").color(TEXT_MUTED).small().strong());
        ui.add_space(4.0);
        let mut target = None;
        for (name, path) in &self.shortcuts {
            if ui.add_sized([190.0, 27.0], egui::Button::new(format!("  {name}"))
                .selected(&self.current().path == path)).clicked() { target = Some(path.clone()); }
        }
        ui.add_space(15.0);
        ui.label(RichText::new("PREFERITI").color(TEXT_MUTED).small().strong());
        ui.add_space(4.0);
        for path in &self.favorites {
            let title = path.file_name().and_then(|s| s.to_str())
                .map(str::to_owned).unwrap_or_else(|| path.display().to_string());
            if ui.add_sized([190.0, 27.0], egui::Button::new(format!("  ★  {title}"))
                .selected(&self.current().path == path))
                .on_hover_text(path.display().to_string()).clicked() { target = Some(path.clone()); }
        }
        ui.add_space(15.0);
        ui.label(RichText::new("UNITÀ").color(TEXT_MUTED).small().strong());
        ui.add_space(4.0);
        for drive in &self.drives {
            if ui.add_sized([190.0, 27.0], egui::Button::new(format!("  ◈  {}", drive.display()))
                .selected(&self.current().path == drive)).clicked() { target = Some(drive.clone()); }
        }
        if let Some(path) = target { self.navigate(path); }
    }

    fn files_ui(&mut self, ui: &mut egui::Ui, right: bool) {
        let mut open = None;
        let mut click = None;
        let tab = self.panel(right);
        let filter = tab.filter.to_lowercase();
        let indices: Vec<usize> = tab.entries.iter().enumerate()
            .filter(|(_, item)| filter.is_empty() || item.name.to_lowercase().contains(&filter))
            .map(|(i, _)| i).collect();
        let count = indices.len();
        let dual = self.secondary.is_some();
        let width = ui.available_width().max(if dual { 250.0 } else { 420.0 });
        let w_name = if dual { (width * 0.66).max(130.0) } else { (width * 0.55).max(200.0) };
        let w_type = (width * 0.17).max(80.0);
        let w_size = if dual { (width * 0.26).max(70.0) } else { (width * 0.11).max(75.0) };
        ui.horizontal(|ui| {
            ui.add_sized([w_name, 24.0], egui::Label::new(RichText::new("NOME").small().color(TEXT_MUTED).strong()));
            if !dual { ui.add_sized([w_type, 24.0], egui::Label::new(RichText::new("TIPO").small().color(TEXT_MUTED).strong())); }
            ui.add_sized([w_size, 24.0], egui::Label::new(RichText::new("DIMENSIONE").small().color(TEXT_MUTED).strong()));
            if !dual { ui.label(RichText::new("MODIFICATO").small().color(TEXT_MUTED).strong()); }
        });
        ui.separator();
        if tab.loading { ui.horizontal(|ui| { ui.spinner(); ui.label("Caricamento cartella..."); }); return; }
        if let Some(e) = &tab.error { ui.colored_label(Color32::LIGHT_RED, e); return; }
        if count == 0 { ui.add_space(18.0); ui.label("Nessun elemento trovato"); return; }
        egui::ScrollArea::vertical().auto_shrink([false, false])
            .id_salt(("rows", tab.id, tab.generation))
            .show_rows(ui, 28.0, count, |ui, range| {
                for row in range {
                    let item = &tab.entries[indices[row]];
                    let selected = tab.selected.contains(&item.path);
                    let name = format!("{}  {}", if item.is_dir { "▸" } else { "·" }, item.name);
                    let file_type = if item.is_dir { "Cartella".to_owned() }
                        else { item.path.extension().and_then(|e| e.to_str())
                            .map(str::to_ascii_uppercase).unwrap_or_else(|| "File".into()) };
                    let size = if item.is_dir { "—".into() } else { pretty_bytes(item.size) };
                    let modified = item.modified.map(|t| {
                        let d: DateTime<Local> = DateTime::from(t);
                        d.format("%d/%m/%Y %H:%M").to_string()
                    }).unwrap_or_else(|| "—".into());
                    let response = ui.horizontal(|ui| {
                        let r = ui.add_sized([w_name, 25.0], egui::Button::new(name)
                            .selected(selected).frame(selected));
                        if !dual { ui.add_sized([w_type, 25.0], egui::Label::new(file_type)); }
                        ui.add_sized([w_size, 25.0], egui::Label::new(size));
                        if !dual { ui.label(RichText::new(modified).color(TEXT_MUTED).small()); }
                        r
                    }).inner.on_hover_text(item.path.display().to_string());
                    if response.clicked() {
                        click = Some((item.path.clone(), ui.input(|i| i.modifiers)));
                    }
                    if response.double_clicked() { open = Some(item.path.clone()); }
                }
            });
        if let Some((path, modifiers)) = click {
            // Selection is resolved against visible, filtered order.
            let ordered: Vec<PathBuf> = indices.iter()
                .map(|&i| self.panel(right).entries[i].path.clone()).collect();
            let tab = self.panel_mut(right);
            apply_selection(&mut tab.selected, &mut tab.anchor, &ordered,
                path, modifiers.shift, modifiers.command || modifiers.ctrl);
            self.focus(right);
        }
        if let Some(path) = open {
            self.focus(right);
            self.open_entry(path);
        }
    }

    fn pane_ui(&mut self, ui: &mut egui::Ui, right: bool) {
        let tab = self.panel(right);
        let path = tab.path.display().to_string();
        let count = tab.selected.len();
        let focused = self.focused_right == right;
        let title = if right { "PANNELLO DESTRO" } else { "PANNELLO SINISTRO" };
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(RichText::new(title).small().color(if focused { ACCENT } else { TEXT_MUTED }))
                .selected(focused)).on_hover_text("Attiva il pannello").clicked() { self.focus(right); }
            ui.label(RichText::new(format!("{count} selezionati")).small().color(TEXT_MUTED));
        });
        ui.label(RichText::new(path).small().color(TEXT_MUTED));
        ui.horizontal(|ui| {
            ui.label(RichText::new("Filtra").color(TEXT_MUTED));
            let filter_width = if self.secondary.is_some() { 170.0 } else { 260.0 };
            let tab = self.panel_mut(right);
            let changed = ui.add(egui::TextEdit::singleline(&mut tab.filter)
                .id(egui::Id::new(if right { "folder_filter_right" } else { "folder_filter" }))
                .hint_text("Nome o estensione")
                .desired_width(filter_width)).changed();
            // Never leave hidden entries selected when a filter changes.
            if changed {
                tab.selected.clear();
                tab.anchor = None;
            }
            if ui.button("×").on_hover_text("Azzera filtro").clicked() {
                tab.filter.clear();
                tab.selected.clear();
                tab.anchor = None;
            }
        });
        ui.add_space(6.0);
        self.files_ui(ui, right);
    }

    /// OS drag-and-drop always asks before copying: it never moves or deletes the source.
    fn accept_os_drop(&mut self, ctx: &egui::Context) {
        let sources: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter()
            .map(|file| file.path().to_path_buf())
            .filter(|path| !path.as_os_str().is_empty())
            .collect());
        if sources.is_empty() { return; }
        if self.busy || !matches!(self.dialog, Dialog::None) {
            self.status = "Trascinamento non acquisito: operazione o finestra già aperta".into();
            return;
        }
        self.dialog = Dialog::ExternalDrop {
            sources,
            destination: self.current().path.clone(),
        };
    }

    fn dialog_ui(&mut self, ctx: &egui::Context) {
        let mut job = None;
        let mut close = false;
        let parent_path = self.current().path.clone();
        match &mut self.dialog {
            Dialog::None => return,
            Dialog::NewFolder(name) => {
                egui::Window::new("Nuova cartella").collapsible(false).resizable(false)
                    .show(ctx, |ui| {
                        ui.label("Nome della nuova cartella:");
                        ui.text_edit_singleline(name);
                        ui.horizontal(|ui| {
                            if ui.button("Crea").clicked() {
                                job = Some(Job::NewFolder { parent: parent_path.clone(), name: name.clone() });
                                close = true;
                            }
                            if ui.button("Annulla").clicked() { close = true; }
                        });
                    });
            }
            Dialog::Rename { source, name } => {
                egui::Window::new("Rinomina").collapsible(false).resizable(false)
                    .show(ctx, |ui| {
                        ui.label("Nuovo nome:");
                        ui.text_edit_singleline(name);
                        ui.horizontal(|ui| {
                            if ui.button("Salva").clicked() {
                                job = Some(Job::Rename { source: source.clone(), name: name.clone() });
                                close = true;
                            }
                            if ui.button("Annulla").clicked() { close = true; }
                        });
                    });
            }
            Dialog::Delete(paths) => {
                egui::Window::new("Sposta nel Cestino?").collapsible(false).resizable(false)
                    .show(ctx, |ui| {
                        ui.label(format!("Spostare {} elementi selezionati nel Cestino?", paths.len()));
                        ui.label(RichText::new("L'eliminazione permanente non è prevista.").color(TEXT_MUTED));
                        ui.horizontal(|ui| {
                            if ui.button("Sposta nel Cestino").clicked() {
                                job = Some(Job::TrashMany { sources: paths.clone() });
                                close = true;
                            }
                            if ui.button("Annulla").clicked() { close = true; }
                        });
                    });
            }
            Dialog::ConfirmMove { sources, destination_folder } => {
                egui::Window::new("Conferma spostamento").collapsible(false).resizable(false)
                    .show(ctx, |ui| {
                        ui.label(format!("Spostare {} elementi in {}?", sources.len(), destination_folder.display()));
                        ui.label(RichText::new("Solo stesso volume. Nessuna sovrascrittura automatica.")
                            .small().color(TEXT_MUTED));
                        ui.horizontal(|ui| {
                            if ui.button("Conferma spostamento").clicked() {
                                job = Some(Job::MoveMany {
                                    sources: sources.clone(), destination_folder: destination_folder.clone(),
                                });
                                close = true;
                            }
                            if ui.button("Annulla").clicked() { close = true; }
                        });
                    });
            }
            Dialog::ExternalDrop { sources, destination } => {
                let left = self.tabs[self.active].path.clone();
                let right = self.secondary.as_ref().map(|p| p.path.clone());
                egui::Window::new("Importa file trascinati").collapsible(false).resizable(false)
                    .show(ctx, |ui| {
                        ui.label(format!("{} elementi trascinati da Windows", sources.len()));
                        ui.label("Scegli dove COPIARLI. Le sorgenti non verranno rimosse.");
                        ui.add_space(6.0);
                        ui.selectable_value(destination, left.clone(), format!("Sinistra: {}", left.display()));
                        if let Some(right) = &right {
                            ui.selectable_value(destination, right.clone(), format!("Destra: {}", right.display()));
                        }
                        ui.label(RichText::new("Nessuna sovrascrittura automatica. In caso di conflitto l'operazione si interrompe.")
                            .small().color(TEXT_MUTED));
                        ui.horizontal(|ui| {
                            if ui.button("Copia file").clicked() {
                                job = Some(Job::CopyMany {
                                    sources: sources.clone(), destination_folder: destination.clone(),
                                });
                                close = true;
                            }
                            if ui.button("Annulla").clicked() { close = true; }
                        });
                    });
            }
        }
        if close { self.dialog = Dialog::None; }
        if let Some(job) = job { self.begin_job(job); }
    }
}

impl eframe::App for Veldir {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.pump();
        self.accept_os_drop(ui.ctx());
        self.keyboard(ui.ctx());
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("◈ VELDIR").strong().size(18.0).color(ACCENT));
            ui.label(RichText::new("0.2.1  ·  LIGHT FILE MANAGER").small().color(TEXT_MUTED));
        });
        ui.add_space(8.0);
        self.tabs_ui(ui);
        ui.add_space(6.0);
        self.toolbar_ui(ui);
        ui.add_space(10.0);
        ui.separator();
        let content_height = (ui.available_height() - 46.0).max(230.0);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), content_height),
            egui::Layout::left_to_right(egui::Align::Min),
            |ui| {
                ui.add_space(4.0);
                ui.vertical(|ui| { self.sidebar_ui(ui); });
                ui.separator();
                if self.secondary.is_some() {
                    let width = (ui.available_width() - 14.0).max(520.0) / 2.0;
                    ui.vertical(|ui| {
                        ui.set_min_width(width);
                        self.pane_ui(ui, false);
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_min_width(width);
                        self.pane_ui(ui, true);
                    });
                } else {
                    ui.vertical(|ui| {
                        ui.set_min_width(ui.available_width().max(400.0));
                        self.pane_ui(ui, false);
                    });
                }
            },
        );
        ui.separator();
        ui.horizontal(|ui| {
            let tab = self.current();
            let count = tab.entries.iter().filter(|e| tab.filter.is_empty() ||
                e.name.to_lowercase().contains(&tab.filter.to_lowercase())).count();
            ui.label(RichText::new(format!("{count} elementi  ·  {}", tab.path.display())).small().color(TEXT_MUTED));
            ui.separator();
            if self.busy { ui.spinner(); }
            ui.label(RichText::new(&self.status).small().color(TEXT_MUTED));
        });
        self.dialog_ui(ui.ctx());
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let saved = Saved {
            favorites: self.favorites.clone(),
            tabs: self.tabs.iter().map(|t| t.path.clone()).collect(),
            active: self.active,
            secondary_path: self.secondary.as_ref().map(|p| p.path.clone()),
            dual_pane: self.secondary.is_some(),
        };
        eframe::set_value(storage, "veldir_state", &saved);
    }
}

/// Pure selection engine. Shift selects a range of currently visible entries;
/// Ctrl toggles an item; Ctrl+Shift extends the current set.
fn apply_selection(
    selected: &mut BTreeSet<PathBuf>,
    anchor: &mut Option<PathBuf>,
    ordered: &[PathBuf],
    path: PathBuf,
    shift: bool,
    additive: bool,
) {
    if shift {
        if let Some(start) = anchor.as_ref().and_then(|a| ordered.iter().position(|p| p == a)) {
            if let Some(end) = ordered.iter().position(|p| p == &path) {
                if !additive { selected.clear(); }
                for item in &ordered[start.min(end)..=start.max(end)] {
                    selected.insert(item.clone());
                }
                return;
            }
        }
    }
    if additive && !shift {
        if !selected.insert(path.clone()) { selected.remove(&path); }
    } else {
        selected.clear();
        selected.insert(path.clone());
    }
    *anchor = Some(path);
}

fn pretty_bytes(n: u64) -> String {
    if n < 1024 { return format!("{n} B"); }
    let mut value = n as f64;
    let mut unit = "B";
    for next in ["KB", "MB", "GB", "TB", "PB"] {
        if value < 1024.0 { break; }
        value /= 1024.0;
        unit = next;
    }
    format!("{value:.1} {unit}")
}

#[cfg(test)]
mod tests {
    use super::{apply_selection, pretty_bytes};
    use std::{collections::BTreeSet, path::PathBuf};

    #[test]
    fn pretty_bytes_uses_correct_binary_units() {
        assert_eq!(pretty_bytes(0), "0 B");
        assert_eq!(pretty_bytes(1023), "1023 B");
        assert_eq!(pretty_bytes(1024), "1.0 KB");
        assert_eq!(pretty_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(pretty_bytes(1024 * 1024 * 1024), "1.0 GB");
    }
    #[test]
    fn selection_range_and_toggle() {
        let entries = ["a", "b", "c", "d"].map(PathBuf::from);
        let mut selected = BTreeSet::new();
        let mut anchor = None;
        apply_selection(&mut selected, &mut anchor, &entries, entries[1].clone(), false, false);
        apply_selection(&mut selected, &mut anchor, &entries, entries[3].clone(), true, false);
        assert_eq!(selected.len(), 3);
        assert!(selected.contains(&entries[1]));
        assert!(selected.contains(&entries[2]));
        assert!(selected.contains(&entries[3]));
        apply_selection(&mut selected, &mut anchor, &entries, entries[2].clone(), false, true);
        assert!(!selected.contains(&entries[2]));
        assert_eq!(selected.len(), 2);
    }

    #[test]
    fn selection_anchor_disappears_under_filter() {
        let entries = [PathBuf::from("a"), PathBuf::from("c")];
        let mut selected = BTreeSet::new();
        let mut anchor = Some(PathBuf::from("b"));
        apply_selection(&mut selected, &mut anchor, &entries, entries[1].clone(), true, false);
        assert_eq!(selected.len(), 1);
        assert!(selected.contains(&entries[1]));
    }

}
