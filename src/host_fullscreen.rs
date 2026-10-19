//! Coordinate player layout with the attached terminal's window, when supported.
//! Host transitions are acknowledged by geometry; a queued key is not success.
use ratatui::crossterm::event::{Event, KeyCode};
use std::time::{Duration, Instant};

pub trait Host {
    fn fullscreen(&self) -> Option<bool>;
    fn toggle(&mut self) -> bool;
}

pub struct Controller<B: Host> {
    backend: B,
    layout: bool,
    owned: bool,
    pending: Option<(bool, Instant)>,
    last_status: Option<Instant>,
    external_exit: bool,
}
impl<B: Host> Controller<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            layout: false,
            owned: false,
            pending: None,
            last_status: None,
            external_exit: false,
        }
    }
    pub fn is_forwarded_key(&self, event: &Event) -> bool {
        self.pending.is_some() && matches!(event, Event::Key(key) if key.code == KeyCode::F(11))
    }
    pub fn take_external_exit(&mut self) -> bool {
        std::mem::take(&mut self.external_exit)
    }
    pub fn update(&mut self, layout: bool, now: Instant) -> Option<&'static str> {
        if let Some((target, since)) = self.pending {
            if self.backend.fullscreen() == Some(target) {
                self.owned = target;
                self.pending = None;
            } else if now.saturating_duration_since(since) >= Duration::from_millis(750) {
                self.pending = None;
                self.layout = layout;
                return Some("Terminal did not confirm fullscreen; use its fullscreen shortcut (Windows Terminal: Alt+Enter)");
            } else {
                // Keep the latest layout intent; execute a reversal after the
                // pending host transition is acknowledged, rather than racing it.
                return None;
            }
        }
        if self.owned
            && layout
            && self.last_status.is_none_or(|last| {
                now.saturating_duration_since(last) >= Duration::from_millis(200)
            })
        {
            self.last_status = Some(now);
            if self.backend.fullscreen() == Some(false) {
                self.owned = false;
                self.layout = false;
                self.external_exit = true;
                return None;
            }
        }
        if self.layout == layout {
            return None;
        }
        self.layout = layout;
        let state = self.backend.fullscreen();
        let needs_toggle = if layout {
            state == Some(false)
        } else {
            self.owned && state == Some(true)
        };
        if needs_toggle && self.backend.toggle() {
            self.pending = Some((layout, now));
            None
        } else if layout && state != Some(true) {
            Some("Player fullscreen; use your terminal's fullscreen shortcut to hide tabs/taskbar (Windows Terminal: Alt+Enter)")
        } else {
            None
        }
    }
}
impl<B: Host> Drop for Controller<B> {
    fn drop(&mut self) {
        if self.pending.is_some_and(|(target, _)| !target) {
            return;
        }
        if self.pending.is_some() {
            // A quick quit can occur between posting F11 and its acknowledgement.
            let deadline = Instant::now() + Duration::from_millis(250);
            while self.backend.fullscreen() == Some(false) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        if (self.owned || self.pending.is_some()) && self.backend.fullscreen() == Some(true) {
            self.backend.toggle();
        }
    }
}

#[cfg(not(windows))]
pub struct NativeHost;
#[cfg(not(windows))]
impl NativeHost {
    pub fn new(_enabled: bool) -> Self {
        Self
    }
}
#[cfg(not(windows))]
impl Host for NativeHost {
    fn fullscreen(&self) -> Option<bool> {
        None
    }
    fn toggle(&mut self) -> bool {
        false
    }
}

#[cfg(windows)]
pub use native::NativeHost;

#[cfg(windows)]
mod native {
    use super::Host;
    use windows_sys::Win32::{
        Foundation::{HWND, RECT},
        Graphics::{
            Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS},
            Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST},
        },
        System::Console::GetConsoleWindow,
        UI::{
            Input::KeyboardAndMouse::{
                GetKeyState, MapVirtualKeyW, MAPVK_VK_TO_VSC, VK_CONTROL, VK_F11, VK_MENU, VK_SHIFT,
            },
            WindowsAndMessaging::*,
        },
    };

    pub struct NativeHost {
        window: HWND,
        process: u32,
    }
    impl NativeHost {
        pub fn new(enabled: bool) -> Self {
            let window = if enabled {
                unsafe { attached_window() }
            } else {
                std::ptr::null_mut()
            };
            let mut process = 0;
            if !window.is_null() {
                unsafe {
                    GetWindowThreadProcessId(window, &mut process);
                }
            }
            Self { window, process }
        }
        fn attached(&self) -> bool {
            if self.window.is_null() {
                return false;
            }
            unsafe {
                let mut process = 0;
                GetWindowThreadProcessId(self.window, &mut process);
                process == self.process && attached_window() == self.window
            }
        }
    }
    unsafe fn attached_window() -> HWND {
        // ConptyReparentPseudoConsole gives the pseudo-console an owner. Never
        // guess from the foreground window or enumerate unrelated terminals.
        let console = GetConsoleWindow();
        if console.is_null() {
            return std::ptr::null_mut();
        }
        let owner = GetWindow(console, GW_OWNER);
        if owner.is_null() {
            return std::ptr::null_mut();
        }
        let root = GetAncestor(owner, GA_ROOT);
        let mut class = [0u16; 128];
        let length = GetClassNameW(root, class.as_mut_ptr(), class.len() as i32);
        if IsWindowVisible(root) == 0
            || length <= 0
            || String::from_utf16_lossy(&class[..length as usize])
                != "CASCADIA_HOSTING_WINDOW_CLASS"
        {
            return std::ptr::null_mut();
        }
        root
    }
    fn covers_monitor(rect: &RECT, monitor: &RECT, style: isize) -> bool {
        // DWM bounds are physical pixels even for a DPI-unaware console app.
        style & WS_THICKFRAME as isize == 0
            && (i64::from(rect.left) - i64::from(monitor.left)).abs() <= 2
            && (i64::from(rect.top) - i64::from(monitor.top)).abs() <= 2
            && (i64::from(rect.right) - i64::from(monitor.right)).abs() <= 2
            && (i64::from(rect.bottom) - i64::from(monitor.bottom)).abs() <= 2
    }
    impl Host for NativeHost {
        fn fullscreen(&self) -> Option<bool> {
            if !self.attached() {
                return None;
            }
            unsafe {
                let mut rect: RECT = std::mem::zeroed();
                if DwmGetWindowAttribute(
                    self.window,
                    DWMWA_EXTENDED_FRAME_BOUNDS as u32,
                    (&mut rect as *mut RECT).cast(),
                    std::mem::size_of::<RECT>() as u32,
                ) != 0
                {
                    return None;
                }
                let monitor = MonitorFromWindow(self.window, MONITOR_DEFAULTTONEAREST);
                let mut info: MONITORINFO = std::mem::zeroed();
                info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
                if GetMonitorInfoW(monitor, &mut info) == 0 {
                    return None;
                }
                Some(covers_monitor(
                    &rect,
                    &info.rcMonitor,
                    GetWindowLongPtrW(self.window, GWL_STYLE),
                ))
            }
        }
        fn toggle(&mut self) -> bool {
            if !self.attached() {
                return false;
            }
            unsafe {
                if GetAncestor(GetForegroundWindow(), GA_ROOT) != self.window {
                    return false;
                }
                if [VK_SHIFT, VK_CONTROL, VK_MENU]
                    .iter()
                    .any(|key| GetKeyState(i32::from(*key)) < 0)
                {
                    return false;
                }
                let thread = GetWindowThreadProcessId(self.window, std::ptr::null_mut());
                let mut gui: GUITHREADINFO = std::mem::zeroed();
                gui.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
                if GetGUIThreadInfo(thread, &mut gui) == 0
                    || gui.hwndFocus.is_null()
                    || GetAncestor(gui.hwndFocus, GA_ROOT) != self.window
                {
                    return false;
                }
                // Post only to this terminal's focused CoreInput child. Do not
                // send global keystrokes, activate windows or rewrite settings.
                let status = (MapVirtualKeyW(u32::from(VK_F11), MAPVK_VK_TO_VSC) << 16) | 1;
                let posted = PostMessageW(
                    gui.hwndFocus,
                    WM_KEYDOWN,
                    usize::from(VK_F11),
                    status as isize,
                ) != 0;
                if posted {
                    PostMessageW(
                        gui.hwndFocus,
                        WM_KEYUP,
                        usize::from(VK_F11),
                        (status | 0xc0000000) as isize,
                    );
                }
                posted
            }
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn maximized_and_fullscreen_are_distinct_even_with_hidden_taskbar_or_high_dpi() {
            let monitor = RECT {
                left: -2560,
                top: 0,
                right: 0,
                bottom: 1440,
            };
            let fullscreen = RECT {
                left: -2560,
                top: 0,
                right: 0,
                bottom: 1440,
            };
            let maximized = RECT {
                left: -2560,
                top: 0,
                right: 0,
                bottom: 1392,
            };
            assert!(covers_monitor(&fullscreen, &monitor, 0));
            assert!(!covers_monitor(&maximized, &monitor, 0));
            assert!(!covers_monitor(
                &fullscreen,
                &monitor,
                WS_THICKFRAME as isize
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;
    struct FakeHost {
        state: Rc<Cell<Option<bool>>>,
        toggles: Rc<Cell<u32>>,
    }
    impl Host for FakeHost {
        fn fullscreen(&self) -> Option<bool> {
            self.state.get()
        }
        fn toggle(&mut self) -> bool {
            self.toggles.set(self.toggles.get() + 1);
            true
        }
    }
    type HostFixture = (Controller<FakeHost>, Rc<Cell<Option<bool>>>, Rc<Cell<u32>>);
    fn fixture(initial: Option<bool>) -> HostFixture {
        let state = Rc::new(Cell::new(initial));
        let toggles = Rc::new(Cell::new(0));
        (
            Controller::new(FakeHost {
                state: state.clone(),
                toggles: toggles.clone(),
            }),
            state,
            toggles,
        )
    }
    #[test]
    fn already_fullscreen_host_is_preserved_on_escape_and_quit() {
        let (mut controller, _, toggles) = fixture(Some(true));
        let now = Instant::now();
        assert!(controller.update(true, now).is_none());
        controller.update(false, now);
        drop(controller);
        assert_eq!(toggles.get(), 0);
    }
    #[test]
    fn quit_restores_only_acknowledged_owned_fullscreen() {
        let (mut controller, state, toggles) = fixture(Some(false));
        let now = Instant::now();
        controller.update(true, now);
        state.set(Some(true));
        controller.update(true, now + Duration::from_millis(20));
        drop(controller);
        assert_eq!(toggles.get(), 2);
    }
    #[test]
    fn rapid_reversal_waits_for_entry_then_restores_without_double_exit_on_quit() {
        let (mut controller, state, toggles) = fixture(Some(false));
        let now = Instant::now();
        controller.update(true, now);
        controller.update(false, now + Duration::from_millis(10));
        assert_eq!(toggles.get(), 1);
        state.set(Some(true));
        controller.update(false, now + Duration::from_millis(20));
        assert_eq!(toggles.get(), 2);
        drop(controller);
        assert_eq!(toggles.get(), 2);
    }
    #[test]
    fn unavailable_or_unacknowledged_host_reports_fallback_once() {
        let now = Instant::now();
        let (mut unavailable, _, toggles) = fixture(None);
        assert!(unavailable.update(true, now).unwrap().contains("shortcut"));
        assert!(unavailable.update(true, now).is_none());
        assert_eq!(toggles.get(), 0);
        let (mut controller, _, toggles) = fixture(Some(false));
        controller.update(true, now);
        let f11 = Event::Key(ratatui::crossterm::event::KeyEvent::new(
            KeyCode::F(11),
            ratatui::crossterm::event::KeyModifiers::NONE,
        ));
        assert!(controller.is_forwarded_key(&f11));
        assert!(controller
            .update(true, now + Duration::from_millis(800))
            .unwrap()
            .contains("did not confirm"));
        assert!(!controller.owned);
        assert!(!controller.is_forwarded_key(&f11));
        assert!(controller
            .update(true, now + Duration::from_secs(1))
            .is_none());
        drop(controller);
        assert_eq!(toggles.get(), 1);
    }
    #[test]
    fn terminal_fullscreen_exit_restores_player_layout_without_retoggling_host() {
        let (mut controller, state, toggles) = fixture(Some(false));
        let now = Instant::now();
        controller.update(true, now);
        state.set(Some(true));
        controller.update(true, now + Duration::from_millis(20));
        state.set(Some(false));
        controller.update(true, now + Duration::from_millis(250));
        assert!(controller.take_external_exit());
        assert!(!controller.take_external_exit());
        controller.update(false, now + Duration::from_millis(300));
        drop(controller);
        assert_eq!(toggles.get(), 1);
    }
    #[test]
    fn player_fullscreen_requests_host_transition_and_waits_for_acknowledgement() {
        let state = Rc::new(Cell::new(Some(false)));
        let toggles = Rc::new(Cell::new(0));
        let mut controller = Controller::new(FakeHost {
            state: state.clone(),
            toggles: toggles.clone(),
        });
        let mut app = crate::app::App::new(None);
        app.toggle_fullscreen();
        let now = Instant::now();
        controller.update(app.fullscreen, now);
        assert_eq!(
            toggles.get(),
            1,
            "Player fullscreen must reach the host action"
        );
        assert!(
            !controller.owned,
            "A queued action does not prove fullscreen"
        );
        state.set(Some(true));
        controller.update(app.fullscreen, now + Duration::from_millis(50));
        assert!(controller.owned);
        app.toggle_fullscreen();
        controller.update(app.fullscreen, now + Duration::from_millis(100));
        assert_eq!(
            toggles.get(),
            2,
            "Leaving fullscreen restores our host transition"
        );
    }
}
