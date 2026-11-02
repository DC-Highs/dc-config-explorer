//! Runs the `dc-config-mapper` mapper that matches a configuration section.

use std::sync::Arc;

use dc_config_mapper::{
    ChestsMapper, FogIslandsMapper, GridIslandsMapper, HeroicRacesMapper, ItemsMapper,
    Localization, MazeIslandsMapper, RunnerIslandsMapper, SkillsMapper, TowerIslandsMapper,
};
use serde_json::Value;

/// Sections that have a mapper.
pub const MAPPED_SECTIONS: &[&str] = &[
    "chests",
    "items",
    "skills",
    "heroic_races",
    "maze_island",
    "tower_island",
    "grid_island",
    "fog_island",
    "runner_island",
];

pub fn has_mapper(section: &str) -> bool {
    MAPPED_SECTIONS.contains(&section)
}

/// Maps (restructures, validates and translates) `value`, the raw `section`.
pub fn map_section(section: &str, value: &Value, loc: Arc<Localization>) -> Result<Value, String> {
    fn out<T: serde::Serialize, E: std::fmt::Display>(r: Result<T, E>) -> Result<Value, String> {
        let mapped = r.map_err(|e| e.to_string())?;
        serde_json::to_value(mapped).map_err(|e| e.to_string())
    }

    match section {
        "chests" => out(ChestsMapper::new(loc).map(value)),
        "items" => out(ItemsMapper::new(loc).map(value)),
        "skills" => out(SkillsMapper::new(loc).map(value)),
        "heroic_races" => out(HeroicRacesMapper::new(loc).map(value)),
        "maze_island" => out(MazeIslandsMapper::new(loc).map(value)),
        "tower_island" => out(TowerIslandsMapper::new(loc).map(value)),
        "grid_island" => out(GridIslandsMapper::new(loc).map(value)),
        "fog_island" => out(FogIslandsMapper::new(loc).map(value)),
        "runner_island" => out(RunnerIslandsMapper::new(loc).map(value)),
        other => Err(format!("não há mapper para a seção `{other}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `DC_CONFIG_FIXTURE=/path/to/config.json cargo test -- --nocapture`
    #[test]
    fn maps_every_section_of_a_real_config() {
        let Ok(path) = std::env::var("DC_CONFIG_FIXTURE") else {
            eprintln!("DC_CONFIG_FIXTURE not set, skipping");
            return;
        };
        let config = crate::worker::read_game_config(std::path::Path::new(&path)).expect("valid config file");
        let loc = Arc::new(Localization::new(Default::default(), Default::default()));

        for section in MAPPED_SECTIONS {
            match config.config().section_by_name(section) {
                None => eprintln!("{section}: absent"),
                Some(value) => match map_section(section, value, loc.clone()) {
                    Ok(_) => eprintln!("{section}: ok"),
                    Err(e) => eprintln!("{section}: FAILED {e}"),
                },
            }
        }
    }
}
