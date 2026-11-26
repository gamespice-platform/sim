//! A precise thread sleep and the OS timer resolution, for a loop that paces itself (a sim host waiting for its next tick).
//!
//! Windows rounds `Sleep` up to its global timer tick (15.6 ms unless a process raised it, and Windows 11 may ignore a
//! raise for a hidden or throttled process). A high-resolution waitable timer (Windows 10 1803+) wakes on time whatever
//! that tick is, so [`Sleeper`] uses one, and falls back to `Sleep`. Elsewhere it waits on the thread's futex, which the
//! kernel already wakes on time.

use std::time::Duration;

/// How [`Sleeper::sleep_usec`] waits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Windows' high-resolution waitable timer: on time whatever the global timer resolution.
    HighResTimer,
    /// Windows' `Sleep`: rounded up to the global timer tick.
    Sleep,
    /// A futex wait (`std::thread::park_timeout`), on every other system.
    Futex,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::HighResTimer => "high-res waitable timer",
            Kind::Sleep => "Sleep (global timer tick)",
            Kind::Futex => "futex wait",
        }
    }
}

/// One thread's sleeper. Not shared between threads at once: it owns one waitable timer.
pub struct Sleeper {
    #[cfg(windows)]
    timer: Option<win::Timer>,
}

impl Default for Sleeper {
    fn default() -> Self {
        Self::new()
    }
}

impl Sleeper {
    pub fn new() -> Self {
        Sleeper {
            #[cfg(windows)]
            timer: win::Timer::new(),
        }
    }

    pub fn kind(&self) -> Kind {
        #[cfg(windows)]
        {
            if self.timer.is_some() {
                Kind::HighResTimer
            } else {
                Kind::Sleep
            }
        }
        #[cfg(not(windows))]
        Kind::Futex
    }

    /// Blocks the calling thread for at least `usec` microseconds.
    pub fn sleep_usec(&self, usec: u64) {
        if usec == 0 {
            return;
        }
        #[cfg(windows)]
        {
            if let Some(t) = &self.timer
                && t.wait_usec(usec)
            {
                return;
            }
            win::sleep_ms(usec.div_ceil(1000));
        }
        #[cfg(not(windows))]
        park_usec(usec);
    }
}

/// A futex wait, not `nanosleep`: the same precision, and a park wakes only for this thread's unpark, which nothing sends.
/// The web build (no threads to park) sleeps plainly.
#[cfg(all(not(windows), target_family = "wasm"))]
fn park_usec(usec: u64) {
    std::thread::sleep(Duration::from_micros(usec));
}

#[cfg(all(not(windows), not(target_family = "wasm")))]
fn park_usec(usec: u64) {
    let until = std::time::Instant::now() + Duration::from_micros(usec);
    loop {
        let now = std::time::Instant::now();
        if now >= until {
            return;
        }
        std::thread::park_timeout(until - now);
    }
}

/// A raised OS timer resolution, held until dropped (Windows `timeBeginPeriod`/`timeEndPeriod`).
pub struct Resolution {
    #[cfg_attr(not(windows), allow(dead_code))]
    ms: u32,
}

impl Resolution {
    pub fn ms(&self) -> u32 {
        self.ms
    }
}

impl Drop for Resolution {
    fn drop(&mut self) {
        #[cfg(windows)]
        win::end_period(self.ms);
    }
}

/// Raises the OS timer resolution to `ms` while the returned guard lives, and asks Windows to honour it even while the
/// process is hidden or power-throttled. None where there is no such setting (every system but Windows) or it was refused.
pub fn request_resolution(ms: u32) -> Option<Resolution> {
    let ms = ms.max(1);
    #[cfg(windows)]
    {
        win::keep_timer_resolution();
        if win::begin_period(ms) {
            return Some(Resolution { ms });
        }
        None
    }
    #[cfg(not(windows))]
    {
        let _ = ms;
        None
    }
}

/// How long one sleep of `usec` took, in microseconds (for a probe of the timer).
pub fn measure_usec(sleeper: &Sleeper, usec: u64) -> u64 {
    let t0 = std::time::Instant::now();
    sleeper.sleep_usec(usec);
    let took: Duration = t0.elapsed();
    took.as_micros() as u64
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    const CREATE_WAITABLE_TIMER_HIGH_RESOLUTION: u32 = 0x0000_0002;
    const TIMER_ALL_ACCESS: u32 = 0x001F_0003;
    const INFINITE: u32 = 0xFFFF_FFFF;
    const WAIT_OBJECT_0: u32 = 0;
    const TIMERR_NOERROR: u32 = 0;
    const PROCESS_POWER_THROTTLING: i32 = 4;
    const PROCESS_POWER_THROTTLING_CURRENT_VERSION: u32 = 1;
    const PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION: u32 = 0x4;

    #[repr(C)]
    struct PowerThrottlingState {
        version: u32,
        control_mask: u32,
        state_mask: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateWaitableTimerExW(
            attrs: *const c_void,
            name: *const u16,
            flags: u32,
            access: u32,
        ) -> *mut c_void;
        fn SetWaitableTimer(
            timer: *mut c_void,
            due: *const i64,
            period: i32,
            routine: *const c_void,
            arg: *const c_void,
            resume: i32,
        ) -> i32;
        fn WaitForSingleObject(handle: *mut c_void, ms: u32) -> u32;
        fn CloseHandle(handle: *mut c_void) -> i32;
        fn Sleep(ms: u32);
        fn GetCurrentProcess() -> *mut c_void;
        fn SetProcessInformation(
            process: *mut c_void,
            class: i32,
            info: *const c_void,
            size: u32,
        ) -> i32;
    }

    #[link(name = "winmm")]
    unsafe extern "system" {
        fn timeBeginPeriod(ms: u32) -> u32;
        fn timeEndPeriod(ms: u32) -> u32;
    }

    pub struct Timer(*mut c_void);

    // SAFETY: a waitable timer handle may be used from any thread; Sleeper's callers keep one waiter at a time.
    unsafe impl Send for Timer {}
    unsafe impl Sync for Timer {}

    impl Timer {
        /// None before Windows 10 1803, which refuses the high-resolution flag.
        pub fn new() -> Option<Timer> {
            // SAFETY: plain Win32 call; null attributes and name are allowed.
            let h = unsafe {
                CreateWaitableTimerExW(
                    std::ptr::null(),
                    std::ptr::null(),
                    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                    TIMER_ALL_ACCESS,
                )
            };
            if h.is_null() { None } else { Some(Timer(h)) }
        }

        /// False when the timer could not be armed or waited on (the caller falls back to Sleep).
        pub fn wait_usec(&self, usec: u64) -> bool {
            // Relative due time, negative, in 100 ns units.
            let due: i64 = -(usec.min(i64::MAX as u64 / 10) as i64 * 10);
            // SAFETY: the handle is live for self's lifetime; due outlives the call.
            unsafe {
                if SetWaitableTimer(self.0, &due, 0, std::ptr::null(), std::ptr::null(), 0) == 0 {
                    return false;
                }
                WaitForSingleObject(self.0, INFINITE) == WAIT_OBJECT_0
            }
        }
    }

    impl Drop for Timer {
        fn drop(&mut self) {
            // SAFETY: the handle came from CreateWaitableTimerExW and is closed once.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    pub fn sleep_ms(ms: u64) {
        // SAFETY: plain Win32 call.
        unsafe { Sleep(ms.min(u64::from(INFINITE - 1)) as u32) }
    }

    pub fn begin_period(ms: u32) -> bool {
        // SAFETY: plain Win32 call; paired with end_period by Resolution's drop.
        unsafe { timeBeginPeriod(ms) == TIMERR_NOERROR }
    }

    pub fn end_period(ms: u32) {
        // SAFETY: plain Win32 call matching a successful begin_period.
        unsafe {
            timeEndPeriod(ms);
        }
    }

    /// Windows 11 may ignore a timer resolution request of a process it throttles; opt out. Older systems refuse the class, harmlessly.
    pub fn keep_timer_resolution() {
        let state = PowerThrottlingState {
            version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            control_mask: PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
            state_mask: 0,
        };
        // SAFETY: the pseudo-handle of this process and a correctly sized, live struct.
        unsafe {
            SetProcessInformation(
                GetCurrentProcess(),
                PROCESS_POWER_THROTTLING,
                (&state as *const PowerThrottlingState).cast(),
                std::mem::size_of::<PowerThrottlingState>() as u32,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleeps_at_least_the_asked_time() {
        let s = Sleeper::new();
        for usec in [200, 1000, 2500] {
            assert!(measure_usec(&s, usec) >= usec, "{usec} us");
        }
    }

    #[test]
    fn zero_returns_at_once() {
        assert!(measure_usec(&Sleeper::new(), 0) < 1000);
    }

    #[test]
    fn kind_matches_the_system() {
        let k = Sleeper::new().kind();
        if cfg!(windows) {
            assert_ne!(k, Kind::Futex);
        } else {
            assert_eq!(k, Kind::Futex);
        }
        assert!(!k.name().is_empty());
    }

    #[test]
    fn resolution_is_windows_only() {
        let r = request_resolution(1);
        assert_eq!(r.is_some(), cfg!(windows));
        if let Some(r) = r {
            assert_eq!(r.ms(), 1);
        }
    }
}
