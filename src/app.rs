use crate::fs_ops::{self, Entry, Job};
use crate::ui_state::{Columns, display_path};
use crate::{filters, folder_watch, image_view, native_icons, preview, search};
use chrono::{DateTime, Local};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant, SystemTime};

const ACCENT: Color32 = Color32::from_rgb(112, 175, 250);
const TEXT_MUTED: Color32 = Color32::from_rgb(144, 155, 171);
fn muted_color(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        TEXT_MUTED
    } else {
        Color32::from_rgb(65, 80, 100)
    }
}
fn accent_color(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        ACCENT
    } else {
        Color32::from_rgb(30, 85, 155)
    }
}

/// Configure both egui theme slots: the native system preference must not
/// mix light widgets with the application's persisted dark surfaces.
fn apply_theme(ctx: &egui::Context, dark: bool) {
    ctx.set_theme(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
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
            } else {
                egui::Visuals::light()
            };
            style.spacing.item_spacing = egui::vec2(7.0, 5.0);
            style.spacing.button_padding = egui::vec2(10.0, 6.0);
        });
    }
}

/// Manual header cells avoid grow atoms consuming the entire remaining pane.
fn configured_header_ui(
    ui: &mut egui::Ui,
    width: f32,
    compact: bool,
    by: SortBy,
    descending: bool,
    columns: &mut Columns,
) -> Option<SortBy> {
    let widths = columns.widths(width, compact);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 26.0), egui::Sense::hover());
    let mut x = rect.left();
    let mut sort = None;
    for (index, label, key) in [
        (0, "NOME", Some(SortBy::Name)),
        (1, "TIPO", None),
        (2, "DIMENSIONE", Some(SortBy::Size)),
        (3, "MODIFICATO", Some(SortBy::Modified)),
    ] {
        let cell_width = widths[index];
        if cell_width == 0.0 {
            continue;
        }
        let cell = egui::Rect::from_min_size(
            egui::pos2(x, rect.top()),
            egui::vec2(cell_width, rect.height()),
        );
        let caption = if key == Some(by) {
            format!("{label} {}", if descending { "v" } else { "^" })
        } else {
            label.to_owned()
        };
        let response = ui.interact(
            cell,
            ui.scope_id().with(("column_header", index)),
            if key.is_some() {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            },
        );
        if response.hovered() && key.is_some() {
            ui.painter().rect_filled(
                cell,
                egui::CornerRadius::same(3),
                ui.visuals().widgets.hovered.weak_bg_fill,
            );
        }
        ui.painter()
            .with_clip_rect(cell.intersect(ui.clip_rect()))
            .text(
                egui::pos2(cell.left() + 8.0, cell.center().y),
                egui::Align2::LEFT_CENTER,
                &caption,
                egui::FontId::proportional(11.0),
                ui.visuals().text_color(),
            );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                if key.is_some() {
                    egui::Role::Button
                } else {
                    egui::Role::ColumnHeader
                },
                true,
                &caption,
            )
        });
        if response
            .on_hover_text(format!("Ordina per {}", label.to_lowercase()))
            .clicked()
        {
            sort = key;
        }
        {
            let edge = egui::Rect::from_center_size(
                egui::pos2(cell.right(), cell.center().y),
                egui::vec2(7.0, 26.0),
            );
            let resize = ui
                .interact(
                    edge,
                    ui.scope_id().with(("column_resize", index)),
                    egui::Sense::drag(),
                )
                .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            if resize.dragged() {
                if index == 0 {
                    if let Some(next) = (1..4).find(|i| widths[*i] > 0.0) {
                        columns.resize(next, -resize.drag_delta().x);
                    }
                } else {
                    columns.resize(index, resize.drag_delta().x);
                }
            }
        }
        x += cell_width + 7.0;
    }
    sort
}

#[cfg(test)]
fn file_header_ui(
    ui: &mut egui::Ui,
    width: f32,
    compact: bool,
    by: SortBy,
    descending: bool,
) -> Option<SortBy> {
    configured_header_ui(ui, width, compact, by, descending, &mut Columns::default())
}
#[cfg(test)]
fn column_widths(width: f32, compact: bool) -> [f32; 4] {
    Columns::default().widths(width, compact)
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
    columns: Columns,
    restore_session: bool,
}

impl Default for Saved {
    fn default() -> Self {
        Self {
            favorites: Vec::new(),
            tabs: Vec::new(),
            active: 0,
            secondary_path: None,
            dual_pane: false,
            sidebar_visible: true,
            split_ratio: 0.5,
            dark_theme: true,
            preview_visible: false,
            thumbs_visible: true,
            live_refresh: true,
            columns: Columns::default(),
            restore_session: true,
        }
    }
}

fn bounded_split_ratio(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.25, 0.75)
    } else {
        0.5
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortBy {
    Name,
    Size,
    Modified,
}

fn sort_entries(entries: &mut [Entry], by: SortBy, descending: bool) {
    entries.sort_unstable_by(|a, b| {
        let order = match by {
            SortBy::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortBy::Size => a.size.cmp(&b.size),
            SortBy::Modified => a.modified.cmp(&b.modified),
        };
        let order = if descending { order.reverse() } else { order };
        b.is_dir
            .cmp(&a.is_dir)
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
    cursor: Option<PathBuf>,
    scroll_to: Option<usize>,
}

impl Tab {
    fn new(id: u64, path: PathBuf) -> Self {
        Self {
            id,
            path,
            entries: Vec::new(),
            back: Vec::new(),
            forward: Vec::new(),
            loading: false,
            generation: 0,
            error: None,
            selected: BTreeSet::new(),
            anchor: None,
            filter: String::new(),
            sort_by: SortBy::Name,
            descending: false,
            criteria: filters::Criteria::default(),
            cursor: None,
            scroll_to: None,
        }
    }
}

enum Message {
    DrivesLoaded(Vec<PathBuf>),
    FolderLoaded {
        tab_id: u64,
        generation: u64,
        result: Result<Vec<Entry>, String>,
    },
    JobFinished {
        result: Result<String, String>,
        is_move: bool,
    },
    JobProgress(fs_ops::Progress),
    SearchFinished {
        generation: u64,
        result: Result<Vec<search::Hit>, String>,
    },
    PreviewFinished {
        generation: u64,
        result: Result<preview::Details, String>,
    },
    PreviewImageLoaded {
        generation: u64,
        result: Result<image_view::Bitmap, String>,
    },
    ThumbnailLoaded {
        generation: u64,
        path: PathBuf,
        result: Result<image_view::Bitmap, String>,
    },
    IconLoaded {
        key: String,
        result: Result<image_view::Bitmap, String>,
    },
    WatchedPath(PathBuf),
}

enum Dialog {
    Transfer {
        sources: Vec<PathBuf>,
        destination: PathBuf,
        cut: bool,
        policy: crate::transfer::ConflictPolicy,
    },
    None,
    NewFolder(String),
    Rename {
        source: PathBuf,
        name: String,
    },
    Delete(Vec<PathBuf>),
    ExternalDrop {
        sources: Vec<PathBuf>,
        destination: PathBuf,
    },
    ConfirmMove {
        sources: Vec<PathBuf>,
        destination_folder: PathBuf,
    },
    BatchRename {
        sources: Vec<PathBuf>,
        prefix: String,
        find: String,
        replace: String,
        start: String,
    },
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
    asset_generation: u64,
    thumb_cache: HashMap<PathBuf, egui::TextureHandle>,
    thumb_order: VecDeque<PathBuf>,
    thumb_pending: HashSet<PathBuf>,
    thumb_failed: HashSet<PathBuf>,
    icon_cache: HashMap<String, egui::TextureHandle>,
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
    watch_dirty: HashMap<PathBuf, Instant>,
    live_refresh: bool,
    columns: Columns,
    restore_session: bool,
    quick_find: String,
    quick_find_at: Option<Instant>,
    drag_tab: Option<u64>,
    closed_tabs: Vec<PathBuf>,
    about_visible: bool,
    history_visible: bool,
    job_history: VecDeque<String>,
    job_sources: Vec<PathBuf>,
    preview_zoom: f32,
    search_filter: String,
    search_descending: bool,
    reveal_path: Option<PathBuf>,
}

fn load_saved(storage: Option<&dyn eframe::Storage>) -> (Saved, &'static str) {
    let Some(storage) = storage else {
        return (Saved::default(), "");
    };
    if let Some(saved) = eframe::get_value(storage, "veldir_state") {
        return (saved, "");
    }
    if let Some(saved) = eframe::get_value(storage, "veldir_state_backup") {
        return (saved, "Impostazioni recuperate dalla copia precedente");
    }
    (
        Saved::default(),
        if storage.get_string("veldir_state").is_some() {
            "Impostazioni non valide: ripristinati i valori predefiniti"
        } else {
            ""
        },
    )
}

impl Veldir {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (saved, note) = load_saved(cc.storage);
        let mut app = Self::from_saved(cc.egui_ctx.clone(), saved);
        if !note.is_empty() {
            app.status = note.into();
        }
        let args: Vec<_> = std::env::args_os().collect();
        if let Some(index) = args.iter().position(|a| a == "--path") {
            if let Some(p) = args.get(index + 1) {
                app.focused_right = false;
                app.secondary = None;
                app.tabs.clear();
                app.active = 0;
                app.add_tab(PathBuf::from(p));
            }
        }
        app
    }

    fn from_saved(context: egui::Context, saved: Saved) -> Self {
        let (tx, rx) = mpsc::channel();
        let watch_tx = tx.clone();
        let watch_ctx = context.clone();
        let watcher = folder_watch::FolderWatcher::start(move |path| {
            let _ = watch_tx.send(Message::WatchedPath(path));
            watch_ctx.request_repaint();
        });
        let home = dirs_next::home_dir()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("C:\\"));
        let mut shortcuts = Vec::new();
        shortcuts.push(("Home".into(), home.clone()));
        if let Some(p) = dirs_next::desktop_dir() {
            shortcuts.push(("Desktop".into(), p));
        }
        if let Some(p) = dirs_next::download_dir() {
            shortcuts.push(("Download".into(), p));
        }
        if let Some(p) = dirs_next::document_dir() {
            shortcuts.push(("Documenti".into(), p));
        }
        if let Some(p) = dirs_next::picture_dir() {
            shortcuts.push(("Immagini".into(), p));
        }

        let drives = Vec::new();
        let drive_sender = tx.clone();
        let drive_context = context.clone();
        std::thread::spawn(move || {
            let drives = if cfg!(windows) {
                ('A'..='Z')
                    .map(|letter| PathBuf::from(format!("{letter}:{}", std::path::MAIN_SEPARATOR)))
                    .filter(|p| p.is_dir())
                    .collect()
            } else {
                vec![PathBuf::from("/")]
            };
            let _ = drive_sender.send(Message::DrivesLoaded(drives));
            drive_context.request_repaint();
        });

        let mut app = Self {
            tabs: Vec::new(),
            active: 0,
            secondary: None,
            focused_right: false,
            next_id: 1,
            tx,
            rx,
            context: context.clone(),
            favorites: saved.favorites,
            shortcuts,
            drives,
            clipboard: None,
            drag_intent: None,
            busy: false,
            dialog: Dialog::None,
            status: "Pronto".into(),
            editing_address: false,
            address_input: String::new(),
            sidebar_visible: saved.sidebar_visible,
            split_ratio: bounded_split_ratio(saved.split_ratio),
            dark_theme: saved.dark_theme,
            preview_visible: saved.preview_visible,
            preview_request: None,
            preview_generation: 0,
            preview_data: None,
            preview_image: None,
            thumbs_visible: saved.thumbs_visible,
            asset_generation: 0,
            thumb_cache: HashMap::new(),
            thumb_order: VecDeque::new(),
            thumb_pending: HashSet::new(),
            thumb_failed: HashSet::new(),
            icon_cache: HashMap::new(),
            icon_pending: HashSet::new(),
            icon_failed: HashSet::new(),
            searching: false,
            search_visible: false,
            search_query: String::new(),
            search_root: PathBuf::new(),
            search_results: Vec::new(),
            search_generation: 0,
            search_cancel: Arc::new(AtomicBool::new(false)),
            job_cancel: Arc::new(AtomicBool::new(false)),
            job_progress: None,
            job_started_at: None,
            watcher,
            watched_paths: Vec::new(),
            watch_dirty: HashMap::new(),
            live_refresh: saved.live_refresh,
            columns: saved.columns,
            restore_session: saved.restore_session,
            quick_find: String::new(),
            quick_find_at: None,
            drag_tab: None,
            closed_tabs: Vec::new(),
            about_visible: false,
            history_visible: false,
            job_history: VecDeque::new(),
            job_sources: Vec::new(),
            preview_zoom: 1.0,
            search_filter: String::new(),
            search_descending: false,
            reveal_path: None,
        };
        apply_theme(&context, app.dark_theme);
        let restored: Vec<PathBuf> = if saved.restore_session {
            saved.tabs.into_iter().take(32).collect()
        } else {
            Vec::new()
        };
        if restored.is_empty() {
            app.add_tab(home);
        } else {
            for path in restored {
                app.add_tab(path);
            }
            app.active = saved.active.min(app.tabs.len().saturating_sub(1));
        }
        if saved.dual_pane && saved.restore_session {
            let right = saved
                .secondary_path
                .unwrap_or_else(|| app.tabs[app.active].path.clone());
            app.open_secondary(right);
        }
        app.address_input = app.current().path.display().to_string();
        app
    }

    fn panel(&self, right: bool) -> &Tab {
        if right {
            self.secondary
                .as_ref()
                .expect("secondo pannello non attivo")
        } else {
            &self.tabs[self.active]
        }
    }
    fn panel_mut(&mut self, right: bool) -> &mut Tab {
        if right {
            self.secondary
                .as_mut()
                .expect("secondo pannello non attivo")
        } else {
            &mut self.tabs[self.active]
        }
    }
    fn current(&self) -> &Tab {
        self.panel(self.focused_right)
    }
    fn current_mut(&mut self) -> &mut Tab {
        self.panel_mut(self.focused_right)
    }

    fn open_secondary(&mut self, path: PathBuf) {
        if self.secondary.is_some() {
            return;
        }
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
        if right && self.secondary.is_none() {
            return;
        }
        if self.focused_right != right {
            self.focused_right = right;
            self.address_input = self.current().path.display().to_string();
            self.editing_address = false;
        }
    }

    fn load_secondary(&mut self) {
        let Some(tab) = self.secondary.as_mut() else {
            return;
        };
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
            let _ = sender.send(Message::FolderLoaded {
                tab_id: id,
                generation,
                result,
            });
            ctx.request_repaint();
        });
    }

    fn reload_current(&mut self) {
        if self.focused_right {
            self.load_secondary();
        } else {
            self.load(self.active);
        }
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
            let _ = sender.send(Message::FolderLoaded {
                tab_id: id,
                generation,
                result,
            });
            ctx.request_repaint();
        });
    }

    fn add_tab(&mut self, path: PathBuf) {
        self.focused_right = false;
        let path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir().unwrap_or_default().join(path)
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
        if let Some(tab) = self.tabs.iter().find(|t| t.id == id) {
            self.closed_tabs.push(tab.path.clone());
            if self.closed_tabs.len() > 20 {
                self.closed_tabs.remove(0);
            }
        }
        if let Some(index) = self.tabs.iter().position(|t| t.id == id) {
            if self.tabs.len() == 1 {
                self.add_tab(dirs_next::home_dir().unwrap_or_else(|| PathBuf::from("C:\\")));
            }
            self.tabs.remove(index);
            if self.active >= index && self.active > 0 {
                self.active -= 1;
            }
            self.active = self.active.min(self.tabs.len() - 1);
            self.focused_right = false;
            self.address_input = self.current().path.display().to_string();
        }
    }

    fn navigate(&mut self, path: PathBuf) {
        let path = if path.is_absolute() {
            path
        } else {
            self.current().path.join(path)
        };
        if self.current().path == path {
            return;
        }
        let tab = self.current_mut();
        tab.back.push(tab.path.clone());
        tab.forward.clear();
        tab.path = path;
        tab.cursor = None;
        tab.scroll_to = None;
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
            tab.cursor = None;
            tab.scroll_to = None;
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
            tab.cursor = None;
            tab.scroll_to = None;
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
        let mut paths: Vec<PathBuf> = if self.live_refresh {
            self.tabs
                .iter()
                .map(|t| t.path.clone())
                .chain(self.secondary.iter().map(|t| t.path.clone()))
                .collect()
        } else {
            Vec::new()
        };
        paths.sort();
        paths.dedup();
        if paths != self.watched_paths {
            self.watcher.set_paths(paths.clone());
            self.watched_paths = paths;
        }
    }

    fn quiet_reload_path(&mut self, path: &Path) {
        let mut requests = Vec::new();
        for tab in self.tabs.iter_mut().chain(self.secondary.iter_mut()) {
            if tab.path == path && !tab.loading {
                tab.generation += 1;
                // Preserve the current list, scrollbar, and selection during reload.
                requests.push((tab.path.clone(), tab.id, tab.generation));
            }
        }
        for (path, id, generation) in requests {
            let sender = self.tx.clone();
            let ctx = self.context.clone();
            std::thread::spawn(move || {
                let result = fs_ops::list(&path);
                let _ = sender.send(Message::FolderLoaded {
                    tab_id: id,
                    generation,
                    result,
                });
                ctx.request_repaint();
            });
        }
    }

    fn apply_watch_changes(&mut self, ctx: &egui::Context) {
        if !self.live_refresh {
            self.watch_dirty.clear();
            return;
        }
        if self.watch_dirty.is_empty() {
            return;
        }
        if self.busy {
            ctx.request_repaint_after(Duration::from_millis(500));
            return;
        }
        let now = Instant::now();
        let mut affected = HashSet::new();
        self.watch_dirty.retain(|changed, last| {
            if now.duration_since(*last) < Duration::from_millis(450) {
                return true;
            }
            for directory in &self.watched_paths {
                if directory == changed || changed.parent() == Some(directory.as_path()) {
                    affected.insert(directory.clone());
                }
            }
            false
        });
        for path in affected {
            self.quiet_reload_path(&path);
        }
        if !self.watch_dirty.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }

    fn refresh(&mut self) {
        self.asset_generation = self.asset_generation.wrapping_add(1);
        self.thumb_cache.clear();
        self.thumb_order.clear();
        self.thumb_failed.clear();
        for i in 0..self.tabs.len() {
            self.load(i);
        }
        if self.secondary.is_some() {
            self.load_secondary();
        }
        self.status = "Aggiornamento richiesto".into();
    }

    fn pump(&mut self) {
        while let Ok(message) = self.rx.try_recv() {
            match message {
                Message::DrivesLoaded(drives) => self.drives = drives,
                Message::WatchedPath(path) => {
                    self.asset_generation = self.asset_generation.wrapping_add(1);
                    if self.live_refresh {
                        if self.thumb_cache.remove(&path).is_some() {
                            self.thumb_order.retain(|p| p != &path);
                        }
                        self.thumb_failed.remove(&path);
                        self.watch_dirty.insert(path, Instant::now());
                    }
                }
                Message::FolderLoaded {
                    tab_id,
                    generation,
                    result,
                } => {
                    let tab = self
                        .tabs
                        .iter_mut()
                        .find(|t| t.id == tab_id && t.generation == generation)
                        .or_else(|| {
                            self.secondary
                                .as_mut()
                                .filter(|t| t.id == tab_id && t.generation == generation)
                        });
                    if let Some(tab) = tab {
                        tab.loading = false;
                        match result {
                            Ok(mut entries) => {
                                sort_entries(&mut entries, tab.sort_by, tab.descending);
                                let known: HashSet<PathBuf> =
                                    entries.iter().map(|e| e.path.clone()).collect();
                                tab.selected.retain(|p| known.contains(p));
                                if tab.anchor.as_ref().is_some_and(|a| !known.contains(a)) {
                                    tab.anchor = None;
                                }
                                if tab.cursor.as_ref().is_some_and(|p| !known.contains(p)) {
                                    tab.cursor = None;
                                }
                                tab.entries = entries;
                                tab.error = None;
                                if let Some(path) = self.reveal_path.as_ref() {
                                    if let Some(row) =
                                        tab.entries.iter().position(|e| &e.path == path)
                                    {
                                        tab.selected.clear();
                                        tab.selected.insert(path.clone());
                                        tab.cursor = Some(path.clone());
                                        tab.scroll_to = Some(row);
                                        self.reveal_path = None;
                                    }
                                }
                            }
                            Err(e) => {
                                tab.error = Some(e);
                            }
                        }
                    }
                }
                Message::JobFinished { result, is_move } => {
                    self.job_history.push_back(format!(
                        "{}\n{}",
                        Local::now().format("%d/%m/%Y %H:%M"),
                        result
                            .as_ref()
                            .map(|s| s.as_str())
                            .unwrap_or_else(|e| e.as_str())
                    ));
                    if let Some(last) = self.job_history.back_mut() {
                        if last.len() > 32 * 1024 {
                            let mut end = 32 * 1024;
                            while !last.is_char_boundary(end) {
                                end -= 1;
                            }
                            last.truncate(end);
                            last.push_str("\nRiepilogo troncato a 32 KiB");
                        }
                    }
                    while self.job_history.len() > 50 {
                        self.job_history.pop_front();
                    }
                    self.busy = false;
                    self.job_progress = None;
                    self.job_started_at = None;
                    match result {
                        Ok(message) => {
                            if is_move {
                                self.clipboard = None;
                            }
                            self.refresh();
                            self.status = message
                                .lines()
                                .next()
                                .unwrap_or("Operazione completata")
                                .to_owned();
                        }
                        Err(error) => {
                            // Some operations may have changed files before reporting an error.
                            self.refresh();
                            self.status = format!(
                                "Errore: {}",
                                error.lines().next().unwrap_or("Operazione non completata")
                            );
                        }
                    }
                }
                Message::JobProgress(progress) => {
                    self.job_progress = Some(progress);
                    self.status = format!(
                        "Completati {}/{} elementi",
                        progress.done_items, progress.total_items
                    );
                }
                Message::SearchFinished { generation, result } => {
                    if generation == self.search_generation {
                        self.searching = false;
                        match result {
                            Ok(hits) => {
                                self.status = format!("{} risultati trovati", hits.len());
                                self.search_results = hits;
                            }
                            Err(e) => self.status = format!("Ricerca: {e}"),
                        }
                    }
                }
                Message::PreviewFinished { generation, result } => {
                    if generation == self.preview_generation {
                        self.preview_data = result.ok();
                    }
                }
                Message::PreviewImageLoaded { generation, result } => {
                    if generation == self.preview_generation {
                        self.preview_image = result.ok().map(|bitmap| {
                            Self::make_texture(&self.context, "veldir-preview", bitmap)
                        });
                    }
                }
                Message::ThumbnailLoaded {
                    path,
                    result,
                    generation,
                } => {
                    self.thumb_pending.remove(&path);
                    if generation != self.asset_generation {
                        continue;
                    }
                    match result {
                        Ok(bitmap) => {
                            // LRU-ish bounded cache; old textures are released.
                            while self.thumb_order.len() >= 128 {
                                if let Some(old) = self.thumb_order.pop_front() {
                                    self.thumb_cache.remove(&old);
                                }
                            }
                            let tex = Self::make_texture(
                                &self.context,
                                &format!("thumb:{}", path.display()),
                                bitmap,
                            );
                            self.thumb_cache.insert(path.clone(), tex);
                            self.thumb_order.push_back(path);
                        }
                        Err(_) => {
                            if self.thumb_failed.len() > 512 {
                                self.thumb_failed.clear();
                            }
                            self.thumb_failed.insert(path);
                        }
                    }
                }
                Message::IconLoaded { key, result } => {
                    self.icon_pending.remove(&key);
                    match result {
                        Ok(bitmap) => {
                            let tex = Self::make_texture(
                                &self.context,
                                &format!("shell-icon:{key}"),
                                bitmap,
                            );
                            if self.icon_cache.len() >= 256 {
                                self.icon_cache.clear();
                            }
                            self.icon_cache.insert(key, tex);
                        }
                        Err(_) => {
                            if self.icon_failed.len() > 512 {
                                self.icon_failed.clear();
                            }
                            self.icon_failed.insert(key);
                        }
                    }
                }
            }
        }
    }

    fn begin_job(&mut self, job: Job) {
        if self.busy {
            return;
        }
        let job = match job {
            Job::CopyMany {
                sources,
                destination_folder,
            } => {
                self.dialog = Dialog::Transfer {
                    sources,
                    destination: destination_folder,
                    cut: false,
                    policy: crate::transfer::ConflictPolicy::Stop,
                };
                return;
            }
            Job::MoveMany {
                sources,
                destination_folder,
            } => {
                self.dialog = Dialog::Transfer {
                    sources,
                    destination: destination_folder,
                    cut: true,
                    policy: crate::transfer::ConflictPolicy::Stop,
                };
                return;
            }
            other => other,
        };
        self.job_sources = match &job {
            Job::Transfer { sources, .. }
            | Job::CopyMany { sources, .. }
            | Job::MoveMany { sources, .. }
            | Job::TrashMany { sources } => sources.clone(),
            Job::Copy { source, .. }
            | Job::Move { source, .. }
            | Job::Trash { source }
            | Job::Rename { source, .. } => vec![source.clone()],
            Job::RenameMany { changes } => changes.iter().map(|(p, _)| p.clone()).collect(),
            _ => Vec::new(),
        };
        self.busy = true;
        self.job_progress = None;
        self.job_started_at = Some(Instant::now());
        self.job_cancel = Arc::new(AtomicBool::new(false));
        self.status = "Operazione in corso...".into();
        let sender = self.tx.clone();
        let ctx = self.context.clone();
        let cancelled = self.job_cancel.clone();
        let is_move = matches!(
            &job,
            Job::Move { .. } | Job::MoveMany { .. } | Job::Transfer { cut: true, .. }
        );
        std::thread::spawn(move || {
            let result = fs_ops::execute_progress(job, &cancelled, |progress| {
                let _ = sender.send(Message::JobProgress(progress));
                ctx.request_repaint();
            });
            let _ = sender.send(Message::JobFinished { result, is_move });
            ctx.request_repaint();
        });
    }

    fn start_search(&mut self) {
        let query = self.search_query.trim().to_owned();
        if query.is_empty() {
            self.status = "Inserisci almeno un carattere per cercare".into();
            return;
        }
        self.search_cancel.store(true, Ordering::Relaxed);
        self.search_cancel = Arc::new(AtomicBool::new(false));
        self.search_generation += 1;
        let generation = self.search_generation;
        let root = self.search_root.clone();
        let sender = self.tx.clone();
        let ctx = self.context.clone();
        let cancelled = self.search_cancel.clone();
        self.searching = true;
        self.status = "Ricerca in corso...".into();
        std::thread::spawn(move || {
            let result = search::find(&root, &query, 250, &cancelled);
            let _ = sender.send(Message::SearchFinished { generation, result });
            ctx.request_repaint();
        });
    }

    fn make_texture(
        ctx: &egui::Context,
        label: &str,
        bitmap: image_view::Bitmap,
    ) -> egui::TextureHandle {
        let image =
            egui::ColorImage::from_rgba_unmultiplied([bitmap.width, bitmap.height], &bitmap.rgba);
        ctx.load_texture(label, image, egui::TextureOptions::LINEAR)
    }

    fn schedule_assets(&mut self, images: Vec<PathBuf>, icons: Vec<String>) {
        if self.thumbs_visible {
            for path in images {
                if self.thumb_pending.len() >= 6 {
                    break;
                }
                if self.thumb_pending.contains(&path)
                    || self.thumb_cache.contains_key(&path)
                    || self.thumb_failed.contains(&path)
                {
                    continue;
                }
                self.thumb_pending.insert(path.clone());
                let generation = self.asset_generation;
                let sender = self.tx.clone();
                let ctx = self.context.clone();
                std::thread::spawn(move || {
                    let result = image_view::load(&path, 40);
                    let _ = sender.send(Message::ThumbnailLoaded {
                        path,
                        result,
                        generation,
                    });
                    ctx.request_repaint();
                });
            }
        }
        for key in icons {
            if self.icon_pending.len() >= 4 {
                break;
            }
            if self.icon_pending.contains(&key)
                || self.icon_cache.contains_key(&key)
                || self.icon_failed.contains(&key)
            {
                continue;
            }
            self.icon_pending.insert(key.clone());
            let sender = self.tx.clone();
            let ctx = self.context.clone();
            std::thread::spawn(move || {
                let result = native_icons::load(&key);
                let _ = sender.send(Message::IconLoaded { key, result });
                ctx.request_repaint();
            });
        }
    }

    fn update_preview(&mut self) {
        if !self.preview_visible {
            return;
        }
        let wanted = self.selected_one();
        if wanted == self.preview_request {
            return;
        }
        self.preview_generation += 1;
        self.preview_request = wanted.clone();
        self.preview_data = None;
        self.preview_image = None;
        if let Some(path) = wanted {
            let generation = self.preview_generation;
            let sender = self.tx.clone();
            let ctx = self.context.clone();
            if image_view::supported(&path) {
                let image_path = path.clone();
                let image_tx = self.tx.clone();
                let image_ctx = self.context.clone();
                std::thread::spawn(move || {
                    let result = image_view::load(&image_path, 1200);
                    let _ = image_tx.send(Message::PreviewImageLoaded { generation, result });
                    image_ctx.request_repaint();
                });
            }
            std::thread::spawn(move || {
                let result = preview::details(&path);
                let _ = sender.send(Message::PreviewFinished { generation, result });
                ctx.request_repaint();
            });
        }
    }

    fn reveal_result(&mut self, path: PathBuf) {
        let Some(parent) = path.parent().map(Path::to_path_buf) else {
            return;
        };
        let same = display_path(&self.current().path).eq_ignore_ascii_case(&display_path(&parent));
        self.current_mut().filter.clear();
        self.current_mut().criteria = filters::Criteria::default();
        if same {
            if let Some(row) = self.current().entries.iter().position(|e| e.path == path) {
                let tab = self.current_mut();
                tab.selected.clear();
                tab.selected.insert(path.clone());
                tab.cursor = Some(path);
                tab.scroll_to = Some(row);
                return;
            }
            self.reveal_path = Some(path);
            self.reload_current();
        } else {
            self.reveal_path = Some(path);
            self.navigate(parent);
        }
    }

    fn search_ui(&mut self, ctx: &egui::Context) {
        if !self.search_visible {
            return;
        }
        let mut open = true;
        let mut execute = false;
        let mut choose = None;
        egui::Window::new("Ricerca nelle sottocartelle").open(&mut open)
            .default_size([720.0,480.0]).show(ctx,|ui|{
            let mut root=display_path(&self.search_root);
            ui.label("Cartella da esplorare:");
            if ui.text_edit_singleline(&mut root).changed(){
                self.search_cancel.store(true,Ordering::Relaxed);self.search_generation+=1;self.searching=false;
                self.search_root=PathBuf::from(root);
            }
            ui.horizontal(|ui|{
                let r=ui.add(egui::TextEdit::singleline(&mut self.search_query).hint_text("Nome o ricerca fuzzy").desired_width(350.0));
                if r.lost_focus()&&ui.input(|i|i.key_pressed(egui::Key::Enter)){execute=true;}
                if ui.add_enabled(!self.searching,egui::Button::new("Cerca")).clicked(){execute=true;}
                if self.searching&&ui.button("Interrompi").clicked(){
                    self.search_cancel.store(true,Ordering::Relaxed);self.search_generation+=1;self.searching=false;
                    self.status="Ricerca interrotta; risultati precedenti conservati".into();
                }
            });
            ui.label("Massimo 250 risultati; collegamenti esclusi. Il filtro qui sotto agisce sui risultati.");
            ui.horizontal(|ui|{
                ui.add(egui::TextEdit::singleline(&mut self.search_filter).hint_text("Filtra risultati"));
                if ui.button("Ordine percorso").clicked(){self.search_descending=!self.search_descending;}
                if self.searching{ui.spinner();}
            });
            let filter=self.search_filter.to_lowercase();
            let mut hits:Vec<_>=self.search_results.iter().filter(|h|display_path(&h.path).to_lowercase().contains(&filter)).collect();
            hits.sort_by(|a,b|a.path.cmp(&b.path));
            if self.search_descending{hits.reverse();}
            ui.label(format!("{} risultati visibili",hits.len()));
            egui::ScrollArea::vertical().max_height(320.0).show(ui,|ui|{
                for hit in hits {
                    if ui.button(format!("{} {}",if hit.is_dir{"Cartella:"}else{"File:"},display_path(&hit.path))).on_hover_text("Apri posizione e seleziona il risultato").clicked(){choose=Some(hit.path.clone());}
                }
            });
        });
        self.search_visible = open;
        if !open && self.searching {
            self.search_cancel.store(true, Ordering::Relaxed);
            self.search_generation += 1;
            self.searching = false;
        }
        if execute {
            self.start_search();
        }
        if let Some(path) = choose {
            if !path.exists() {
                self.status = "Il risultato non è più disponibile".into();
            } else {
                self.search_visible = false;
                if path.is_dir() {
                    self.navigate(path);
                } else if let Some(parent) = path.parent() {
                    let _ = parent;
                    self.reveal_result(path);
                }
            }
        }
    }

    fn information_ui(&mut self, ctx: &egui::Context) {
        egui::Window::new("Informazioni su Veldir")
            .open(&mut self.about_visible)
            .show(ctx, |ui| {
                ui.heading(concat!("Veldir ", env!("CARGO_PKG_VERSION")));
                ui.label(format!("Build: {}", env!("VELDIR_BUILD_COMMIT")));
                ui.label("Ctrl+T: nuova scheda · Ctrl+Maiusc+T: riapri · Ctrl+W: chiudi");
                ui.label("F6: pannello · F2: rinomina · F3: anteprima · F5: aggiorna");
                ui.label("Frecce/Home/Fine/Pagina: navigazione · Maiusc: estendi selezione");
                ui.label("Ctrl+L: percorso · Ctrl+F: filtro · Ctrl+Maiusc+F: ricerca");
                ui.label(
                    "Ctrl+A/C/X/V: seleziona/copia/taglia/incolla · Backspace: cartella superiore",
                );
                ui.label("Trascina le intestazioni dei metadati per regolare le colonne.");
            });
        egui::Window::new("Operazioni recenti")
            .open(&mut self.history_visible)
            .show(ctx, |ui| {
                if self.job_history.is_empty() {
                    ui.label("Nessuna operazione in questa sessione");
                }
                egui::ScrollArea::vertical()
                    .max_height(450.0)
                    .show(ui, |ui| {
                        for entry in self.job_history.iter().rev() {
                            ui.label(entry);
                            ui.separator();
                        }
                    });
            });
    }

    fn preview_ui(&mut self, ctx: &egui::Context) {
        if !self.preview_visible {
            return;
        }
        let mut open = self.preview_visible;
        egui::Window::new("Dettagli e anteprima")
            .open(&mut open)
            .default_size([430.0, 480.0])
            .resizable(true)
            .show(ctx, |ui| {
                if let Some(data) = &self.preview_data {
                    ui.heading(data.path.file_name().unwrap_or_default().to_string_lossy());
                    ui.label(format!(
                        "Tipo: {}  •  Dimensione: {}",
                        data.kind,
                        pretty_bytes(data.bytes)
                    ));
                    ui.label(
                        RichText::new(data.path.display().to_string())
                            .small()
                            .color(muted_color(ui)),
                    );
                    ui.separator();
                    if let Some(tex) = &self.preview_image {
                        ui.horizontal(|ui| {
                            if ui.button("Adatta").clicked() {
                                self.preview_zoom = 0.0;
                            }
                            if ui.button("100%").clicked() {
                                self.preview_zoom = 1.0;
                            }
                            ui.add(
                                egui::Slider::new(&mut self.preview_zoom, 0.0..=4.0).text("Zoom"),
                            );
                        });
                        let size = tex.size_vec2();
                        let scale = if self.preview_zoom == 0.0 {
                            ((ui.available_width() - 18.0) / size.x)
                                .min(340.0 / size.y)
                                .min(1.0)
                        } else {
                            self.preview_zoom
                        };
                        egui::ScrollArea::both().max_height(360.0).show(ui, |ui| {
                            ui.add(egui::Image::new(tex).fit_to_exact_size(size * scale.max(0.05)));
                        });
                    }
                    if let Some(note) = &data.note {
                        if self.preview_image.is_none() {
                            ui.label(RichText::new(note).small().color(muted_color(ui)));
                        }
                    }
                    if let Some(content) = &data.text {
                        egui::ScrollArea::both().max_height(350.0).show(ui, |ui| {
                            ui.add(egui::Label::new(RichText::new(content).monospace()).wrap());
                        });
                    }
                } else {
                    ui.label("Seleziona un file per visualizzarne i dettagli");
                }
            });
        self.preview_visible = open;
        if !open {
            self.preview_request = None;
            self.preview_data = None;
            self.preview_image = None;
        }
    }

    fn selected(&self) -> Vec<PathBuf> {
        if self.current().loading || self.current().error.is_some() {
            return Vec::new();
        }
        self.current().selected.iter().cloned().collect()
    }

    fn selected_one(&self) -> Option<PathBuf> {
        if self.current().loading || self.current().error.is_some() {
            return None;
        }
        if self.current().selected.len() == 1 {
            self.current().selected.iter().next().cloned()
        } else {
            None
        }
    }

    fn open_entry(&mut self, path: PathBuf) {
        if path.is_dir() {
            self.navigate(path);
        } else if let Err(error) = opener::open(&path) {
            self.status = format!("Impossibile aprire {}: {error}", path.display());
        }
    }

    fn open_selected(&mut self) {
        if self.current().loading {
            return;
        }
        let cursor = self
            .current()
            .cursor
            .clone()
            .filter(|p| self.current().entries.iter().any(|e| &e.path == p));
        if let Some(path) = cursor.or_else(|| self.selected_one()) {
            self.open_entry(path);
        }
    }

    fn prepare_rename(&mut self) {
        if let Some(source) = self.selected_one() {
            let name = source
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            self.dialog = Dialog::Rename { source, name };
        }
    }

    fn prepare_trash(&mut self) {
        let sources = self.selected();
        if !sources.is_empty() {
            self.dialog = Dialog::Delete(sources);
        }
    }

    fn clipboard_set(&mut self, cut: bool) {
        let sources = self.selected();
        if sources.is_empty() {
            return;
        }
        self.clipboard = Some(InternalClipboard {
            sources: sources.clone(),
            cut,
        });
        if cut {
            // Cut stays local until a confirmed move. Never publish a MOVE intent to Explorer.
            self.status = "Taglia pronto in Veldir: incolla con Ctrl+V (solo interno)".into();
        } else {
            match crate::shell_clipboard::copy_files_to_explorer(&sources) {
                Ok(()) => {
                    self.status = format!(
                        "{} elementi copiati: clipboard Veldir e Windows",
                        sources.len()
                    )
                }
                Err(error) => {
                    self.status =
                        format!("Copia interna pronta. Clipboard Windows non aggiornata: {error}")
                }
            }
        }
    }

    fn copy_selection_to_windows(&mut self) {
        let sources = self.selected();
        if sources.is_empty() {
            return;
        }
        match crate::shell_clipboard::copy_files_to_explorer(&sources) {
            Ok(()) => {
                self.status = format!("{} elementi copiati nella clipboard Windows", sources.len())
            }
            Err(err) => self.status = format!("Clipboard Windows: {err}"),
        }
    }

    fn import_explorer_clipboard(&mut self) {
        if self.busy || !matches!(self.dialog, Dialog::None) {
            return;
        }
        match crate::shell_clipboard::files_from_explorer() {
            Ok(sources) => {
                self.dialog = Dialog::ExternalDrop {
                    sources,
                    destination: self.current().path.clone(),
                };
            }
            Err(error) => {
                self.status = format!("Clipboard Windows: {error}");
            }
        }
    }

    fn paste(&mut self) {
        if let Some(clip) = self.clipboard.clone() {
            let destination_folder = self.current().path.clone();
            let job = if clip.cut {
                Job::MoveMany {
                    sources: clip.sources,
                    destination_folder,
                }
            } else {
                Job::CopyMany {
                    sources: clip.sources,
                    destination_folder,
                }
            };
            self.begin_job(job);
        }
    }

    fn transfer_to_other(&mut self, cut: bool) {
        let sources = self.selected();
        if sources.is_empty() {
            return;
        }
        let Some(right) = &self.secondary else {
            return;
        };
        let destination_folder = if self.focused_right {
            self.tabs[self.active].path.clone()
        } else {
            right.path.clone()
        };
        if cut {
            self.dialog = Dialog::ConfirmMove {
                sources,
                destination_folder,
            };
        } else {
            self.begin_job(Job::CopyMany {
                sources,
                destination_folder,
            });
        }
    }

    fn change_sort(&mut self, right: bool, by: SortBy) {
        let tab = self.panel_mut(right);
        if tab.sort_by == by {
            tab.descending = !tab.descending;
        } else {
            tab.sort_by = by;
            tab.descending = false;
        }
        sort_entries(&mut tab.entries, tab.sort_by, tab.descending);
    }

    fn select_all_visible(&mut self) {
        let tab = self.current_mut();
        let filter = tab.filter.to_lowercase();
        let criteria = tab.criteria;
        let now = SystemTime::now();
        tab.selected = tab
            .entries
            .iter()
            .filter(|item| filters::matches(item, &filter, criteria, now))
            .map(|item| item.path.clone())
            .collect();
        tab.anchor = None;
    }

    fn toggle_favorite(&mut self) {
        let path = self.current().path.clone();
        if let Some(i) = self.favorites.iter().position(|p| p == &path) {
            self.favorites.remove(i);
        } else {
            self.favorites.push(path);
        }
    }

    fn navigate_rows(&mut self, ctx: &egui::Context) {
        let filter = self.current().filter.to_lowercase();
        let now = SystemTime::now();
        let visible: Vec<PathBuf> = self
            .current()
            .entries
            .iter()
            .filter(|e| filters::matches(e, &filter, self.current().criteria, now))
            .map(|e| e.path.clone())
            .collect();
        if visible.is_empty() {
            return;
        }
        let current = self
            .current()
            .cursor
            .as_ref()
            .and_then(|p| visible.iter().position(|v| v == p));
        let page = ((ctx.content_rect().height() - 350.0) / 39.0).max(1.0) as isize;
        let mut next = None;
        for (key, delta) in [
            (egui::Key::ArrowDown, 1),
            (egui::Key::ArrowUp, -1),
            (egui::Key::PageDown, page),
            (egui::Key::PageUp, -page),
        ] {
            if ctx.input(|i| i.key_pressed(key)) {
                next = crate::ui_state::step_index(current, delta, visible.len());
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Home)) {
            next = Some(0);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::End)) {
            next = Some(visible.len() - 1);
        }
        let text = ctx.input(|i| {
            if i.modifiers.command || i.modifiers.alt {
                String::new()
            } else {
                i.events
                    .iter()
                    .filter_map(|e| {
                        if let egui::Event::Text(s) = e {
                            Some(s.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<String>()
            }
        });
        if !text.is_empty() {
            if self
                .quick_find_at
                .is_none_or(|t| t.elapsed() > Duration::from_secs(1))
            {
                self.quick_find.clear();
            }
            self.quick_find.push_str(&text.to_lowercase());
            self.quick_find_at = Some(Instant::now());
            next = visible.iter().position(|p| {
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase()
                    .starts_with(&self.quick_find)
            });
        }
        if let Some(row) = next {
            let modifiers = ctx.input(|i| i.modifiers);
            let tab = self.current_mut();
            let path = visible[row].clone();
            tab.cursor = Some(path.clone());
            tab.scroll_to = Some(row);
            if !modifiers.command || modifiers.shift {
                apply_selection(
                    &mut tab.selected,
                    &mut tab.anchor,
                    &visible,
                    path,
                    modifiers.shift,
                    modifiers.command,
                );
            }
        }
    }

    fn keyboard(&mut self, ctx: &egui::Context) {
        // Dialogs are modal: navigation and file shortcuts must not change their context.
        if !matches!(self.dialog, Dialog::None) {
            return;
        }
        let shortcut = |key: egui::Key| ctx.input(|i| i.modifiers.command && i.key_pressed(key));
        if shortcut(egui::Key::T) {
            if ctx.input(|i| i.modifiers.shift) {
                if let Some(path) = self.closed_tabs.pop() {
                    self.add_tab(path);
                }
            } else {
                self.add_tab(self.current().path.clone());
            }
        }
        if shortcut(egui::Key::W) {
            let id = self.tabs[self.active].id;
            self.close_tab(id);
        }
        if shortcut(egui::Key::L) {
            self.editing_address = true;
            self.address_input = self.current().path.display().to_string();
            ctx.memory_mut(|m| m.request_focus(egui::Id::unique("address_bar")));
        }
        if ctx.input(|i| i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::F)) {
            self.search_visible = true;
            self.search_root = self.current().path.clone();
            self.search_results.clear();
        } else if shortcut(egui::Key::F) {
            let filter_id = if self.focused_right {
                "folder_filter_right"
            } else {
                "folder_filter"
            };
            ctx.memory_mut(|m| m.request_focus(egui::Id::unique(filter_id)));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F3)) {
            self.preview_visible = !self.preview_visible;
            self.preview_request = None;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F5)) {
            self.refresh();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F6)) {
            if self.secondary.is_some() {
                self.focus(!self.focused_right);
            } else {
                self.toggle_dual();
            }
        }
        if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft)) {
            self.history_back();
        }
        if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight)) {
            self.history_forward();
        }
        if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowUp)) {
            self.up();
        }

        if !ctx.egui_wants_keyboard_input() && !self.busy && matches!(self.dialog, Dialog::None) {
            self.navigate_rows(ctx);
            if ctx.input(|i| i.key_pressed(egui::Key::Backspace)) {
                self.up();
            }
            if shortcut(egui::Key::C) {
                self.clipboard_set(false);
            }
            if shortcut(egui::Key::X) {
                self.clipboard_set(true);
            }
            if ctx
                .input(|i| i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::V))
            {
                self.import_explorer_clipboard();
            } else if shortcut(egui::Key::V) {
                self.paste();
            }
            if shortcut(egui::Key::A) {
                self.select_all_visible();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.current_mut().selected.clear();
                self.current_mut().anchor = None;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::F2)) {
                self.prepare_rename();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Delete)) {
                self.prepare_trash();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.open_selected();
            }
        }
    }

    fn tabs_ui(&mut self, ui: &mut egui::Ui) {
        let mut switch = None;
        let mut close = None;
        let mut action = None;
        let mut reorder = None;
        let mut drag = self.drag_tab;
        ui.horizontal(|ui| {
            ui.menu_button("Schede", |ui| {
                for (index, tab) in self.tabs.iter().enumerate() {
                    if ui
                        .selectable_label(self.active == index, display_path(&tab.path))
                        .clicked()
                    {
                        switch = Some(index);
                        ui.close();
                    }
                }
                if ui.button("Riapri scheda chiusa").clicked() {
                    action = Some(("reopen", self.current().path.clone()));
                    ui.close();
                }
            });
            egui::ScrollArea::horizontal()
                .id_salt("tabs_scroll")
                .auto_shrink([false, true])
                .max_height(38.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (index, tab) in self.tabs.iter().enumerate() {
                            let name = tab
                                .path
                                .file_name()
                                .unwrap_or(tab.path.as_os_str())
                                .to_string_lossy();
                            let title = if name.chars().count() > 22 {
                                format!("{}…", name.chars().take(21).collect::<String>())
                            } else {
                                name.to_string()
                            };
                            let r = ui
                                .add(
                                    egui::Button::new(title)
                                        .selected(self.active == index)
                                        .sense(egui::Sense::click_and_drag()),
                                )
                                .on_hover_text(display_path(&tab.path));
                            if r.clicked() {
                                switch = Some(index);
                            }
                            if r.drag_started() {
                                drag = Some(tab.id);
                            }
                            if r.hovered() && ui.input(|i| i.pointer.any_released()) {
                                if let Some(id) = drag {
                                    if id != tab.id {
                                        reorder = Some((id, index));
                                    }
                                }
                            }
                            r.context_menu(|ui| {
                                for (label, kind) in [
                                    ("Duplica", "duplicate"),
                                    ("Apri nel pannello opposto", "other"),
                                    ("Chiudi", "close"),
                                ] {
                                    if ui.button(label).clicked() {
                                        action = Some((kind, tab.path.clone()));
                                        if kind == "close" {
                                            close = Some(tab.id);
                                        }
                                        ui.close();
                                    }
                                }
                            });
                            if ui
                                .small_button("×")
                                .on_hover_text("Chiudi scheda")
                                .clicked()
                            {
                                close = Some(tab.id);
                            }
                        }
                        if ui
                            .button("+")
                            .on_hover_text("Nuova scheda · Ctrl+T")
                            .clicked()
                        {
                            action = Some(("duplicate", self.current().path.clone()));
                        }
                    });
                });
        });
        self.drag_tab = if ui.input(|i| i.pointer.any_released()) {
            None
        } else {
            drag
        };
        if let Some((id, target)) = reorder {
            let active_id = self.tabs[self.active].id;
            if let Some(from) = self.tabs.iter().position(|t| t.id == id) {
                let tab = self.tabs.remove(from);
                self.tabs.insert(target.min(self.tabs.len()), tab);
                self.active = self
                    .tabs
                    .iter()
                    .position(|t| t.id == active_id)
                    .unwrap_or(0);
            }
        }
        if let Some(index) = switch {
            if index < self.tabs.len() {
                self.active = index;
                self.focused_right = false;
                self.address_input = display_path(&self.current().path);
                self.editing_address = false;
            }
        }
        if let Some(id) = close {
            self.close_tab(id);
        }
        if let Some((kind, path)) = action {
            match kind {
                "duplicate" => self.add_tab(path),
                "other" => {
                    if self.secondary.is_none() {
                        self.open_secondary(path);
                    } else {
                        self.focus(true);
                        self.navigate(path);
                    }
                }
                "reopen" => {
                    if let Some(p) = self.closed_tabs.pop() {
                        self.add_tab(p);
                    }
                }
                _ => {}
            }
        }
    }

    fn breadcrumbs_ui(&mut self, ui: &mut egui::Ui) {
        let path = self.current().path.clone();
        let ancestors: Vec<PathBuf> = path.ancestors().map(Path::to_path_buf).collect();
        let mut navigation = None;
        ui.horizontal(|ui| {
            for (index, crumb) in ancestors.iter().rev().enumerate() {
                if index > 0 {
                    ui.label(RichText::new("›").color(muted_color(ui)));
                }
                let part = crumb
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(str::to_owned)
                    .unwrap_or_else(|| crumb.display().to_string());
                let is_current = crumb == &path;
                let part = display_path(Path::new(&part));
                let label = if part.chars().count() > 22 {
                    format!("{}…", part.chars().take(21).collect::<String>())
                } else {
                    part
                };
                if is_current {
                    ui.label(RichText::new(label).strong().color(accent_color(ui)));
                } else if ui
                    .add(egui::Button::new(label).frame(false))
                    .on_hover_text(crumb.display().to_string())
                    .clicked()
                {
                    navigation = Some(crumb.clone());
                }
            }
        });
        if let Some(target) = navigation {
            self.navigate(target);
        }
    }

    fn toolbar_ui(&mut self, ui: &mut egui::Ui) {
        let dark = self.dark_theme;
        let background = if dark {
            Color32::from_rgb(29, 37, 49)
        } else {
            Color32::from_rgb(238, 243, 249)
        };
        egui::Frame::new()
            .fill(background)
            .corner_radius(egui::CornerRadius::same(11))
            .inner_margin(egui::Margin::symmetric(10, 9))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!self.current().back.is_empty(), egui::Button::new("<"))
                        .on_hover_text("Indietro · Alt+←")
                        .clicked()
                    {
                        self.history_back();
                    }
                    if ui
                        .add_enabled(!self.current().forward.is_empty(), egui::Button::new(">"))
                        .on_hover_text("Avanti · Alt+→")
                        .clicked()
                    {
                        self.history_forward();
                    }
                    if ui
                        .button("Su")
                        .on_hover_text("Cartella superiore")
                        .clicked()
                    {
                        self.up();
                    }
                    if ui
                        .button("Aggiorna")
                        .on_hover_text("Aggiorna · F5")
                        .clicked()
                    {
                        self.refresh();
                    }
                    ui.separator();
                    if self.editing_address {
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.address_input)
                                .desired_width((ui.available_width() - 75.0).max(180.0))
                                .id(egui::Id::unique("address_bar")),
                        );
                        let go = ui.button("Vai").clicked();
                        if (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                            || go
                        {
                            self.navigate(PathBuf::from(self.address_input.trim()));
                        }
                    } else {
                        if ui
                            .small_button("Percorso")
                            .on_hover_text("Modifica percorso · Ctrl+L")
                            .clicked()
                        {
                            self.editing_address = true;
                            self.address_input = self.current().path.display().to_string();
                        }
                        egui::ScrollArea::horizontal()
                            .id_salt("breadcrumbs_scroll")
                            .auto_shrink([false, true])
                            .max_width(ui.available_width())
                            .show(ui, |ui| self.breadcrumbs_ui(ui));
                    }
                });
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    if ui
                        .button("+ Nuova cartella")
                        .on_hover_text("Crea una cartella")
                        .clicked()
                        && !self.busy
                    {
                        self.dialog = Dialog::NewFolder("Nuova cartella".into());
                    }
                    let selected = !self.selected().is_empty();
                    let one = self.selected_one().is_some();
                    let many = self.selected().len() > 1;
                    if ui
                        .add_enabled(!self.busy && selected, egui::Button::new("Copia"))
                        .on_hover_text("Ctrl+C · copia anche in Esplora")
                        .clicked()
                    {
                        self.clipboard_set(false);
                    }
                    if ui
                        .add_enabled(
                            !self.busy && self.clipboard.is_some(),
                            egui::Button::new("Incolla"),
                        )
                        .on_hover_text("Ctrl+V · clipboard interna")
                        .clicked()
                    {
                        self.paste();
                    }
                    if ui
                        .button("Cerca")
                        .on_hover_text("Ricerca ricorsiva · Ctrl+Maiusc+F")
                        .clicked()
                    {
                        self.search_visible = true;
                        self.search_root = self.current().path.clone();
                        self.search_results.clear();
                    }
                    if ui
                        .button(if self.preview_visible {
                            "Chiudi anteprima"
                        } else {
                            "Anteprima"
                        })
                        .on_hover_text("Immagini, PDF (testo), dettagli · F3")
                        .clicked()
                    {
                        self.preview_visible = !self.preview_visible;
                        self.preview_request = None;
                    }
                    let dual = self.secondary.is_some();
                    if ui
                        .button(if dual { "Un pannello" } else { "Due pannelli" })
                        .on_hover_text("Mostra o nascondi il secondo pannello")
                        .clicked()
                    {
                        self.toggle_dual();
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let favorite = self.favorites.contains(&self.current().path);
                        if ui
                            .button(if favorite { "Preferito" } else { "Preferiti" })
                            .on_hover_text("Aggiungi o rimuovi dai preferiti")
                            .clicked()
                        {
                            self.toggle_favorite();
                        }
                        ui.menu_button("Visualizza", |ui| {
                            ui.checkbox(&mut self.columns.kind, "Colonna tipo");
                            ui.checkbox(&mut self.columns.size, "Colonna dimensione");
                            ui.checkbox(&mut self.columns.modified, "Colonna modificato");
                            ui.checkbox(
                                &mut self.columns.metadata_in_dual,
                                "Metadati nel doppio pannello",
                            );
                            if ui.button("Ripristina colonne").clicked() {
                                self.columns = Columns::default();
                            }
                            ui.checkbox(&mut self.restore_session, "Ripristina sessione all’avvio");
                            if ui.button("Informazioni e scorciatoie").clicked() {
                                self.about_visible = true;
                                ui.close();
                            }
                            if ui.button("Operazioni recenti").clicked() {
                                self.history_visible = true;
                                ui.close();
                            }
                            if ui
                                .checkbox(&mut self.thumbs_visible, "Miniature immagini")
                                .changed()
                            {
                                ui.close();
                            }
                            if ui
                                .checkbox(&mut self.sidebar_visible, "Barra laterale")
                                .changed()
                            {
                                ui.close();
                            }
                            if ui
                                .checkbox(
                                    &mut self.live_refresh,
                                    "Aggiornamento automatico cartelle",
                                )
                                .changed()
                            {
                                ui.close();
                            }
                            let was_dark = self.dark_theme;
                            if ui.checkbox(&mut self.dark_theme, "Tema scuro").changed() {
                                ui.close();
                            }
                            if was_dark != self.dark_theme {
                                apply_theme(&self.context, self.dark_theme);
                            }
                        });
                        ui.menu_button("Operazioni", |ui| {
                            if ui
                                .add_enabled(
                                    !self.busy && one,
                                    egui::Button::new("Rinomina  ·  F2"),
                                )
                                .clicked()
                            {
                                self.prepare_rename();
                                ui.close();
                            }
                            if ui
                                .add_enabled(
                                    !self.busy && many,
                                    egui::Button::new("Rinomina multipla"),
                                )
                                .clicked()
                            {
                                self.dialog = Dialog::BatchRename {
                                    sources: self.selected(),
                                    prefix: String::new(),
                                    find: String::new(),
                                    replace: String::new(),
                                    start: "1".into(),
                                };
                                ui.close();
                            }
                            if ui
                                .add_enabled(
                                    !self.busy && selected,
                                    egui::Button::new("Taglia  ·  Ctrl+X"),
                                )
                                .clicked()
                            {
                                self.clipboard_set(true);
                                ui.close();
                            }
                            if ui
                                .add_enabled(
                                    !self.busy && selected,
                                    egui::Button::new("Cestino  ·  Canc"),
                                )
                                .clicked()
                            {
                                self.prepare_trash();
                                ui.close();
                            }
                            ui.separator();
                            if ui
                                .add_enabled(!self.busy, egui::Button::new("Importa da Esplora"))
                                .clicked()
                            {
                                self.import_explorer_clipboard();
                                ui.close();
                            }
                            if ui
                                .add_enabled(
                                    !self.busy && selected,
                                    egui::Button::new("Copia verso Esplora"),
                                )
                                .clicked()
                            {
                                self.copy_selection_to_windows();
                                ui.close();
                            }
                            if dual {
                                ui.separator();
                                if ui
                                    .add_enabled(
                                        !self.busy && selected,
                                        egui::Button::new("Copia nel pannello opposto"),
                                    )
                                    .clicked()
                                {
                                    self.transfer_to_other(false);
                                    ui.close();
                                }
                                if ui
                                    .add_enabled(
                                        !self.busy && selected,
                                        egui::Button::new("Sposta nel pannello opposto"),
                                    )
                                    .clicked()
                                {
                                    self.transfer_to_other(true);
                                    ui.close();
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
        ui.label(
            RichText::new("ACCESSO RAPIDO")
                .color(muted_color(ui))
                .small()
                .strong(),
        );
        ui.add_space(4.0);
        let mut target = None;
        for (name, path) in &self.shortcuts {
            if ui
                .add_sized(
                    [190.0, 27.0],
                    egui::Button::new(format!("  {name}"))
                        .shortcut_text("")
                        .frame(false)
                        .selected(&self.current().path == path),
                )
                .clicked()
            {
                target = Some(path.clone());
            }
        }
        ui.add_space(15.0);
        ui.label(
            RichText::new("PREFERITI")
                .color(muted_color(ui))
                .small()
                .strong(),
        );
        ui.add_space(4.0);
        for path in &self.favorites {
            let title = path
                .file_name()
                .and_then(|s| s.to_str())
                .map(str::to_owned)
                .unwrap_or_else(|| path.display().to_string());
            if ui
                .add_sized(
                    [190.0, 27.0],
                    egui::Button::new(format!("  {title}"))
                        .shortcut_text("")
                        .frame(false)
                        .selected(&self.current().path == path),
                )
                .on_hover_text(path.display().to_string())
                .clicked()
            {
                target = Some(path.clone());
            }
        }
        ui.add_space(15.0);
        ui.label(
            RichText::new("UNITÀ")
                .color(muted_color(ui))
                .small()
                .strong(),
        );
        ui.add_space(4.0);
        for drive in &self.drives {
            if ui
                .add_sized(
                    [190.0, 27.0],
                    egui::Button::new(format!("  {}", drive.display()))
                        .shortcut_text("")
                        .frame(false)
                        .selected(&self.current().path == drive),
                )
                .clicked()
            {
                target = Some(drive.clone());
            }
        }
        if let Some(path) = target {
            self.navigate(path);
        }
    }

    fn files_ui(&mut self, ui: &mut egui::Ui, right: bool) {
        let mut open = None;
        let mut click = None;
        let mut drag_start: Option<PathBuf> = None;
        let mut menu_action: Option<(&'static str, PathBuf)> = None;
        let requested_sort;
        let mut thumbnails_needed = Vec::new();
        let mut icons_needed = Vec::new();
        let mut columns = self.columns.clone();
        let tab = self.panel(right);
        let filter = tab.filter.to_lowercase();
        let now = SystemTime::now();
        let criteria = tab.criteria;
        let indices: Vec<usize> = tab
            .entries
            .iter()
            .enumerate()
            .filter(|(_, item)| filters::matches(item, &filter, criteria, now))
            .map(|(i, _)| i)
            .collect();
        let count = indices.len();
        // Egui children may grow beyond their requested size. Bound the table
        // to the visible viewport, not to an already-expanded child max_rect.
        let table_width = ui
            .available_width()
            .min((ui.clip_rect().right() - ui.next_widget_position().x).max(0.0));
        let dual = table_width < 650.0 || (self.secondary.is_some() && !columns.metadata_in_dual);
        let [w_name, w_type, w_size, w_date] = columns.widths(table_width, dual);
        requested_sort = configured_header_ui(
            ui,
            table_width,
            dual,
            tab.sort_by,
            tab.descending,
            &mut columns,
        );
        ui.separator();
        if tab.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Caricamento cartella...");
            });
            return;
        }
        if let Some(e) = tab.error.clone() {
            ui.colored_label(
                if ui.visuals().dark_mode {
                    Color32::LIGHT_RED
                } else {
                    Color32::from_rgb(170, 25, 25)
                },
                e,
            );
            if ui.button("Riprova ad aprire la cartella").clicked() {
                self.focus(right);
                self.reload_current();
            }
            return;
        }
        if count == 0 {
            ui.add_space(18.0);
            ui.label("Nessun elemento trovato");
            return;
        }
        let mut scroll = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .id_salt(("rows", tab.id));
        if let Some(row) = tab.scroll_to {
            scroll = scroll.vertical_scroll_offset(row as f32 * 39.0);
        }
        scroll.show_rows(ui, 34.0, count, |ui, range| {
            for row in range {
                let item = &tab.entries[indices[row]];
                let selected = tab.selected.contains(&item.path);
                let name = item.name.clone();
                let key = native_icons::key(&item.name, item.is_dir);
                if !self.icon_cache.contains_key(&key)
                    && !self.icon_pending.contains(&key)
                    && !self.icon_failed.contains(&key)
                {
                    icons_needed.push(key.clone());
                }
                if self.thumbs_visible
                    && image_view::supported(&item.path)
                    && !self.thumb_cache.contains_key(&item.path)
                    && !self.thumb_pending.contains(&item.path)
                    && !self.thumb_failed.contains(&item.path)
                {
                    thumbnails_needed.push(item.path.clone());
                }
                let icon = if self.thumbs_visible {
                    self.thumb_cache
                        .get(&item.path)
                        .or_else(|| self.icon_cache.get(&key))
                } else {
                    self.icon_cache.get(&key)
                };
                let file_type = if item.is_dir {
                    "Cartella".to_owned()
                } else {
                    item.path
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(str::to_ascii_uppercase)
                        .unwrap_or_else(|| "File".into())
                };
                let size = if item.is_dir {
                    "—".into()
                } else {
                    pretty_bytes(item.size)
                };
                let modified = item
                    .modified
                    .map(|t| {
                        let d: DateTime<Local> = DateTime::from(t);
                        d.format("%d/%m/%Y %H:%M").to_string()
                    })
                    .unwrap_or_else(|| "—".into());
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(table_width.min(ui.available_width()), 34.0),
                    egui::Sense::click_and_drag(),
                );
                let fill = if selected {
                    ui.visuals().selection.bg_fill
                } else if response.hovered() {
                    ui.visuals().widgets.hovered.weak_bg_fill
                } else if row % 2 == 0 {
                    ui.visuals().faint_bg_color
                } else {
                    ui.visuals().panel_fill
                };
                ui.painter()
                    .rect_filled(rect, egui::CornerRadius::same(4), fill);
                let text = ui.visuals().text_color();
                let left = rect.left() + 8.0;
                let center = rect.center().y;
                let icon_rect = egui::Rect::from_center_size(
                    egui::pos2(left + 10.0, center),
                    egui::vec2(20.0, 20.0),
                );
                if let Some(tex) = icon {
                    let aspect = tex.size_vec2();
                    let scale = (20.0 / aspect.x).min(20.0 / aspect.y);
                    let fitted = egui::Rect::from_center_size(icon_rect.center(), aspect * scale);
                    ui.painter().image(
                        tex.id(),
                        fitted,
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                } else {
                    // Vector fallback has no dependency on Unicode font coverage.
                    ui.painter().rect_filled(
                        icon_rect.shrink(2.0),
                        egui::CornerRadius::same(2),
                        if item.is_dir {
                            Color32::from_rgb(226, 181, 77)
                        } else {
                            ACCENT
                        },
                    );
                }
                let mut name_job = egui::text::LayoutJob::simple(
                    name,
                    egui::FontId::proportional(14.0),
                    text,
                    (w_name - 40.0).max(30.0),
                );
                name_job.wrap.max_rows = 1;
                let galley = ui.fonts_mut(|f| f.layout_job(name_job));
                ui.painter().galley(
                    egui::pos2(left + 30.0, center - galley.size().y / 2.0),
                    galley,
                    text,
                );
                let mut x = rect.left() + w_name + 7.0;
                if w_type > 0.0 {
                    ui.painter().text(
                        egui::pos2(x, center),
                        egui::Align2::LEFT_CENTER,
                        file_type,
                        egui::FontId::proportional(13.0),
                        text,
                    );
                    x += w_type + 7.0;
                }
                if w_size > 0.0 {
                    ui.painter().text(
                        egui::pos2(x + w_size - 8.0, center),
                        egui::Align2::RIGHT_CENTER,
                        size,
                        egui::FontId::proportional(13.0),
                        text,
                    );
                }
                if w_date > 0.0 {
                    if w_size > 0.0 {
                        x += w_size + 7.0;
                    }
                    ui.painter().text(
                        egui::pos2(x, center),
                        egui::Align2::LEFT_CENTER,
                        modified,
                        egui::FontId::proportional(12.0),
                        text,
                    );
                }
                if tab.cursor.as_ref() == Some(&item.path) {
                    ui.painter().hline(
                        rect.x_range(),
                        rect.top(),
                        egui::Stroke::new(1.0, accent_color(ui)),
                    );
                    ui.painter().hline(
                        rect.x_range(),
                        rect.bottom(),
                        egui::Stroke::new(1.0, accent_color(ui)),
                    );
                }
                response.widget_info(|| {
                    egui::WidgetInfo::selected(egui::Role::Row, true, selected, &item.name)
                });
                let response = response.on_hover_text(display_path(&item.path));
                response.context_menu(|ui| {
                    ui.label(RichText::new(&item.name).strong());
                    ui.separator();
                    if item.is_dir {
                        if ui.button("Apri in nuova scheda").clicked() {
                            menu_action = Some(("tab", item.path.clone()));
                            ui.close();
                        }
                        if ui.button("Apri nel pannello opposto").clicked() {
                            menu_action = Some(("other", item.path.clone()));
                            ui.close();
                        }
                    }
                    for (label, action) in [
                        ("Apri", "open"),
                        ("Copia", "copy"),
                        ("Taglia", "cut"),
                        ("Rinomina", "rename"),
                        ("Dettagli", "details"),
                        ("Cestino", "trash"),
                    ] {
                        if ui.button(label).clicked() {
                            menu_action = Some((action, item.path.clone()));
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
                if response.double_clicked() {
                    open = Some(item.path.clone());
                }
            }
        });
        self.columns = columns;
        self.panel_mut(right).scroll_to = None;
        self.schedule_assets(thumbnails_needed, icons_needed);
        if let Some((path, modifiers)) = click {
            // Selection is resolved against visible, filtered order.
            let ordered: Vec<PathBuf> = indices
                .iter()
                .map(|&i| self.panel(right).entries[i].path.clone())
                .collect();
            let tab = self.panel_mut(right);
            tab.cursor = Some(path.clone());
            apply_selection(
                &mut tab.selected,
                &mut tab.anchor,
                &ordered,
                path,
                modifiers.shift,
                modifiers.command || modifiers.ctrl,
            );
            self.focus(right);
        }
        if let Some(path) = open {
            self.focus(right);
            self.open_entry(path);
        }
        if let Some(by) = requested_sort {
            self.change_sort(right, by);
        }
        if let Some(path) = drag_start {
            if self.secondary.is_some() && !self.busy && matches!(self.dialog, Dialog::None) {
                let tab = self.panel(right);
                let sources: Vec<PathBuf> = if tab.selected.contains(&path) {
                    tab.selected.iter().cloned().collect()
                } else {
                    vec![path]
                };
                self.drag_intent = Some(DragIntent {
                    sources,
                    source_right: right,
                });
                self.status = "Rilascia nel pannello opposto per proporre una copia".into();
            }
        }
        if let Some((action, path)) = menu_action {
            self.focus(right);
            let tab = self.current_mut();
            tab.selected.clear();
            tab.selected.insert(path.clone());
            tab.anchor = Some(path.clone());
            match action {
                "tab" => self.add_tab(path),
                "other" => {
                    if self.secondary.is_none() {
                        self.open_secondary(path);
                    } else {
                        self.focus(!right);
                        self.navigate(path);
                    }
                }
                "open" => self.open_entry(path),
                "copy" => self.clipboard_set(false),
                "cut" => self.clipboard_set(true),
                "rename" => self.prepare_rename(),
                "details" => {
                    self.preview_visible = true;
                    self.preview_request = None;
                }
                "trash" => self.prepare_trash(),
                _ => {}
            }
        }
    }

    /// UI-only drag between panels. No OS file drop is synthesized: the
    /// result always opens an explicit copy-only confirmation dialog.
    fn finish_internal_drag(&mut self, ctx: &egui::Context, areas: [Option<egui::Rect>; 2]) {
        if !ctx.input(|i| i.pointer.any_released()) {
            return;
        }
        let Some(drag) = self.drag_intent.take() else {
            return;
        };
        if self.busy || !matches!(self.dialog, Dialog::None) {
            return;
        }
        let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) else {
            return;
        };
        let target_right = !drag.source_right;
        if let Some(area) = areas[usize::from(target_right)] {
            if area.contains(pos) {
                let destination = self.panel(target_right).path.clone();
                self.dialog = Dialog::ExternalDrop {
                    sources: drag.sources,
                    destination,
                };
                return;
            }
        }
        self.status = "Trascinamento annullato (nessuna modifica)".into();
    }

    /// Reserve the pane in the parent before rendering its child. Widgets in
    /// one pane cannot grow the other pane or paint beyond the divider.
    fn allocate_pane(
        &mut self,
        ui: &mut egui::Ui,
        width: f32,
        height: f32,
        right: bool,
    ) -> egui::Rect {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("file_pane", right))
                .max_rect(rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        child.set_clip_rect(rect.intersect(ui.clip_rect()));
        self.pane_ui(&mut child, right);
        rect
    }

    fn pane_ui(&mut self, ui: &mut egui::Ui, right: bool) {
        let tab = self.panel(right);
        let path = display_path(&tab.path);
        let count = tab.selected.len();
        let focused = self.focused_right == right;
        let title = if right { "DESTRA" } else { "SINISTRA" };
        let background = if self.dark_theme {
            if focused {
                Color32::from_rgb(36, 52, 70)
            } else {
                Color32::from_rgb(26, 34, 45)
            }
        } else if focused {
            Color32::from_rgb(223, 237, 250)
        } else {
            Color32::from_rgb(237, 241, 247)
        };
        egui::Frame::new()
            .fill(background)
            .corner_radius(egui::CornerRadius::same(9))
            .inner_margin(egui::Margin::symmetric(10, 7))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new(format!("{title}")).strong().color(
                                if focused {
                                    accent_color(ui)
                                } else {
                                    muted_color(ui)
                                },
                            ))
                            .frame(false),
                        )
                        .on_hover_text("Attiva il pannello")
                        .clicked()
                    {
                        self.focus(right);
                    }
                    if focused {
                        ui.label(RichText::new("ATTIVO").small().color(accent_color(ui)));
                    }
                    ui.label(
                        RichText::new(format!("{count} selezionati"))
                            .small()
                            .color(muted_color(ui)),
                    );
                });
                ui.label(RichText::new(&path).small().color(muted_color(ui)))
                    .on_hover_text(path.clone());
            });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Filtra").color(muted_color(ui)));
            let filter_width = (ui.available_width() - 80.0).clamp(55.0, 260.0);
            let tab = self.panel_mut(right);
            let changed = ui
                .add(
                    egui::TextEdit::singleline(&mut tab.filter)
                        .id(egui::Id::unique(if right {
                            "folder_filter_right"
                        } else {
                            "folder_filter"
                        }))
                        .hint_text("Nome o estensione")
                        .desired_width(filter_width),
                )
                .changed();
            // Never leave hidden entries selected when a filter changes.
            if changed {
                tab.selected.clear();
                tab.anchor = None;
            }
            if ui.button("×").on_hover_text("Azzera filtri").clicked() {
                tab.filter.clear();
                tab.criteria = filters::Criteria::default();
                tab.selected.clear();
                tab.anchor = None;
            }
        });
        ui.horizontal_wrapped(|ui| {
            let tab = self.panel_mut(right);
            let previous = tab.criteria;
            egui::ComboBox::from_id_salt(("kind_filter", right))
                .selected_text(match tab.criteria.kind {
                    filters::FileKind::All => "Tutti i tipi",
                    filters::FileKind::Documents => "Documenti",
                    filters::FileKind::Images => "Immagini",
                    filters::FileKind::Archives => "Archivi",
                    filters::FileKind::Video => "Video",
                    filters::FileKind::Audio => "Audio",
                    filters::FileKind::Other => "Altri",
                })
                .show_ui(ui, |ui| {
                    for (kind, label) in [
                        (filters::FileKind::All, "Tutti i tipi"),
                        (filters::FileKind::Documents, "Documenti"),
                        (filters::FileKind::Images, "Immagini"),
                        (filters::FileKind::Archives, "Archivi"),
                        (filters::FileKind::Video, "Video"),
                        (filters::FileKind::Audio, "Audio"),
                        (filters::FileKind::Other, "Altri"),
                    ] {
                        ui.selectable_value(&mut tab.criteria.kind, kind, label);
                    }
                });
            egui::ComboBox::from_id_salt(("size_filter", right))
                .selected_text(match tab.criteria.size {
                    filters::SizeRange::All => "Qualsiasi dimensione",
                    filters::SizeRange::Small => "< 1 MiB",
                    filters::SizeRange::Medium => "1–100 MiB",
                    filters::SizeRange::Large => ">= 100 MiB",
                })
                .show_ui(ui, |ui| {
                    for (kind, label) in [
                        (filters::SizeRange::All, "Qualsiasi dimensione"),
                        (filters::SizeRange::Small, "< 1 MiB"),
                        (filters::SizeRange::Medium, "1–100 MiB"),
                        (filters::SizeRange::Large, ">= 100 MiB"),
                    ] {
                        ui.selectable_value(&mut tab.criteria.size, kind, label);
                    }
                });
            egui::ComboBox::from_id_salt(("date_filter", right))
                .selected_text(match tab.criteria.modified {
                    filters::ModifiedRange::All => "Qualsiasi data",
                    filters::ModifiedRange::Day => "Ultime 24 ore",
                    filters::ModifiedRange::Week => "Ultimi 7 giorni",
                    filters::ModifiedRange::Month => "Ultimi 30 giorni",
                })
                .show_ui(ui, |ui| {
                    for (kind, label) in [
                        (filters::ModifiedRange::All, "Qualsiasi data"),
                        (filters::ModifiedRange::Day, "Ultime 24 ore"),
                        (filters::ModifiedRange::Week, "Ultimi 7 giorni"),
                        (filters::ModifiedRange::Month, "Ultimi 30 giorni"),
                    ] {
                        ui.selectable_value(&mut tab.criteria.modified, kind, label);
                    }
                });
            if previous != tab.criteria {
                tab.selected.clear();
                tab.anchor = None;
            }
        });
        let tab = self.panel(right);
        if !tab.filter.is_empty() || !tab.criteria.is_default() {
            ui.label(
                RichText::new("Filtri attivi · × per azzerare")
                    .small()
                    .color(accent_color(ui)),
            );
        }
        ui.add_space(6.0);
        self.files_ui(ui, right);
    }

    /// OS drag-and-drop always asks before copying: it never moves or deletes the source.
    fn accept_os_drop(&mut self, ctx: &egui::Context) {
        let sources: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .filter(|path| !path.as_os_str().is_empty())
                .collect()
        });
        if sources.is_empty() {
            return;
        }
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
        self.dialog = match std::mem::replace(&mut self.dialog, Dialog::None) {
            Dialog::ExternalDrop {
                sources,
                destination,
            } => Dialog::Transfer {
                sources,
                destination,
                cut: false,
                policy: crate::transfer::ConflictPolicy::Stop,
            },
            Dialog::ConfirmMove {
                sources,
                destination_folder,
            } => Dialog::Transfer {
                sources,
                destination: destination_folder,
                cut: true,
                policy: crate::transfer::ConflictPolicy::Stop,
            },
            other => other,
        };
        match &mut self.dialog {
            Dialog::None => return,
            Dialog::Transfer {
                sources,
                destination,
                cut,
                policy,
            } => {
                egui::Window::new(if *cut {
                    "Conferma spostamento"
                } else {
                    "Conferma copia"
                })
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "{} elementi verso {}",
                        sources.len(),
                        display_path(destination)
                    ));
                    egui::ScrollArea::vertical()
                        .max_height(130.0)
                        .show(ui, |ui| {
                            for p in sources.iter() {
                                ui.label(display_path(p));
                            }
                        });
                    ui.label("Quando il nome esiste già:");
                    for (value, label) in [
                        (
                            crate::transfer::ConflictPolicy::Stop,
                            "Interrompi prima di iniziare",
                        ),
                        (crate::transfer::ConflictPolicy::Skip, "Salta i conflitti"),
                        (
                            crate::transfer::ConflictPolicy::KeepBoth,
                            "Conserva entrambi",
                        ),
                    ] {
                        ui.radio_value(policy, value, label);
                    }
                    ui.label("Il riepilogo completo resta in Visualizza → Operazioni recenti.");
                    ui.horizontal(|ui| {
                        if ui.button("Esegui").clicked() {
                            job = Some(Job::Transfer {
                                sources: sources.clone(),
                                destination: destination.clone(),
                                cut: *cut,
                                policy: *policy,
                            });
                            close = true;
                        }
                        if ui.button("Annulla").clicked() {
                            close = true;
                        }
                    });
                });
            }
            Dialog::NewFolder(name) => {
                egui::Window::new("Nuova cartella")
                    .collapsible(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.label("Nome della nuova cartella:");
                        ui.text_edit_singleline(name);
                        ui.horizontal(|ui| {
                            if ui.button("Crea").clicked() {
                                job = Some(Job::NewFolder {
                                    parent: parent_path.clone(),
                                    name: name.clone(),
                                });
                                close = true;
                            }
                            if ui.button("Annulla").clicked() {
                                close = true;
                            }
                        });
                    });
            }
            Dialog::Rename { source, name } => {
                egui::Window::new("Rinomina")
                    .collapsible(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.label("Nuovo nome:");
                        ui.text_edit_singleline(name);
                        ui.horizontal(|ui| {
                            if ui.button("Salva").clicked() {
                                job = Some(Job::Rename {
                                    source: source.clone(),
                                    name: name.clone(),
                                });
                                close = true;
                            }
                            if ui.button("Annulla").clicked() {
                                close = true;
                            }
                        });
                    });
            }
            Dialog::BatchRename {
                sources,
                prefix,
                find,
                replace,
                start,
            } => {
                egui::Window::new("Rinomina multipla")
                    .collapsible(false)
                    .resizable(true)
                    .default_width(480.0)
                    .show(ctx, |ui| {
                        ui.label(format!(
                            "{} file selezionati. Numero + prefisso + nome originale.",
                            sources.len()
                        ));
                        ui.horizontal(|ui| {
                            ui.label("Prefisso");
                            ui.text_edit_singleline(prefix);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Cerca testo");
                            ui.text_edit_singleline(find);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Sostituisci con");
                            ui.text_edit_singleline(replace);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Numerazione da");
                            ui.text_edit_singleline(start);
                        });
                        let plan = start
                            .parse::<u32>()
                            .map_err(|_| "Numero iniziale non valido".to_owned())
                            .and_then(|n| fs_ops::rename_plan(sources, prefix, find, replace, n))
                            .and_then(|plan| {
                                fs_ops::preflight_rename_many(&plan)?;
                                Ok(plan)
                            });
                        ui.separator();
                        match &plan {
                            Ok(plan) => {
                                ui.label("Anteprima completa (conflitti verificati):");
                                egui::ScrollArea::vertical()
                                    .max_height(180.0)
                                    .show(ui, |ui| {
                                        for (source, name) in plan.iter() {
                                            ui.label(format!(
                                                "{}  →  {name}",
                                                source
                                                    .file_name()
                                                    .unwrap_or_default()
                                                    .to_string_lossy()
                                            ));
                                        }
                                    });
                            }
                            Err(err) => {
                                ui.colored_label(
                                    if ui.visuals().dark_mode {
                                        Color32::LIGHT_RED
                                    } else {
                                        Color32::from_rgb(170, 25, 25)
                                    },
                                    err,
                                );
                            }
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Rinomina i file").clicked() {
                                match plan {
                                    Ok(changes) => {
                                        job = Some(Job::RenameMany { changes });
                                        close = true;
                                    }
                                    Err(err) => {
                                        dialog_error = Some(err);
                                    }
                                }
                            }
                            if ui.button("Annulla").clicked() {
                                close = true;
                            }
                        });
                    });
            }
            Dialog::Delete(paths) => {
                egui::Window::new("Sposta nel Cestino?")
                    .collapsible(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.label(format!(
                            "Spostare {} elementi selezionati nel Cestino?",
                            paths.len()
                        ));
                        ui.label(
                            RichText::new("L'eliminazione permanente non è prevista.")
                                .color(muted_color(ui)),
                        );
                        ui.horizontal(|ui| {
                            if ui.button("Sposta nel Cestino").clicked() {
                                job = Some(Job::TrashMany {
                                    sources: paths.clone(),
                                });
                                close = true;
                            }
                            if ui.button("Annulla").clicked() {
                                close = true;
                            }
                        });
                    });
            }
            Dialog::ConfirmMove {
                sources,
                destination_folder,
            } => {
                egui::Window::new("Conferma spostamento")
                    .collapsible(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.label(format!(
                            "Spostare {} elementi in {}?",
                            sources.len(),
                            destination_folder.display()
                        ));
                        ui.label(
                            RichText::new("Solo stesso volume. Nessuna sovrascrittura automatica.")
                                .small()
                                .color(muted_color(ui)),
                        );
                        ui.horizontal(|ui| {
                            if ui.button("Conferma spostamento").clicked() {
                                job = Some(Job::MoveMany {
                                    sources: sources.clone(),
                                    destination_folder: destination_folder.clone(),
                                });
                                close = true;
                            }
                            if ui.button("Annulla").clicked() {
                                close = true;
                            }
                        });
                    });
            }
            Dialog::ExternalDrop {
                sources,
                destination,
            } => {
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
                            .small().color(muted_color(ui)));
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
        if close {
            self.dialog = Dialog::None;
        }
        if let Some(err) = dialog_error {
            self.status = format!("Rinomina: {err}");
        }
        if let Some(job) = job {
            self.begin_job(job);
        }
    }
}

impl Veldir {
    fn render_ui(&mut self, ui: &mut egui::Ui) -> [Option<egui::Rect>; 2] {
        ui.painter().rect_filled(
            ui.max_rect(),
            egui::CornerRadius::ZERO,
            ui.visuals().panel_fill,
        );
        self.pump();
        self.sync_watch_paths();
        self.apply_watch_changes(ui.ctx());
        self.update_preview();
        self.accept_os_drop(ui.ctx());
        self.keyboard(ui.ctx());
        ui.add_space(5.0);
        let dark = self.dark_theme;
        egui::Frame::new()
            .fill(if dark {
                Color32::from_rgb(22, 30, 41)
            } else {
                Color32::from_rgb(225, 234, 245)
            })
            .corner_radius(egui::CornerRadius::same(11))
            .inner_margin(egui::Margin::symmetric(12, 9))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("VELDIR")
                            .strong()
                            .size(21.0)
                            .color(accent_color(ui)),
                    );
                    ui.label(
                        RichText::new(concat!(
                            env!("CARGO_PKG_VERSION"),
                            "  ·  WINDOWS FILE MANAGER"
                        ))
                        .small()
                        .color(muted_color(ui)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.busy {
                            ui.spinner();
                        }
                        let path = self.current().path.clone();
                        let label = path.file_name().and_then(|n| n.to_str()).unwrap_or("Unità");
                        ui.add(
                            egui::Label::new(RichText::new(label).strong().color(muted_color(ui)))
                                .truncate(),
                        )
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
        let mut pane_areas: [Option<egui::Rect>; 2] = [None, None];
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), content_height),
            egui::Layout::left_to_right(egui::Align::Min),
            |ui| {
                ui.add_space(4.0);
                if self.sidebar_visible {
                    ui.vertical(|ui| {
                        self.sidebar_ui(ui);
                    });
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
                        egui::vec2(10.0, content_height),
                        egui::Sense::click_and_drag(),
                    );
                    let splitter = splitter
                        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
                        .on_hover_text("Trascina per ridimensionare i pannelli");
                    let color = if splitter.dragged() || splitter.hovered() {
                        ACCENT
                    } else {
                        Color32::from_rgb(71, 83, 101)
                    };
                    let center = rect.center();
                    ui.painter().rect_filled(
                        egui::Rect::from_center_size(center, egui::vec2(3.0, 46.0)),
                        egui::CornerRadius::same(2),
                        color,
                    );
                    if splitter.dragged() {
                        self.split_ratio = bounded_split_ratio(
                            self.split_ratio + splitter.drag_delta().x / usable,
                        );
                        ui.ctx().request_repaint();
                    }

                    pane_areas[1] = Some(self.allocate_pane(ui, right_width, content_height, true));
                } else {
                    let width = ui.available_width();
                    pane_areas[0] = Some(self.allocate_pane(ui, width, content_height, false));
                }
            },
        );
        self.finish_internal_drag(ui.ctx(), pane_areas);
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
            ui.label(RichText::new(selected_text).small().color(muted_color(ui)));
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
                    let current=self.job_sources.get(p.done_items.min(self.job_sources.len().saturating_sub(1)));
                    let caption=if let Some(path)=current{format!("{} · {caption}",path.file_name().unwrap_or_default().to_string_lossy().chars().take(22).collect::<String>())}else{caption};
                    let progress=ui.add(egui::ProgressBar::new(ratio.clamp(0.0,1.0)).desired_width(200.0).text(caption));
                    if let Some(path)=current{progress.on_hover_text(display_path(path));}
                    if let Some(started)=self.job_started_at {
                        let elapsed=started.elapsed().as_secs_f64();
                        if p.done_bytes > 0 && elapsed >= 0.2 {
                            let rate=(p.done_bytes as f64/elapsed).round() as u64;
                            ui.label(RichText::new(format!("{}/s",pretty_bytes(rate)))
                                .small().color(muted_color(ui)));
                            if rate>0 && p.total_bytes>p.done_bytes && elapsed>=2.0 {ui.label(format!("Stima: ~{} s",(p.total_bytes-p.done_bytes)/rate));}
                        }
                    }
                }
                if ui.button("Annulla").on_hover_text("Durante la copia interrompe anche il file in corso, senza pubblicare il file parziale").clicked() {
                    self.job_cancel.store(true,Ordering::Relaxed);
                    self.status="Annullamento richiesto…".into();
                }
            }
            ui.label(RichText::new(&self.status).small().color(muted_color(ui)));
        });
        self.dialog_ui(ui.ctx());
        self.search_ui(ui.ctx());
        self.preview_ui(ui.ctx());
        self.information_ui(ui.ctx());
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
            live_refresh: self.live_refresh,
            columns: self.columns.clone(),
            restore_session: self.restore_session,
        };
        if eframe::get_value::<Saved>(storage, "veldir_state").is_some() {
            if let Some(previous) = storage.get_string("veldir_state") {
                storage.set_string("veldir_state_backup", previous);
            }
        }
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
        if let Some(start) = anchor
            .as_ref()
            .and_then(|a| ordered.iter().position(|p| p == a))
        {
            if let Some(end) = ordered.iter().position(|p| p == &path) {
                if !additive {
                    selected.clear();
                }
                for item in &ordered[start.min(end)..=start.max(end)] {
                    selected.insert(item.clone());
                }
                return;
            }
        }
    }
    if additive && !shift {
        if !selected.insert(path.clone()) {
            selected.remove(&path);
        }
    } else {
        selected.clear();
        selected.insert(path.clone());
    }
    *anchor = Some(path);
}

fn pretty_bytes(n: u64) -> String {
    if n < 1024 {
        return format!("{n} B");
    }
    let mut value = n as f64;
    let mut unit = "B";
    for next in ["KB", "MB", "GB", "TB", "PB"] {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = next;
    }
    format!("{value:.1} {unit}")
}

#[cfg(test)]
mod tests {
    use super::{
        Saved, SortBy, Veldir, apply_selection, apply_theme, bounded_split_ratio, column_widths,
        egui, file_header_ui, pretty_bytes, sort_entries,
    };
    use crate::fs_ops::Entry;
    use std::{collections::BTreeSet, path::PathBuf};

    #[derive(Default)]
    struct MemoryStore(std::collections::HashMap<String, String>);
    impl eframe::Storage for MemoryStore {
        fn get_string(&self, k: &str) -> Option<String> {
            self.0.get(k).cloned()
        }
        fn set_string(&mut self, k: &str, v: String) {
            self.0.insert(k.into(), v);
        }
        fn remove_string(&mut self, k: &str) {
            self.0.remove(k);
        }
        fn flush(&mut self) {}
    }

    #[test]
    fn secondary_text_and_accent_meet_contrast_in_both_themes() {
        fn luminance(c: egui::Color32) -> f32 {
            let channel = |v: u8| {
                let s = v as f32 / 255.0;
                if s <= 0.04045 {
                    s / 12.92
                } else {
                    ((s + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
        }
        fn contrast(a: egui::Color32, b: egui::Color32) -> f32 {
            let a = luminance(a);
            let b = luminance(b);
            (a.max(b) + 0.05) / (a.min(b) + 0.05)
        }
        for dark in [true, false] {
            let ctx = egui::Context::default();
            apply_theme(&ctx, dark);
            let output = ctx.run_ui(egui::RawInput::default(), |ui| {
                assert!(contrast(super::muted_color(ui), ui.visuals().panel_fill) >= 4.5);
                assert!(contrast(super::accent_color(ui), ui.visuals().panel_fill) >= 4.5);
            });
            output.drop_without_applying_deltas();
        }
    }

    #[test]
    fn corrupted_settings_recover_last_valid_backup() {
        let mut store = MemoryStore::default();
        eframe::set_value(
            &mut store,
            "veldir_state_backup",
            &Saved {
                dark_theme: false,
                ..Default::default()
            },
        );
        eframe::Storage::set_string(&mut store, "veldir_state", "not valid RON".into());
        let (saved, note) = super::load_saved(Some(&store));
        assert!(!saved.dark_theme);
        assert!(note.contains("copia precedente"));
    }
    #[test]
    fn corrupt_settings_without_backup_use_defaults_with_notice() {
        let mut store = MemoryStore::default();
        eframe::Storage::set_string(&mut store, "veldir_state", "not valid RON".into());
        let (saved, note) = super::load_saved(Some(&store));
        assert!(saved.dark_theme);
        assert!(saved.restore_session);
        assert!(!note.is_empty());
    }

    #[test]
    fn saved_columns_and_session_options_roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = Veldir::from_saved(
            ctx,
            Saved {
                tabs: vec![d.path().to_path_buf()],
                live_refresh: false,
                ..Default::default()
            },
        );
        app.columns.kind = false;
        app.columns.modified = false;
        app.columns.size_width = 155.0;
        app.restore_session = false;
        let mut store = MemoryStore::default();
        eframe::App::save(&mut app, &mut store);
        let saved: Saved = eframe::get_value(&store, "veldir_state").unwrap();
        assert!(!saved.columns.kind);
        assert!(!saved.columns.modified);
        assert_eq!(saved.columns.size_width, 155.0);
        assert!(!saved.restore_session);
    }
    #[test]
    fn unavailable_saved_path_is_not_discarded() {
        let d = tempfile::tempdir().unwrap();
        let missing = d.path().join("temporaneamente-offline");
        let app = Veldir::from_saved(
            egui::Context::default(),
            Saved {
                tabs: vec![missing.clone()],
                live_refresh: false,
                ..Default::default()
            },
        );
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.current().path, missing);
    }
    #[test]
    fn arrow_shift_extends_selection_and_requests_scroll() {
        let d = tempfile::tempdir().unwrap();
        for name in ["aa.txt", "ab.txt", "b.txt"] {
            std::fs::write(d.path().join(name), "x").unwrap();
        }
        let ctx = egui::Context::default();
        let mut app = Veldir::from_saved(
            ctx.clone(),
            Saved {
                tabs: vec![d.path().to_path_buf()],
                live_refresh: false,
                ..Default::default()
            },
        );
        let tab = app.current_mut();
        tab.entries = crate::fs_ops::list(d.path()).unwrap();
        tab.loading = false;
        tab.cursor = Some(tab.entries[0].path.clone());
        tab.anchor = tab.cursor.clone();
        tab.selected.insert(tab.entries[0].path.clone());
        let m = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        let output = ctx.run_ui(
            egui::RawInput {
                events: vec![
                    egui::Event::ModifiersChanged(m),
                    egui::Event::Key {
                        key: egui::Key::ArrowDown,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: m,
                    },
                ],
                ..Default::default()
            },
            |ui| app.keyboard(ui.ctx()),
        );
        output.drop_without_applying_deltas();
        assert_eq!(app.current().selected.len(), 2);
        assert_eq!(app.current().scroll_to, Some(1));
    }
    #[test]
    fn reveal_in_current_folder_clears_filter_and_selects_result() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("nota.txt");
        std::fs::write(&p, "x").unwrap();
        let mut app = Veldir::from_saved(
            egui::Context::default(),
            Saved {
                tabs: vec![d.path().to_path_buf()],
                live_refresh: false,
                ..Default::default()
            },
        );
        let tab = app.current_mut();
        tab.entries = crate::fs_ops::list(d.path()).unwrap();
        tab.filter = "non-corrisponde".into();
        tab.loading = false;
        app.reveal_result(p.clone());
        assert!(app.current().filter.is_empty());
        assert!(app.current().selected.contains(&p));
        assert_eq!(app.current().scroll_to, Some(0));
    }

    #[test]
    fn full_ui_keeps_both_panes_inside_viewport_at_split_limits() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir
            .path()
            .join("cartella-con-un-nome-lungo-per-verificare-il-layout-dei-pannelli");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("nota.txt"), "prova").unwrap();
        for dark in [true, false] {
            let ctx = egui::Context::default();
            let mut app = Veldir::from_saved(
                ctx.clone(),
                Saved {
                    tabs: vec![folder.clone()],
                    dual_pane: true,
                    secondary_path: Some(folder.clone()),
                    dark_theme: dark,
                    thumbs_visible: false,
                    live_refresh: false,
                    ..Default::default()
                },
            );
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
        let mut app = Veldir::from_saved(
            ctx.clone(),
            Saved {
                tabs: vec![dir.path().to_path_buf()],
                dual_pane: true,
                secondary_path: Some(dir.path().to_path_buf()),
                live_refresh: false,
                thumbs_visible: false,
                ..Default::default()
            },
        );
        let output = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::F6,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ui| app.keyboard(ui.ctx()),
        );
        output.drop_without_applying_deltas();
        assert!(app.focused_right);
        // Wait-free setup after the asynchronous folder result is drained.
        while app.rx.try_recv().is_ok() {}
        let tab = app.current_mut();
        tab.loading = false;
        tab.entries = crate::fs_ops::list(dir.path()).unwrap();
        tab.filter = "txt".into();
        let modifiers = egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        };
        let output = ctx.run_ui(
            egui::RawInput {
                events: vec![
                    egui::Event::ModifiersChanged(modifiers),
                    egui::Event::Key {
                        key: egui::Key::A,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers,
                    },
                ],
                ..Default::default()
            },
            |ui| app.keyboard(ui.ctx()),
        );
        output.drop_without_applying_deltas();
        assert_eq!(app.current().selected.len(), 1);
        assert!(
            app.current()
                .selected
                .contains(&dir.path().join("nota.txt"))
        );
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
                    egui::Pos2::ZERO,
                    egui::vec2(width, 500.0),
                )),
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| {
                let viewport = ui.clip_rect();
                // Reproduce an expanded child left by a wide widget.
                ui.set_width(width + 300.0);
                let visible = ui
                    .available_width()
                    .min((ui.clip_rect().right() - ui.next_widget_position().x).max(0.0));
                let before = ui.next_widget_position();
                file_header_ui(ui, visible, visible < 650.0, SortBy::Name, false);
                let header = egui::Rect::from_min_size(before, egui::vec2(visible, 26.0));
                assert!(header.right() <= viewport.right() + 0.1);
                assert!(header.bottom() <= viewport.bottom());
                let columns = column_widths(visible, visible < 650.0);
                let count = columns.iter().filter(|w| **w > 0.0).count();
                let last_edge = header.left()
                    + columns.iter().sum::<f32>()
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
            if !compact {
                assert!(cols[3] <= 145.0);
            }
        }
    }

    #[test]
    fn sorting_keeps_folders_first_in_both_directions() {
        let make = |name: &str, size, is_dir| Entry {
            name: name.to_owned(),
            path: PathBuf::from(name),
            is_dir,
            size,
            modified: None,
        };
        let mut files = vec![
            make("small.txt", 1, false),
            make("folder", 0, true),
            make("big.txt", 100, false),
        ];
        sort_entries(&mut files, SortBy::Size, false);
        assert_eq!(
            files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            vec!["folder", "small.txt", "big.txt"]
        );
        sort_entries(&mut files, SortBy::Size, true);
        assert_eq!(
            files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            vec!["folder", "big.txt", "small.txt"]
        );
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
        apply_selection(
            &mut selected,
            &mut anchor,
            &entries,
            entries[1].clone(),
            false,
            false,
        );
        apply_selection(
            &mut selected,
            &mut anchor,
            &entries,
            entries[3].clone(),
            true,
            false,
        );
        assert_eq!(selected.len(), 3);
        assert!(selected.contains(&entries[1]));
        assert!(selected.contains(&entries[2]));
        assert!(selected.contains(&entries[3]));
        apply_selection(
            &mut selected,
            &mut anchor,
            &entries,
            entries[2].clone(),
            false,
            true,
        );
        assert!(!selected.contains(&entries[2]));
        assert_eq!(selected.len(), 2);
    }

    #[test]
    fn selection_anchor_disappears_under_filter() {
        let entries = [PathBuf::from("a"), PathBuf::from("c")];
        let mut selected = BTreeSet::new();
        let mut anchor = Some(PathBuf::from("b"));
        apply_selection(
            &mut selected,
            &mut anchor,
            &entries,
            entries[1].clone(),
            true,
            false,
        );
        assert_eq!(selected.len(), 1);
        assert!(selected.contains(&entries[1]));
    }
}
