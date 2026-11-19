//! Project and user configuration for the command line.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{Result, RewindError};

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct Settings {
    pub network: Option<String>,
    pub rpc_urls: Option<Vec<String>>,
    pub timeout: Option<u64>,
    pub retries: Option<u64>,
    pub deadline: Option<u64>,
    pub format: Option<String>,
    pub color: Option<String>,
}

impl Settings {
    pub(crate) fn discover() -> Result<Self> {
        let project = Path::new("sororewind.toml");
        let user = user_path();
        Self::load(project, user.as_deref())
    }

    fn load(project: &Path, user: Option<&Path>) -> Result<Self> {
        let project_settings = Self::read(project)?;
        let user_settings = user.map(Self::read).transpose()?.unwrap_or_default();
        Ok(Self {
            network: project_settings.network.or(user_settings.network),
            rpc_urls: project_settings.rpc_urls.or(user_settings.rpc_urls),
            timeout: project_settings.timeout.or(user_settings.timeout),
            retries: project_settings.retries.or(user_settings.retries),
            deadline: project_settings.deadline.or(user_settings.deadline),
            format: project_settings.format.or(user_settings.format),
            color: project_settings.color.or(user_settings.color),
        })
    }

    fn read(path: &Path) -> Result<Self> {
        let invalid = |reason: String| RewindError::InvalidOption {
            name: "configuration",
            reason: format!("{}: {reason}", path.display()),
        };
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(invalid(error.to_string())),
        };
        let settings = toml::from_str(&text).map_err(|error| {
            let line = error.span().map_or(1, |span| {
                text.as_bytes()[..span.start]
                    .iter()
                    .filter(|&&byte| byte == b'\n')
                    .count()
                    + 1
            });
            // TOML diagnostics can contain the original line, including RPC credentials.
            invalid(format!(
                "invalid TOML, unknown key or incorrect value type at line {line}"
            ))
        })?;
        log::debug!(target: "config", "loaded {}", path.display());
        Ok(settings)
    }
}

fn user_path() -> Option<PathBuf> {
    let absolute = |name| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    absolute("XDG_CONFIG_HOME")
        .or_else(|| absolute("APPDATA"))
        .or_else(|| absolute("HOME").map(|path| path.join(".config")))
        .map(|path| path.join("sororewind").join("sororewind.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_overrides_user_one_setting_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project.toml");
        let user = dir.path().join("user.toml");
        std::fs::write(&project, "timeout = 3\n").unwrap();
        std::fs::write(&user, "timeout = 5\nretries = 0\n").unwrap();
        let settings = Settings::load(&project, Some(&user)).unwrap();
        assert_eq!(settings.timeout, Some(3));
        assert_eq!(settings.retries, Some(0));

        std::fs::write(&project, "token = 'secret'\n").unwrap();
        let error = Settings::load(&project, Some(&user))
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown key"));
        assert!(!error.contains("secret"));
    }
}
