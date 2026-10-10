//! 凭据存储：macOS 使用系统钥匙串；其他平台使用权限 0600 的本地文件。

use std::path::{Path, PathBuf};

pub struct Secrets {
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    file: PathBuf,
}

const SERVICE: &str = "com.mjgroup.tmuxide";

impl Secrets {
    pub fn new(data_dir: &Path) -> Secrets {
        Secrets {
            file: data_dir.join("credentials.toml"),
        }
    }

    pub fn get(&self, key: &str) -> Option<String> {
        #[cfg(target_os = "macos")]
        {
            keyring::Entry::new(SERVICE, key).ok()?.get_password().ok()
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = SERVICE;
            self.read_file()
                .ok()?
                .get(key)
                .and_then(|v| v.as_str())
                .map(str::to_string)
        }
    }

    pub fn set(&self, key: &str, value: &str) -> anyhow::Result<()> {
        #[cfg(target_os = "macos")]
        {
            let entry = keyring::Entry::new(SERVICE, key)?;
            if value.is_empty() {
                let _ = entry.delete_credential();
            } else {
                entry.set_password(value)?;
            }
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            let mut table = self.read_file().unwrap_or_default();
            if value.is_empty() {
                table.remove(key);
            } else {
                table.insert(key.to_string(), toml::Value::String(value.to_string()));
            }
            let text = toml::to_string(&table)?;
            crate::config::write_atomic(&self.file, text.as_bytes())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&self.file, std::fs::Permissions::from_mode(0o600))?;
            }
            Ok(())
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn read_file(&self) -> anyhow::Result<toml::Table> {
        Ok(toml::from_str(&std::fs::read_to_string(&self.file)?)?)
    }
}
