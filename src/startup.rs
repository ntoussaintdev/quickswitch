//! "Start with Windows": a per-user `HKCU\...\Run` value named `QuickSwitch`
//! (plan §7.2). Off by default; never needs administrator rights.

use std::path::Path;

pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const VALUE_NAME: &str = "QuickSwitch";
pub const BACKGROUND_ARG: &str = "--background";

/// The Run value data: absolute exe path, always quoted, plus `--background`.
pub fn run_command(exe: &Path) -> String {
    format!("\"{}\" {BACKGROUND_ARG}", exe.display())
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, WIN32_ERROR};
    use windows::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
    };
    use windows::core::HSTRING;

    fn check(err: WIN32_ERROR) -> Result<(), String> {
        if err.is_ok() {
            Ok(())
        } else {
            Err(windows::core::Error::from(err.to_hresult()).message())
        }
    }

    /// The current Run value, or `None` when startup is disabled.
    pub fn current_command() -> Result<Option<String>, String> {
        let key = HSTRING::from(RUN_KEY);
        let name = HSTRING::from(VALUE_NAME);
        let mut size: u32 = 0;
        let err = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                &key,
                &name,
                RRF_RT_REG_SZ,
                None,
                None,
                Some(&mut size),
            )
        };
        if err == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        check(err)?;
        let mut buf = vec![0u16; (size as usize).div_ceil(2)];
        let err = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                &key,
                &name,
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut size),
            )
        };
        check(err)?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Ok(Some(String::from_utf16_lossy(&buf[..len])))
    }

    pub fn is_enabled() -> bool {
        matches!(current_command(), Ok(Some(_)))
    }

    /// Enable (writing the given exe path) or disable startup. Disabling
    /// deletes only the QuickSwitch value.
    pub fn set_enabled(enabled: bool, exe: &Path) -> Result<(), String> {
        let key = HSTRING::from(RUN_KEY);
        let name = HSTRING::from(VALUE_NAME);
        if enabled {
            let data: Vec<u16> = run_command(exe).encode_utf16().chain(std::iter::once(0)).collect();
            let err = unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    &key,
                    &name,
                    REG_SZ.0,
                    Some(data.as_ptr().cast()),
                    (data.len() * 2) as u32,
                )
            };
            check(err)
        } else {
            let err = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, &key, &name) };
            if err == ERROR_FILE_NOT_FOUND {
                Ok(())
            } else {
                check(err)
            }
        }
    }
}

#[cfg(windows)]
pub use imp::{current_command, is_enabled, set_enabled};
