//! Values under the current user's `...\CurrentVersion\Run` key.
//!
//! `anda autostart` registers the daemon here, and the retired `anda_launcher`
//! removes its old entry. The launcher includes this file via `#[path]`, so it
//! must stay free of crate-relative imports.

use std::{ffi::OsStr, io, mem::size_of, os::windows::ffi::OsStrExt, ptr};

use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
    System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ, RegCloseKey,
        RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    },
};

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

/// Returns the command registered under `name`, if any.
pub fn get(name: &str) -> Option<String> {
    let value_name = wide_null(name);
    let Ok(Some(key)) = RegistryKey::open_optional(KEY_QUERY_VALUE) else {
        return None;
    };
    let mut value_type = 0;
    let mut value_len = 0;
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            value_name.as_ptr(),
            ptr::null(),
            &mut value_type,
            ptr::null_mut(),
            &mut value_len,
        )
    };
    if status != ERROR_SUCCESS || value_type != REG_SZ || value_len == 0 {
        return None;
    }
    let mut value = vec![0u16; (value_len as usize).div_ceil(size_of::<u16>())];
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            value_name.as_ptr(),
            ptr::null(),
            &mut value_type,
            value.as_mut_ptr().cast(),
            &mut value_len,
        )
    };
    if status != ERROR_SUCCESS || value_type != REG_SZ {
        return None;
    }
    let end = value.iter().position(|&ch| ch == 0).unwrap_or(value.len());
    let command = String::from_utf16_lossy(&value[..end]);
    (!command.is_empty()).then_some(command)
}

/// Registers `command` to run when the current user logs in.
pub fn set(name: &str, command: &str) -> io::Result<()> {
    if get(name).as_deref() == Some(command) {
        return Ok(());
    }
    let value = wide_null(command);
    let value_name = wide_null(name);
    let key = RegistryKey::create(KEY_SET_VALUE)?;
    let status = unsafe {
        RegSetValueExW(
            key.0,
            value_name.as_ptr(),
            0,
            REG_SZ,
            value.as_ptr().cast::<u8>(),
            (value.len() * size_of::<u16>()) as u32,
        )
    };
    if status == ERROR_SUCCESS {
        return Ok(());
    }
    Err(registry_error("set a login entry", status))
}

/// Removes the login entry `name`; a missing entry is not an error.
pub fn delete(name: &str) -> io::Result<()> {
    let value_name = wide_null(name);
    let Some(key) = RegistryKey::open_optional(KEY_SET_VALUE)? else {
        return Ok(());
    };
    let status = unsafe { RegDeleteValueW(key.0, value_name.as_ptr()) };
    if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    Err(registry_error("delete a login entry", status))
}

struct RegistryKey(HKEY);

impl RegistryKey {
    fn create(access: u32) -> io::Result<Self> {
        let mut key = ptr::null_mut();
        let path = wide_null(RUN_KEY);
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                path.as_ptr(),
                0,
                ptr::null(),
                0,
                access,
                ptr::null(),
                &mut key,
                ptr::null_mut(),
            )
        };
        if status == ERROR_SUCCESS {
            Ok(Self(key))
        } else {
            Err(registry_error("open the HKCU Run key", status))
        }
    }

    fn open_optional(access: u32) -> io::Result<Option<Self>> {
        let mut key = ptr::null_mut();
        let path = wide_null(RUN_KEY);
        let status =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, access, &mut key) };
        if status == ERROR_SUCCESS {
            Ok(Some(Self(key)))
        } else if status == ERROR_FILE_NOT_FOUND {
            Ok(None)
        } else {
            Err(registry_error("open the HKCU Run key", status))
        }
    }
}

impl Drop for RegistryKey {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

fn registry_error(action: &str, code: u32) -> io::Error {
    io::Error::other(format!(
        "Windows registry error while trying to {action}: {code} ({})",
        io::Error::from_raw_os_error(code as i32)
    ))
}

fn wide_null(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}
