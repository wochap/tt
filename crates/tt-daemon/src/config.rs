//! `config.toml`: server endpoint and token, index document, preferences.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::Weekday;
use chrono_tz::Tz;
use rustls_pki_types::CertificateDer;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Absolute path of a PEM file with extra trusted CA certificates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ca_cert: Option<String>,
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
    "server.ca_cert",
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
            "server.ca_cert" => self.server.ca_cert.clone(),
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
        let mut value = value.filter(|value| !value.is_empty());
        if key == "server.ca_cert"
            && let Some(path) = &value
        {
            value = Some(validate_ca_cert(Path::new(path))?.display().to_string());
        }
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
            "server.ca_cert" => &mut self.server.ca_cert,
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

    /// Certificates from `server.ca_cert`; empty when unset.
    pub fn extra_roots(&self) -> Result<Vec<CertificateDer<'static>>> {
        match self.server.ca_cert.as_deref() {
            Some(path) => crate::tls::load_extra_roots(Path::new(path)),
            None => Ok(Vec::new()),
        }
    }

    /// Sync is enabled only with both a server URL and a token. `Err` when
    /// `server.ca_cert` is set but cannot be loaded: then nothing may sync.
    pub fn sync_endpoint(&self) -> Result<Option<SyncEndpoint>, InvalidCaCert> {
        let (Some(url), Some(token)) = (self.server.url.as_deref(), self.server.token.as_deref())
        else {
            return Ok(None);
        };
        let (url, token) = (url.trim(), token.trim());
        if url.is_empty() || token.is_empty() {
            return Ok(None);
        }
        let extra_roots = self.extra_roots().map_err(|error| InvalidCaCert {
            path: self.server.ca_cert.clone().unwrap_or_default(),
            message: format!("{error:#}"),
        })?;
        Ok(Some(SyncEndpoint {
            base: url.trim_end_matches('/').to_owned(),
            websocket: websocket_url(url),
            token: token.to_owned(),
            extra_roots,
        }))
    }
}

/// Canonicalizes `path` and checks it holds at least one PEM certificate.
pub fn validate_ca_cert(path: &Path) -> Result<PathBuf> {
    let absolute = std::fs::canonicalize(path)
        .with_context(|| format!("CA certificate {}", path.display()))?;
    crate::tls::load_extra_roots(&absolute)?;
    Ok(absolute)
}

/// `server.ca_cert` is set but unreadable or holds no certificate.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct InvalidCaCert {
    pub path: String,
    pub message: String,
}

/// Where and as whom the daemon syncs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncEndpoint {
    /// `https://host/base`: HTTP API root (`/api/ws-ticket`).
    pub base: String,
    /// `wss://host/base/sync`.
    pub websocket: String,
    pub token: String,
    /// Certificates trusted on top of the webpki roots (`server.ca_cert`).
    pub extra_roots: Vec<CertificateDer<'static>>,
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
            Ok(Some(SyncEndpoint {
                base: "https://tt.example".into(),
                websocket: "wss://tt.example/sync".into(),
                token: "secret".into(),
                extra_roots: Vec::new(),
            }))
        );
        assert!(config.set("tz", Some("Mars/Base".into())).is_err());
        assert!(config.set("nope", None).is_err());
        assert_eq!(Config::default().sync_endpoint(), Ok(None));
    }

    #[test]
    fn ca_cert_is_validated_and_stored_absolute() {
        let dir = tempfile::tempdir().unwrap();
        let key = rcgen::KeyPair::generate().unwrap();
        let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let ca = params.self_signed(&key).unwrap();
        let ca_path = dir.path().join("ca.pem");
        std::fs::write(&ca_path, ca.pem()).unwrap();
        let notes = dir.path().join("notes.txt");
        std::fs::write(&notes, "not a certificate\n").unwrap();

        let mut config = Config::default();
        config
            .set("server.url", Some("https://tt.example".into()))
            .unwrap();
        config.set("server.token", Some("secret".into())).unwrap();
        // A relative path is stored canonicalized.
        let relative = pathdiff(&ca_path);
        config.set("server.ca_cert", Some(relative)).unwrap();
        let stored = config.get("server.ca_cert").unwrap().unwrap();
        assert_eq!(Path::new(&stored), std::fs::canonicalize(&ca_path).unwrap());
        let endpoint = config.sync_endpoint().unwrap().unwrap();
        assert_eq!(endpoint.extra_roots, vec![ca.der().clone()]);

        // Invalid files are rejected and leave the value unchanged.
        let error = config
            .set("server.ca_cert", Some(notes.display().to_string()))
            .unwrap_err();
        assert!(format!("{error:#}").contains("notes.txt"), "{error:#}");
        let error = config
            .set("server.ca_cert", Some("/nope.pem".into()))
            .unwrap_err();
        assert!(format!("{error:#}").contains("/nope.pem"), "{error:#}");
        assert_eq!(config.get("server.ca_cert").unwrap(), Some(stored.clone()));

        // A CA file removed later makes the endpoint invalid, never webpki-only.
        std::fs::remove_file(&ca_path).unwrap();
        let invalid = config.sync_endpoint().unwrap_err();
        assert_eq!(invalid.path, stored);
        assert!(invalid.message.contains("ca.pem"), "{}", invalid.message);

        config.set("server.ca_cert", None).unwrap();
        assert!(config.sync_endpoint().unwrap().is_some());
    }

    /// `path` relative to the current directory when possible.
    fn pathdiff(path: &Path) -> String {
        let cwd = std::env::current_dir().unwrap();
        let mut relative = PathBuf::new();
        for _ in cwd.components().skip(1) {
            relative.push("..");
        }
        relative.push(path.strip_prefix("/").unwrap());
        relative.display().to_string()
    }
}
