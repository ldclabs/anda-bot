use anda_core::BoxError;

#[cfg(unix)]
fn limit(resource: libc::c_int, value: libc::rlim_t) -> std::io::Result<()> {
    let limits = libc::rlimit {
        rlim_cur: value,
        rlim_max: value,
    };
    // Both fields are initialized, and the pointer is valid for this call.
    if unsafe { libc::setrlimit(resource as _, &limits) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn limit_address_space() -> Result<(), BoxError> {
    // RLIMIT_AS includes the executable and shared libraries. Account for
    // the startup mappings and fit new mappings in the remaining budget.
    let statm = std::fs::read_to_string("/proc/self/statm")?;
    let page_size = libc::rlim_t::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) })?;
    let mut pages = statm
        .split_ascii_whitespace()
        .map(str::parse::<libc::rlim_t>);
    let virtual_size = pages.next().ok_or("missing worker virtual size")?? * page_size;
    let resident = pages.next().ok_or("missing worker resident size")?? * page_size;
    let memory_budget: libc::rlim_t = 256 * 1024 * 1024;
    let remaining = memory_budget
        .checked_sub(resident)
        .ok_or("worker exceeds its memory limit at startup")?;
    let address_limit = virtual_size
        .checked_add(remaining)
        .ok_or("worker address space limit overflowed")?;
    limit(libc::RLIMIT_AS as _, address_limit)?;
    Ok(())
}

pub(super) fn enter() -> Result<(), BoxError> {
    #[cfg(unix)]
    {
        // A daemon dependency may have opened a descriptor without CLOEXEC.
        // The dedicated worker owns only stdin, stdout and stderr.
        #[cfg(target_os = "linux")]
        let close_individually =
            unsafe { libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 0u32) } < 0;
        #[cfg(not(target_os = "linux"))]
        let close_individually = true;
        if close_individually {
            for fd in 3..unsafe { libc::getdtablesize() } {
                unsafe {
                    libc::close(fd);
                }
            }
        }
        limit(libc::RLIMIT_CPU as _, 5)?;
        limit(libc::RLIMIT_CORE as _, 0)?;
        #[cfg(target_os = "linux")]
        limit_address_space()?;
        #[cfg(target_os = "macos")]
        {
            // dyld's shared cache occupies hundreds of GiB of virtual address
            // space at entry. Bound additional mappings against that baseline.
            let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::uninit();
            let size = std::mem::size_of::<libc::proc_taskinfo>() as i32;
            if unsafe {
                libc::proc_pidinfo(
                    libc::getpid(),
                    libc::PROC_PIDTASKINFO,
                    0,
                    info.as_mut_ptr().cast(),
                    size,
                )
            } != size
            {
                return Err(std::io::Error::last_os_error().into());
            }
            let info = unsafe { info.assume_init() };
            let budget = (256u64 * 1024 * 1024)
                .checked_sub(info.pti_resident_size)
                .ok_or("worker exceeds its memory limit at startup")?;
            limit(libc::RLIMIT_AS as _, info.pti_virtual_size + budget)?;
        }
    }
    platform()
}

#[cfg(target_os = "macos")]
fn platform() -> Result<(), BoxError> {
    use std::ffi::{CStr, c_char};
    #[link(name = "sandbox")]
    unsafe extern "C" {
        fn sandbox_init(profile: *const c_char, flags: u64, error: *mut *mut c_char) -> i32;
        fn sandbox_free_error(error: *mut c_char);
    }
    // No filesystem, networking, process creation or new Mach services. The
    // interpreter and protocol pipes are already loaded before applying this.
    let profile = c"(version 1)(deny default)(allow sysctl-read)(allow signal (target self))";
    let mut error = std::ptr::null_mut();
    if unsafe { sandbox_init(profile.as_ptr(), 0, &mut error) } == 0 {
        return Ok(());
    }
    if error.is_null() {
        return Err("macOS Code Mode sandbox unavailable".into());
    }
    let message = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { sandbox_free_error(error) };
    Err(message.into())
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
fn platform() -> Result<(), BoxError> {
    use libc::*;
    // Check the syscall ABI before the allowlist. All I/O is through the
    // inherited descriptors; opening a path, connecting or spawning is denied.
    #[cfg(target_arch = "aarch64")]
    const ARCH: u32 = 0xc00000b7;
    #[cfg(target_arch = "x86_64")]
    const ARCH: u32 = 0xc000003e;
    let stmt = |code, k| sock_filter {
        code,
        jt: 0,
        jf: 0,
        k,
    };
    let equal = |k| sock_filter {
        code: 0x15,
        jt: 0,
        jf: 1,
        k,
    };
    let mut filter = vec![
        stmt(0x20, 4),
        equal(ARCH),
        stmt(0x05, 1),
        stmt(0x06, SECCOMP_RET_KILL_PROCESS),
        stmt(0x20, 0),
    ];
    let mut allowed = vec![
        SYS_read,
        SYS_write,
        SYS_readv,
        SYS_writev,
        SYS_close,
        SYS_fstat,
        SYS_lseek,
        SYS_mmap,
        SYS_munmap,
        SYS_mremap,
        SYS_mprotect,
        SYS_madvise,
        SYS_brk,
        SYS_futex,
        SYS_clock_gettime,
        SYS_gettimeofday,
        SYS_getrandom,
        SYS_getpid,
        SYS_gettid,
        SYS_getuid,
        SYS_geteuid,
        SYS_getgid,
        SYS_getegid,
        SYS_sigaltstack,
        SYS_rt_sigaction,
        SYS_rt_sigprocmask,
        SYS_rt_sigreturn,
        SYS_sched_yield,
        SYS_sched_getaffinity,
        SYS_uname,
        SYS_exit,
        SYS_exit_group,
    ];
    #[cfg(target_arch = "x86_64")]
    allowed.push(SYS_arch_prctl);
    for call in allowed.drain(..) {
        filter.push(equal(call as u32));
        filter.push(stmt(0x06, SECCOMP_RET_ALLOW));
    }
    filter.push(stmt(0x06, SECCOMP_RET_ERRNO | EPERM as u32));
    let prog = sock_fprog {
        len: filter.len() as u16,
        filter: filter.as_mut_ptr(),
    };
    // Kernel copies the live BPF array during the call.
    if unsafe { prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
        return Err(format!("setting no_new_privs: {}", std::io::Error::last_os_error()).into());
    }
    if unsafe { prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &prog) } != 0 {
        return Err(format!(
            "installing seccomp filter: {}",
            std::io::Error::last_os_error()
        )
        .into());
    }
    Ok(())
}

#[cfg(windows)]
#[path = "windows.rs"]
mod windows;
#[cfg(windows)]
use windows::enter as platform;

#[cfg(not(any(
    windows,
    target_os = "macos",
    all(
        target_os = "linux",
        any(target_arch = "aarch64", target_arch = "x86_64")
    )
)))]
fn platform() -> Result<(), BoxError> {
    Err("Code Mode process isolation is unavailable on this platform".into())
}
