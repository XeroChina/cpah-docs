#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const PROGRAM_DIR_NAME: &str = "Program";
const PORTABLE_MARKER: &str = "portable";
const DATA_DIR_NAME: &str = "Data";
const WEBVIEW2_DIR_NAME: &str = "WebView2";
const PORTABLE_DIR_ENV: &str = "CPAHDOCS_PORTABLE_DIR";
const WEBVIEW2_USER_DATA_ENV: &str = "WEBVIEW2_USER_DATA_FOLDER";

#[cfg(windows)]
const TARGET_EXECUTABLE: &str = "cpah-docs.exe";
#[cfg(not(windows))]
const TARGET_EXECUTABLE: &str = "cpah-docs";

#[derive(Debug, PartialEq, Eq)]
struct LaunchPlan {
    executable: PathBuf,
    portable_data_dir: Option<PathBuf>,
}

fn build_launch_plan(root_dir: &Path) -> LaunchPlan {
    let executable = root_dir.join(PROGRAM_DIR_NAME).join(TARGET_EXECUTABLE);
    let portable_data_dir = if root_dir.join(PORTABLE_MARKER).exists() {
        Some(root_dir.join(DATA_DIR_NAME))
    } else {
        None
    };
    LaunchPlan {
        executable,
        portable_data_dir,
    }
}

fn run() -> Result<(), String> {
    let current_exe = env::current_exe().map_err(|error| format!("无法获取启动器路径：{error}"))?;
    let root_dir = current_exe
        .parent()
        .ok_or_else(|| "无法解析启动器所在目录。".to_string())?;
    let plan = build_launch_plan(root_dir);

    if !plan.executable.is_file() {
        return Err(format!(
            "找不到主程序：{}\n请确认 Program 目录完整。",
            plan.executable.display()
        ));
    }

    let mut command = Command::new(&plan.executable);
    command.args(env::args_os().skip(1));

    if let Some(data_dir) = plan.portable_data_dir {
        let webview_dir = data_dir.join(WEBVIEW2_DIR_NAME);
        fs::create_dir_all(&webview_dir)
            .map_err(|error| format!("无法创建便携数据目录 {}：{error}", data_dir.display()))?;
        command.env(PORTABLE_DIR_ENV, &data_dir);
        command.env(WEBVIEW2_USER_DATA_ENV, &webview_dir);
    }

    command
        .spawn()
        .map_err(|error| format!("无法启动主程序 {}：{error}", plan.executable.display()))?;
    Ok(())
}

#[cfg(windows)]
fn show_error(message: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(
            hwnd: *mut std::ffi::c_void,
            text: *const u16,
            caption: *const u16,
            flags: u32,
        ) -> i32;
    }

    const MB_ICONERROR: u32 = 0x0000_0010;
    let text: Vec<u16> = OsStr::new(message)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let caption: Vec<u16> = OsStr::new("CPAH Docs")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: Both pointers are valid null-terminated UTF-16 strings for the call duration.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
fn show_error(message: &str) {
    eprintln!("CPAH Docs: {message}");
}

fn main() -> ExitCode {
    if let Err(message) = run() {
        show_error(&message);
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!("cpah-launcher-{name}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn enables_portable_mode_when_marker_exists() {
        let root = unique_temp_dir("portable");
        fs::write(root.join(PORTABLE_MARKER), b"").unwrap();
        let plan = build_launch_plan(&root);
        assert_eq!(
            plan,
            LaunchPlan {
                executable: root.join(PROGRAM_DIR_NAME).join(TARGET_EXECUTABLE),
                portable_data_dir: Some(root.join(DATA_DIR_NAME)),
            }
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn disables_portable_mode_without_marker() {
        let root = unique_temp_dir("standard");
        let plan = build_launch_plan(&root);
        assert_eq!(
            plan,
            LaunchPlan {
                executable: root.join(PROGRAM_DIR_NAME).join(TARGET_EXECUTABLE),
                portable_data_dir: None,
            }
        );
        let _ = fs::remove_dir_all(root);
    }
}
