use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const PORTABLE_DIR_ENV: &str = "CPAHDOCS_PORTABLE_DIR";
const PORTABLE_MARKER: &str = "portable";
const DATA_DIR_NAME: &str = "Data";
const PROGRAM_DIR_NAME: &str = "Program";
#[cfg(windows)]
const WEBVIEW2_USER_DATA_ENV: &str = "WEBVIEW2_USER_DATA_FOLDER";
#[cfg(windows)]
const WEBVIEW2_DIR_NAME: &str = "WebView2";
const QPDF_DIR_NAME: &str = "qpdf";
const SECRETS_FILE_NAME: &str = "secrets.json";

static PORTABLE_DATA_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
static SECRETS_LOCK: Mutex<()> = Mutex::new(());

pub fn init() {
    let data_dir = PORTABLE_DATA_DIR.get_or_init(|| {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf));
        detect_portable_data_dir(exe_dir.as_deref(), std::env::var_os(PORTABLE_DIR_ENV))
    });

    if let Some(dir) = data_dir {
        let _ = fs::create_dir_all(dir);
        #[cfg(windows)]
        if std::env::var_os(WEBVIEW2_USER_DATA_ENV).is_none() {
            let webview_dir = dir.join(WEBVIEW2_DIR_NAME);
            let _ = fs::create_dir_all(&webview_dir);
            // SAFETY: Called once at the very start of application startup before
            // background threads or WebView2 are initialized.
            unsafe {
                std::env::set_var(WEBVIEW2_USER_DATA_ENV, webview_dir);
            }
        }
    }
}

pub fn data_dir() -> Option<&'static Path> {
    PORTABLE_DATA_DIR.get().and_then(|dir| dir.as_deref())
}

pub fn is_portable() -> bool {
    data_dir().is_some()
}

pub fn qpdf_cache_root() -> Option<PathBuf> {
    data_dir().map(|dir| dir.join(QPDF_DIR_NAME))
}

pub fn read_secret(key: &str) -> Result<Option<String>> {
    let Some(dir) = data_dir() else {
        return Ok(None);
    };
    read_secret_in(dir, key)
}

pub fn write_secret(key: &str, value: &str) -> Result<()> {
    let dir = data_dir().context("便携数据目录未初始化")?;
    write_secret_in(dir, key, value)
}

fn read_secret_in(dir: &Path, key: &str) -> Result<Option<String>> {
    let _guard = SECRETS_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("便携凭据锁异常"))?;
    let path = dir.join(SECRETS_FILE_NAME);
    if !path.is_file() {
        return Ok(None);
    }
    let bytes =
        fs::read(&path).with_context(|| format!("无法读取便携凭据文件：{}", path.display()))?;
    let secrets: BTreeMap<String, String> = serde_json::from_slice(&bytes)
        .with_context(|| format!("便携凭据文件格式无效：{}", path.display()))?;
    Ok(secrets
        .get(key)
        .filter(|secret| !secret.is_empty())
        .cloned())
}

fn write_secret_in(dir: &Path, key: &str, value: &str) -> Result<()> {
    let _guard = SECRETS_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("便携凭据锁异常"))?;
    fs::create_dir_all(dir).with_context(|| format!("无法创建便携数据目录：{}", dir.display()))?;
    let path = dir.join(SECRETS_FILE_NAME);
    let mut secrets: BTreeMap<String, String> = if path.is_file() {
        fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    } else {
        BTreeMap::new()
    };
    if value.is_empty() {
        secrets.remove(key);
    } else {
        secrets.insert(key.to_string(), value.to_string());
    }
    let bytes = serde_json::to_vec_pretty(&secrets).context("无法序列化便携凭据")?;
    crate::atomic_file::write_atomic(&path, &bytes)
        .with_context(|| format!("无法写入便携凭据文件：{}", path.display()))
}

fn detect_portable_data_dir(exe_dir: Option<&Path>, env_dir: Option<OsString>) -> Option<PathBuf> {
    if let Some(dir) = env_dir {
        let candidate = PathBuf::from(dir);
        if !candidate.as_os_str().is_empty() {
            return Some(candidate);
        }
    }

    let exe_dir = exe_dir?;
    if exe_dir.join(PORTABLE_MARKER).exists() {
        return Some(exe_dir.join(DATA_DIR_NAME));
    }

    if exe_dir
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(PROGRAM_DIR_NAME))
        && let Some(root) = exe_dir.parent()
        && root.join(PORTABLE_MARKER).exists()
    {
        return Some(root.join(DATA_DIR_NAME));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_portable_dir_from_environment_first() {
        let temp = tempfile::tempdir().unwrap();
        let env_data = temp.path().join("CustomData");
        let resolved = detect_portable_data_dir(Some(temp.path()), Some(env_data.clone().into()));
        assert_eq!(resolved, Some(env_data));
    }

    #[test]
    fn detects_portable_marker_beside_executable() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join(PORTABLE_MARKER), b"").unwrap();
        let resolved = detect_portable_data_dir(Some(temp.path()), None);
        assert_eq!(resolved, Some(temp.path().join(DATA_DIR_NAME)));
    }

    #[test]
    fn detects_portable_marker_in_parent_of_program_directory() {
        let temp = tempfile::tempdir().unwrap();
        let program_dir = temp.path().join(PROGRAM_DIR_NAME);
        fs::create_dir_all(&program_dir).unwrap();
        fs::write(temp.path().join(PORTABLE_MARKER), b"").unwrap();
        let resolved = detect_portable_data_dir(Some(&program_dir), None);
        assert_eq!(resolved, Some(temp.path().join(DATA_DIR_NAME)));
    }

    #[test]
    fn returns_none_without_marker_or_env() {
        let temp = tempfile::tempdir().unwrap();
        let program_dir = temp.path().join(PROGRAM_DIR_NAME);
        fs::create_dir_all(&program_dir).unwrap();
        assert_eq!(detect_portable_data_dir(Some(&program_dir), None), None);
    }

    #[test]
    fn reads_writes_and_clears_portable_secrets() {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join(DATA_DIR_NAME);
        assert_eq!(read_secret_in(&data_dir, "mineru-token").unwrap(), None);

        write_secret_in(&data_dir, "mineru-token", "secret-123").unwrap();
        assert_eq!(
            read_secret_in(&data_dir, "mineru-token")
                .unwrap()
                .as_deref(),
            Some("secret-123")
        );

        write_secret_in(&data_dir, "mineru-token", "").unwrap();
        assert_eq!(read_secret_in(&data_dir, "mineru-token").unwrap(), None);
    }
}
