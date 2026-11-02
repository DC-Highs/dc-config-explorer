//! The statistics `dc-statistics` generates for the loaded configuration.

use dc_core::DragonElement;
use dc_statistics::{GameStatistics, IslandKindStatistics};
use egui::{Color32, RichText, Sense, Ui, vec2};
use fastframe_fonts::Weight;
use serde_json::Value;

use crate::explorer::{self, Explorer};

pub struct StatsView {
    pub statistics: GameStatistics,
    json: Value,
    show_json: bool,
    explorer: Explorer,
}

impl StatsView {
    pub fn new(statistics: GameStatistics) -> Self {
        let json = serde_json::to_value(&statistics).unwrap_or_default();
        Self {
            statistics,
            json,
            show_json: false,
            explorer: Explorer::default(),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) -> Option<explorer::Action> {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Estatísticas").font(Weight::SemiBold.font_id(20.0)));
            ui.label(RichText::new("geradas por dc-statistics").weak());
            ui.add_space(12.0);
            ui.selectable_value(&mut self.show_json, false, "📊 Painel");
            ui.selectable_value(&mut self.show_json, true, "{ } JSON");
        });
        ui.add_space(4.0);

        if self.show_json {
            return self.explorer.ui(ui, "statistics", &self.json, None);
        }

        let s = &self.statistics;
        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
            cards(ui, &[
                ("Dragões", s.dragons.total_count),
                ("Itens", s.items.total_count),
                ("Ilhas ativas", s.islands.actives_count),
                ("Ilhas futuras", s.islands.upcomings_count),
                ("Nível máximo", s.max_level as usize),
                ("Baús", s.chests_count),
                ("Recompensas de baús", s.chest_rewards_count),
                ("Coleções", s.dragon_collections_count),
                ("Skins", s.dragon_skins_count),
                ("Na loja", s.dragons.in_store_count),
                ("Cruzáveis", s.dragons.breedable_count),
            ]);
            ui.add_space(12.0);

            ui.columns(2, |cols| {
                let d = &s.dragons;
                section(&mut cols[0], "Dragões por raridade", |ui| {
                    bars(ui, &[
                        ("Comum".into(), d.common_count, Color32::from_rgb(150, 160, 170)),
                        ("Raro".into(), d.rare_count, Color32::from_rgb(90, 160, 240)),
                        ("Muito raro".into(), d.very_rare_count, Color32::from_rgb(240, 200, 70)),
                        ("Épico".into(), d.epic_count, Color32::from_rgb(170, 110, 230)),
                        ("Lendário".into(), d.legendary_count, Color32::from_rgb(240, 150, 60)),
                        ("Mítico".into(), d.mythical_count, Color32::from_rgb(230, 80, 120)),
                        ("Heroico".into(), d.heroic_count, Color32::from_rgb(80, 200, 160)),
                    ]);
                });
                let e = &d.elements;
                section(&mut cols[0], "Elementos por dragão", |ui| {
                    let c = Color32::from_rgb(110, 150, 230);
                    bars(ui, &[
                        ("1 elemento".into(), e.dragons_with_one_element, c),
                        ("2 elementos".into(), e.dragons_with_two_elements, c),
                        ("3 elementos".into(), e.dragons_with_three_elements, c),
                        ("4 elementos".into(), e.dragons_with_four_elements, c),
                        ("Outros".into(), e.dragons_with_other_elements, c),
                    ]);
                });
                section(&mut cols[0], "Ilhas de evento", |ui| islands_table(ui, s));

                section(&mut cols[1], &format!("Dragões por elemento ({} elementos)", e.unique_elements_count), |ui| {
                    let mut rows: Vec<_> = e
                        .dragon_counts_per_element
                        .iter()
                        .map(|(code, count)| (element_label(code), *count, element_color(code)))
                        .collect();
                    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                    bars(ui, &rows);
                });
                section(&mut cols[1], &format!("Dragões por categoria ({} categorias)", d.categories.unique_categories_count), |ui| {
                    let mut rows: Vec<_> = d.categories.dragon_counts_per_category.iter().collect();
                    rows.sort_by_key(|(category, _)| **category);
                    let rows: Vec<_> = rows
                        .into_iter()
                        .map(|(category, count)| (format!("Categoria {category}"), *count, Color32::from_rgb(120, 180, 140)))
                        .collect();
                    bars(ui, &rows);
                });
            });

            section(ui, "Itens por grupo", |ui| {
                let mut rows: Vec<(String, usize, Color32)> = self
                    .json
                    .get("items")
                    .and_then(Value::as_object)
                    .into_iter()
                    .flatten()
                    .filter(|(k, _)| k.as_str() != "totalCount")
                    .filter_map(|(k, v)| Some((humanize(k), v.as_u64()? as usize, Color32::from_rgb(200, 160, 90))))
                    .filter(|(_, n, _)| *n > 0)
                    .collect();
                rows.sort_by_key(|a| std::cmp::Reverse(a.1));
                bars(ui, &rows);
            });
        });
        None
    }
}

/// Stat cards laid out in as many equal columns as fit (frames cannot wrap on their own).
fn cards(ui: &mut Ui, items: &[(&str, usize)]) {
    const MIN_CARD: f32 = 150.0;
    let gap = ui.spacing().item_spacing.x;
    let per_row = (((ui.available_width() + gap) / (MIN_CARD + gap)).floor() as usize).clamp(1, items.len());
    let card_width = (ui.available_width() - gap * (per_row - 1) as f32) / per_row as f32;
    // Frame::group: 14px horizontal margin and a 1px stroke on each side.
    let inner_width = (card_width - 2.0 * 14.0 - 2.0).max(0.0);

    for row in items.chunks(per_row) {
        ui.horizontal(|ui| {
            for (label, value) in row {
                egui::Frame::group(ui.style()).inner_margin(egui::Margin::symmetric(14, 8)).show(ui, |ui| {
                    ui.set_width(inner_width);
                    ui.vertical(|ui| {
                        ui.add(egui::Label::new(RichText::new(*label).small().weak()).truncate());
                        ui.label(RichText::new(value.to_string()).font(Weight::SemiBold.font_id(20.0)));
                    });
                });
            }
        });
    }
}

fn section(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    // The frame adds its margin and stroke around the content: leave room for them.
    let width = (ui.available_width() - 2.0 * 12.0 - 2.0).max(0.0);
    egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
        ui.set_width(width);
        ui.label(RichText::new(title).font(Weight::SemiBold.font_id(15.0)));
        ui.add_space(6.0);
        add(ui);
    });
    ui.add_space(8.0);
}

/// Horizontal bar chart: label, bar scaled to the largest value, count.
fn bars(ui: &mut Ui, rows: &[(String, usize, Color32)]) {
    if rows.is_empty() {
        ui.label(RichText::new("Sem dados.").weak());
        return;
    }
    let max = rows.iter().map(|r| r.1).max().unwrap_or(1).max(1) as f32;
    let label_w = 150.0;
    let count_w = 56.0;

    for (label, count, color) in rows {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
        let painter = ui.painter_at(rect);
        let text = ui.visuals().text_color();

        painter.text(rect.left_center(), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(13.0), text);
        let track = egui::Rect::from_min_max(
            egui::pos2(rect.left() + label_w, rect.top() + 4.0),
            egui::pos2(rect.right() - count_w, rect.bottom() - 4.0),
        );
        painter.rect_filled(track, 3.0, ui.visuals().faint_bg_color);
        let mut bar = track;
        bar.set_width((track.width() * (*count as f32 / max)).max(if *count > 0 { 2.0 } else { 0.0 }));
        painter.rect_filled(bar, 3.0, *color);
        painter.text(rect.right_center(), egui::Align2::RIGHT_CENTER, count.to_string(), egui::FontId::monospace(12.5), text);
        resp.on_hover_text(format!("{label}: {count}"));
    }
}

fn islands_table(ui: &mut Ui, s: &GameStatistics) {
    let i = &s.islands;
    let kinds: [(&str, &IslandKindStatistics); 7] = [
        ("Maze", &i.maze),
        ("Tower", &i.tower),
        ("Fog", &i.fog),
        ("Grid", &i.grid),
        ("Heroic Races", &i.heroic_races),
        ("Runner", &i.runner),
        ("Puzzle", &i.puzzle),
    ];
    egui::Grid::new("islands").striped(true).num_columns(5).spacing([18.0, 4.0]).show(ui, |ui| {
        for h in ["Tipo", "Total", "Ativas", "Futuras", "Passadas"] {
            ui.strong(h);
        }
        ui.end_row();
        for (name, k) in kinds {
            ui.label(name);
            ui.label(k.total_count.to_string());
            ui.label(RichText::new(k.actives_count.to_string()).color(Color32::from_rgb(110, 190, 120)));
            ui.label(k.upcomings_count.to_string());
            ui.label(RichText::new(k.pasts_count.to_string()).weak());
            ui.end_row();
        }
        ui.strong("Total");
        ui.strong(i.total_count.to_string());
        ui.strong(i.actives_count.to_string());
        ui.strong(i.upcomings_count.to_string());
        ui.strong(i.pasts_count.to_string());
        ui.end_row();
    });
}

fn element_label(code: &str) -> String {
    match DragonElement::from_acronym(code) {
        Some(element) => format!("{} ({code})", capitalize(element.as_str())),
        None => code.to_owned(),
    }
}

fn element_color(code: &str) -> Color32 {
    use DragonElement::*;
    match DragonElement::from_acronym(code) {
        Some(Terra) => Color32::from_rgb(170, 120, 70),
        Some(Flame) => Color32::from_rgb(235, 90, 60),
        Some(Sea) => Color32::from_rgb(60, 140, 230),
        Some(Nature) => Color32::from_rgb(90, 190, 80),
        Some(Electric) => Color32::from_rgb(240, 210, 60),
        Some(Ice) => Color32::from_rgb(140, 220, 240),
        Some(Metal) => Color32::from_rgb(160, 165, 175),
        Some(Dark) => Color32::from_rgb(110, 80, 150),
        Some(Light) => Color32::from_rgb(250, 230, 150),
        Some(War) => Color32::from_rgb(180, 60, 60),
        Some(Pure) => Color32::from_rgb(230, 230, 240),
        Some(Legend) => Color32::from_rgb(230, 170, 60),
        Some(Primal) => Color32::from_rgb(200, 110, 60),
        Some(Wind) => Color32::from_rgb(150, 210, 190),
        Some(Time) => Color32::from_rgb(120, 170, 200),
        Some(Happy) => Color32::from_rgb(250, 160, 200),
        Some(Chaos) => Color32::from_rgb(200, 80, 200),
        Some(Magic) => Color32::from_rgb(150, 100, 240),
        Some(Soul) => Color32::from_rgb(120, 220, 200),
        Some(Beauty) => Color32::from_rgb(240, 120, 170),
        Some(Dream) => Color32::from_rgb(180, 150, 240),
        Some(Physical) => Color32::from_rgb(200, 150, 110),
        None => Color32::from_rgb(140, 140, 150),
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// `socialBuildingsCount` -> `Social buildings`.
fn humanize(key: &str) -> String {
    let key = key.strip_suffix("Count").unwrap_or(key);
    let mut out = String::new();
    for (i, c) in key.chars().enumerate() {
        if c.is_uppercase() {
            out.push(' ');
            out.extend(c.to_lowercase());
        } else if i == 0 {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
    }
    out
}
