//! `config.toml`: server endpoint and token, index document, preferences.

use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::Weekday;
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UserConfig {
    /// bs58check id of the index document.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_doc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub server: ServerConfig,
    pub user: UserConfig,
    /// `mon` (default) … `sun`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub week_start: Option<String>,
    /// Default rounding grid for exports, e.g. `15m`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snap: Option<String>,
    /// IANA zone; unset or `local` means the system zone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tz: Option<String>,
    /// Editor command overriding `$EDITOR`/`$VISUAL`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,
}

/// Keys accepted by `tt config get|set`.
pub const KEYS: &[&str] = &[
    "server.url",
    "server.token",
    "user.index_doc",
    "user.id",
    "user.name",
    "week_start",
    "snap",
    "tz",
    "editor",
];

impl Config {
    /// Loads the file; a missing file is the default config.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
        }
    }

    /// Writes atomically with mode 0600.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            crate::paths::ensure_private_dir(parent)?;
        }
        let text = toml::to_string_pretty(self)?;
        let temporary = path.with_extension(format!("toml.tmp-{}", std::process::id()));
        {
            use std::io::Write;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&temporary, path)?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(match key {
            "server.url" => self.server.url.clone(),
            "server.token" => self.server.token.clone(),
            "user.index_doc" => self.user.index_doc.clone(),
            "user.id" => self.user.id.clone(),
            "user.name" => self.user.name.clone(),
            "week_start" => self.week_start.clone(),
            "snap" => self.snap.clone(),
            "tz" => self.tz.clone(),
            "editor" => self.editor.clone(),
            _ => bail!("unknown config key {key:?} (known: {})", KEYS.join(", ")),
        })
    }

    /// Sets (or with `None`, unsets) a key, validating values.
    pub fn set(&mut self, key: &str, value: Option<String>) -> Result<()> {
        let value = value.filter(|value| !value.is_empty());
        if let Some(value) = &value {
            match key {
                "week_start" => {
                    tt_core::time::parse_weekday(value).map_err(|e| anyhow::anyhow!("{e}"))?;
                }
                "snap" => {
                    tt_core::time::parse_duration(value).map_err(|e| anyhow::anyhow!("{e}"))?;
                }
                "tz" if value != "local" => {
                    value
                        .parse::<Tz>()
                        .map_err(|_| anyhow::anyhow!("unknown time zone {value:?}"))?;
                }
                "server.url" if !value.starts_with("http://") && !value.starts_with("https://") => {
                    bail!("server.url must start with http:// or https://");
                }
                _ => {}
            }
        }
        let slot = match key {
            "server.url" => &mut self.server.url,
            "server.token" => &mut self.server.token,
            "user.index_doc" => &mut self.user.index_doc,
            "user.id" => &mut self.user.id,
            "user.name" => &mut self.user.name,
            "week_start" => &mut self.week_start,
            "snap" => &mut self.snap,
            "tz" => &mut self.tz,
            "editor" => &mut self.editor,
            _ => bail!("unknown config key {key:?} (known: {})", KEYS.join(", ")),
        };
        *slot = value;
        Ok(())
    }

    /// Effective time zone.
    #[must_use]
    pub fn time_zone(&self) -> Tz {
        let system = std::env::var("TZ")
            .ok()
            .or_else(|| iana_time_zone::get_timezone().ok());
        tt_core::time::resolve_tz(self.tz.as_deref(), system.as_deref())
    }

    #[must_use]
    pub fn week_start(&self) -> Weekday {
        self.week_start
            .as_deref()
            .and_then(|value| tt_core::time::parse_weekday(value).ok())
            .unwrap_or(Weekday::Mon)
    }

    /// Sync is enabled only with both a server URL and a token.
    #[must_use]
    pub fn sync_endpoint(&self) -> Option<(String, String)> {
        let url = self.server.url.as_deref()?.trim();
        let token = self.server.token.as_deref()?.trim();
        if url.is_empty() || token.is_empty() {
            return None;
        }
        Some((websocket_url(url), token.to_owned()))
    }
}

/// `https://host/base` → `wss://host/base/sync`.
#[must_use]
pub fn websocket_url(url: &str) -> String {
    let url = url.trim_end_matches('/');
    let converted = if let Some(rest) = url.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = url.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        url.to_owned()
    };
    if converted.ends_with("/sync") {
        converted
    } else {
        format!("{converted}/sync")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_with_restricted_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tt/config.toml");
        let mut config = Config::default();
        config
            .set("server.url", Some("https://tt.example".into()))
            .unwrap();
        config.set("server.token", Some("secret".into())).unwrap();
        config.set("tz", Some("Europe/Berlin".into())).unwrap();
        config.save(&path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded, config);
        assert_eq!(
            loaded.sync_endpoint(),
            Some(("wss://tt.example/sync".into(), "secret".into()))
        );
        assert!(config.set("tz", Some("Mars/Base".into())).is_err());
        assert!(config.set("nope", None).is_err());
        assert!(Config::default().sync_endpoint().is_none());
    }
}
