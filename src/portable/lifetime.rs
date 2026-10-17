//! Own the extracted process until it exits, including interrupted launches.
use crate::termination::Termination;
use std::{
    io,
    process::{Child, Command, ExitStatus},
    thread,
    time::{Duration, Instant},
};

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

pub fn wait(command: &mut Command, termination: &Termination) -> io::Result<ExitStatus> {
    #[cfg(windows)]
    let job = WindowsJob::new()?;
    let mut child = OwnedChild(command.spawn()?);
    #[cfg(windows)]
    job.assign(&child.0)?;
    let mut stopping = None;
    loop {
        if let Some(status) = child.0.try_wait()? {
            return Ok(status);
        }
        if termination.requested() {
            if stopping.is_none() {
                #[cfg(unix)]
                unsafe {
                    libc::kill(child.0.id() as libc::pid_t, libc::SIGTERM);
                }
                // Console control events reach both inherited Windows processes.
                stopping = Some(Instant::now());
            }
            if stopping.is_some_and(|start| start.elapsed() > Duration::from_secs(5)) {
                child.0.kill()?;
                return child.0.wait();
            }
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(windows)]
struct WindowsJob(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl WindowsJob {
    fn new() -> io::Result<Self> {
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = Self(handle);
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as _,
                std::mem::size_of_val(&info) as u32,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(job)
        }
    }
    fn assign(&self, child: &Child) -> io::Result<()> {
        use std::os::windows::io::AsRawHandle;
        if unsafe {
            windows_sys::Win32::System::JobObjects::AssignProcessToJobObject(
                self.0,
                child.as_raw_handle(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}
#[cfg(windows)]
impl Drop for WindowsJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
