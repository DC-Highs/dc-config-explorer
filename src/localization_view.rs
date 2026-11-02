//! Browse, search, export and compare the game's translations (`dc-localization`).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dc_config::ConfigLanguage;
use dc_localization::{ComparisonResult, Localization, NormalizeOptions};
use egui::{RichText, Sense, Ui};
use egui_extras::{Column, TableBuilder};
use fastframe_fonts::Weight;

const SEARCH_DEBOUNCE: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, PartialEq, Eq)]
enum SearchIn {
    Both,
    Keys,
    Values,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CompareTab {
    New,
    Edited,
    Deleted,
}

pub enum Action {
    Download(ConfigLanguage),
    Export(Arc<Localization>),
    Compare(ConfigLanguage),
}

struct Results {
    language: ConfigLanguage,
    query: String,
    search_in: SearchIn,
    keys: Vec<String>,
}

pub struct LocalizationView {
    query: String,
    search_in: SearchIn,
    last_edit: Instant,
    results: Option<Results>,
    selected: Option<String>,
    comparison: Option<(PathBuf, ComparisonResult)>,
    compare_tab: CompareTab,
}

impl Default for LocalizationView {
    fn default() -> Self {
        Self {
            query: String::new(),
            search_in: SearchIn::Both,
            last_edit: Instant::now(),
            results: None,
            selected: None,
            comparison: None,
            compare_tab: CompareTab::New,
        }
    }
}

impl LocalizationView {
    pub fn set_comparison(&mut self, path: PathBuf, result: ComparisonResult) {
        self.comparison = Some((path, result));
    }

    fn refresh(&mut self, loc: &Localization) {
        let query = self.query.trim().to_owned();
        let stale = self.results.as_ref().is_none_or(|r| {
            r.language != loc.language || r.query != query || r.search_in != self.search_in
        });
        if !stale {
            return;
        }

        let mut keys: Vec<String> = if query.is_empty() {
            loc.data.keys().cloned().collect()
        } else {
            // dc-localization's accent- and case-insensitive search.
            let options = NormalizeOptions::default();
            let mut found: HashSet<&str> = HashSet::new();
            if self.search_in != SearchIn::Values {
                found.extend(loc.search_keys(&query, options));
            }
            if self.search_in != SearchIn::Keys {
                let values: HashSet<&str> = loc.search_values(&query, options).into_iter().collect();
                found.extend(
                    loc.data
                        .iter()
                        .filter(|(_, v)| values.contains(v.as_str()))
                        .map(|(k, _)| k.as_str()),
                );
            }
            found.into_iter().map(str::to_owned).collect()
        };
        keys.sort_unstable();

        self.results = Some(Results {
            language: loc.language,
            query,
            search_in: self.search_in,
            keys,
        });
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        language: &mut ConfigLanguage,
        loc: Option<&Arc<Localization>>,
        loading: bool,
    ) -> Option<Action> {
        let mut action = None;

        ui.horizontal(|ui| {
            ui.label(RichText::new("Localização").font(Weight::SemiBold.font_id(20.0)));
            ui.label(RichText::new("via dc-localization").weak());
            ui.add_space(12.0);
            crate::app::language_combo(ui, "lang_loc", language);
            if let Some(loc) = loc {
                ui.label(RichText::new(format!("{} chaves", loc.data.len())).weak());
                if ui.button("💾 Exportar…").on_hover_text("Salvar como JSON chave → valor (ordenado)").clicked() {
                    action = Some(Action::Export(loc.clone()));
                }
                if ui
                    .button("⇄ Comparar com arquivo…")
                    .on_hover_text("Compara um arquivo de localização salvo (antigo) com o atual (novo)")
                    .clicked()
                {
                    action = Some(Action::Compare(*language));
                }
            }
        });
        ui.add_space(4.0);

        let Some(loc) = loc else {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                if loading {
                    ui.spinner();
                    ui.label("Baixando traduções…");
                } else if ui.button(format!("⬇ Baixar traduções ({language})")).clicked() {
                    action = Some(Action::Download(*language));
                }
            });
            return action;
        };

        if self.comparison.is_some() {
            egui::Panel::bottom("loc_compare")
                .resizable(true)
                .default_size(260.0)
                .min_size(120.0)
                .show(ui, |ui| self.comparison_ui(ui));
        }

        egui::Panel::bottom("loc_detail").show(ui, |ui| {
            ui.add_space(4.0);
            match self.selected.as_ref().and_then(|k| loc.data.get_key_value(k)) {
                Some((key, value)) => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(key).monospace().strong());
                        if ui.small_button("📋 chave").clicked() {
                            ui.ctx().copy_text(key.clone());
                        }
                        if ui.small_button("📋 valor").clicked() {
                            ui.ctx().copy_text(value.clone());
                        }
                    });
                    egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                        ui.add(egui::Label::new(value.as_str()).wrap().selectable(true));
                    });
                }
                None => {
                    ui.label(RichText::new("Selecione uma chave para ver o texto completo.").weak());
                }
            }
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("🔍 Buscar (sem distinção de acentos/maiúsculas)…")
                        .desired_width(360.0),
                );
                if resp.changed() {
                    self.last_edit = Instant::now();
                }
                ui.selectable_value(&mut self.search_in, SearchIn::Both, "Chaves e valores");
                ui.selectable_value(&mut self.search_in, SearchIn::Keys, "Chaves");
                ui.selectable_value(&mut self.search_in, SearchIn::Values, "Valores");
            });

            // Search after the user pauses typing: every search normalizes ~100k strings.
            let waited = self.last_edit.elapsed();
            if waited >= SEARCH_DEBOUNCE {
                self.refresh(loc);
            } else {
                ui.ctx().request_repaint_after(SEARCH_DEBOUNCE - waited);
            }

            let Some(results) = &self.results else { return };
            ui.label(RichText::new(format!("{} resultado(s)", results.keys.len())).weak());

            let mut clicked = None;
            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .sense(Sense::click())
                .auto_shrink([false; 2])
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::initial(340.0).at_least(120.0).clip(true))
                .column(Column::remainder().clip(true))
                .header(24.0, |mut h| {
                    h.col(|ui| {
                        ui.strong("Chave");
                    });
                    h.col(|ui| {
                        ui.strong("Texto");
                    });
                })
                .body(|body| {
                    body.rows(22.0, results.keys.len(), |mut row| {
                        let key = &results.keys[row.index()];
                        row.set_selected(self.selected.as_ref() == Some(key));
                        row.col(|ui| {
                            ui.add(egui::Label::new(RichText::new(key).monospace().size(12.0)).truncate());
                        });
                        row.col(|ui| {
                            let value = loc.data.get(key).map(String::as_str).unwrap_or_default();
                            ui.add(egui::Label::new(value.replace('\n', " ⏎ ")).truncate());
                        });
                        if row.response().clicked() {
                            clicked = Some(key.clone());
                        }
                    });
                });
            if clicked.is_some() {
                self.selected = clicked;
            }
        });

        action
    }

    fn comparison_ui(&mut self, ui: &mut Ui) {
        let Some((path, result)) = &self.comparison else { return };
        let mut close = false;

        ui.horizontal(|ui| {
            ui.label(RichText::new("Comparação").font(Weight::SemiBold.font_id(15.0)));
            ui.add(egui::Label::new(RichText::new(format!("{} → atual", path.display())).weak()).truncate());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = ui.small_button("✖").clicked();
            });
        });
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.compare_tab, CompareTab::New, format!("➕ Novas ({})", result.new_fields.len()));
            ui.selectable_value(&mut self.compare_tab, CompareTab::Edited, format!("✏ Editadas ({})", result.edited_fields.len()));
            ui.selectable_value(&mut self.compare_tab, CompareTab::Deleted, format!("➖ Removidas ({})", result.deleted_fields.len()));
        });

        let rows: Vec<(&str, String)> = match self.compare_tab {
            CompareTab::New => result.new_fields.iter().map(|f| (f.key.as_str(), f.value.clone())).collect(),
            CompareTab::Edited => result
                .edited_fields
                .iter()
                .map(|f| (f.key.as_str(), format!("{}  →  {}", f.values.old, f.values.new)))
                .collect(),
            CompareTab::Deleted => result.deleted_fields.iter().map(|f| (f.key.as_str(), f.value.clone())).collect(),
        };

        egui::ScrollArea::vertical()
            .id_salt("compare_rows")
            .auto_shrink([false; 2])
            .show_rows(ui, 20.0, rows.len(), |ui, range| {
                for (key, text) in &rows[range] {
                    ui.horizontal(|ui| {
                        ui.add_sized([300.0, 20.0], egui::Label::new(RichText::new(*key).monospace().size(12.0)).truncate());
                        ui.add(egui::Label::new(text.replace('\n', " ⏎ ")).truncate());
                    });
                }
            });

        if close {
            self.comparison = None;
        }
    }
}
