//! The bootstrap process owns a kill-on-close job. Only its AppContainer
//! child reads the script. Both use the same inherited protocol pipes.
use anda_core::BoxError;
use std::{
    ffi::OsStr,
    io,
    mem::{size_of, size_of_val},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation, WAIT_OBJECT_0},
    Security::{
        FreeSid, GetTokenInformation, Isolation::DeriveAppContainerSidFromAppContainerName, PSID,
        SECURITY_CAPABILITIES, TOKEN_QUERY, TokenIsAppContainer,
    },
    System::{Console::*, JobObjects::*, Threading::*},
};

fn checked(ok: i32) -> io::Result<()> {
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}
struct Sid(PSID);
impl Drop for Sid {
    fn drop(&mut self) {
        unsafe {
            FreeSid(self.0);
        }
    }
}
struct Attributes(Vec<usize>);
impl Attributes {
    fn new(count: u32) -> io::Result<Self> {
        let mut bytes = 0;
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), count, 0, &mut bytes);
        }
        let mut buf = vec![0usize; bytes.div_ceil(size_of::<usize>())];
        checked(unsafe {
            InitializeProcThreadAttributeList(buf.as_mut_ptr().cast(), count, 0, &mut bytes)
        })?;
        Ok(Self(buf))
    }
    // Windows borrows each value through CreateProcessW. All four values
    // below are local to the bootstrap and outlive the attribute list.
    fn add<T>(&mut self, key: u32, value: &T) -> io::Result<()> {
        checked(unsafe {
            UpdateProcThreadAttribute(
                self.0.as_mut_ptr().cast(),
                0,
                key as usize,
                std::ptr::from_ref(value).cast(),
                size_of_val(value),
                null_mut(),
                null_mut(),
            )
        })
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.0.as_mut_ptr().cast()) }
    }
}

pub(super) fn enter() -> Result<(), BoxError> {
    // Each successful Win32 call initializes the indicated output handle or
    // buffer. OwnedHandle closes acquired handles on every return path.
    unsafe {
        let mut token = null_mut();
        checked(OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY,
            &mut token,
        ))?;
        let token = OwnedHandle::from_raw_handle(token);
        let mut confined = 0u32;
        let mut bytes = 0;
        checked(GetTokenInformation(
            token.as_raw_handle(),
            TokenIsAppContainer,
            (&mut confined as *mut u32).cast(),
            4,
            &mut bytes,
        ))?;
        if confined != 0 {
            let mut in_job = 0;
            checked(IsProcessInJob(GetCurrentProcess(), null_mut(), &mut in_job))?;
            if in_job == 0 {
                return Err("Code Mode worker is missing its job limits".into());
            }
            return Ok(());
        }
        let job = CreateJobObjectW(null(), null());
        if job.is_null() {
            return Err(io::Error::last_os_error().into());
        }
        let job = OwnedHandle::from_raw_handle(job);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_PROCESS_TIME
            | JOB_OBJECT_LIMIT_PROCESS_MEMORY
            | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
            | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        limits.BasicLimitInformation.ActiveProcessLimit = 1;
        limits.BasicLimitInformation.PerProcessUserTimeLimit = 5 * 10_000_000;
        limits.ProcessMemoryLimit = 256 * 1024 * 1024;
        checked(SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            std::ptr::from_ref(&limits).cast(),
            size_of_val(&limits) as u32,
        ))?;

        let mut sid = null_mut();
        let moniker = wide(OsStr::new(&format!(
            "Anda.CodeMode.{}",
            ic_auth_types::Xid::new()
        )));
        let profile = std::env::var_os("ANDA_CI_PROFILE_PROBE").is_some();
        let hr = if profile {
            windows_sys::Win32::Security::Isolation::CreateAppContainerProfile(
                moniker.as_ptr(),
                moniker.as_ptr(),
                moniker.as_ptr(),
                null(),
                0,
                &mut sid,
            )
        } else {
            DeriveAppContainerSidFromAppContainerName(moniker.as_ptr(), &mut sid)
        };
        if hr < 0 {
            return Err(format!("AppContainer SID creation failed ({hr:#x})").into());
        }
        let sid = Sid(sid);
        let capabilities = SECURITY_CAPABILITIES {
            AppContainerSid: sid.0,
            ..Default::default()
        };
        let handles: [HANDLE; 3] = [
            GetStdHandle(STD_INPUT_HANDLE),
            GetStdHandle(STD_OUTPUT_HANDLE),
            GetStdHandle(STD_ERROR_HANDLE),
        ];
        for h in handles {
            checked(SetHandleInformation(
                h,
                HANDLE_FLAG_INHERIT,
                HANDLE_FLAG_INHERIT,
            ))?;
        }
        let job_list = [job.as_raw_handle()];
        let opt_out = 1u32; // PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT (LPAC).
        let mut attributes = Attributes::new(4)?;
        attributes.add(PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, &capabilities)?;
        attributes.add(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, &handles)?;
        attributes.add(PROC_THREAD_ATTRIBUTE_JOB_LIST, &job_list)?;
        attributes.add(
            PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY,
            &opt_out,
        )?;
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = size_of_val(&startup) as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        [
            startup.StartupInfo.hStdInput,
            startup.StartupInfo.hStdOutput,
            startup.StartupInfo.hStdError,
        ] = handles;
        startup.lpAttributeList = attributes.0.as_mut_ptr().cast();
        let application = wide(std::env::current_exe()?.as_os_str());
        let mut command = wide(OsStr::new(&format!("anda {}", super::super::worker::ARG)));

        let mut env = Vec::new();
        if let Ok(names) = std::env::var("ANDA_CI_ENV_PROBE") {
            for name in names.split(',') {
                if let Some(value) = std::env::var_os(name) {
                    let mut entry = std::ffi::OsString::from(name);
                    entry.push("=");
                    entry.push(value);
                    env.extend(wide(&entry));
                }
            }
        }
        if env.is_empty() {
            env.push(0);
        }
        env.push(0);
        let cwd = std::env::var_os("ANDA_CI_SYSTEM_CWD")
            .and_then(|_| std::env::var_os("SystemRoot"))
            .map(|path| wide(&path));
        let mut info = PROCESS_INFORMATION::default();
        if CreateProcessW(
            application.as_ptr(),
            command.as_mut_ptr(),
            null(),
            null(),
            1,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            env.as_ptr().cast(),
            cwd.as_ref().map_or(null(), |path| path.as_ptr()),
            &startup.StartupInfo,
            &mut info,
        ) == 0
        {
            let err = io::Error::last_os_error();
            return Err(format!("CreateProcessW: {err}").into());
        }
        let child = OwnedHandle::from_raw_handle(info.hProcess);
        drop(OwnedHandle::from_raw_handle(info.hThread));
        if WaitForSingleObject(child.as_raw_handle(), INFINITE) != WAIT_OBJECT_0 {
            return Err(io::Error::last_os_error().into());
        }
        let mut code = 0;
        checked(GetExitCodeProcess(child.as_raw_handle(), &mut code))?;
        std::process::exit(code as i32)
    }
}
