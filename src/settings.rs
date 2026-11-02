//! Persisted settings, stored in the app's config folder (via `fastframe-dirs`)
//! and written atomically off the UI thread (via `fastframe-file`).

use std::path::PathBuf;

use dc_config::{ConfigLanguage, ConfigPlatform};
use fastframe_dirs::{App, Dirs};
use fastframe_file::{Access, Writer};
use serde::{Deserialize, Serialize};

pub const DIRS: App = App::new("dc-config-explorer", "dev", "dchighs", "dc-config-explorer");

const MAX_RECENT: usize = 8;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceKind {
    #[default]
    File,
    RawUrl,
    Authenticated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub source: SourceKind,
    pub file_path: String,
    pub raw_url: String,
    pub api_url: String,
    pub user_id: String,
    pub remember_token: bool,
    pub language: ConfigLanguage,
    pub platform: ConfigPlatform,
    pub dark_mode: bool,
    pub recent_files: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            source: SourceKind::File,
            file_path: String::new(),
            raw_url: String::new(),
            api_url: String::new(),
            user_id: String::new(),
            remember_token: false,
            language: ConfigLanguage::default(),
            platform: ConfigPlatform::default(),
            dark_mode: true,
            recent_files: Vec::new(),
        }
    }
}

impl Settings {
    pub fn push_recent(&mut self, path: &str) {
        self.recent_files.retain(|p| p != path);
        self.recent_files.insert(0, path.to_owned());
        self.recent_files.truncate(MAX_RECENT);
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Credentials {
    auth_token: String,
}

/// Loads and saves [`Settings`] (and, when asked, the auth token in a private file).
pub struct Store {
    dirs: Option<Dirs>,
    writer: Writer,
}

impl Store {
    pub fn new(dirs: Option<Dirs>) -> Self {
        Self {
            dirs,
            writer: Writer::new(),
        }
    }

    fn settings_file(&self) -> Option<PathBuf> {
        self.dirs.as_ref().map(|d| d.config.join("settings.json"))
    }

    fn credentials_file(&self) -> Option<PathBuf> {
        self.dirs.as_ref().map(|d| d.config.join("credentials.json"))
    }

    pub fn load(&self) -> (Settings, String) {
        let settings = self
            .settings_file()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();

        let token = self
            .credentials_file()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| serde_json::from_str::<Credentials>(&text).ok())
            .map(|c| c.auth_token)
            .unwrap_or_default();

        (settings, token)
    }

    pub fn save(&self, settings: &Settings, token: &str) {
        if let (Some(path), Ok(bytes)) = (self.settings_file(), serde_json::to_vec_pretty(settings)) {
            self.writer.write(&path, bytes, Access::Default);
        }

        if let Some(path) = self.credentials_file() {
            let credentials = Credentials {
                auth_token: if settings.remember_token { token.to_owned() } else { String::new() },
            };
            if let Ok(bytes) = serde_json::to_vec(&credentials) {
                self.writer.write(&path, bytes, Access::Private);
            }
        }
    }

    pub fn flush(&self) {
        self.writer.flush(fastframe_file::FLUSH_TIMEOUT);
    }
}
