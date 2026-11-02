use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use dc_config::{Config, ConfigLanguage, ConfigPlatform};
use dc_localization::Localization;
use egui::{Color32, RichText, Ui};
use fastframe_dirs::Dirs;
use fastframe_fonts::Weight;
use serde_json::Value;

use crate::explorer::{self, Explorer};
use crate::localization_view::{self, LocalizationView};
use crate::mapping;
use crate::stats_view::StatsView;
use crate::settings::{Settings, SourceKind, Store};
use crate::worker::{Event, Job, Loaded, Worker};

struct Section {
    name: String,
    value: Arc<Value>,
    len: usize,
    kind: &'static str,
}

struct Document {
    label: String,
    language: ConfigLanguage,
    platform: ConfigPlatform,
    sections: Vec<Section>,
}

impl Document {
    fn from_config(label: String, config: Config) -> Self {
        let sections = config
            .data
            .game_data
            .config
            .0
            .into_iter()
            .map(|(name, value)| {
                let (kind, len) = match &value {
                    Value::Array(a) => ("array", a.len()),
                    Value::Object(o) => ("objeto", o.len()),
                    Value::String(_) => ("texto", 1),
                    Value::Number(_) => ("número", 1),
                    Value::Bool(_) => ("bool", 1),
                    Value::Null => ("null", 0),
                };
                Section { name, value: Arc::new(value), len, kind }
            })
            .collect();

        Self {
            label,
            language: config.language,
            platform: config.platform,
            sections,
        }
    }

    fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }
}

#[derive(Clone, PartialEq, Eq)]
enum View {
    Overview,
    Statistics,
    Localization,
    Section(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Raw,
    Mapped,
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum DocKey {
    Raw(String),
    Mapped(String, ConfigLanguage),
}

enum MapState {
    Loading,
    Ready(Arc<Value>),
    Failed(String),
}

struct Status {
    text: String,
    error: bool,
}

pub struct ExplorerApp {
    store: Store,
    settings: Settings,
    token: String,
    worker: Worker,
    pending: usize,
    doc: Option<Document>,
    view: View,
    mode: Mode,
    explorers: HashMap<DocKey, Explorer>,
    mapped: HashMap<(String, ConfigLanguage), MapState>,
    section_filter: String,
    show_source: bool,
    status: Option<Status>,
    stats: Option<StatsView>,
    loc_view: LocalizationView,
    localizations: HashMap<ConfigLanguage, Arc<Localization>>,
    loc_loading: HashSet<ConfigLanguage>,
}

impl ExplorerApp {
    pub fn new(cc: &eframe::CreationContext<'_>, dirs: Option<Dirs>) -> Self {
        let store = Store::new(dirs);
        let (settings, token) = store.load();
        cc.egui_ctx.set_theme(if settings.dark_mode { egui::Theme::Dark } else { egui::Theme::Light });

        let mut app = Self {
            store,
            settings,
            token,
            worker: Worker::new(cc.egui_ctx.clone()),
            pending: 0,
            doc: None,
            view: View::Overview,
            mode: Mode::Raw,
            explorers: HashMap::new(),
            mapped: HashMap::new(),
            section_filter: String::new(),
            show_source: false,
            status: None,
            stats: None,
            loc_view: LocalizationView::default(),
            localizations: HashMap::new(),
            loc_loading: HashSet::new(),
        };

        // `dc-config-explorer config.json` (or "Open with…") opens the file right away.
        if let Some(path) = std::env::args_os().nth(1).map(PathBuf::from).filter(|p| p.is_file()) {
            app.load_file(path);
        }
        app
    }

    fn spawn(&mut self, job: Job) {
        self.pending += 1;
        self.worker.spawn(job);
    }

    fn set_status(&mut self, text: impl Into<String>, error: bool) {
        self.status = Some(Status { text: text.into(), error });
    }

    fn load_file(&mut self, path: PathBuf) {
        let display = path.display().to_string();
        self.settings.file_path = display.clone();
        self.settings.push_recent(&display);
        self.set_status(format!("Lendo {display}…"), false);
        self.spawn(Job::LoadFile {
            path,
            language: self.settings.language,
            platform: self.settings.platform,
        });
        self.store.save(&self.settings, &self.token);
    }

    fn pick_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Abrir game_config")
            .add_filter("JSON", &["json"])
            .pick_file()
        {
            self.load_file(path);
        }
    }

    fn load_from_source(&mut self) {
        let s = &self.settings;
        let job = match s.source {
            SourceKind::File => {
                let path = PathBuf::from(s.file_path.trim());
                self.load_file(path);
                return;
            }
            SourceKind::RawUrl => Job::FetchRaw {
                url: s.raw_url.trim().to_owned(),
                language: s.language,
                platform: s.platform,
            },
            SourceKind::Authenticated => Job::Fetch {
                url: s.api_url.trim().to_owned(),
                user_id: s.user_id.trim().to_owned(),
                auth_token: self.token.trim().to_owned(),
                language: s.language,
                platform: s.platform,
            },
        };
        self.set_status("Baixando configuração…", false);
        self.spawn(job);
        self.store.save(&self.settings, &self.token);
    }

    fn request_mapping(&mut self, section: &str, language: ConfigLanguage) {
        let key = (section.to_owned(), language);
        if self.mapped.contains_key(&key) {
            return;
        }
        let Some(value) = self.doc.as_ref().and_then(|d| d.section(section)).map(|s| s.value.clone()) else {
            return;
        };
        self.mapped.insert(key, MapState::Loading);
        self.spawn(Job::Map {
            section: section.to_owned(),
            value,
            language,
        });
    }

    fn request_localization(&mut self, language: ConfigLanguage) {
        if self.localizations.contains_key(&language) || !self.loc_loading.insert(language) {
            return;
        }
        self.spawn(Job::LoadLocalization { language });
    }

    fn handle_localization_action(&mut self, action: localization_view::Action) {
        match action {
            localization_view::Action::Download(language) => self.request_localization(language),
            localization_view::Action::Export(localization) => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Exportar localização")
                    .set_file_name(format!("localization_{}.json", localization.language))
                    .add_filter("JSON", &["json"])
                    .save_file()
                {
                    self.spawn(Job::SaveLocalization { path, localization });
                }
            }
            localization_view::Action::Compare(language) => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Arquivo de localização antigo")
                    .add_filter("JSON", &["json"])
                    .pick_file()
                {
                    self.set_status("Comparando localizações…", false);
                    self.spawn(Job::CompareLocalization { path, language });
                }
            }
        }
    }

    fn export(&mut self, value: Value, suggested_name: String) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Exportar JSON")
            .set_file_name(suggested_name)
            .add_filter("JSON", &["json"])
            .save_file()
        {
            self.spawn(Job::SaveJson {
                path,
                value: Arc::new(value),
                pretty: true,
            });
        }
    }

    fn poll_events(&mut self) {
        while let Ok(event) = self.worker.receiver.try_recv() {
            self.pending = self.pending.saturating_sub(1);
            match event {
                Event::ConfigLoaded { label, result } => match result {
                    Ok(loaded) => {
                        let Loaded { config, statistics } = *loaded;
                        self.stats = Some(StatsView::new(statistics));
                        let doc = Document::from_config(label, config);
                        self.set_status(format!("{} seções carregadas de {}", doc.sections.len(), doc.label), false);
                        self.doc = Some(doc);
                        self.view = View::Overview;
                        self.explorers.clear();
                        self.mapped.clear();
                        self.show_source = false;
                    }
                    Err(e) => self.set_status(format!("Falha ao carregar: {e}"), true),
                },
                Event::Mapped { section, language, result } => {
                    let state = match result {
                        Ok(v) => {
                            self.set_status(format!("`{section}` mapeado ({language})"), false);
                            MapState::Ready(Arc::new(v))
                        }
                        Err(e) => {
                            self.set_status(format!("Falha ao mapear `{section}`: {e}"), true);
                            MapState::Failed(e)
                        }
                    };
                    self.mapped.insert((section, language), state);
                }
                Event::Saved(result) => match result {
                    Ok(path) => self.set_status(format!("Salvo em {}", path.display()), false),
                    Err(e) => self.set_status(format!("Falha ao salvar: {e}"), true),
                },
                Event::Localization { language, result } => {
                    self.loc_loading.remove(&language);
                    match result {
                        Ok(loc) => {
                            self.set_status(format!("Traduções `{language}` carregadas ({} chaves)", loc.data.len()), false);
                            self.localizations.insert(language, loc);
                        }
                        Err(e) => self.set_status(e, true),
                    }
                }
                Event::Compared { path, result } => match result {
                    Ok(result) => {
                        self.set_status(
                            format!(
                                "Comparação: {} novas, {} editadas, {} removidas",
                                result.new_fields.len(),
                                result.edited_fields.len(),
                                result.deleted_fields.len()
                            ),
                            false,
                        );
                        self.loc_view.set_comparison(path, result);
                    }
                    Err(e) => self.set_status(format!("Falha ao comparar: {e}"), true),
                },
            }
        }
    }

    // ---------------------------------------------------------------- UI

    fn top_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("🐉 DC Config Explorer").font(Weight::Bold.font_id(17.0)));
            ui.separator();
            if ui.button("📂 Abrir arquivo…").clicked() {
                self.pick_file();
            }
            if ui.button("🌐 Carregar de…").clicked() {
                self.show_source = true;
            }
            if ui.selectable_label(self.view == View::Localization, "🌍 Localização").clicked() {
                self.view = View::Localization;
            }
            if !self.settings.recent_files.is_empty() {
                let mut chosen = None;
                egui::ComboBox::from_id_salt("recent")
                    .selected_text("🕘 Recentes")
                    .width(120.0)
                    .show_ui(ui, |ui| {
                        for path in &self.settings.recent_files {
                            if ui.selectable_label(false, path).clicked() {
                                chosen = Some(PathBuf::from(path));
                            }
                        }
                    });
                if let Some(path) = chosen {
                    self.load_file(path);
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let icon = if self.settings.dark_mode { "☀" } else { "🌙" };
                if ui.button(icon).on_hover_text("Alternar tema").clicked() {
                    self.settings.dark_mode = !self.settings.dark_mode;
                    ui.ctx().set_theme(if self.settings.dark_mode { egui::Theme::Dark } else { egui::Theme::Light });
                    self.store.save(&self.settings, &self.token);
                }
                language_combo(ui, "lang_top", &mut self.settings.language);
                ui.label(RichText::new("Idioma das traduções").weak());
            });
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if self.pending > 0 {
                ui.spinner();
                ui.label(format!("{} tarefa(s) em andamento", self.pending));
                ui.separator();
            }
            if let Some(status) = &self.status {
                let color = if status.error { ui.visuals().error_fg_color } else { ui.visuals().weak_text_color() };
                ui.add(egui::Label::new(RichText::new(&status.text).color(color)).truncate());
            }
            if let Some(doc) = &self.doc {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("{} · {}", doc.platform, doc.language)).weak());
                });
            }
        });
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        let Some(doc) = &self.doc else {
            ui.add_space(8.0);
            ui.label(RichText::new("Nenhuma configuração carregada.").weak());
            return;
        };

        ui.add_space(6.0);
        if ui.selectable_label(self.view == View::Overview, RichText::new("📊 Visão geral").strong()).clicked() {
            self.view = View::Overview;
        }
        if ui.selectable_label(self.view == View::Statistics, RichText::new("📈 Estatísticas").strong()).clicked() {
            self.view = View::Statistics;
        }
        if ui.selectable_label(self.view == View::Localization, RichText::new("🌍 Localização").strong()).clicked() {
            self.view = View::Localization;
        }
        ui.add_space(4.0);
        ui.add(egui::TextEdit::singleline(&mut self.section_filter).hint_text("🔍 Seções…"));
        ui.add_space(4.0);

        let needle = self.section_filter.to_lowercase();
        let mut chosen = None;
        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                for section in doc.sections.iter().filter(|s| s.name.to_lowercase().contains(&needle)) {
                    let selected = self.view == View::Section(section.name.clone());
                    let has_mapper = mapping::has_mapper(&section.name);
                    let mut text = RichText::new(&section.name);
                    if has_mapper {
                        text = text.strong();
                    }
                    let resp = ui.selectable_label(selected, text);
                    // Entry count, right-aligned inside the row.
                    let color = if selected { ui.visuals().selection.stroke.color } else { ui.visuals().weak_text_color() };
                    ui.painter().text(
                        resp.rect.right_center() - egui::vec2(16.0, 0.0), // clear of the floating scrollbar
                        egui::Align2::RIGHT_CENTER,
                        section.len.to_string(),
                        egui::FontId::proportional(11.0),
                        color,
                    );
                    let resp = if has_mapper { resp.on_hover_text("Possui mapper (dc-config-mapper)") } else { resp };
                    if resp.clicked() {
                        chosen = Some(section.name.clone());
                    }
                }
            });
        });

        if let Some(name) = chosen {
            if !mapping::has_mapper(&name) {
                self.mode = Mode::Raw;
            }
            self.view = View::Section(name);
        }
    }

    fn welcome(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.25);
            ui.label(RichText::new("🐉").size(56.0));
            ui.label(RichText::new("DC Config Explorer").font(Weight::Bold.font_id(28.0)));
            ui.label(RichText::new("Explore o game_config do Dragon City com dc-config e dc-config-mapper.").weak());
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                let width = 340.0;
                ui.add_space((ui.available_width() - width).max(0.0) / 2.0);
                if ui.add(egui::Button::new(RichText::new("📂 Abrir arquivo…").size(15.0)).min_size(egui::vec2(160.0, 34.0))).clicked() {
                    self.pick_file();
                }
                if ui.add(egui::Button::new(RichText::new("🌐 Carregar de…").size(15.0)).min_size(egui::vec2(160.0, 34.0))).clicked() {
                    self.show_source = true;
                }
            });
            ui.add_space(10.0);
            ui.label(RichText::new("Ou arraste um arquivo .json para a janela.").weak().small());
        });
    }

    fn overview(&mut self, ui: &mut Ui) {
        let Some(doc) = &self.doc else { return };
        let mut chosen = None;

        ui.label(RichText::new("Visão geral").font(Weight::SemiBold.font_id(20.0)));
        ui.add(egui::Label::new(RichText::new(&doc.label).weak()).truncate());
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            stat(ui, "Seções", doc.sections.len().to_string());
            let mapped = doc.sections.iter().filter(|s| mapping::has_mapper(&s.name)).count();
            stat(ui, "Com mapper", mapped.to_string());
            if let Some(items) = doc.section("items") {
                stat(ui, "Itens", items.len.to_string());
                let dragons = items
                    .value
                    .as_array()
                    .map(|a| a.iter().filter(|i| i.get("group_type").and_then(Value::as_str) == Some("DRAGON")).count())
                    .unwrap_or(0);
                stat(ui, "Dragões", dragons.to_string());
            }
            stat(ui, "Plataforma", doc.platform.to_string());
            stat(ui, "Idioma", doc.language.to_string());
        });
        ui.add_space(12.0);

        egui_extras::TableBuilder::new(ui)
            .striped(true)
            .sense(egui::Sense::click())
            .auto_shrink([false; 2])
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(egui_extras::Column::initial(260.0).at_least(120.0))
            .column(egui_extras::Column::initial(90.0))
            .column(egui_extras::Column::initial(110.0))
            .column(egui_extras::Column::remainder())
            .header(26.0, |mut h| {
                h.col(|ui| { ui.strong("Seção"); });
                h.col(|ui| { ui.strong("Tipo"); });
                h.col(|ui| { ui.strong("Entradas"); });
                h.col(|ui| { ui.strong("Mapper"); });
            })
            .body(|body| {
                body.rows(24.0, doc.sections.len(), |mut row| {
                    let section = &doc.sections[row.index()];
                    row.col(|ui| { ui.label(&section.name); });
                    row.col(|ui| { ui.label(RichText::new(section.kind).weak()); });
                    row.col(|ui| { ui.label(section.len.to_string()); });
                    row.col(|ui| {
                        if mapping::has_mapper(&section.name) {
                            ui.label(RichText::new("✔ disponível").color(Color32::from_rgb(110, 190, 120)));
                        }
                    });
                    if row.response().clicked() {
                        chosen = Some(section.name.clone());
                    }
                });
            });

        if let Some(name) = chosen {
            self.view = View::Section(name);
        }
    }

    fn section_view(&mut self, ui: &mut Ui, name: String) {
        let Some(section_value) = self.doc.as_ref().and_then(|d| d.section(&name)).map(|s| s.value.clone()) else {
            self.view = View::Overview;
            return;
        };
        let has_mapper = mapping::has_mapper(&name);
        let language = self.settings.language;

        ui.horizontal(|ui| {
            ui.label(RichText::new(&name).font(Weight::SemiBold.font_id(20.0)));
            ui.add_space(12.0);
            ui.selectable_value(&mut self.mode, Mode::Raw, "Bruto (dc-config)");
            ui.add_enabled_ui(has_mapper, |ui| {
                ui.selectable_value(&mut self.mode, Mode::Mapped, "Mapeado (dc-config-mapper)")
                    .on_disabled_hover_text("Esta seção não possui mapper");
            });
            if self.mode == Mode::Mapped && has_mapper {
                ui.label(RichText::new(format!("traduzido em `{language}`")).weak());
                if matches!(self.mapped.get(&(name.clone(), language)), Some(MapState::Failed(_)) | Some(MapState::Ready(_)))
                    && ui.small_button("↻").on_hover_text("Mapear novamente").clicked()
                {
                    self.mapped.remove(&(name.clone(), language));
                }
            }
        });
        ui.add_space(4.0);

        let mode = if has_mapper { self.mode } else { Mode::Raw };
        let (key, root): (DocKey, Arc<Value>) = match mode {
            Mode::Raw => (DocKey::Raw(name.clone()), section_value),
            Mode::Mapped => {
                self.request_mapping(&name, language);
                match self.mapped.get(&(name.clone(), language)) {
                    Some(MapState::Ready(v)) => (DocKey::Mapped(name.clone(), language), v.clone()),
                    Some(MapState::Failed(e)) => {
                        ui.add_space(12.0);
                        ui.colored_label(ui.visuals().error_fg_color, "O mapper falhou para esta seção:");
                        ui.add(egui::Label::new(RichText::new(e).monospace()).wrap());
                        return;
                    }
                    _ => {
                        ui.add_space(24.0);
                        ui.vertical_centered(|ui| {
                            ui.spinner();
                            ui.label("Baixando localização e mapeando…");
                        });
                        return;
                    }
                }
            }
        };

        let loc = self.localizations.get(&language).cloned();
        let explorer = self.explorers.entry(key).or_default();
        if let Some(explorer::Action::Export { value, suggested_name }) = explorer.ui(ui, &name, &root, loc.as_deref()) {
            self.export(value, suggested_name);
        }
    }

    fn source_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_source;
        let mut load = false;

        egui::Window::new("Carregar configuração")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                let s = &mut self.settings;
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut s.source, SourceKind::File, "📄 Arquivo local");
                    ui.selectable_value(&mut s.source, SourceKind::RawUrl, "🌐 URL pública");
                    ui.selectable_value(&mut s.source, SourceKind::Authenticated, "🔐 Endpoint autenticado");
                });
                ui.separator();

                egui::Grid::new("source_grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    match s.source {
                        SourceKind::File => {
                            ui.label("Arquivo");
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(&mut s.file_path).desired_width(300.0).hint_text("C:\\…\\config.json"));
                                if ui.button("…").clicked()
                                    && let Some(path) = rfd::FileDialog::new().add_filter("JSON", &["json"]).pick_file()
                                {
                                    s.file_path = path.display().to_string();
                                }
                            });
                            ui.end_row();
                        }
                        SourceKind::RawUrl => {
                            ui.label("URL");
                            ui.add(egui::TextEdit::singleline(&mut s.raw_url).desired_width(340.0).hint_text("https://…/config.json"));
                            ui.end_row();
                        }
                        SourceKind::Authenticated => {
                            ui.label("URL");
                            ui.add(egui::TextEdit::singleline(&mut s.api_url).desired_width(340.0));
                            ui.end_row();
                            ui.label("User ID");
                            ui.add(egui::TextEdit::singleline(&mut s.user_id).desired_width(340.0));
                            ui.end_row();
                            ui.label("Auth token");
                            ui.add(egui::TextEdit::singleline(&mut self.token).password(true).desired_width(340.0));
                            ui.end_row();
                            ui.label("");
                            ui.checkbox(&mut s.remember_token, "Lembrar token (arquivo privado)");
                            ui.end_row();
                        }
                    }
                    ui.label("Idioma");
                    language_combo(ui, "lang_source", &mut s.language);
                    ui.end_row();
                    ui.label("Plataforma");
                    egui::ComboBox::from_id_salt("platform")
                        .selected_text(s.platform.as_str())
                        .show_ui(ui, |ui| {
                            for p in ConfigPlatform::ALL {
                                ui.selectable_value(&mut s.platform, *p, p.as_str());
                            }
                        });
                    ui.end_row();
                });

                ui.add_space(8.0);
                let ready = match s.source {
                    SourceKind::File => !s.file_path.trim().is_empty(),
                    SourceKind::RawUrl => !s.raw_url.trim().is_empty(),
                    SourceKind::Authenticated => {
                        !s.api_url.trim().is_empty() && !s.user_id.trim().is_empty() && !self.token.trim().is_empty()
                    }
                };
                ui.horizontal(|ui| {
                    if ui.add_enabled(ready && self.pending == 0, egui::Button::new("Carregar")).clicked() {
                        load = true;
                    }
                    if self.pending > 0 {
                        ui.spinner();
                    }
                });
            });

        self.show_source = open && self.show_source;
        if load {
            self.load_from_source();
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        if let Some(path) = dropped.into_iter().next() {
            self.load_file(path);
        }
    }
}

fn stat(ui: &mut Ui, label: &str, value: String) {
    egui::Frame::group(ui.style()).inner_margin(egui::Margin::symmetric(14, 8)).show(ui, |ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(label).small().weak());
            ui.label(RichText::new(value).font(Weight::SemiBold.font_id(18.0)));
        });
    });
}

pub fn language_combo(ui: &mut Ui, id: &str, language: &mut ConfigLanguage) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(language_name(*language))
        .show_ui(ui, |ui| {
            for l in ConfigLanguage::ALL {
                ui.selectable_value(language, *l, language_name(*l));
            }
        });
}

fn language_name(language: ConfigLanguage) -> String {
    let name = match language {
        ConfigLanguage::English => "English",
        ConfigLanguage::Portuguese => "Português",
        ConfigLanguage::BrazilianPortuguese => "Português (BR)",
        ConfigLanguage::Spanish => "Español",
        ConfigLanguage::French => "Français",
        ConfigLanguage::German => "Deutsch",
        ConfigLanguage::Italian => "Italiano",
        ConfigLanguage::Japanese => "日本語",
        ConfigLanguage::Russian => "Русский",
        ConfigLanguage::Korean => "한국어",
        ConfigLanguage::Turkish => "Türkçe",
    };
    format!("{name} ({})", language.as_str())
}

impl eframe::App for ExplorerApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_events();
        self.handle_dropped_files(&ctx);

        egui::Panel::top("top").show(ui, |ui| {
            ui.add_space(4.0);
            self.top_bar(ui);
            ui.add_space(2.0);
        });
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));

        if self.doc.is_some() {
            egui::Panel::left("sections")
                .resizable(true)
                .default_size(240.0)
                .min_size(160.0)
                .show(ui, |ui| self.sidebar(ui));
        }

        // Translations for the inspector follow the chosen language.
        if self.doc.is_some() {
            self.request_localization(self.settings.language);
        }

        egui::CentralPanel::default().show(ui, |ui| {
            if self.view == View::Localization {
                let mut language = self.settings.language;
                let loc = self.localizations.get(&language).cloned();
                let loading = self.loc_loading.contains(&language);
                let action = self.loc_view.ui(ui, &mut language, loc.as_ref(), loading);
                self.settings.language = language;
                if let Some(action) = action {
                    self.handle_localization_action(action);
                }
                return;
            }
            if self.doc.is_none() {
                self.welcome(ui);
                return;
            }
            match self.view.clone() {
                View::Overview => self.overview(ui),
                View::Statistics => {
                    if let Some(explorer::Action::Export { value, suggested_name }) =
                        self.stats.as_mut().and_then(|s| s.ui(ui))
                    {
                        self.export(value, suggested_name);
                    }
                }
                View::Section(name) => self.section_view(ui, name),
                View::Localization => {}
            }
        });

        if self.show_source {
            self.source_window(&ctx);
        }
    }

    fn on_exit(&mut self) {
        self.store.save(&self.settings, &self.token);
        self.store.flush();
    }
}
