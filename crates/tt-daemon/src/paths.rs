//! XDG locations.
//!
//! ```text
//! $XDG_CONFIG_HOME/tt/config.toml     configuration (0600)
//! $XDG_CONFIG_HOME/tt/hooks/          hook scripts
//! $XDG_DATA_HOME/tt/tt.db             SQLite store
//! $XDG_DATA_HOME/tt/tt.lock           daemon lock (flock)
//! $XDG_RUNTIME_DIR/tt.sock            socket (fallback $XDG_STATE_HOME/tt/tt.sock)
//! $XDG_STATE_HOME/tt/daemon.log       daemon log
//! ```

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
    pub socket: PathBuf,
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from)
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home().join(fallback))
}

impl Paths {
    /// Resolves from the environment. `TT_SOCKET` overrides the socket path.
    #[must_use]
    pub fn from_env() -> Self {
        let config_dir = xdg("XDG_CONFIG_HOME", ".config").join("tt");
        let data_dir = xdg("XDG_DATA_HOME", ".local/share").join("tt");
        let state_dir = xdg("XDG_STATE_HOME", ".local/state").join("tt");
        let socket = std::env::var_os("TT_SOCKET")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("XDG_RUNTIME_DIR")
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute())
                    .map(|dir| dir.join("tt.sock"))
            })
            .unwrap_or_else(|| state_dir.join("tt.sock"));
        Self {
            config_dir,
            data_dir,
            state_dir,
            socket,
        }
    }

    #[must_use]
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }
    #[must_use]
    pub fn hooks_dir(&self) -> PathBuf {
        self.config_dir.join("hooks")
    }
    #[must_use]
    pub fn database(&self) -> PathBuf {
        self.data_dir.join("tt.db")
    }
    #[must_use]
    pub fn lock_file(&self) -> PathBuf {
        self.data_dir.join("tt.lock")
    }
    #[must_use]
    pub fn log_file(&self) -> PathBuf {
        self.state_dir.join("daemon.log")
    }
}

/// Creates a directory (and parents) with mode 0700.
pub fn ensure_private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
