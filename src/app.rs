use crate::fs_ops::{self, Entry, Job};
use crate::{preview,search};
use chrono::{DateTime, Local};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc,atomic::{AtomicBool,Ordering}};
use std::time::Instant;

const ACCENT: Color32 = Color32::from_rgb(112, 175, 250);
const TEXT_MUTED: Color32 = Color32::from_rgb(144, 155, 171);

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    favorites: Vec<PathBuf>,
    tabs: Vec<PathBuf>,
    active: usize,
    secondary_path: Option<PathBuf>,
    dual_pane: bool,
    sidebar_visible: bool,
    split_ratio: f32,
    dark_theme: bool,
    preview_visible: bool,
}

impl Default for Saved {
    fn default() -> Self {
        Self {
            favorites: Vec::new(), tabs: Vec::new(), active: 0,
            secondary_path: None, dual_pane: false,
            sidebar_visible: true, split_ratio: 0.5,
            dark_theme: true, preview_visible: false,
        }
    }
}

fn bounded_split_ratio(value: f32) -> f32 {
    if value.is_finite() { value.clamp(0.25, 0.75) } else { 0.5 }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortBy { Name, Size, Modified }

fn sort_entries(entries: &mut [Entry], by: SortBy, descending: bool) {
    entries.sort_unstable_by(|a, b| {
        let order = match by {
            SortBy::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortBy::Size => a.size.cmp(&b.size),
            SortBy::Modified => a.modified.cmp(&b.modified),
        };
        let order = if descending { order.reverse() } else { order };
        b.is_dir.cmp(&a.is_dir)
            .then(order)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
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
    sort_by: SortBy,
    descending: bool,
}

impl Tab {
    fn new(id: u64, path: PathBuf) -> Self {
        Self {
            id, path, entries: Vec::new(), back: Vec::new(), forward: Vec::new(),
            loading: false, generation: 0, error: None,
            selected: BTreeSet::new(), anchor: None, filter: String::new(),
            sort_by: SortBy::Name, descending: false,
        }
    }
}

enum Message {
    FolderLoaded { tab_id: u64, generation: u64, result: Result<Vec<Entry>, String> },
    JobFinished { result: Result<String, String>, is_move: bool },
    JobProgress(fs_ops::Progress),
    SearchFinished { generation: u64, result: Result<Vec<search::Hit>, String> },
    PreviewFinished { generation: u64, result: Result<preview::Details, String> },
}

enum Dialog {
    None,
    NewFolder(String),
    Rename { source: PathBuf, name: String },
    Delete(Vec<PathBuf>),
    ExternalDrop { sources: Vec<PathBuf>, destination: PathBuf },
    ConfirmMove { sources: Vec<PathBuf>, destination_folder: PathBuf },
    BatchRename { sources: Vec<PathBuf>, prefix: String, find: String,
                  replace: String, start: String },
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
    sidebar_visible: bool,
    split_ratio: f32,
    dark_theme: bool,
    preview_visible: bool,
    preview_request: Option<PathBuf>,
    preview_generation: u64,
    preview_data: Option<preview::Details>,
    searching: bool,
    search_visible: bool,
    search_query: String,
    search_root: PathBuf,
    search_results: Vec<search::Hit>,
    search_generation: u64,
    search_cancel: Arc<AtomicBool>,
    job_cancel: Arc<AtomicBool>,
    job_progress: Option<fs_ops::Progress>,
    job_started_at: Option<Instant>,
}

impl Veldir {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visual = egui::Visuals::dark();
        visual.panel_fill = Color32::from_rgb(19, 24, 32);
        visual.window_fill = Color32::from_rgb(28, 34, 44);
        visual.extreme_bg_color = Color32::from_rgb(13, 18, 26);
        visual.selection.bg_fill = Color32::from_rgb(50, 86, 127);
        visual.hyperlink_color = ACCENT;
        visual.widgets.inactive.corner_radius = egui::CornerRadius::same(7);
        visual.widgets.hovered.corner_radius = egui::CornerRadius::same(7);
        visual.widgets.active.corner_radius = egui::CornerRadius::same(7);
        visual.widgets.inactive.weak_bg_fill = Color32::from_rgb(34, 42, 54);
        visual.widgets.hovered.weak_bg_fill = Color32::from_rgb(47, 59, 74);
        cc.egui_ctx.set_visuals(visual);
        cc.egui_ctx.style_mut_of(egui::Theme::Dark, |style| {
            style.spacing.item_spacing = egui::vec2(7.0, 6.0);
            style.spacing.button_padding = egui::vec2(10.0, 5.0);
        });

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
            sidebar_visible: saved.sidebar_visible,
            split_ratio: bounded_split_ratio(saved.split_ratio),
            dark_theme: saved.dark_theme,
            preview_visible: saved.preview_visible,
            preview_request: None, preview_generation: 0, preview_data: None,
            searching: false, search_visible: false, search_query: String::new(),
            search_root: PathBuf::new(), search_results: Vec::new(),
            search_generation: 0, search_cancel: Arc::new(AtomicBool::new(false)),
            job_cancel: Arc::new(AtomicBool::new(false)), job_progress: None,
            job_started_at: None,
        };
        if !app.dark_theme { cc.egui_ctx.set_visuals(egui::Visuals::light()); }
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
                            Ok(mut entries) => {
                                sort_entries(&mut entries, tab.sort_by, tab.descending);
                                tab.entries = entries;
                                tab.error = None;
                            }
                            Err(e) => { tab.error = Some(e); }
                        }
                    }
                }
                Message::JobFinished { result, is_move } => {
                    self.busy = false;
                    self.job_progress = None;
                    self.job_started_at = None;
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
                Message::JobProgress(progress) => {
                    self.job_progress = Some(progress);
                    self.status = format!("Completati {}/{} elementi",
                        progress.done_items, progress.total_items);
                }
                Message::SearchFinished { generation, result } => {
                    if generation == self.search_generation {
                        self.searching = false;
                        match result {
                            Ok(hits) => {
                                self.status = format!("{} risultati trovati",hits.len());
                                self.search_results = hits;
                            }
                            Err(e) => self.status = format!("Ricerca: {e}"),
                        }
                    }
                }
                Message::PreviewFinished {generation,result} => {
                    if generation == self.preview_generation {
                        self.preview_data = result.ok();
                    }
                }
            }
        }
    }

    fn begin_job(&mut self, job: Job) {
        if self.busy { return; }
        self.busy = true;
        self.job_progress = None;
        self.job_started_at = Some(Instant::now());
        self.job_cancel = Arc::new(AtomicBool::new(false));
        self.status = "Operazione in corso...".into();
        let sender = self.tx.clone();
        let ctx = self.context.clone();
        let cancelled = self.job_cancel.clone();
        let is_move = matches!(&job, Job::Move { .. } | Job::MoveMany { .. });
        std::thread::spawn(move || {
            let result = fs_ops::execute_progress(job,&cancelled,|progress| {
                let _ = sender.send(Message::JobProgress(progress));
                ctx.request_repaint();
            });
            let _ = sender.send(Message::JobFinished { result, is_move });
            ctx.request_repaint();
        });
    }

    fn start_search(&mut self) {
        let query = self.search_query.trim().to_owned();
        if query.is_empty() { self.status = "Inserisci almeno un carattere per cercare".into(); return; }
        self.search_cancel.store(true,Ordering::Relaxed);
        self.search_cancel = Arc::new(AtomicBool::new(false));
        self.search_generation += 1;
        let generation = self.search_generation;
        let root = self.search_root.clone();
        let sender = self.tx.clone();
        let ctx = self.context.clone();
        let cancelled = self.search_cancel.clone();
        self.searching = true;
        self.search_results.clear();
        self.status = "Ricerca in corso...".into();
        std::thread::spawn(move || {
            let result = search::find(&root,&query,250,&cancelled);
            let _ = sender.send(Message::SearchFinished{generation,result});
            ctx.request_repaint();
        });
    }

    fn update_preview(&mut self) {
        if !self.preview_visible { return; }
        let wanted = self.selected_one();
        if wanted == self.preview_request { return; }
        self.preview_generation += 1;
        self.preview_request = wanted.clone();
        self.preview_data = None;
        if let Some(path) = wanted {
            let generation = self.preview_generation;
            let sender = self.tx.clone();
            let ctx = self.context.clone();
            std::thread::spawn(move || {
                let result = preview::details(&path);
                let _ = sender.send(Message::PreviewFinished{generation,result});
                ctx.request_repaint();
            });
        }
    }

    fn search_ui(&mut self, ctx: &egui::Context) {
        if !self.search_visible { return; }
        let mut open = self.search_visible;
        let mut execute_search = false;
        let mut choose = None;
        let mut cancel_search = false;
        egui::Window::new("Ricerca ricorsiva").open(&mut open)
            .default_size([690.0,450.0]).resizable(true).show(ctx,|ui| {
            ui.label(RichText::new(format!("Cartella: {}",self.search_root.display())).small().color(TEXT_MUTED));
            ui.horizontal(|ui| {
                let response = ui.add(egui::TextEdit::singleline(&mut self.search_query)
                    .desired_width(380.0).hint_text("Nome file o ricerca fuzzy"));
                if response.lost_focus() && ui.input(|i|i.key_pressed(egui::Key::Enter)) { execute_search=true; }
                if ui.add_enabled(!self.searching,egui::Button::new("Cerca")).clicked() { execute_search=true; }
                if self.searching && ui.button("Interrompi").clicked() { cancel_search=true; }
            });
            ui.label(RichText::new("Ricerca in sottocartelle, massimo 250 risultati. Collegamenti esclusi.")
                .small().color(TEXT_MUTED));
            ui.separator();
            if self.searching { ui.spinner(); }
            egui::ScrollArea::vertical().max_height(330.0).show(ui,|ui| {
                for hit in &self.search_results {
                    let icon = if hit.is_dir {"▣"} else {"▤"};
                    if ui.add(egui::Button::new(format!("{icon} {}",hit.path.display())).frame(false))
                        .on_hover_text("Apri posizione").clicked() { choose=Some(hit.path.clone()); }
                }
            });
        });
        self.search_visible = open;
        if cancel_search {
            self.search_cancel.store(true,Ordering::Relaxed);
            self.search_generation += 1;
            self.searching = false;
            self.status="Ricerca interrotta".into();
        }
        if execute_search { self.start_search(); }
        if let Some(path)=choose {
            self.search_visible=false;
            if path.is_dir() { self.navigate(path); }
            else if let Some(parent)=path.parent() { self.navigate(parent.to_path_buf()); }
        }
    }

    fn preview_ui(&mut self, ctx: &egui::Context) {
        if !self.preview_visible { return; }
        let mut open = self.preview_visible;
        egui::Window::new("Dettagli e anteprima").open(&mut open)
            .default_size([430.0,480.0]).resizable(true).show(ctx,|ui| {
            if let Some(data)=&self.preview_data {
                ui.heading(data.path.file_name().unwrap_or_default().to_string_lossy());
                ui.label(format!("Tipo: {}  •  Dimensione: {}",data.kind,pretty_bytes(data.bytes)));
                ui.label(RichText::new(data.path.display().to_string()).small().color(TEXT_MUTED));
                ui.separator();
                if let Some(note)=&data.note { ui.label(RichText::new(note).small().color(TEXT_MUTED)); }
                if let Some(content)=&data.text {
                    egui::ScrollArea::both().max_height(350.0).show(ui,|ui| {
                        ui.add(egui::Label::new(RichText::new(content).monospace()).wrap());
                    });
                }
            } else {
                ui.label("Seleziona un file per visualizzarne i dettagli");
            }
        });
        self.preview_visible=open;
        if !open { self.preview_request=None; self.preview_data=None; }
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

    fn import_explorer_clipboard(&mut self) {
        if self.busy || !matches!(self.dialog, Dialog::None) { return; }
        match crate::shell_clipboard::files_from_explorer() {
            Ok(sources) => {
                self.dialog = Dialog::ExternalDrop {
                    sources, destination: self.current().path.clone(),
                };
            }
            Err(error) => { self.status = format!("Clipboard Windows: {error}"); }
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

    fn change_sort(&mut self, right: bool, by: SortBy) {
        let tab = self.panel_mut(right);
        if tab.sort_by == by { tab.descending = !tab.descending; }
        else { tab.sort_by = by; tab.descending = false; }
        sort_entries(&mut tab.entries, tab.sort_by, tab.descending);
    }

    fn select_all_visible(&mut self) {
        let tab = self.current_mut();
        let filter = tab.filter.to_lowercase();
        tab.selected = tab.entries.iter().filter(|item|
            filter.is_empty() || item.name.to_lowercase().contains(&filter))
            .map(|item| item.path.clone()).collect();
        tab.anchor = None;
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
            ctx.memory_mut(|m| m.request_focus(egui::Id::unique("address_bar")));
        }
        if ctx.input(|i|i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::F)) {
            self.search_visible = true;
            self.search_root = self.current().path.clone();
            self.search_results.clear();
        } else if shortcut(egui::Key::F) {
            let filter_id = if self.focused_right { "folder_filter_right" } else { "folder_filter" };
            ctx.memory_mut(|m| m.request_focus(egui::Id::unique(filter_id)));
        }
        if ctx.input(|i|i.key_pressed(egui::Key::F3)) {
            self.preview_visible = !self.preview_visible;
            self.preview_request = None;
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
            if ctx.input(|i| i.modifiers.command && i.modifiers.shift
                && i.key_pressed(egui::Key::V)) { self.import_explorer_clipboard(); }
            else if shortcut(egui::Key::V) { self.paste(); }
            if shortcut(egui::Key::A) { self.select_all_visible(); }
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.current_mut().selected.clear();
                self.current_mut().anchor = None;
            }
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

    fn breadcrumbs_ui(&mut self, ui: &mut egui::Ui) {
        let path = self.current().path.clone();
        let ancestors: Vec<PathBuf> = path.ancestors().map(Path::to_path_buf).collect();
        let mut navigation = None;
        ui.horizontal(|ui| {
            for (index, crumb) in ancestors.iter().rev().enumerate() {
                if index > 0 { ui.label(RichText::new("›").color(TEXT_MUTED)); }
                let part = crumb.file_name().and_then(|n| n.to_str())
                    .map(str::to_owned).unwrap_or_else(|| crumb.display().to_string());
                let is_current = crumb == &path;
                let label = if part.chars().count() > 22 {
                    format!("{}…", part.chars().take(21).collect::<String>())
                } else { part };
                if is_current {
                    ui.label(RichText::new(label).strong().color(ACCENT));
                } else if ui.add(egui::Button::new(label).frame(false))
                    .on_hover_text(crumb.display().to_string()).clicked() {
                    navigation = Some(crumb.clone());
                }
            }
        });
        if let Some(target) = navigation { self.navigate(target); }
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
                    .id(egui::Id::unique("address_bar")));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let target = PathBuf::from(self.address_input.trim());
                    self.navigate(target);
                }
                if ui.button("Vai").clicked() { self.navigate(PathBuf::from(self.address_input.trim())); }
            } else {
                if ui.small_button("⌁").on_hover_text("Modifica percorso · Ctrl+L").clicked() {
                    self.editing_address = true;
                    self.address_input = self.current().path.display().to_string();
                }
                self.breadcrumbs_ui(ui);
            }
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button("⌕ Cerca").on_hover_text("Ricerca nelle sottocartelle · Ctrl+Maiusc+F").clicked() {
                self.search_visible = true;
                self.search_root = self.current().path.clone();
                self.search_results.clear();
            }
            if ui.button(if self.preview_visible { "▤ Chiudi anteprima" } else { "▤ Anteprima" })
                .on_hover_text("Anteprima testuale e dettagli · F3").clicked() {
                self.preview_visible = !self.preview_visible;
                self.preview_request = None;
            }
            if ui.button(if self.dark_theme { "☼ Tema chiaro" } else { "☾ Tema scuro" })
                .on_hover_text("Cambia il tema di Veldir").clicked() {
                self.dark_theme = !self.dark_theme;
                if self.dark_theme { self.context.set_visuals(egui::Visuals::dark()); }
                else { self.context.set_visuals(egui::Visuals::light()); }
            }
            if ui.button(if self.sidebar_visible { "◧ Nascondi barra" } else { "☰ Mostra barra" })
                .on_hover_text("Mostra/nascondi la barra laterale").clicked() {
                self.sidebar_visible = !self.sidebar_visible;
            }
            ui.separator();
            if ui.add_enabled(!self.busy, egui::Button::new("＋ Cartella"))
                .on_hover_text("Crea cartella").clicked() {
                self.dialog = Dialog::NewFolder("Nuova cartella".into());
            }
            let selected = !self.selected().is_empty();
            let single = self.selected_one().is_some();
            if ui.add_enabled(!self.busy && single, egui::Button::new("Rinomina"))
                .on_hover_text("F2").clicked() { self.prepare_rename(); }
            if ui.add_enabled(!self.busy && self.selected().len()>1, egui::Button::new("Rinomina più file"))
                .on_hover_text("Numerazione e sostituzione di testo").clicked() {
                self.dialog = Dialog::BatchRename {
                    sources:self.selected(), prefix:String::new(), find:String::new(),
                    replace:String::new(), start:"1".into()
                };
            }
            if ui.add_enabled(!self.busy && selected, egui::Button::new("Copia"))
                .on_hover_text("Ctrl+C").clicked() { self.clipboard_set(false); }
            if ui.add_enabled(!self.busy && selected, egui::Button::new("Taglia"))
                .on_hover_text("Ctrl+X").clicked() { self.clipboard_set(true); }
            if ui.add_enabled(!self.busy && self.clipboard.is_some(), egui::Button::new("Incolla"))
                .on_hover_text("Ctrl+V · clipboard interna").clicked() { self.paste(); }
            if ui.add_enabled(!self.busy, egui::Button::new("Da Esplora"))
                .on_hover_text("Copia file dalla clipboard Windows · Ctrl+Maiusc+V").clicked() {
                self.import_explorer_clipboard();
            }
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
        let mut menu_action: Option<(&'static str,PathBuf)> = None;
        let mut requested_sort = None;
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
        let heading = |by: SortBy, label: &str| -> String {
            if tab.sort_by == by {
                format!("{label} {}", if tab.descending { "↓" } else { "↑" })
            } else { label.to_owned() }
        };
        ui.horizontal(|ui| {
            if ui.add_sized([w_name, 24.0], egui::Button::new(
                RichText::new(heading(SortBy::Name, "NOME")).small().color(TEXT_MUTED).strong())
                .frame(false)).on_hover_text("Ordina per nome").clicked() {
                requested_sort = Some(SortBy::Name);
            }
            if !dual { ui.add_sized([w_type, 24.0], egui::Label::new(RichText::new("TIPO").small().color(TEXT_MUTED).strong())); }
            if ui.add_sized([w_size, 24.0], egui::Button::new(
                RichText::new(heading(SortBy::Size, "DIMENSIONE")).small().color(TEXT_MUTED).strong())
                .frame(false)).on_hover_text("Ordina per dimensione").clicked() {
                requested_sort = Some(SortBy::Size);
            }
            if !dual && ui.add(egui::Button::new(
                RichText::new(heading(SortBy::Modified, "MODIFICATO")).small().color(TEXT_MUTED).strong())
                .frame(false)).on_hover_text("Ordina per data").clicked() {
                requested_sort = Some(SortBy::Modified);
            }
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
                    let extension = item.path.extension().and_then(|s|s.to_str())
                        .unwrap_or("").to_ascii_lowercase();
                    let symbol = if item.is_dir { "▣" } else { match extension.as_str() {
                        "pdf" => "▧", "doc" | "docx" | "txt" | "md" => "▤",
                        "xls" | "xlsx" | "csv" => "▥",
                        "jpg" | "jpeg" | "png" | "webp" | "gif" => "▨",
                        "zip" | "7z" | "rar" => "▦",
                        "exe" | "msi" => "◈", _ => "▫",
                    }};
                    let name = format!("{symbol}   {}",item.name);
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
                    response.context_menu(|ui| {
                        ui.label(RichText::new(&item.name).strong());
                        ui.separator();
                        for (label,action) in [
                            ("Apri","open"),("Copia","copy"),("Taglia","cut"),
                            ("Rinomina","rename"),("Dettagli","details"),("Cestino","trash")
                        ] {
                            if ui.button(label).clicked() {
                                menu_action = Some((action,item.path.clone()));
                                ui.close();
                            }
                        }
                    });
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
        if let Some(by) = requested_sort { self.change_sort(right, by); }
        if let Some((action,path)) = menu_action {
            self.focus(right);
            let tab=self.current_mut();
            tab.selected.clear();tab.selected.insert(path.clone());
            tab.anchor=Some(path.clone());
            match action {
                "open" => self.open_entry(path),
                "copy" => self.clipboard_set(false),
                "cut" => self.clipboard_set(true),
                "rename" => self.prepare_rename(),
                "details" => {self.preview_visible=true;self.preview_request=None;},
                "trash" => self.prepare_trash(),
                _ => {}
            }
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
                .id(egui::Id::unique(if right { "folder_filter_right" } else { "folder_filter" }))
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
        let mut dialog_error = None;
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
            Dialog::BatchRename {sources,prefix,find,replace,start} => {
                egui::Window::new("Rinomina multipla").collapsible(false).resizable(true)
                    .default_width(480.0).show(ctx,|ui| {
                    ui.label(format!("{} file selezionati. Numero + prefisso + nome originale.",sources.len()));
                    ui.horizontal(|ui| { ui.label("Prefisso"); ui.text_edit_singleline(prefix); });
                    ui.horizontal(|ui| { ui.label("Cerca testo"); ui.text_edit_singleline(find); });
                    ui.horizontal(|ui| { ui.label("Sostituisci con"); ui.text_edit_singleline(replace); });
                    ui.horizontal(|ui| { ui.label("Numerazione da"); ui.text_edit_singleline(start); });
                    let plan = start.parse::<u32>().map_err(|_|"Numero iniziale non valido".to_owned())
                        .and_then(|n| fs_ops::rename_plan(sources,prefix,find,replace,n));
                    ui.separator();
                    match &plan {
                        Ok(plan) => {
                            ui.label("Anteprima dei primi cinque nomi:");
                            for (source,name) in plan.iter().take(5) {
                                ui.label(format!("{}  →  {name}",
                                    source.file_name().unwrap_or_default().to_string_lossy()));
                            }
                        }
                        Err(err) => {ui.colored_label(Color32::LIGHT_RED,err); }
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Rinomina i file").clicked() {
                            match plan {
                                Ok(changes) => {job=Some(Job::RenameMany{changes});close=true;}
                                Err(err) => {dialog_error=Some(err);}
                            }
                        }
                        if ui.button("Annulla").clicked() {close=true;}
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
                egui::Window::new("Importa file da Windows").collapsible(false).resizable(false)
                    .show(ctx, |ui| {
                        ui.label(format!("{} file/cartelle selezionati da Windows", sources.len()));
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
        if let Some(err) = dialog_error { self.status = format!("Rinomina: {err}"); }
        if let Some(job) = job { self.begin_job(job); }
    }
}

impl eframe::App for Veldir {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.pump();
        self.update_preview();
        self.accept_os_drop(ui.ctx());
        self.keyboard(ui.ctx());
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("◈ VELDIR").strong().size(18.0).color(ACCENT));
            ui.label(RichText::new("0.5.0   •   FILE MANAGER FOR WINDOWS").small().color(TEXT_MUTED));
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
                if self.sidebar_visible {
                    ui.vertical(|ui| { self.sidebar_ui(ui); });
                    ui.separator();
                }
                if self.secondary.is_some() {
                    let available = ui.available_width().max(470.0);
                    let usable = (available - 12.0).max(450.0);
                    let left_width = (usable * self.split_ratio).clamp(220.0, usable - 220.0);
                    let right_width = usable - left_width;
                    ui.allocate_ui_with_layout(
                        egui::vec2(left_width, content_height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| { ui.set_width(left_width); self.pane_ui(ui, false); },
                    );

                    // A draggable splitter, persisting its relative width across launches.
                    let (rect, splitter) = ui.allocate_exact_size(
                        egui::vec2(10.0, content_height), egui::Sense::click_and_drag());
                    let splitter = splitter.on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
                        .on_hover_text("Trascina per ridimensionare i pannelli");
                    let color = if splitter.dragged() || splitter.hovered() {
                        ACCENT
                    } else { Color32::from_rgb(71, 83, 101) };
                    let center = rect.center();
                    ui.painter().rect_filled(
                        egui::Rect::from_center_size(center, egui::vec2(3.0, 46.0)),
                        egui::CornerRadius::same(2), color);
                    if splitter.dragged() {
                        self.split_ratio = bounded_split_ratio(
                            self.split_ratio + splitter.drag_delta().x / usable);
                        ui.ctx().request_repaint();
                    }

                    ui.allocate_ui_with_layout(
                        egui::vec2(right_width, content_height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| { ui.set_width(right_width); self.pane_ui(ui, true); },
                    );
                } else {
                    let width = ui.available_width().max(400.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(width, content_height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| { ui.set_width(width); self.pane_ui(ui, false); },
                    );
                }
            },
        );
        ui.separator();
        ui.horizontal(|ui| {
            let tab = self.current();
            let count = tab.entries.iter().filter(|e| tab.filter.is_empty() ||
                e.name.to_lowercase().contains(&tab.filter.to_lowercase())).count();
            ui.label(RichText::new(format!("{count} elementi  ·  {} selezionati", tab.selected.len())).small().color(TEXT_MUTED));
            ui.label(RichText::new(tab.path.display().to_string()).small().color(TEXT_MUTED));
            ui.separator();
            if self.busy {
                ui.spinner();
                if let Some(p)=self.job_progress {
                    let ratio = if p.total_bytes > 0 {
                        p.done_bytes as f32 / p.total_bytes as f32
                    } else {
                        p.done_items as f32 / p.total_items.max(1) as f32
                    };
                    let caption = if p.total_bytes > 0 {
                        format!("{} / {}", pretty_bytes(p.done_bytes), pretty_bytes(p.total_bytes))
                    } else {
                        format!("{} / {} elementi", p.done_items, p.total_items)
                    };
                    ui.add(egui::ProgressBar::new(ratio.clamp(0.0,1.0))
                        .desired_width(160.0).text(caption));
                    if let Some(started)=self.job_started_at {
                        let elapsed=started.elapsed().as_secs_f64();
                        if p.done_bytes > 0 && elapsed >= 0.2 {
                            let rate=(p.done_bytes as f64/elapsed).round() as u64;
                            ui.label(RichText::new(format!("{}/s",pretty_bytes(rate)))
                                .small().color(TEXT_MUTED));
                        }
                    }
                }
                if ui.button("Annulla").on_hover_text("Durante la copia interrompe anche il file in corso, senza pubblicare il file parziale").clicked() {
                    self.job_cancel.store(true,Ordering::Relaxed);
                    self.status="Annullamento richiesto…".into();
                }
            }
            ui.label(RichText::new(&self.status).small().color(TEXT_MUTED));
        });
        self.dialog_ui(ui.ctx());
        self.search_ui(ui.ctx());
        self.preview_ui(ui.ctx());
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let saved = Saved {
            favorites: self.favorites.clone(),
            tabs: self.tabs.iter().map(|t| t.path.clone()).collect(),
            active: self.active,
            secondary_path: self.secondary.as_ref().map(|p| p.path.clone()),
            dual_pane: self.secondary.is_some(),
            sidebar_visible: self.sidebar_visible,
            split_ratio: self.split_ratio,
            dark_theme: self.dark_theme,
            preview_visible: self.preview_visible,
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
    use super::{apply_selection, pretty_bytes, bounded_split_ratio, sort_entries, Saved, SortBy};
    use crate::fs_ops::Entry;
    use std::{collections::BTreeSet, path::PathBuf};

    #[test]
    fn sorting_keeps_folders_first_in_both_directions() {
        let make = |name: &str, size, is_dir| Entry {
            name: name.to_owned(), path: PathBuf::from(name), is_dir,
            size, modified: None,
        };
        let mut files = vec![make("small.txt", 1, false), make("folder", 0, true),
                             make("big.txt", 100, false)];
        sort_entries(&mut files, SortBy::Size, false);
        assert_eq!(files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
                   vec!["folder", "small.txt", "big.txt"]);
        sort_entries(&mut files, SortBy::Size, true);
        assert_eq!(files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
                   vec!["folder", "big.txt", "small.txt"]);
    }

    #[test]
    fn previous_sessions_default_to_visible_sidebar_and_even_split() {
        let saved = Saved::default();
        assert!(saved.sidebar_visible);
        assert!((saved.split_ratio - 0.5).abs() < f32::EPSILON);
        assert_eq!(bounded_split_ratio(f32::NAN), 0.5);
        assert_eq!(bounded_split_ratio(-1.0), 0.25);
        assert_eq!(bounded_split_ratio(2.0), 0.75);
    }

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
