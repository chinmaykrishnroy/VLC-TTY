//! Translate catchable process termination into a normal return through guards.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub struct Termination(Arc<AtomicBool>);
impl Termination {
    pub fn install() -> std::io::Result<Self> {
        let flag = Arc::new(AtomicBool::new(false));
        #[cfg(unix)]
        for signal in [
            signal_hook::consts::SIGTERM,
            signal_hook::consts::SIGINT,
            signal_hook::consts::SIGHUP,
        ] {
            signal_hook::flag::register(signal, Arc::clone(&flag))?;
        }
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
            if SetConsoleCtrlHandler(Some(console_handler), 1) == 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(Self(flag))
    }
    pub fn requested(&self) -> bool {
        self.0.load(Ordering::Relaxed) || {
            #[cfg(windows)]
            {
                WINDOWS_STOP.load(Ordering::Relaxed)
            }
            #[cfg(not(windows))]
            {
                false
            }
        }
    }
}

#[cfg(windows)]
static WINDOWS_STOP: AtomicBool = AtomicBool::new(false);
#[cfg(windows)]
unsafe extern "system" fn console_handler(event: u32) -> i32 {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT};
    // Windows grants only a short deadline for console-close/logoff events;
    // the Job Object owns child lifetime if Windows kills the supervisor.
    if event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT {
        WINDOWS_STOP.store(true, Ordering::Relaxed);
        1
    } else {
        0
    }
}
