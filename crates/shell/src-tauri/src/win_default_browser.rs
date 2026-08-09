//! HKCU browser registration for Windows.
//!
//! An app registers under `Software\Clients\StartMenuInternet` plus
//! `RegisteredApplications`, then the user picks it in Settings > Default
//! apps. Everything here is per-user (HKCU), so no elevation is needed and
//! re-running registration is idempotent.

use windows::core::PCWSTR;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegGetValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE,
    REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
};
use wis_core::{WisError, WisResult};

/// The ProgID Windows resolves http/https and .html associations to.
const PROG_ID: &str = "WisHTML";
const CLIENT_KEY: &str = r"Software\Clients\StartMenuInternet\Wis";

/// True when the user's https handler resolves to Wis's ProgID.
pub fn is_default() -> bool {
    read_user_choice("https").as_deref() == Some(PROG_ID)
}

/// Write every key Windows needs to list Wis in Settings > Default apps.
pub fn register() -> WisResult<()> {
    let exe = std::env::current_exe().map_err(|err| WisError::InvalidInput {
        message: format!("could not resolve the Wis executable path: {err}"),
    })?;
    let exe = dunce::canonicalize(&exe).unwrap_or(exe);
    let exe = exe.to_string_lossy();
    let open_cmd = format!("\"{exe}\" \"%1\"");
    let icon = format!("{exe},0");

    let prog_id_key = format!(r"Software\Classes\{PROG_ID}");
    set_value(&prog_id_key, "", "Wis HTML Document")?;
    set_value(&format!(r"{prog_id_key}\DefaultIcon"), "", &icon)?;
    set_value(&format!(r"{prog_id_key}\shell\open\command"), "", &open_cmd)?;

    set_value(CLIENT_KEY, "", "Wis")?;
    set_value(&format!(r"{CLIENT_KEY}\DefaultIcon"), "", &icon)?;
    set_value(
        &format!(r"{CLIENT_KEY}\shell\open\command"),
        "",
        &format!("\"{exe}\""),
    )?;
    let capabilities = format!(r"{CLIENT_KEY}\Capabilities");
    set_value(&capabilities, "ApplicationName", "Wis")?;
    set_value(
        &capabilities,
        "ApplicationDescription",
        "Lightweight, secure, fast browser",
    )?;
    set_value(&capabilities, "ApplicationIcon", &icon)?;
    set_value(
        &format!(r"{capabilities}\StartMenu"),
        "StartMenuInternet",
        "Wis",
    )?;

    for scheme in ["http", "https"] {
        set_value(&format!(r"{capabilities}\URLAssociations"), scheme, PROG_ID)?;
    }
    for ext in [".htm", ".html"] {
        set_value(&format!(r"{capabilities}\FileAssociations"), ext, PROG_ID)?;
    }

    set_value(r"Software\RegisteredApplications", "Wis", &capabilities)
}

/// Open Settings > Default apps, where the user completes the switch.
pub fn open_default_apps_settings() -> WisResult<()> {
    use std::os::windows::process::CommandExt;

    // Stops the console-subsystem cmd.exe flashing a window from this
    // GUI-subsystem process.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // `cmd /C start "" <uri>` launches the settings deep link; the empty
    // quoted string is `start`'s window-title argument.
    let mut cmd = std::process::Command::new("cmd");
    cmd.args(["/C", "start", "", "ms-settings:defaultapps"])
        .creation_flags(CREATE_NO_WINDOW);
    cmd.spawn()
        .map(|_| ())
        .map_err(|err| WisError::InvalidInput {
            message: format!("failed to open Default Apps settings: {err}"),
        })
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn reg_err(op: &str, subkey: &str, status: windows::Win32::Foundation::WIN32_ERROR) -> WisError {
    WisError::InvalidInput {
        message: format!("registry {op} of {subkey} failed: {}", status.0),
    }
}

/// Create (or open) `subkey` under HKCU and set one REG_SZ value; an empty
/// `name` sets the key's default value.
fn set_value(subkey: &str, name: &str, data: &str) -> WisResult<()> {
    let subkey_w = wide(subkey);
    let name_w = wide(name);
    let data_w = wide(data);
    let mut key = HKEY::default();

    // SAFETY: all pointers reference NUL-terminated buffers alive on this
    // stack frame; the REG_SZ payload length covers the terminator; the
    // created key handle is closed before returning on every path.
    unsafe {
        let status = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey_w.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        );
        if status != ERROR_SUCCESS {
            return Err(reg_err("create", subkey, status));
        }

        let name_ptr = if name.is_empty() {
            PCWSTR::null()
        } else {
            PCWSTR(name_w.as_ptr())
        };
        let bytes = std::slice::from_raw_parts(data_w.as_ptr().cast::<u8>(), data_w.len() * 2);
        let status = RegSetValueExW(key, name_ptr, None, REG_SZ, Some(bytes));
        let _ = RegCloseKey(key);
        if status != ERROR_SUCCESS {
            return Err(reg_err("write", subkey, status));
        }
    }

    Ok(())
}

fn read_user_choice(scheme: &str) -> Option<String> {
    let subkey = wide(&format!(
        r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\{scheme}\UserChoice"
    ));
    let value = wide("ProgId");
    let mut buf = [0u16; 128];
    let mut size = (std::mem::size_of_val(&buf)) as u32;

    // SAFETY: buffers live for the duration of the call; on success
    // RRF_RT_REG_SZ guarantees NUL termination, and an overflowing value
    // fails with ERROR_MORE_DATA, mapped to None below.
    unsafe {
        let status = RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        );
        if status != ERROR_SUCCESS {
            return None;
        }
    }

    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}
