use anda_core::BoxError;
use std::{
    ffi::OsStr,
    os::windows::ffi::OsStrExt,
    process::Command,
    ptr::{null, null_mut},
};
use windows_sys::Win32::Security::{
    FreeSid,
    Isolation::{CreateAppContainerProfile, DeleteAppContainerProfile},
};

pub(super) const PROFILE_ENV: &str = "ANDA_CODE_MODE_PROFILE";

pub(super) fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

// The host owns the profile so it can remove it after cancellation kills the bootstrap.
pub(super) struct Profile(String);

impl Profile {
    pub(super) fn create() -> Result<Self, BoxError> {
        let name = format!("Anda.CodeMode.{}", ic_auth_types::Xid::new());
        let moniker = wide(OsStr::new(&name));
        let mut sid = null_mut();
        let hr = unsafe {
            CreateAppContainerProfile(
                moniker.as_ptr(),
                moniker.as_ptr(),
                moniker.as_ptr(),
                null(),
                0,
                &mut sid,
            )
        };
        if hr < 0 {
            return Err(format!("creating Code Mode AppContainer profile failed ({hr:#x})").into());
        }
        unsafe { FreeSid(sid) };
        Ok(Self(name))
    }

    pub(super) fn configure(&self, command: &mut Command) {
        command.env(PROFILE_ENV, &self.0);
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            command.env("LOCALAPPDATA", local_app_data);
        }
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        let moniker = wide(OsStr::new(&self.0));
        let hr = unsafe { DeleteAppContainerProfile(moniker.as_ptr()) };
        if hr < 0 {
            log::warn!("removing Code Mode AppContainer profile failed ({hr:#x})");
        }
    }
}
