use crate::fs_ops::{self, Entry, Job};
use crate::{preview,search,image_view,native_icons,filters,folder_watch};
use chrono::{DateTime, Local};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet,HashMap,HashSet,VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc,atomic::{AtomicBool,Ordering}};
use std::time::{Instant,Duration,SystemTime};

const ACCENT: Color32 = Color32::from_rgb(112, 175, 250);
const TEXT_MUTED: Color32 = Color32::from_rgb(144, 155, 171);


/// Configure both egui theme slots: the native system preference must not
/// mix light widgets with the application's persisted dark surfaces.
fn apply_theme(ctx: &egui::Context, dark: bool) {
    ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |style| {
            style.visuals = if theme == egui::Theme::Dark {
                let mut v = egui::Visuals::dark();
                v.override_text_color = Some(Color32::from_rgb(228, 234, 243));
                v.panel_fill = Color32::from_rgb(19, 24, 32);
                v.window_fill = Color32::from_rgb(28, 34, 44);
                v.extreme_bg_color = Color32::from_rgb(15, 20, 28);
                v.selection.bg_fill = Color32::from_rgb(42, 79, 119);
                v.selection.stroke = egui::Stroke::new(1.0, Color32::WHITE);
                for (widget, fill) in [
                    (&mut v.widgets.inactive, Color32::from_rgb(34, 43, 56)),
                    (&mut v.widgets.hovered, Color32::from_rgb(49, 63, 81)),
                    (&mut v.widgets.active, Color32::from_rgb(42, 79, 119)),
                ] {
                    widget.bg_fill = fill;
                    widget.weak_bg_fill = fill;
                    widget.fg_stroke = egui::Stroke::new(1.0, Color32::from_rgb(228, 234, 243));
                    widget.corner_radius = egui::CornerRadius::same(5);
                }
                v
            } else { egui::Visuals::light() };
            style.spacing.item_spacing = egui::vec2(7.0, 5.0);
            style.spacing.button_padding = egui::vec2(10.0, 6.0);
        });
    }
}


/// Manual header cells avoid grow atoms consuming the entire remaining pane.
fn file_header_ui(
    ui: &mut egui::Ui, width: f32, compact: bool, by: SortBy, descending: bool,
) -> Option<SortBy> {
    let widths = column_widths(width, compact);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 26.0), egui::Sense::hover());
    let mut x = rect.left();
    let mut sort = None;
    for (index, label, key) in [
        (0, "NOME", Some(SortBy::Name)), (1, "TIPO", None),
        (2, "DIMENSIONE", Some(SortBy::Size)), (3, "MODIFICATO", Some(SortBy::Modified)),
    ] {
        let cell_width = widths[index];
        if cell_width == 0.0 { continue; }
        let cell = egui::Rect::from_min_size(egui::pos2(x, rect.top()),
            egui::vec2(cell_width, rect.height()));
        let caption = if key == Some(by) {
            format!("{label} {}", if descending { "v" } else { "^" })
        } else { label.to_owned() };
        let response = ui.interact(cell, ui.scope_id().with(("column_header", index)),
            if key.is_some() { egui::Sense::click() } else { egui::Sense::hover() });
        if response.hovered() && key.is_some() {
            ui.painter().rect_filled(cell, egui::CornerRadius::same(3),
                ui.visuals().widgets.hovered.weak_bg_fill);
        }
        ui.painter().with_clip_rect(cell.intersect(ui.clip_rect())).text(
            egui::pos2(cell.left() + 8.0, cell.center().y),
            egui::Align2::LEFT_CENTER, caption, egui::FontId::proportional(11.0),
            ui.visuals().text_color());
        if response.on_hover_text(format!("Ordina per {}", label.to_lowercase())).clicked() {
            sort = key;
        }
        x += cell_width + 7.0;
    }
    sort
}

fn column_widths(width: f32, compact: bool) -> [f32; 4] {
    let usable = (width - 24.0).max(180.0);
    if compact { [usable - 80.0, 0.0, 80.0, 0.0] }
    else {
        let kind = (usable * 0.16).clamp(72.0, 150.0);
        let size = (usable * 0.13).clamp(80.0, 110.0);
        let date = 145.0;
        [ (usable - kind - size - date).max(100.0), kind, size, date ]
    }
}

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
    thumbs_visible: bool,
    live_refresh: bool,
}

impl Default for Saved {
    fn default() -> Self {
        Self {
            favorites: Vec::new(), tabs: Vec::new(), active: 0,
            secondary_path: None, dual_pane: false,
            sidebar_visible: true, split_ratio: 0.5,
            dark_theme: true, preview_visible: false,
            thumbs_visible: true,
            live_refresh: true,
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
    criteria: filters::Criteria,
}

impl Tab {
    fn new(id: u64, path: PathBuf) -> Self {
        Self {
            id, path, entries: Vec::new(), back: Vec::new(), forward: Vec::new(),
            loading: false, generation: 0, error: None,
            selected: BTreeSet::new(), anchor: None, filter: String::new(),
            sort_by: SortBy::Name, descending: false,
            criteria: filters::Criteria::default(),
        }
    }
}

enum Message {
    FolderLoaded { tab_id: u64, generation: u64, result: Result<Vec<Entry>, String> },
    JobFinished { result: Result<String, String>, is_move: bool },
    JobProgress(fs_ops::Progress),
    SearchFinished { generation: u64, result: Result<Vec<search::Hit>, String> },
    PreviewFinished { generation: u64, result: Result<preview::Details, String> },
    PreviewImageLoaded { generation:u64, result:Result<image_view::Bitmap,String> },
    ThumbnailLoaded {path:PathBuf,result:Result<image_view::Bitmap,String>},
    IconLoaded {key:String,result:Result<image_view::Bitmap,String>},
    WatchedPath(PathBuf),
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

struct DragIntent {
    sources: Vec<PathBuf>,
    source_right: bool,
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
    drag_intent: Option<DragIntent>,
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
    preview_image: Option<egui::TextureHandle>,
    thumbs_visible: bool,
    thumb_cache: HashMap<PathBuf,egui::TextureHandle>,
    thumb_order: VecDeque<PathBuf>,
    thumb_pending: HashSet<PathBuf>,
    thumb_failed: HashSet<PathBuf>,
    icon_cache: HashMap<String,egui::TextureHandle>,
    icon_pending: HashSet<String>,
    icon_failed: HashSet<String>,
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
    watcher: folder_watch::FolderWatcher,
    watched_paths: Vec<PathBuf>,
    watch_dirty: HashMap<PathBuf,Instant>,
    live_refresh: bool,
}

impl Veldir {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let saved: Saved = cc.storage.and_then(|s| eframe::get_value(s, "veldir_state"))
            .unwrap_or_default();
        Self::from_saved(cc.egui_ctx.clone(), saved)
    }

    fn from_saved(context: egui::Context, saved: Saved) -> Self {
        let (tx, rx) = mpsc::channel();
        let watch_tx=tx.clone();
        let watch_ctx=context.clone();
        let watcher=folder_watch::FolderWatcher::start(move |path| {
            let _=watch_tx.send(Message::WatchedPath(path));
            watch_ctx.request_repaint();
        });
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
            next_id: 1, tx, rx, context: context.clone(),
            favorites: saved.favorites, shortcuts, drives,
            clipboard: None, drag_intent: None,
            busy: false, dialog: Dialog::None,
            status: "Pronto".into(), editing_address: false, address_input: String::new(),
            sidebar_visible: saved.sidebar_visible,
            split_ratio: bounded_split_ratio(saved.split_ratio),
            dark_theme: saved.dark_theme,
            preview_visible: saved.preview_visible,
            preview_request: None, preview_generation: 0, preview_data: None,
            preview_image: None, thumbs_visible: saved.thumbs_visible,
            thumb_cache: HashMap::new(), thumb_order: VecDeque::new(),
            thumb_pending: HashSet::new(), thumb_failed: HashSet::new(),
            icon_cache: HashMap::new(), icon_pending: HashSet::new(),
            icon_failed: HashSet::new(),
            searching: false, search_visible: false, search_query: String::new(),
            search_root: PathBuf::new(), search_results: Vec::new(),
            search_generation: 0, search_cancel: Arc::new(AtomicBool::new(false)),
            job_cancel: Arc::new(AtomicBool::new(false)), job_progress: None,
            job_started_at: None,
            watcher,watched_paths:Vec::new(),watch_dirty:HashMap::new(),
            live_refresh:saved.live_refresh,
        };
        apply_theme(&context, app.dark_theme);
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

    fn sync_watch_paths(&mut self) {
        let mut paths:Vec<PathBuf>=if self.live_refresh {
            self.tabs.iter().map(|t|t.path.clone())
                .chain(self.secondary.iter().map(|t|t.path.clone()))
                .filter(|p|p.is_dir()).collect()
        }else{Vec::new()};
        paths.sort();paths.dedup();
        if paths!=self.watched_paths {
            self.watcher.set_paths(paths.clone());
            self.watched_paths=paths;
        }
    }

    fn quiet_reload_path(&mut self,path:&Path) {
        let mut requests=Vec::new();
        for tab in self.tabs.iter_mut().chain(self.secondary.iter_mut()) {
            if tab.path==path && !tab.loading {
                tab.generation+=1;
                // Preserve the current list, scrollbar, and selection during reload.
                requests.push((tab.path.clone(),tab.id,tab.generation));
            }
        }
        for (path,id,generation) in requests {
            let sender=self.tx.clone();
            let ctx=self.context.clone();
            std::thread::spawn(move || {
                let result=fs_ops::list(&path);
                let _=sender.send(Message::FolderLoaded{tab_id:id,generation,result});
                ctx.request_repaint();
            });
        }
    }

    fn apply_watch_changes(&mut self,ctx:&egui::Context) {
        if !self.live_refresh {
            self.watch_dirty.clear();
            return;
        }
        if self.watch_dirty.is_empty(){return;}
        if self.busy {
            ctx.request_repaint_after(Duration::from_millis(500));
            return;
        }
        let now=Instant::now();
        let mut affected=HashSet::new();
        self.watch_dirty.retain(|changed,last| {
            if now.duration_since(*last)<Duration::from_millis(450) {return true;}
            for directory in &self.watched_paths {
                if directory==changed || changed.parent()==Some(directory.as_path()) {
                    affected.insert(directory.clone());
                }
            }
            false
        });
        for path in affected {self.quiet_reload_path(&path);}
        if !self.watch_dirty.is_empty(){
            ctx.request_repaint_after(Duration::from_millis(500));
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
                Message::WatchedPath(path)=> {
                    if self.live_refresh {
                        if self.thumb_cache.remove(&path).is_some(){
                            self.thumb_order.retain(|p|p!=&path);
                        }
                        self.thumb_failed.remove(&path);
                        self.watch_dirty.insert(path,Instant::now());
                    }
                }
                Message::FolderLoaded { tab_id, generation, result } => {
                    let tab = self.tabs.iter_mut().find(|t| t.id == tab_id && t.generation == generation)
                        .or_else(|| self.secondary.as_mut()
                            .filter(|t| t.id == tab_id && t.generation == generation));
                    if let Some(tab) = tab {
                        tab.loading = false;
                        match result {
                            Ok(mut entries) => {
                                sort_entries(&mut entries, tab.sort_by, tab.descending);
                                let known:HashSet<PathBuf>=entries.iter()
                                    .map(|e|e.path.clone()).collect();
                                tab.selected.retain(|p|known.contains(p));
                                if tab.anchor.as_ref().is_some_and(|a|!known.contains(a)) {
                                    tab.anchor=None;
                                }
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
                Message::PreviewImageLoaded {generation,result} => {
                    if generation == self.preview_generation {
                        self.preview_image = result.ok().map(|bitmap|
                            Self::make_texture(&self.context,"veldir-preview",bitmap));
                    }
                }
                Message::ThumbnailLoaded {path,result} => {
                    self.thumb_pending.remove(&path);
                    match result {
                        Ok(bitmap) => {
                            // LRU-ish bounded cache; old textures are released.
                            while self.thumb_order.len() >= 128 {
                                if let Some(old)=self.thumb_order.pop_front() {
                                    self.thumb_cache.remove(&old);
                                }
                            }
                            let tex=Self::make_texture(&self.context,
                                &format!("thumb:{}",path.display()),bitmap);
                            self.thumb_cache.insert(path.clone(),tex);
                            self.thumb_order.push_back(path);
                        }
                        Err(_) => {self.thumb_failed.insert(path);}
                    }
                }
                Message::IconLoaded {key,result} => {
                    self.icon_pending.remove(&key);
                    match result {
                        Ok(bitmap) => {
                            let tex=Self::make_texture(&self.context,
                                &format!("shell-icon:{key}"),bitmap);
                            self.icon_cache.insert(key,tex);
                        }
                        Err(_) => {self.icon_failed.insert(key);}
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

    fn make_texture(ctx:&egui::Context,label:&str,bitmap:image_view::Bitmap)
        -> egui::TextureHandle {
        let image=egui::ColorImage::from_rgba_unmultiplied(
            [bitmap.width,bitmap.height],&bitmap.rgba);
        ctx.load_texture(label,image,egui::TextureOptions::LINEAR)
    }

    fn schedule_assets(&mut self,images:Vec<PathBuf>,icons:Vec<String>) {
        if self.thumbs_visible {
            for path in images {
                if self.thumb_pending.len() >= 6 {break;}
                if self.thumb_pending.contains(&path) || self.thumb_cache.contains_key(&path)
                   || self.thumb_failed.contains(&path) {continue;}
                self.thumb_pending.insert(path.clone());
                let sender=self.tx.clone();
                let ctx=self.context.clone();
                std::thread::spawn(move || {
                    let result=image_view::load(&path,40);
                    let _=sender.send(Message::ThumbnailLoaded{path,result});
                    ctx.request_repaint();
                });
            }
        }
        for key in icons {
            if self.icon_pending.len() >= 4 {break;}
            if self.icon_pending.contains(&key) || self.icon_cache.contains_key(&key)
                || self.icon_failed.contains(&key) {continue;}
            self.icon_pending.insert(key.clone());
            let sender=self.tx.clone();
            let ctx=self.context.clone();
            std::thread::spawn(move || {
                let result=native_icons::load(&key);
                let _=sender.send(Message::IconLoaded{key,result});
                ctx.request_repaint();
            });
        }
    }

    fn update_preview(&mut self) {
        if !self.preview_visible { return; }
        let wanted = self.selected_one();
        if wanted == self.preview_request { return; }
        self.preview_generation += 1;
        self.preview_request = wanted.clone();
        self.preview_data = None;
        self.preview_image = None;
        if let Some(path) = wanted {
            let generation = self.preview_generation;
            let sender = self.tx.clone();
            let ctx = self.context.clone();
            if image_view::supported(&path) {
                let image_path=path.clone();
                let image_tx=self.tx.clone();
                let image_ctx=self.context.clone();
                std::thread::spawn(move || {
                    let result=image_view::load(&image_path,1200);
                    let _=image_tx.send(Message::PreviewImageLoaded{generation,result});
                    image_ctx.request_repaint();
                });
            }
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
                if let Some(tex)=&self.preview_image {
                    let size=tex.size_vec2();
                    let scale=(440.0/size.x).min(340.0/size.y).min(1.0);
                    ui.add(egui::Image::new(tex).fit_to_exact_size(size*scale));
                }
                if let Some(note)=&data.note {
                    if self.preview_image.is_none() {
                        ui.label(RichText::new(note).small().color(TEXT_MUTED));
                    }
                }
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
        if !open {
            self.preview_request=None;
            self.preview_data=None;
            self.preview_image=None;
        }
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
        if sources.is_empty() { return; }
        self.clipboard = Some(InternalClipboard { sources: sources.clone(), cut });
        if cut {
            // Cut stays local until a confirmed move. Never publish a MOVE intent to Explorer.
            self.status = "Taglia pronto in Veldir: incolla con Ctrl+V (solo interno)".into();
        } else {
            match crate::shell_clipboard::copy_files_to_explorer(&sources) {
                Ok(()) => self.status = format!(
                    "{} elementi copiati: clipboard Veldir e Windows", sources.len()),
                Err(error) => self.status = format!(
                    "Copia interna pronta. Clipboard Windows non aggiornata: {error}"),
            }
        }
    }

    fn copy_selection_to_windows(&mut self) {
        let sources=self.selected();
        if sources.is_empty() { return; }
        match crate::shell_clipboard::copy_files_to_explorer(&sources) {
            Ok(()) => self.status = format!(
                "{} elementi copiati nella clipboard Windows",sources.len()),
            Err(err) => self.status = format!("Clipboard Windows: {err}"),
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
        let criteria = tab.criteria;
        let now = SystemTime::now();
        tab.selected = tab.entries.iter().filter(|item|
            filters::matches(item,&filter,criteria,now))
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
        let dark=self.dark_theme;
        ui.horizontal_wrapped(|ui| {
            for (i,tab) in self.tabs.iter().enumerate() {
                let selected=self.active==i;
                let name=tab.path.file_name().and_then(|p|p.to_str())
                    .map(str::to_owned).unwrap_or_else(||tab.path.display().to_string());
                let title=if name.chars().count()>19 {
                    format!("{}…",name.chars().take(18).collect::<String>())
                }else{name};
                let fill=if selected {
                    if dark {Color32::from_rgb(42,63,88)}
                    else {Color32::from_rgb(218,233,249)}
                }else if dark {Color32::from_rgb(29,36,47)}
                    else {Color32::from_rgb(235,238,243)};
                egui::Frame::new().fill(fill)
                    .corner_radius(egui::CornerRadius::same(9))
                    .inner_margin(egui::Margin::symmetric(7,3)).show(ui,|ui|{
                    ui.horizontal(|ui|{
                        let caption=if selected {
                            RichText::new(format!("{title}")).strong()
                        }else {RichText::new(format!("{title}"))};
                        if ui.add(egui::Button::new(caption).frame(false))
                            .on_hover_text(tab.path.display().to_string()).clicked(){
                            switch=Some(i);
                        }
                        if ui.add(egui::Button::new("×").frame(false))
                            .on_hover_text("Chiudi scheda").clicked(){close=Some(tab.id);}
                    });
                });
            }
            if ui.button("+").on_hover_text("Nuova scheda · Ctrl+T").clicked(){
                switch=Some(usize::MAX);
            }
        });
        if let Some(id)=close{self.close_tab(id);}
        if let Some(index)=switch{
            if index==usize::MAX{self.add_tab(self.current().path.clone());}
            else if index<self.tabs.len(){
                self.focused_right=false;
                self.active=index;
                self.address_input=self.current().path.display().to_string();
                self.editing_address=false;
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

    fn toolbar_ui(&mut self, ui:&mut egui::Ui) {
        let dark=self.dark_theme;
        let background=if dark{Color32::from_rgb(29,37,49)}
            else{Color32::from_rgb(238,243,249)};
        egui::Frame::new()
            .fill(background)
            .corner_radius(egui::CornerRadius::same(11))
            .inner_margin(egui::Margin::symmetric(10,9))
            .show(ui,|ui|{
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui|{
                if ui.add_enabled(!self.current().back.is_empty(),
                    egui::Button::new("<")).on_hover_text("Indietro · Alt+←").clicked(){
                    self.history_back();
                }
                if ui.add_enabled(!self.current().forward.is_empty(),
                    egui::Button::new(">")).on_hover_text("Avanti · Alt+→").clicked(){
                    self.history_forward();
                }
                if ui.button("Su").on_hover_text("Cartella superiore").clicked(){self.up();}
                if ui.button("Aggiorna").on_hover_text("Aggiorna · F5").clicked(){self.refresh();}
                ui.separator();
                if self.editing_address {
                    let response=ui.add(egui::TextEdit::singleline(&mut self.address_input)
                        .desired_width((ui.available_width()-75.0).max(180.0))
                        .id(egui::Id::unique("address_bar")));
                    let go=ui.button("Vai").clicked();
                    if (response.lost_focus() && ui.input(|i|i.key_pressed(egui::Key::Enter))) || go {
                        self.navigate(PathBuf::from(self.address_input.trim()));
                    }
                }else{
                    if ui.small_button("Percorso").on_hover_text("Modifica percorso · Ctrl+L").clicked(){
                        self.editing_address=true;
                        self.address_input=self.current().path.display().to_string();
                    }
                    egui::ScrollArea::horizontal().id_salt("breadcrumbs_scroll")
                        .auto_shrink([false, true]).max_width(ui.available_width())
                        .show(ui, |ui| self.breadcrumbs_ui(ui));
                }
            });
            ui.add_space(5.0);
            ui.horizontal(|ui|{
                if ui.button("+ Nuova cartella").on_hover_text("Crea una cartella").clicked() && !self.busy{
                    self.dialog=Dialog::NewFolder("Nuova cartella".into());
                }
                let selected=!self.selected().is_empty();
                let one=self.selected_one().is_some();
                let many=self.selected().len()>1;
                if ui.add_enabled(!self.busy && selected,egui::Button::new("Copia"))
                    .on_hover_text("Ctrl+C · copia anche in Esplora").clicked(){
                    self.clipboard_set(false);
                }
                if ui.add_enabled(!self.busy && self.clipboard.is_some(),
                    egui::Button::new("Incolla"))
                    .on_hover_text("Ctrl+V · clipboard interna").clicked(){self.paste();}
                if ui.button("Cerca").on_hover_text("Ricerca ricorsiva · Ctrl+Maiusc+F").clicked(){
                    self.search_visible=true;
                    self.search_root=self.current().path.clone();
                    self.search_results.clear();
                }
                if ui.button(if self.preview_visible{"Chiudi anteprima"}else{"Anteprima"})
                    .on_hover_text("Immagini, PDF (testo), dettagli · F3").clicked(){
                    self.preview_visible=!self.preview_visible;
                    self.preview_request=None;
                }
                let dual=self.secondary.is_some();
                if ui.button(if dual{"Un pannello"}else{"Due pannelli"})
                    .on_hover_text("Mostra o nascondi il secondo pannello").clicked(){
                    self.toggle_dual();
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let favorite=self.favorites.contains(&self.current().path);
                if ui.button(if favorite{"Preferito"}else{"Preferiti"})
                    .on_hover_text("Aggiungi o rimuovi dai preferiti").clicked(){
                    self.toggle_favorite();
                }
                ui.menu_button("Visualizza",|ui|{
                    if ui.checkbox(&mut self.thumbs_visible,"Miniature immagini").changed(){
                        ui.close();
                    }
                    if ui.checkbox(&mut self.sidebar_visible,"Barra laterale").changed(){
                        ui.close();
                    }
                    if ui.checkbox(&mut self.live_refresh,"Aggiornamento automatico cartelle").changed(){
                        ui.close();
                    }
                    let was_dark=self.dark_theme;
                    if ui.checkbox(&mut self.dark_theme,"Tema scuro").changed(){ui.close();}
                    if was_dark!=self.dark_theme{
                        apply_theme(&self.context, self.dark_theme);
                    }
                });
                ui.menu_button("Operazioni",|ui|{
                    if ui.add_enabled(!self.busy && one,egui::Button::new("Rinomina  ·  F2"))
                        .clicked(){self.prepare_rename();ui.close();}
                    if ui.add_enabled(!self.busy && many,egui::Button::new("Rinomina multipla"))
                        .clicked(){
                        self.dialog=Dialog::BatchRename{
                            sources:self.selected(),prefix:String::new(),find:String::new(),
                            replace:String::new(),start:"1".into()
                        };
                        ui.close();
                    }
                    if ui.add_enabled(!self.busy && selected,egui::Button::new("Taglia  ·  Ctrl+X"))
                        .clicked(){self.clipboard_set(true);ui.close();}
                    if ui.add_enabled(!self.busy && selected,egui::Button::new("Cestino  ·  Canc"))
                        .clicked(){self.prepare_trash();ui.close();}
                    ui.separator();
                    if ui.add_enabled(!self.busy,egui::Button::new("Importa da Esplora"))
                        .clicked(){self.import_explorer_clipboard();ui.close();}
                    if ui.add_enabled(!self.busy && selected,
                        egui::Button::new("Copia verso Esplora")).clicked(){
                        self.copy_selection_to_windows();ui.close();
                    }
                    if dual {
                        ui.separator();
                        if ui.add_enabled(!self.busy && selected,
                            egui::Button::new("Copia nel pannello opposto")).clicked(){
                            self.transfer_to_other(false);ui.close();
                        }
                        if ui.add_enabled(!self.busy && selected,
                            egui::Button::new("Sposta nel pannello opposto")).clicked(){
                            self.transfer_to_other(true);ui.close();
                        }
                    }
                });

                });
            });
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
                .shortcut_text("").frame(false).selected(&self.current().path == path)).clicked() { target = Some(path.clone()); }
        }
        ui.add_space(15.0);
        ui.label(RichText::new("PREFERITI").color(TEXT_MUTED).small().strong());
        ui.add_space(4.0);
        for path in &self.favorites {
            let title = path.file_name().and_then(|s| s.to_str())
                .map(str::to_owned).unwrap_or_else(|| path.display().to_string());
            if ui.add_sized([190.0, 27.0], egui::Button::new(format!("  {title}"))
                .shortcut_text("").frame(false).selected(&self.current().path == path))
                .on_hover_text(path.display().to_string()).clicked() { target = Some(path.clone()); }
        }
        ui.add_space(15.0);
        ui.label(RichText::new("UNITÀ").color(TEXT_MUTED).small().strong());
        ui.add_space(4.0);
        for drive in &self.drives {
            if ui.add_sized([190.0, 27.0], egui::Button::new(format!("  {}", drive.display()))
                .shortcut_text("").frame(false).selected(&self.current().path == drive)).clicked() { target = Some(drive.clone()); }
        }
        if let Some(path) = target { self.navigate(path); }
    }

    fn files_ui(&mut self, ui: &mut egui::Ui, right: bool) {
        let mut open = None;
        let mut click = None;
        let mut drag_start: Option<PathBuf> = None;
        let mut menu_action: Option<(&'static str,PathBuf)> = None;
        let requested_sort;
        let mut thumbnails_needed = Vec::new();
        let mut icons_needed = Vec::new();
        let tab = self.panel(right);
        let filter = tab.filter.to_lowercase();
        let now=SystemTime::now();
        let criteria=tab.criteria;
        let indices: Vec<usize> = tab.entries.iter().enumerate()
            .filter(|(_, item)| filters::matches(item,&filter,criteria,now))
            .map(|(i, _)| i).collect();
        let count = indices.len();
        // Egui children may grow beyond their requested size. Bound the table
        // to the visible viewport, not to an already-expanded child max_rect.
        let table_width = ui.available_width()
            .min((ui.clip_rect().right() - ui.next_widget_position().x).max(0.0));
        let dual = self.secondary.is_some() || table_width < 650.0;
        let [w_name, w_type, w_size, _w_date] = column_widths(table_width, dual);
        requested_sort = file_header_ui(ui, table_width, dual, tab.sort_by, tab.descending);
        ui.separator();
        if tab.loading { ui.horizontal(|ui| { ui.spinner(); ui.label("Caricamento cartella..."); }); return; }
        if let Some(e) = &tab.error { ui.colored_label(Color32::LIGHT_RED, e); return; }
        if count == 0 { ui.add_space(18.0); ui.label("Nessun elemento trovato"); return; }
        egui::ScrollArea::vertical().auto_shrink([false, false])
            .id_salt(("rows", tab.id, tab.generation))
            .show_rows(ui, 34.0, count, |ui, range| {
                for row in range {
                    let item = &tab.entries[indices[row]];
                    let selected = tab.selected.contains(&item.path);
                    let name = item.name.clone();
                    let key = native_icons::key(&item.name,item.is_dir);
                    if !self.icon_cache.contains_key(&key)
                        && !self.icon_pending.contains(&key)
                        && !self.icon_failed.contains(&key) {
                        icons_needed.push(key.clone());
                    }
                    if self.thumbs_visible && image_view::supported(&item.path)
                        && !self.thumb_cache.contains_key(&item.path)
                        && !self.thumb_pending.contains(&item.path)
                        && !self.thumb_failed.contains(&item.path) {
                        thumbnails_needed.push(item.path.clone());
                    }
                    let icon = if self.thumbs_visible {
                        self.thumb_cache.get(&item.path)
                            .or_else(|| self.icon_cache.get(&key))
                    } else {
                        self.icon_cache.get(&key)
                    };
                    let file_type = if item.is_dir { "Cartella".to_owned() }
                        else { item.path.extension().and_then(|e| e.to_str())
                            .map(str::to_ascii_uppercase).unwrap_or_else(|| "File".into()) };
                    let size = if item.is_dir { "—".into() } else { pretty_bytes(item.size) };
                    let modified = item.modified.map(|t| {
                        let d: DateTime<Local> = DateTime::from(t);
                        d.format("%d/%m/%Y %H:%M").to_string()
                    }).unwrap_or_else(|| "—".into());
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(table_width.min(ui.available_width()), 34.0), egui::Sense::click_and_drag());
                    let fill = if selected { ui.visuals().selection.bg_fill }
                        else if response.hovered() { ui.visuals().widgets.hovered.weak_bg_fill }
                        else if row % 2 == 0 { ui.visuals().faint_bg_color }
                        else { ui.visuals().panel_fill };
                    ui.painter().rect_filled(rect, egui::CornerRadius::same(4), fill);
                    let text = ui.visuals().text_color();
                    let left = rect.left() + 8.0;
                    let center = rect.center().y;
                    let icon_rect = egui::Rect::from_center_size(
                        egui::pos2(left + 10.0, center), egui::vec2(20.0, 20.0));
                    if let Some(tex) = icon {
                        let aspect = tex.size_vec2();
                        let scale = (20.0 / aspect.x).min(20.0 / aspect.y);
                        let fitted = egui::Rect::from_center_size(icon_rect.center(), aspect * scale);
                        ui.painter().image(tex.id(), fitted,
                            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                            Color32::WHITE);
                    } else {
                        // Vector fallback has no dependency on Unicode font coverage.
                        ui.painter().rect_filled(icon_rect.shrink(2.0),
                            egui::CornerRadius::same(2),
                            if item.is_dir { Color32::from_rgb(226, 181, 77) } else { ACCENT });
                    }
                    let mut name_job = egui::text::LayoutJob::simple(name,
                        egui::FontId::proportional(14.0), text, (w_name - 40.0).max(30.0));
                    name_job.wrap.max_rows = 1;
                    let galley = ui.fonts_mut(|f| f.layout_job(name_job));
                    ui.painter().galley(egui::pos2(left + 30.0, center - galley.size().y / 2.0),
                        galley, text);
                    let mut x = rect.left() + w_name + 7.0;
                    if !dual {
                        ui.painter().text(egui::pos2(x, center), egui::Align2::LEFT_CENTER,
                            file_type, egui::FontId::proportional(13.0), text);
                        x += w_type + 7.0;
                    }
                    ui.painter().text(egui::pos2(x + w_size - 8.0, center),
                        egui::Align2::RIGHT_CENTER, size, egui::FontId::proportional(13.0), text);
                    if !dual {
                        x += w_size + 7.0;
                        ui.painter().text(egui::pos2(x, center), egui::Align2::LEFT_CENTER,
                            modified, egui::FontId::proportional(12.0), text);
                    }
                    let response = response.on_hover_text(item.path.display().to_string());
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
                    if response.drag_started() {
                        drag_start = Some(item.path.clone());
                    }
                    if response.double_clicked() { open = Some(item.path.clone()); }
                }
            });
        self.schedule_assets(thumbnails_needed,icons_needed);
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
        if let Some(path) = drag_start {
            if self.secondary.is_some() && !self.busy && matches!(self.dialog, Dialog::None) {
                let tab = self.panel(right);
                let sources: Vec<PathBuf> = if tab.selected.contains(&path) {
                    tab.selected.iter().cloned().collect()
                } else { vec![path] };
                self.drag_intent = Some(DragIntent { sources, source_right: right });
                self.status = "Rilascia nel pannello opposto per proporre una copia".into();
            }
        }
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

    /// UI-only drag between panels. No OS file drop is synthesized: the
    /// result always opens an explicit copy-only confirmation dialog.
    fn finish_internal_drag(&mut self, ctx: &egui::Context, areas: [Option<egui::Rect>;2]) {
        if !ctx.input(|i| i.pointer.any_released()) { return; }
        let Some(drag)=self.drag_intent.take() else {return;};
        if self.busy || !matches!(self.dialog, Dialog::None) { return; }
        let Some(pos)=ctx.input(|i|i.pointer.interact_pos()) else {return;};
        let target_right = !drag.source_right;
        if let Some(area)=areas[usize::from(target_right)] {
            if area.contains(pos) {
                let destination=self.panel(target_right).path.clone();
                self.dialog=Dialog::ExternalDrop {
                    sources:drag.sources,
                    destination,
                };
                return;
            }
        }
        self.status="Trascinamento annullato (nessuna modifica)".into();
    }


    /// Reserve the pane in the parent before rendering its child. Widgets in
    /// one pane cannot grow the other pane or paint beyond the divider.
    fn allocate_pane(&mut self, ui: &mut egui::Ui, width: f32, height: f32, right: bool) -> egui::Rect {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        let mut child = ui.new_child(egui::UiBuilder::new()
            .id_salt(("file_pane", right))
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)));
        child.set_clip_rect(rect.intersect(ui.clip_rect()));
        self.pane_ui(&mut child, right);
        rect
    }

    fn pane_ui(&mut self, ui: &mut egui::Ui, right: bool) {
        let tab = self.panel(right);
        let path = tab.path.display().to_string();
        let count = tab.selected.len();
        let focused = self.focused_right == right;
        let title = if right { "DESTRA" } else { "SINISTRA" };
        let background=if self.dark_theme {
            if focused {Color32::from_rgb(36,52,70)} else {Color32::from_rgb(26,34,45)}
        }else if focused {Color32::from_rgb(223,237,250)}
            else {Color32::from_rgb(237,241,247)};
        egui::Frame::new().fill(background)
            .corner_radius(egui::CornerRadius::same(9))
            .inner_margin(egui::Margin::symmetric(10,7)).show(ui,|ui|{
            ui.horizontal(|ui|{
                if ui.add(egui::Button::new(RichText::new(format!("{title}")).strong()
                    .color(if focused{ACCENT}else{TEXT_MUTED}))
                    .frame(false)).on_hover_text("Attiva il pannello").clicked() {
                    self.focus(right);
                }
                if focused {ui.label(RichText::new("ATTIVO").small().color(ACCENT));}
                ui.label(RichText::new(format!("{count} selezionati")).small().color(TEXT_MUTED));
            });
            ui.label(RichText::new(&path).small().color(TEXT_MUTED)).on_hover_text(path.clone());
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Filtra").color(TEXT_MUTED));
            let filter_width = (ui.available_width() - 80.0).clamp(55.0, 260.0);
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
            if ui.button("×").on_hover_text("Azzera filtri").clicked() {
                tab.filter.clear();
                tab.criteria=filters::Criteria::default();
                tab.selected.clear();
                tab.anchor = None;
            }
        });
        ui.horizontal_wrapped(|ui|{
            let tab=self.panel_mut(right);
            let previous=tab.criteria;
            egui::ComboBox::from_id_salt(("kind_filter",right))
                .selected_text(match tab.criteria.kind{
                    filters::FileKind::All=>"Tutti i tipi",
                    filters::FileKind::Documents=>"Documenti",
                    filters::FileKind::Images=>"Immagini",
                    filters::FileKind::Archives=>"Archivi",
                    filters::FileKind::Video=>"Video",
                    filters::FileKind::Audio=>"Audio",
                    filters::FileKind::Other=>"Altri",
                }).show_ui(ui,|ui|{
                    for (kind,label) in [
                        (filters::FileKind::All,"Tutti i tipi"),
                        (filters::FileKind::Documents,"Documenti"),
                        (filters::FileKind::Images,"Immagini"),
                        (filters::FileKind::Archives,"Archivi"),
                        (filters::FileKind::Video,"Video"),
                        (filters::FileKind::Audio,"Audio"),
                        (filters::FileKind::Other,"Altri"),
                    ]{ui.selectable_value(&mut tab.criteria.kind,kind,label);}
                });
            egui::ComboBox::from_id_salt(("size_filter",right))
                .selected_text(match tab.criteria.size{
                    filters::SizeRange::All=>"Qualsiasi dimensione",
                    filters::SizeRange::Small=>"< 1 MiB",
                    filters::SizeRange::Medium=>"1–100 MiB",
                    filters::SizeRange::Large=>">= 100 MiB",
                }).show_ui(ui,|ui|{
                    for (kind,label) in [
                        (filters::SizeRange::All,"Qualsiasi dimensione"),
                        (filters::SizeRange::Small,"< 1 MiB"),
                        (filters::SizeRange::Medium,"1–100 MiB"),
                        (filters::SizeRange::Large,">= 100 MiB"),
                    ]{ui.selectable_value(&mut tab.criteria.size,kind,label);}
                });
            egui::ComboBox::from_id_salt(("date_filter",right))
                .selected_text(match tab.criteria.modified{
                    filters::ModifiedRange::All=>"Qualsiasi data",
                    filters::ModifiedRange::Day=>"Ultime 24 ore",
                    filters::ModifiedRange::Week=>"Ultimi 7 giorni",
                    filters::ModifiedRange::Month=>"Ultimi 30 giorni",
                }).show_ui(ui,|ui|{
                    for (kind,label) in [
                        (filters::ModifiedRange::All,"Qualsiasi data"),
                        (filters::ModifiedRange::Day,"Ultime 24 ore"),
                        (filters::ModifiedRange::Week,"Ultimi 7 giorni"),
                        (filters::ModifiedRange::Month,"Ultimi 30 giorni"),
                    ]{ui.selectable_value(&mut tab.criteria.modified,kind,label);}
                });
            if previous!=tab.criteria{
                tab.selected.clear();
                tab.anchor=None;
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
                egui::Window::new("Conferma copia dei file").collapsible(false).resizable(false)
                    .show(ctx, |ui| {
                        ui.label(format!("{} elementi da copiare", sources.len()));
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

impl Veldir {
    fn render_ui(&mut self, ui: &mut egui::Ui) -> [Option<egui::Rect>; 2] {
        ui.painter().rect_filled(ui.max_rect(), egui::CornerRadius::ZERO, ui.visuals().panel_fill);
        self.pump();
        self.sync_watch_paths();
        self.apply_watch_changes(ui.ctx());
        self.update_preview();
        self.accept_os_drop(ui.ctx());
        self.keyboard(ui.ctx());
        ui.add_space(5.0);
        let dark=self.dark_theme;
        egui::Frame::new().fill(if dark{Color32::from_rgb(22,30,41)}
            else{Color32::from_rgb(225,234,245)})
            .corner_radius(egui::CornerRadius::same(11))
            .inner_margin(egui::Margin::symmetric(12,9)).show(ui,|ui|{
            ui.horizontal(|ui|{
                ui.label(RichText::new("VELDIR").strong().size(21.0).color(ACCENT));
                ui.label(RichText::new("0.9.0  ·  WINDOWS FILE MANAGER")
                    .small().color(TEXT_MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                    if self.busy {ui.spinner();}
                    let path=self.current().path.clone();
                    let label=path.file_name().and_then(|n|n.to_str())
                        .unwrap_or("Unità");
                    ui.add(egui::Label::new(RichText::new(label).strong().color(TEXT_MUTED)).truncate())
                        .on_hover_text(path.display().to_string());
                });
            });
        });
        ui.add_space(8.0);
        self.tabs_ui(ui);
        ui.add_space(6.0);
        self.toolbar_ui(ui);
        ui.add_space(10.0);
        ui.separator();
        let content_height = (ui.available_height() - 46.0).max(230.0);
        let mut pane_areas: [Option<egui::Rect>;2] = [None,None];
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
                    let usable = (available - 10.0 - 2.0 * ui.spacing().item_spacing.x).max(440.0);
                    let left_width = (usable * self.split_ratio).clamp(220.0, usable - 220.0);
                    let right_width = usable - left_width;
                    pane_areas[0] = Some(self.allocate_pane(ui, left_width, content_height, false));

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

                    pane_areas[1] = Some(self.allocate_pane(ui, right_width, content_height, true));
                } else {
                    let width = ui.available_width();
                    pane_areas[0] = Some(self.allocate_pane(ui, width, content_height, false));
                }
            },
        );
        self.finish_internal_drag(ui.ctx(),pane_areas);
        ui.separator();
        ui.horizontal(|ui| {
            let tab = self.current();
            let name_filter=tab.filter.to_lowercase();
            let now=SystemTime::now();
            let count = tab.entries.iter().filter(|e|
                filters::matches(e,&name_filter,tab.criteria,now)).count();
            let bytes=tab.entries.iter().filter(|e| !e.is_dir && tab.selected.contains(&e.path))
                .fold(0u64,|acc,e|acc.saturating_add(e.size));
            let selected_text=if tab.selected.is_empty() {
                format!("{count} elementi")
            } else {
                format!("{count} elementi  ·  {} selezionati  ·  {}",
                    tab.selected.len(),pretty_bytes(bytes))
            };
            ui.label(RichText::new(selected_text).small().color(TEXT_MUTED));
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
        pane_areas
    }

}

impl eframe::App for Veldir {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.render_ui(ui);
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
            thumbs_visible: self.thumbs_visible,
            live_refresh:self.live_refresh,
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
    use super::{apply_theme, column_widths, file_header_ui, egui, Veldir, apply_selection, pretty_bytes, bounded_split_ratio, sort_entries, Saved, SortBy};
    use crate::fs_ops::Entry;
    use std::{collections::BTreeSet, path::PathBuf};



    #[test]
    fn full_ui_keeps_both_panes_inside_viewport_at_split_limits() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("cartella-con-un-nome-lungo-per-verificare-il-layout-dei-pannelli");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("nota.txt"), "prova").unwrap();
        for dark in [true, false] {
            let ctx = egui::Context::default();
            let mut app = Veldir::from_saved(ctx.clone(), Saved {
                tabs: vec![folder.clone()], dual_pane: true,
                secondary_path: Some(folder.clone()), dark_theme: dark,
                thumbs_visible: false, live_refresh: false,
                ..Default::default()
            });
            for width in [900.0, 1230.0, 1920.0] {
                for ratio in [0.25, 0.5, 0.75] {
                    app.split_ratio = ratio;
                    let output = ctx.run_ui(egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO, egui::vec2(width, 790.0))),
                        ..Default::default()
                    }, |ui| {
                        let viewport = ui.clip_rect();
                        let panes = app.render_ui(ui);
                        let left = panes[0].unwrap();
                        let right = panes[1].unwrap();
                        assert!(left.right() <= right.left(), "overlapping panes at {width}/{ratio}");
                        assert!(right.right() <= viewport.right() + 1.0,
                            "pane outside viewport at {width}/{ratio}: {right:?} vs {viewport:?}");
                        assert!(ui.min_rect().right() <= viewport.right() + 1.0,
                            "root grew past viewport at {width}/{ratio}");
                        assert!(left.width() >= 219.0 && right.width() >= 219.0);
                    });
                    output.drop_without_applying_deltas();
                }
            }
        }
    }

    #[test]
    fn f6_switches_focus_and_ctrl_a_selects_only_filtered_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("nota.txt"), "prova").unwrap();
        std::fs::write(dir.path().join("dati.csv"), "prova").unwrap();
        let ctx = egui::Context::default();
        let mut app = Veldir::from_saved(ctx.clone(), Saved {
            tabs: vec![dir.path().to_path_buf()], dual_pane: true,
            secondary_path: Some(dir.path().to_path_buf()),
            live_refresh: false, thumbs_visible: false, ..Default::default()
        });
        let output = ctx.run_ui(egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::F6, physical_key: None, pressed: true,
                repeat: false, modifiers: egui::Modifiers::NONE,
            }], ..Default::default()
        }, |ui| app.keyboard(ui.ctx()));
        output.drop_without_applying_deltas();
        assert!(app.focused_right);
        // Wait-free setup after the asynchronous folder result is drained.
        while app.rx.try_recv().is_ok() {}
        let tab = app.current_mut();
        tab.loading = false;
        tab.entries = crate::fs_ops::list(dir.path()).unwrap();
        tab.filter = "txt".into();
        let modifiers = egui::Modifiers { ctrl: true, command: true, ..Default::default() };
        let output = ctx.run_ui(egui::RawInput {
            events: vec![egui::Event::ModifiersChanged(modifiers), egui::Event::Key {
                key: egui::Key::A, physical_key: None, pressed: true,
                repeat: false, modifiers,
            }], ..Default::default()
        }, |ui| app.keyboard(ui.ctx()));
        output.drop_without_applying_deltas();
        assert_eq!(app.current().selected.len(), 1);
        assert!(app.current().selected.contains(&dir.path().join("nota.txt")));
        assert!(app.panel(false).selected.is_empty());
    }

    #[test]
    fn theme_is_explicit_and_readable_in_both_system_modes() {
        let ctx = egui::Context::default();
        apply_theme(&ctx, true);
        assert_eq!(ctx.theme(), egui::Theme::Dark);
        let dark = ctx.style_of(egui::Theme::Dark);
        assert!(dark.visuals.text_color().r() > 200);
        assert!(dark.visuals.widgets.inactive.bg_fill.r() < 60);
        apply_theme(&ctx, false);
        assert_eq!(ctx.theme(), egui::Theme::Light);
        apply_theme(&ctx, true);
        assert_eq!(ctx.theme(), egui::Theme::Dark);
    }


    #[test]
    fn header_stays_inside_viewport_even_when_parent_has_expanded() {
        for width in [280.0, 450.0, 680.0, 1230.0, 1698.0] {
            let ctx = egui::Context::default();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO, egui::vec2(width, 500.0))),
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| {
                let viewport = ui.clip_rect();
                // Reproduce an expanded child left by a wide widget.
                ui.set_width(width + 300.0);
                let visible = ui.available_width()
                    .min((ui.clip_rect().right() - ui.next_widget_position().x).max(0.0));
                let before = ui.next_widget_position();
                file_header_ui(ui, visible, visible < 650.0, SortBy::Name, false);
                let header = egui::Rect::from_min_size(before, egui::vec2(visible, 26.0));
                assert!(header.right() <= viewport.right() + 0.1);
                assert!(header.bottom() <= viewport.bottom());
                let columns = column_widths(visible, visible < 650.0);
                let count = columns.iter().filter(|w| **w > 0.0).count();
                let last_edge = header.left() + columns.iter().sum::<f32>()
                    + 7.0 * (count.saturating_sub(1) as f32);
                assert!(last_edge <= viewport.right() + 0.1);
            });
            output.drop_without_applying_deltas();
        }
    }

    #[test]
    fn columns_fit_wide_and_compact_panes() {
        for width in [250.0, 400.0, 650.0, 1200.0, 1900.0] {
            let compact = width < 650.0;
            let cols = column_widths(width, compact);
            assert!(cols.iter().all(|w| *w >= 0.0));
            assert!(cols.iter().sum::<f32>() + 24.0 <= width + 0.1);
            if !compact { assert!(cols[3] <= 145.0); }
        }
    }

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
