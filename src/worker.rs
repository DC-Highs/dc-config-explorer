//! Background work (network, disk, mapping) on a tokio runtime, so the window never waits.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use dc_config::{
    Config, ConfigLanguage, ConfigOptions, ConfigPlatform, CreateRawOptions, FetchOptions,
    GameConfig,
};
use dc_config_mapper::Localization;
use dc_localization::{ComparisonResult, LocalizationData};
use dc_statistics::GameStatistics;
use serde_json::Value;

use crate::mapping;

pub enum Job {
    LoadFile {
        path: PathBuf,
        language: ConfigLanguage,
        platform: ConfigPlatform,
    },
    FetchRaw {
        url: String,
        language: ConfigLanguage,
        platform: ConfigPlatform,
    },
    Fetch {
        url: String,
        user_id: String,
        auth_token: String,
        language: ConfigLanguage,
        platform: ConfigPlatform,
    },
    Map {
        section: String,
        value: Arc<Value>,
        language: ConfigLanguage,
    },
    SaveJson {
        path: PathBuf,
        value: Arc<Value>,
        pretty: bool,
    },
    LoadLocalization {
        language: ConfigLanguage,
    },
    /// Compares a saved localization file (old) with the current download (new).
    CompareLocalization {
        path: PathBuf,
        language: ConfigLanguage,
    },
    SaveLocalization {
        path: PathBuf,
        localization: Arc<Localization>,
    },
}

/// A loaded configuration and the statistics `dc-statistics` generated for it.
pub struct Loaded {
    pub config: Config,
    pub statistics: GameStatistics,
}

pub enum Event {
    ConfigLoaded {
        label: String,
        result: Result<Box<Loaded>, String>,
    },
    Mapped {
        section: String,
        language: ConfigLanguage,
        result: Result<Value, String>,
    },
    Saved(Result<PathBuf, String>),
    Localization {
        language: ConfigLanguage,
        result: Result<Arc<Localization>, String>,
    },
    Compared {
        path: PathBuf,
        result: Result<ComparisonResult, String>,
    },
}

pub struct Worker {
    runtime: tokio::runtime::Runtime,
    sender: Sender<Event>,
    pub receiver: Receiver<Event>,
    ctx: egui::Context,
    localizations: Arc<Mutex<HashMap<ConfigLanguage, Arc<Localization>>>>,
}

impl Worker {
    pub fn new(ctx: egui::Context) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("dc-worker")
            .build()
            .expect("failed to start the tokio runtime");
        let (sender, receiver) = channel();

        Self {
            runtime,
            sender,
            receiver,
            ctx,
            localizations: Arc::default(),
        }
    }

    pub fn spawn(&self, job: Job) {
        let sender = self.sender.clone();
        let ctx = self.ctx.clone();
        let localizations = self.localizations.clone();

        self.runtime.spawn(async move {
            let event = run(job, localizations).await;
            let _ = sender.send(event);
            ctx.request_repaint();
        });
    }
}

async fn run(job: Job, localizations: Arc<Mutex<HashMap<ConfigLanguage, Arc<Localization>>>>) -> Event {
    match job {
        Job::LoadFile { path, language, platform } => {
            let label = path.display().to_string();
            let result = tokio::task::spawn_blocking(move || read_game_config(&path))
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r)
                .map(|data| {
                    Config::new(ConfigOptions {
                        language,
                        platform,
                        filter: None,
                        data,
                    })
                });
            let result = with_statistics(result).await;
            Event::ConfigLoaded { label, result }
        }
        Job::FetchRaw { url, language, platform } => {
            let result = Config::create_raw(CreateRawOptions::new(url.clone()).language(language).platform(platform))
                .await
                .map_err(|e| e.to_string());
            let result = with_statistics(result).await;
            Event::ConfigLoaded { label: url, result }
        }
        Job::Fetch { url, user_id, auth_token, language, platform } => {
            let options = FetchOptions::new(url.clone(), user_id, auth_token)
                .language(language)
                .platform(platform);
            let result = Config::create(options).await.map_err(|e| e.to_string());
            let result = with_statistics(result).await;
            Event::ConfigLoaded { label: url, result }
        }
        Job::Map { section, value, language } => {
            let result = async {
                let loc = localization(&localizations, language).await?;
                let name = section.clone();
                tokio::task::spawn_blocking(move || mapping::map_section(&name, &value, loc))
                    .await
                    .map_err(|e| e.to_string())?
            }
            .await;
            Event::Mapped { section, language, result }
        }
        Job::SaveJson { path, value, pretty } => {
            let result = tokio::task::spawn_blocking(move || {
                let bytes = if pretty {
                    serde_json::to_vec_pretty(&*value)
                } else {
                    serde_json::to_vec(&*value)
                }
                .map_err(|e| e.to_string())?;
                fastframe_file::write(&path, bytes).map_err(|e| e.to_string())?;
                Ok(path)
            })
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r);
            Event::Saved(result)
        }
        Job::LoadLocalization { language } => Event::Localization {
            language,
            result: localization(&localizations, language).await,
        },
        Job::CompareLocalization { path, language } => {
            let result = async {
                let new = localization(&localizations, language).await?;
                let file = path.clone();
                tokio::task::spawn_blocking(move || {
                    let old = Localization::new(language, read_localization_file(&file)?);
                    old.compare(&new).map_err(|e| e.to_string())
                })
                .await
                .map_err(|e| e.to_string())?
            }
            .await;
            Event::Compared { path, result }
        }
        Job::SaveLocalization { path, localization } => {
            let result = tokio::task::spawn_blocking(move || {
                // Sorted, so two exports diff cleanly.
                let sorted: std::collections::BTreeMap<_, _> = localization.data.iter().collect();
                let bytes = serde_json::to_vec_pretty(&sorted).map_err(|e| e.to_string())?;
                fastframe_file::write(&path, bytes).map_err(|e| e.to_string())?;
                Ok(path)
            })
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r);
            Event::Saved(result)
        }
    }
}

/// Runs `dc-statistics` over the loaded sections (moved in and out, never copied).
async fn with_statistics(config: Result<Config, String>) -> Result<Box<Loaded>, String> {
    let mut config = config?;
    tokio::task::spawn_blocking(move || {
        let sections = std::mem::take(&mut config.data.game_data.config.0);
        let root = Value::Object(sections);
        let statistics = dc_statistics::generate_statistics(&root);
        if let Value::Object(sections) = root {
            config.data.game_data.config.0 = sections;
        }
        Box::new(Loaded { config, statistics })
    })
    .await
    .map_err(|e| e.to_string())
}

/// A localization file: the game's format (an array of small `{key: value}` objects)
/// or a flat `{key: value}` object, as this app exports it.
fn read_localization_file(path: &std::path::Path) -> Result<LocalizationData, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    if let Ok(parts) = serde_json::from_str::<Vec<LocalizationData>>(&text) {
        return Ok(parts.into_iter().flatten().collect());
    }
    serde_json::from_str::<LocalizationData>(&text)
        .map_err(|_| "formato de localização não reconhecido (esperado array de objetos ou objeto chave→valor)".into())
}

/// Reads a `{ "game_data": { "config": { ... } } }` file, or a file holding only the
/// sections object (as some tools save it), which is wrapped.
pub(crate) fn read_game_config(path: &std::path::Path) -> Result<GameConfig, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let value: Value = serde_json::from_str(&text).map_err(|e| format!("JSON inválido: {e}"))?;

    let is_wrapped = value.get("game_data").and_then(|g| g.get("config")).is_some();
    let value = if is_wrapped {
        value
    } else if value.is_object() {
        serde_json::json!({ "game_data": { "config": value } })
    } else {
        return Err("o arquivo não contém um objeto de configuração".into());
    };

    serde_json::from_value(value).map_err(|e| e.to_string())
}

/// The localization for `language`, downloaded once and shared.
async fn localization(
    cache: &Mutex<HashMap<ConfigLanguage, Arc<Localization>>>,
    language: ConfigLanguage,
) -> Result<Arc<Localization>, String> {
    if let Some(loc) = cache.lock().unwrap().get(&language) {
        return Ok(loc.clone());
    }

    let loc = Arc::new(
        Localization::create(language)
            .await
            .map_err(|e| format!("falha ao baixar a localização ({language}): {e}"))?,
    );
    cache.lock().unwrap().insert(language, loc.clone());
    Ok(loc)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `DC_CONFIG_FIXTURE=/path/to/config.json cargo test -- --nocapture`
    #[test]
    fn loads_a_real_config_with_statistics() {
        let Ok(path) = std::env::var("DC_CONFIG_FIXTURE") else { return };
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let config = read_game_config(std::path::Path::new(&path)).map(|data| {
            Config::new(ConfigOptions {
                language: ConfigLanguage::English,
                platform: ConfigPlatform::Android,
                filter: None,
                data,
            })
        });
        let loaded = runtime.block_on(with_statistics(config)).unwrap();
        let s = &loaded.statistics;
        eprintln!(
            "sections={} dragons={} items={} islands={} max_level={}",
            loaded.config.data.config().keys().count(),
            s.dragons.total_count,
            s.items.total_count,
            s.islands.total_count,
            s.max_level
        );
        assert!(loaded.config.data.config().keys().count() > 0, "sections survive statistics");
        assert!(s.dragons.total_count > 0);
    }

    /// Network: `DC_NETWORK_TESTS=1 cargo test -- --nocapture`
    #[test]
    fn downloads_and_compares_localization() {
        if std::env::var("DC_NETWORK_TESTS").is_err() {
            return;
        }
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let cache = Mutex::default();
        let loc = runtime.block_on(localization(&cache, ConfigLanguage::BrazilianPortuguese)).unwrap();
        eprintln!("keys={} dragon 1000 = {:?}", loc.data.len(), loc.get_dragon_name(1000));
        assert!(!loc.data.is_empty());

        let dir = std::env::temp_dir().join("dc-config-explorer-test.json");
        let sorted: std::collections::BTreeMap<_, _> = loc.data.iter().collect();
        std::fs::write(&dir, serde_json::to_vec(&sorted).unwrap()).unwrap();
        let old = Localization::new(loc.language, read_localization_file(&dir).unwrap());
        let diff = old.compare(&loc).unwrap();
        assert!(diff.new_fields.is_empty() && diff.edited_fields.is_empty() && diff.deleted_fields.is_empty());
        let _ = std::fs::remove_file(dir);
    }
}
