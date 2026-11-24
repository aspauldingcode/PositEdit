//! Memory-pressure helpers and history budgets for constrained hosts.
//!
//! Undo/redo snapshots are the dominant editor RAM cost. Caps keep RSS bounded
//! when the OS is under jetsam / memory pressure.

/// Default undo/redo depth in normal conditions.
/// AFK tick23: 40 (was 48/64) — rope snapshots dominate idle RSS.
pub const HISTORY_NORMAL: usize = 40;
/// Aggressive depth when the OS reports elevated memory pressure.
pub const HISTORY_PRESSURE: usize = 8;
/// Hard ceiling — never retain more than this many snapshots.
pub const HISTORY_HARD_MAX: usize = 96;

/// macOS `kern.memorystatus_vm_pressure_level` style levels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum PressureLevel {
    Normal = 0,
    Warning = 1,
    Urgent = 2,
    Critical = 4,
}

impl PressureLevel {
    pub fn history_limit(self) -> usize {
        match self {
            PressureLevel::Normal => HISTORY_NORMAL,
            // AFK: Warning was 24 — still too many rope clones on constrained hosts.
            PressureLevel::Warning => 16,
            PressureLevel::Urgent | PressureLevel::Critical => HISTORY_PRESSURE,
        }
    }

    pub fn scrollback_limit(self) -> usize {
        match self {
            // AFK tick23: tighter PTY scrollback (was 500/300/200/80).
            PressureLevel::Normal => 240,
            PressureLevel::Warning => 120,
            PressureLevel::Urgent | PressureLevel::Critical => 60,
        }
    }

    pub fn is_elevated(self) -> bool {
        self != PressureLevel::Normal
    }

    /// Jetsam-class pressure — hide expensive editor chrome (Kitty minimap).
    pub fn is_critical(self) -> bool {
        matches!(self, PressureLevel::Urgent | PressureLevel::Critical)
    }
}

/// Best-effort host memory pressure. Unknown platforms report Normal.
pub fn pressure_level() -> PressureLevel {
    #[cfg(target_os = "macos")]
    {
        macos_pressure_level().unwrap_or(PressureLevel::Normal)
    }
    #[cfg(not(target_os = "macos"))]
    {
        PressureLevel::Normal
    }
}

/// Current process resident set size in bytes, when available.
pub fn process_rss_bytes() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        macos_rss_bytes()
    }
    #[cfg(target_os = "linux")]
    {
        linux_rss_bytes()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
fn macos_pressure_level() -> Option<PressureLevel> {
    let mut level: i32 = 0;
    let mut len = std::mem::size_of::<i32>();
    let name = b"kern.memorystatus_vm_pressure_level\0";
    extern "C" {
        fn sysctlbyname(
            name: *const i8,
            oldp: *mut std::ffi::c_void,
            oldlenp: *mut usize,
            newp: *mut std::ffi::c_void,
            newlen: usize,
        ) -> i32;
    }
    let rc = unsafe {
        sysctlbyname(
            name.as_ptr() as *const i8,
            &mut level as *mut i32 as *mut std::ffi::c_void,
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    Some(match level {
        0 => PressureLevel::Normal,
        1 => PressureLevel::Warning,
        2 => PressureLevel::Urgent,
        4 => PressureLevel::Critical,
        n if n > 0 => PressureLevel::Warning,
        _ => PressureLevel::Normal,
    })
}

#[cfg(target_os = "macos")]
fn macos_rss_bytes() -> Option<u64> {
    // getrusage.ru_maxrss is bytes on Darwin (kilobytes on Linux).
    #[repr(C)]
    struct TimeVal {
        tv_sec: i64,
        tv_usec: i32,
    }
    #[repr(C)]
    struct Rusage {
        ru_utime: TimeVal,
        ru_stime: TimeVal,
        ru_maxrss: i64,
        ru_ixrss: i64,
        ru_idrss: i64,
        ru_isrss: i64,
        ru_minflt: i64,
        ru_majflt: i64,
        ru_nswap: i64,
        ru_inblock: i64,
        ru_oublock: i64,
        ru_msgsnd: i64,
        ru_msgrcv: i64,
        ru_nsignals: i64,
        ru_nvcsw: i64,
        ru_nivcsw: i64,
    }
    extern "C" {
        fn getrusage(who: i32, usage: *mut Rusage) -> i32;
    }
    const RUSAGE_SELF: i32 = 0;
    let mut usage = unsafe { std::mem::zeroed::<Rusage>() };
    let rc = unsafe { getrusage(RUSAGE_SELF, &mut usage) };
    if rc != 0 || usage.ru_maxrss <= 0 {
        None
    } else {
        Some(usage.ru_maxrss as u64)
    }
}

#[cfg(target_os = "linux")]
fn linux_rss_bytes() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/self/statm").ok()?;
    let rss_pages: u64 = text.split_whitespace().nth(1)?.parse().ok()?;
    // 4 KiB is the common default; prefer sysconf when linked.
    let page = 4096u64;
    Some(rss_pages.saturating_mul(page))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_limits_tighten_under_pressure() {
        assert!(PressureLevel::Normal.history_limit() > PressureLevel::Urgent.history_limit());
        assert_eq!(PressureLevel::Critical.history_limit(), HISTORY_PRESSURE);
        assert_eq!(HISTORY_NORMAL, 40);
        assert!(HISTORY_HARD_MAX <= 96);
        assert_eq!(PressureLevel::Warning.history_limit(), 16);
        assert_eq!(PressureLevel::Normal.scrollback_limit(), 240);
        assert!(
            PressureLevel::Normal.scrollback_limit()
                > PressureLevel::Warning.scrollback_limit()
        );
    }

    #[test]
    fn pressure_probe_does_not_panic() {
        let _ = pressure_level();
        let _ = process_rss_bytes();
    }
}
