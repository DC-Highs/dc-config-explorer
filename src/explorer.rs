//! A navigator for one JSON document: breadcrumb, a virtualized list (or table) of the
//! children of the current node, and an inspector for the selected child.
//!
//! The configuration sections are large (thousands of items), so only visible rows are
//! laid out and the filtered child list is rebuilt only when the path or filter changes.

use std::ops::Range;

use dc_localization::Localization;
use egui::{Align2, Color32, FontId, Key, RichText, Sense, Ui, vec2};
use egui_extras::{Column, TableBuilder};
use serde_json::Value;

const ROW_HEIGHT: f32 = 24.0;
const INSPECTOR_LIMIT: usize = 200_000;
const TABLE_SAMPLE: usize = 300;
const TABLE_MAX_COLUMNS: usize = 40;
const LABEL_FIELDS: &[&str] = &["name", "title", "tag", "id", "tid", "type", "group_type", "element"];

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Seg {
    Key(String),
    Index(usize),
}

impl Seg {
    fn label(&self) -> String {
        match self {
            Seg::Key(k) => k.clone(),
            Seg::Index(i) => format!("[{i}]"),
        }
    }
}

/// `items[12].name`-style path.
pub fn path_string(root: &str, path: &[Seg]) -> String {
    let mut out = root.to_owned();
    for seg in path {
        match seg {
            Seg::Index(i) => out.push_str(&format!("[{i}]")),
            Seg::Key(k) if !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => {
                out.push('.');
                out.push_str(k);
            }
            Seg::Key(k) => out.push_str(&format!("[{k:?}]")),
        }
    }
    out
}

fn child<'a>(value: &'a Value, seg: &Seg) -> Option<&'a Value> {
    match (value, seg) {
        (Value::Object(map), Seg::Key(k)) => map.get(k),
        (Value::Array(arr), Seg::Index(i)) => arr.get(*i),
        _ => None,
    }
}

pub fn resolve<'a>(root: &'a Value, path: &[Seg]) -> Option<&'a Value> {
    path.iter().try_fold(root, child)
}

fn is_container(value: &Value) -> bool {
    matches!(value, Value::Object(_) | Value::Array(_))
}

/// One-line summary of a value: `{12}`, `[340]`, `"text"`, `42`...
pub fn summary(value: &Value) -> (String, Color32) {
    match value {
        Value::Null => ("null".into(), Color32::from_rgb(150, 150, 160)),
        Value::Bool(b) => (b.to_string(), Color32::from_rgb(220, 130, 200)),
        Value::Number(n) => (n.to_string(), Color32::from_rgb(120, 190, 250)),
        Value::String(s) => {
            let mut text: String = s.chars().take(120).collect();
            if s.chars().count() > 120 {
                text.push('…');
            }
            (format!("\"{text}\""), Color32::from_rgb(140, 200, 120))
        }
        Value::Array(a) => (format!("[ {} ]", a.len()), Color32::from_rgb(240, 180, 90)),
        Value::Object(o) => (format!("{{ {} }}", o.len()), Color32::from_rgb(240, 180, 90)),
    }
}

/// `name: "Fire Dragon" · id: 1000` for objects that carry recognizable fields.
fn object_hint(value: &Value) -> Option<String> {
    let map = value.as_object()?;
    let parts: Vec<String> = LABEL_FIELDS
        .iter()
        .filter_map(|field| {
            let v = map.get(*field)?;
            match v {
                Value::String(s) if !s.is_empty() => Some(format!("{field}: {s}")),
                Value::Number(n) => Some(format!("{field}: {n}")),
                _ => None,
            }
        })
        .take(3)
        .collect();
    (!parts.is_empty()).then(|| parts.join("  ·  "))
}

fn cell_text(value: Option<&Value>) -> String {
    match value {
        None => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) => "null".into(),
        Some(v @ (Value::Array(_) | Value::Object(_))) => summary(v).0,
        Some(v) => v.to_string(),
    }
}

struct ListCache {
    path: Vec<Seg>,
    filter: String,
    children: Vec<Seg>,
    columns: Vec<String>,
}

struct InspectorCache {
    path: Vec<Seg>,
    text: String,
    truncated: bool,
}

/// What the explorer asks its owner to do.
pub enum Action {
    Export { value: Value, suggested_name: String },
}

#[derive(Default)]
pub struct Explorer {
    pub path: Vec<Seg>,
    pub selected: Option<Seg>,
    pub filter: String,
    pub table: bool,
    list: Option<ListCache>,
    inspector: Option<InspectorCache>,
    visible: Range<usize>,
    scroll_to: Option<usize>,
}

impl Explorer {
    fn navigate(&mut self, path: Vec<Seg>, selected: Option<Seg>) {
        self.path = path;
        self.selected = selected;
        self.filter.clear();
        self.scroll_to = None;
    }

    fn enter(&mut self, seg: Seg) {
        let mut path = self.path.clone();
        path.push(seg);
        self.navigate(path, None);
    }

    fn up(&mut self) {
        let mut path = self.path.clone();
        if let Some(last) = path.pop() {
            self.navigate(path, Some(last));
            self.list = None;
        }
    }

    fn refresh_list(&mut self, node: &Value) {
        let fresh = self
            .list
            .as_ref()
            .is_some_and(|c| c.path == self.path && c.filter == self.filter);
        if fresh {
            return;
        }

        let needle = self.filter.trim().to_lowercase();
        let matches = |key: &str, value: &Value| {
            needle.is_empty()
                || key.to_lowercase().contains(&needle)
                || serde_json::to_string(value).is_ok_and(|s| s.to_lowercase().contains(&needle))
        };

        let children: Vec<Seg> = match node {
            Value::Object(map) => map
                .iter()
                .filter(|(k, v)| matches(k, v))
                .map(|(k, _)| Seg::Key(k.clone()))
                .collect(),
            Value::Array(arr) => arr
                .iter()
                .enumerate()
                .filter(|(i, v)| matches(&i.to_string(), v))
                .map(|(i, _)| Seg::Index(i))
                .collect(),
            _ => Vec::new(),
        };

        // Table columns: scalar keys of the first objects, in order of appearance.
        let mut columns: Vec<String> = Vec::new();
        for seg in children.iter().take(TABLE_SAMPLE) {
            if let Some(Value::Object(obj)) = child(node, seg) {
                for (k, v) in obj {
                    if columns.len() >= TABLE_MAX_COLUMNS {
                        break;
                    }
                    if !is_container(v) && !columns.contains(k) {
                        columns.push(k.clone());
                    }
                }
            }
        }

        self.list = Some(ListCache {
            path: self.path.clone(),
            filter: self.filter.clone(),
            children,
            columns,
        });
    }

    fn inspector_text(&mut self, node: &Value) -> (&str, bool) {
        let mut target = self.path.clone();
        if let Some(sel) = &self.selected {
            target.push(sel.clone());
        }
        let fresh = self.inspector.as_ref().is_some_and(|c| c.path == target);
        if !fresh {
            let value = self.selected.as_ref().and_then(|s| child(node, s)).unwrap_or(node);
            let mut text = serde_json::to_string_pretty(value).unwrap_or_default();
            let truncated = text.len() > INSPECTOR_LIMIT;
            if truncated {
                let mut cut = INSPECTOR_LIMIT;
                while !text.is_char_boundary(cut) {
                    cut -= 1;
                }
                text.truncate(cut);
            }
            self.inspector = Some(InspectorCache { path: target, text, truncated });
        }
        let cache = self.inspector.as_ref().unwrap();
        (&cache.text, cache.truncated)
    }

    /// Draws the explorer for `root` (named `root_name` in the breadcrumb).
    pub fn ui(&mut self, ui: &mut Ui, root_name: &str, root: &Value, loc: Option<&Localization>) -> Option<Action> {
        // A path can go stale when the document changes (e.g. re-mapped in another language).
        if resolve(root, &self.path).is_none() {
            self.navigate(Vec::new(), None);
        }
        let node = resolve(root, &self.path).unwrap_or(root);
        self.refresh_list(node);

        let mut action = None;
        let mut go_to: Option<Vec<Seg>> = None;

        // Breadcrumb + toolbar.
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(!self.path.is_empty(), |ui| {
                if ui.button("⬆").on_hover_text("Subir um nível (Backspace)").clicked() {
                    go_to = Some(self.path[..self.path.len().saturating_sub(1)].to_vec());
                }
            });
            if ui.link(RichText::new(root_name).strong()).clicked() {
                go_to = Some(Vec::new());
            }
            for (i, seg) in self.path.iter().enumerate() {
                ui.label(RichText::new("›").weak());
                let last = i + 1 == self.path.len();
                let text = RichText::new(seg.label());
                if last {
                    ui.label(text.strong());
                } else if ui.link(text).clicked() {
                    go_to = Some(self.path[..=i].to_vec());
                }
            }
        });

        let list_len = self.list.as_ref().map_or(0, |l| l.children.len());
        let total = match node {
            Value::Object(m) => m.len(),
            Value::Array(a) => a.len(),
            _ => 0,
        };
        let can_table = self.list.as_ref().is_some_and(|l| !l.columns.is_empty());

        ui.horizontal(|ui| {
            let filter = ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("🔍 Filtrar filhos (chave ou conteúdo)…")
                    .desired_width(320.0),
            );
            if !self.filter.is_empty() && ui.small_button("✖").clicked() {
                self.filter.clear();
                filter.request_focus();
            }
            ui.label(RichText::new(if list_len == total {
                format!("{total} itens")
            } else {
                format!("{list_len} de {total} itens")
            }).weak());

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("💾 Exportar nó").on_hover_text("Salvar o nó atual como JSON").clicked() {
                    action = Some(Action::Export {
                        value: node.clone(),
                        suggested_name: format!("{}.json", path_string(root_name, &self.path).replace(['[', ']', '"', '.'], "_")),
                    });
                }
                if ui.button("📋 Copiar nó").clicked() {
                    ui.ctx().copy_text(serde_json::to_string_pretty(node).unwrap_or_default());
                }
                ui.add_enabled_ui(can_table, |ui| {
                    ui.selectable_value(&mut self.table, true, "▦ Tabela");
                });
                ui.selectable_value(&mut self.table, false, "☰ Lista");
            });
        });
        ui.separator();

        if let Some(path) = go_to.take() {
            let selected = (path.len() < self.path.len()).then(|| self.path[path.len()].clone());
            self.navigate(path, selected);
            return None;
        }

        // Inspector on the right.
        egui::Panel::right("inspector")
            .resizable(true)
            .default_size(420.0)
            .min_size(240.0)
            .show(ui, |ui| {
                if let Some(a) = self.inspector_ui(ui, root_name, node, loc) {
                    action = Some(a);
                }
            });

        egui::CentralPanel::default().show(ui, |ui| {
            if !is_container(node) {
                ui.label("Valor escalar — veja o inspetor.");
                return;
            }
            if list_len == 0 {
                ui.label(RichText::new("Nada encontrado.").weak());
                return;
            }
            if self.table && can_table {
                self.table_ui(ui, node);
            } else {
                self.list_ui(ui, node);
            }
        });

        self.keyboard(ui, node);
        action
    }

    fn keyboard(&mut self, ui: &mut Ui, node: &Value) {
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let Some(list) = &self.list else { return };
        let len = list.children.len();
        let current = self
            .selected
            .as_ref()
            .and_then(|s| list.children.iter().position(|c| c == s));

        let (down, up, enter, back) = ui.input(|i| {
            (
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::Enter),
                i.key_pressed(Key::Backspace),
            )
        });

        if len > 0 && (down || up) {
            let next = match current {
                None => 0,
                Some(i) if down => (i + 1).min(len - 1),
                Some(i) => i.saturating_sub(1),
            };
            self.selected = Some(list.children[next].clone());
            self.scroll_to = Some(next);
        }
        if enter
            && let Some(sel) = self.selected.clone()
            && child(node, &sel).is_some_and(is_container)
        {
            self.enter(sel);
        }
        if back {
            self.up();
        }
    }

    fn list_ui(&mut self, ui: &mut Ui, node: &Value) {
        let Some(list) = self.list.take() else { return };
        let mut enter = None;
        let spacing = ui.spacing().item_spacing.y;

        let mut area = egui::ScrollArea::vertical().auto_shrink([false; 2]);
        if let Some(idx) = self.scroll_to
            && !(self.visible.start + 1..self.visible.end.saturating_sub(1)).contains(&idx)
        {
            let rows_visible = self.visible.len().max(1) as f32;
            let offset = (idx as f32 - rows_visible / 2.0).max(0.0) * (ROW_HEIGHT + spacing);
            area = area.vertical_scroll_offset(offset);
        }
        self.scroll_to = None;

        let output = area.show_rows(ui, ROW_HEIGHT, list.children.len(), |ui, range| {
            let key_width = (ui.available_width() * 0.32).clamp(80.0, 320.0);
            for seg in &list.children[range] {
                let Some(value) = child(node, seg) else { continue };
                let selected = self.selected.as_ref() == Some(seg);
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
                let visuals = ui.visuals();
                let painter = ui.painter_at(rect);

                if selected {
                    painter.rect_filled(rect, 4.0, visuals.selection.bg_fill);
                } else if resp.hovered() {
                    painter.rect_filled(rect, 4.0, visuals.widgets.hovered.weak_bg_fill);
                }

                let text_color = if selected { visuals.selection.stroke.color } else { visuals.text_color() };
                let font = FontId::proportional(13.5);
                let key_rect = egui::Rect::from_min_size(rect.min, vec2(key_width, rect.height()));
                painter.with_clip_rect(key_rect.shrink2(vec2(4.0, 0.0))).text(
                    key_rect.left_center() + vec2(10.0, 0.0),
                    Align2::LEFT_CENTER,
                    seg.label(),
                    fastframe_fonts::Weight::Medium.font_id(13.5),
                    text_color,
                );

                let (sum, color) = summary(value);
                let x = rect.left() + key_width + 8.0;
                let sum_rect = painter.text(
                    egui::pos2(x, rect.center().y),
                    Align2::LEFT_CENTER,
                    &sum,
                    FontId::monospace(12.5),
                    if selected { text_color } else { color },
                );
                if let Some(hint) = object_hint(value) {
                    painter.text(
                        egui::pos2(sum_rect.right() + 12.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        hint,
                        font,
                        if selected { text_color } else { visuals.weak_text_color() },
                    );
                }
                if is_container(value) {
                    painter.text(
                        rect.right_center() - vec2(10.0, 0.0),
                        Align2::RIGHT_CENTER,
                        "›",
                        FontId::proportional(16.0),
                        visuals.weak_text_color(),
                    );
                }

                if resp.clicked() {
                    self.selected = Some(seg.clone());
                }
                if resp.double_clicked() && is_container(value) {
                    enter = Some(seg.clone());
                }
            }
        });

        let first = (output.state.offset.y / (ROW_HEIGHT + spacing)).floor() as usize;
        let count = (output.inner_rect.height() / (ROW_HEIGHT + spacing)).ceil() as usize;
        self.visible = first..first + count;

        self.list = Some(list);
        if let Some(seg) = enter {
            self.enter(seg);
        }
    }

    fn table_ui(&mut self, ui: &mut Ui, node: &Value) {
        let Some(list) = self.list.take() else { return };
        let mut enter = None;

        let mut builder = TableBuilder::new(ui)
            .id_salt(("table", &self.path))
            .striped(true)
            .resizable(true)
            .sense(Sense::click())
            .auto_shrink([false; 2])
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(70.0).at_least(40.0).clip(true))
            .columns(Column::initial(130.0).at_least(40.0).clip(true), list.columns.len());

        if let Some(idx) = self.scroll_to.take() {
            builder = builder.scroll_to_row(idx, None);
        }

        builder
            .header(26.0, |mut header| {
                header.col(|ui| {
                    ui.strong("#");
                });
                for column in &list.columns {
                    header.col(|ui| {
                        ui.strong(column);
                    });
                }
            })
            .body(|body| {
                body.rows(ROW_HEIGHT, list.children.len(), |mut row| {
                    let seg = &list.children[row.index()];
                    let value = child(node, seg);
                    row.set_selected(self.selected.as_ref() == Some(seg));
                    row.col(|ui| {
                        ui.label(RichText::new(seg.label()).weak());
                    });
                    for column in &list.columns {
                        row.col(|ui| {
                            let cell = value.and_then(|v| v.get(column));
                            ui.add(egui::Label::new(cell_text(cell)).truncate());
                        });
                    }
                    let resp = row.response();
                    if resp.clicked() {
                        self.selected = Some(seg.clone());
                    }
                    if resp.double_clicked() && value.is_some_and(is_container) {
                        enter = Some(seg.clone());
                    }
                });
            });

        self.list = Some(list);
        if let Some(seg) = enter {
            self.enter(seg);
        }
    }

    fn inspector_ui(&mut self, ui: &mut Ui, root_name: &str, node: &Value, loc: Option<&Localization>) -> Option<Action> {
        let mut action = None;
        let mut full_path = self.path.clone();
        if let Some(sel) = &self.selected {
            full_path.push(sel.clone());
        }
        let path = path_string(root_name, &full_path);
        let value = self.selected.as_ref().and_then(|s| child(node, s)).unwrap_or(node);
        let container = is_container(value);
        let can_enter = self.selected.is_some() && container;

        ui.add_space(4.0);
        ui.label(RichText::new("Inspetor").font(fastframe_fonts::Weight::SemiBold.font_id(15.0)));
        ui.add(egui::Label::new(RichText::new(&path).monospace().weak()).wrap());
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(can_enter, egui::Button::new("➡ Abrir")).clicked()
                && let Some(sel) = self.selected.clone()
            {
                self.enter(sel);
            }
            if ui.button("📋 Caminho").clicked() {
                ui.ctx().copy_text(path.clone());
            }
            if ui.button("📋 JSON").clicked() {
                ui.ctx().copy_text(serde_json::to_string_pretty(value).unwrap_or_default());
            }
            if ui.button("💾 Exportar").clicked() {
                action = Some(Action::Export {
                    value: value.clone(),
                    suggested_name: format!("{}.json", path.replace(['[', ']', '"', '.'], "_")),
                });
            }
        });
        ui.separator();

        if let Some(loc) = loc {
            let found = translations(loc, value);
            if !found.is_empty() {
                egui::CollapsingHeader::new(RichText::new(format!("🌍 Traduções ({})", loc.language)).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        egui::Grid::new("translations").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                            for (field, text) in found {
                                ui.label(RichText::new(field).monospace().weak());
                                ui.add(egui::Label::new(text).wrap().selectable(true));
                                ui.end_row();
                            }
                        });
                    });
                ui.separator();
            }
        }

        let (text, truncated) = self.inspector_text(node);
        if truncated {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!("Mostrando os primeiros {} KB. Use Exportar para o JSON completo.", INSPECTOR_LIMIT / 1000),
            );
        }
        egui::ScrollArea::both().auto_shrink([false; 2]).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(text).monospace().size(12.5)).extend().selectable(true));
        });
        action
    }
}

/// Translations of the localization keys a value holds: the string itself, or the
/// string fields of an object (`tid_name`, `name_key`, `description_key`, ...).
fn translations(loc: &Localization, value: &Value) -> Vec<(String, String)> {
    const MAX: usize = 12;
    match value {
        Value::String(key) => loc
            .get_value_from_key(key)
            .map(|text| vec![(key.clone(), text.to_owned())])
            .unwrap_or_default(),
        Value::Object(map) => map
            .iter()
            .filter_map(|(field, v)| {
                let text = loc.get_value_from_key(v.as_str()?)?;
                (!text.is_empty()).then(|| (field.clone(), text.to_owned()))
            })
            .take(MAX)
            .collect(),
        _ => Vec::new(),
    }
}
