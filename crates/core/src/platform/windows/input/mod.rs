//! Windows capture. Low-level hooks hide events from the local system. Raw Input gives mouse
//! movement without Windows' pointer acceleration, so the controlled computer applies its own,
//! as with a real mouse.
//!
//! If this process exits or the hook thread hangs, Windows removes the hooks and input returns
//! to the PC. The hotkey's way back to the PC is handled here, so it doesn't depend on the core
//! thread either.

use std::cell::RefCell;
use std::io;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Instant;

use windows_sys::Win32::Foundation::{ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{
    GetCurrentThread, GetCurrentThreadId, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_PACKET;
use windows_sys::Win32::UI::Input::{
    GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT, RIDEV_INPUTSINK, RIM_TYPEMOUSE,
    RegisterRawInputDevices,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, HC_ACTION, HHOOK,
    HWND_MESSAGE, KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT, PostThreadMessageW, RegisterClassW, SetTimer,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_APP, WM_INPUT,
    WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEWHEEL, WM_QUIT,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_TIMER, WM_XBUTTONDOWN, WM_XBUTTONUP, WNDCLASSW,
};

use crate::input::keymap::{self, KeyUsage};
use crate::input::{CaptureControl, InputEvent, InputSink, SwitchHotkey};
use crate::platform::windows::wide;

mod keys;
mod stuck_keys;
#[cfg(test)]
mod tests;

use keys::key_usage;
use stuck_keys::{INJECTED_TAG, modifier_bit};

pub const SYSTEM_KEEPS_COMBOS: bool = true;

const MOUSE_MOVE_ABSOLUTE: u16 = 0x01;
const WM_REFRESH: u32 = WM_APP + 1;
const REFRESH_INTERVAL_MS: u32 = 5000;

pub struct Capture {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
    held: Arc<AtomicU8>,
}

impl Capture {
    pub fn refresh(&self) {
        // SAFETY: plain Win32 call; the thread owns a message queue (it called GetMessageW).
        unsafe { PostThreadMessageW(self.thread_id, WM_REFRESH, 0, 0) };
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        // SAFETY: plain Win32 call; the thread owns a message queue (it called GetMessageW).
        unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0) };
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct HookState {
    control: Arc<CaptureControl>,
    sink: InputSink,
    hotkey: SwitchHotkey,
    held: Arc<AtomicU8>,
    buttons: u8,
    trace: bool,
}

thread_local! {
    static STATE: RefCell<Option<HookState>> = const { RefCell::new(None) };
}

pub fn start_capture(control: Arc<CaptureControl>, sink: InputSink) -> io::Result<Capture> {
    let (ready_tx, ready_rx) = mpsc::channel::<io::Result<u32>>();
    let held = Arc::new(AtomicU8::new(0));
    let hook_held = held.clone();
    let thread = thread::Builder::new().name("input-capture".into()).spawn(move || {
        STATE.with(|s| {
            *s.borrow_mut() = Some(HookState {
                hotkey: SwitchHotkey::new(control.hotkey()),
                control,
                sink,
                held: hook_held,
                buttons: 0,
                trace: std::env::var_os("KVMATE_TRACE_KEYS").is_some(),
            })
        });
        // SAFETY: Win32 setup and message loop on this thread; handles are released below.
        unsafe {
            SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
            let window = match create_raw_input_window() {
                Ok(w) => w,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let mut hooks = Hooks::install();
            if hooks.kb.is_null() || hooks.mouse.is_null() {
                let err = io::Error::last_os_error();
                hooks.remove();
                DestroyWindow(window);
                let _ = ready_tx.send(Err(err));
                return;
            }
            let _ = ready_tx.send(Ok(GetCurrentThreadId()));
            SetTimer(null_mut(), 0, REFRESH_INTERVAL_MS, None);

            let mut msg: MSG = zeroed();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                match msg.message {
                    WM_REFRESH => refresh(&mut hooks, window),
                    WM_TIMER if msg.hwnd.is_null() => {
                        let forwarding = STATE.with(|s| {
                            return s.borrow().as_ref().is_some_and(|st| return st.control.forwarding());
                        });
                        if forwarding {
                            refresh(&mut hooks, window);
                        }
                    }
                    _ => {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }

            hooks.remove();
            DestroyWindow(window);
        }
        STATE.with(|s| *s.borrow_mut() = None);
    })?;

    match ready_rx.recv() {
        Ok(Ok(thread_id)) => return Ok(Capture { thread_id, thread: Some(thread), held }),
        Ok(Err(e)) => {
            let _ = thread.join();
            return Err(e);
        }
        Err(_) => return Err(io::Error::other("input capture thread exited")),
    }
}

struct Hooks {
    kb: HHOOK,
    mouse: HHOOK,
}

impl Hooks {
    fn install() -> Hooks {
        // SAFETY: the procedures live as long as the process; the module is this executable.
        unsafe {
            let module = GetModuleHandleW(null());
            let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0);
            let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0);
            return Hooks { kb, mouse };
        }
    }

    fn remove(&mut self) {
        for hook in [&mut self.kb, &mut self.mouse] {
            if !hook.is_null() {
                // SAFETY: hook was returned by SetWindowsHookExW.
                unsafe { UnhookWindowsHookEx(*hook) };
                *hook = null_mut();
            }
        }
    }
}

fn refresh(hooks: &mut Hooks, window: HWND) {
    let mut new = Hooks::install();
    if !new.kb.is_null() {
        std::mem::swap(&mut hooks.kb, &mut new.kb);
    }
    if !new.mouse.is_null() {
        std::mem::swap(&mut hooks.mouse, &mut new.mouse);
    }
    new.remove();
    // SAFETY: the window belongs to this thread.
    unsafe { register_raw_mouse(window) };
}

unsafe fn register_raw_mouse(hwnd: HWND) -> bool {
    let device = RAWINPUTDEVICE { usUsagePage: 0x01, usUsage: 0x02, dwFlags: RIDEV_INPUTSINK, hwndTarget: hwnd };
    // SAFETY: one valid RAWINPUTDEVICE.
    unsafe { return RegisterRawInputDevices(&device, 1, size_of::<RAWINPUTDEVICE>() as u32) != 0 }
}

unsafe fn create_raw_input_window() -> io::Result<HWND> {
    // SAFETY: standard window class / window creation; strings outlive the calls.
    unsafe {
        let class = wide("kvmate-input");
        let module = GetModuleHandleW(null());
        let wc =
            WNDCLASSW { lpfnWndProc: Some(window_proc), hInstance: module, lpszClassName: class.as_ptr(), ..zeroed() };
        if RegisterClassW(&wc) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err(io::Error::last_os_error());
        }
        let hwnd =
            CreateWindowExW(0, class.as_ptr(), class.as_ptr(), 0, 0, 0, 0, 0, HWND_MESSAGE, null_mut(), module, null());
        if hwnd.is_null() {
            return Err(io::Error::last_os_error());
        }
        if !register_raw_mouse(hwnd) {
            let err = io::Error::last_os_error();
            DestroyWindow(hwnd);
            return Err(err);
        }
        return Ok(hwnd);
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for HC_ACTION, lparam points to a KBDLLHOOKSTRUCT.
        let kb = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        if kb.dwExtraInfo == INJECTED_TAG || kb.vkCode == VK_PACKET as u32 {
            // SAFETY: pass the event on to the next hook.
            return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
        }
        let down = matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        let suppress = STATE.with(|s| {
            let mut s = s.borrow_mut();
            let Some(st) = s.as_mut() else { return false };
            let forwarding = st.control.forwarding();
            if let Some(bit) = modifier_bit(kb.vkCode) {
                if down {
                    st.held.fetch_or(bit, Ordering::Relaxed);
                } else {
                    st.held.fetch_and(!bit, Ordering::Relaxed);
                }
            }
            let usage = key_usage(kb);
            if st.trace {
                crate::logfile::write(
                    crate::logfile::Level::Trace,
                    &format!(
                        "key vk={:#04x} scan={:#04x} flags={:#04x} {} {} t={}",
                        kb.vkCode,
                        kb.scanCode,
                        kb.flags,
                        usage.and_then(keymap::usage_name).unwrap_or("?"),
                        if down { "down" } else { "up" },
                        kb.time
                    ),
                );
            }
            if let Some(usage) = usage {
                if forwarding {
                    (st.sink)(InputEvent::Key { usage, down });
                }
                let hotkey = KeyUsage::Keyboard(st.control.hotkey());
                if st.hotkey.key() != hotkey {
                    st.hotkey = SwitchHotkey::new(st.control.hotkey());
                }
                if st.hotkey.on_key(usage, down, Instant::now()) && st.buttons == 0 {
                    if forwarding {
                        st.control.set_forwarding(false);
                    }
                    (st.sink)(InputEvent::Hotkey { back_here: forwarding });
                }
            }
            return forwarding && st.control.suppress;
        });
        if suppress {
            return 1;
        }
    }
    // SAFETY: pass the event on to the next hook.
    unsafe { return CallNextHookEx(null_mut(), code, wparam, lparam) }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for HC_ACTION, lparam points to an MSLLHOOKSTRUCT.
        let ms = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        let high = (ms.mouseData >> 16) as u16;
        let event = match wparam as u32 {
            WM_LBUTTONDOWN => Some(InputEvent::MouseButton { button: 0, down: true }),
            WM_LBUTTONUP => Some(InputEvent::MouseButton { button: 0, down: false }),
            WM_RBUTTONDOWN => Some(InputEvent::MouseButton { button: 1, down: true }),
            WM_RBUTTONUP => Some(InputEvent::MouseButton { button: 1, down: false }),
            WM_MBUTTONDOWN => Some(InputEvent::MouseButton { button: 2, down: true }),
            WM_MBUTTONUP => Some(InputEvent::MouseButton { button: 2, down: false }),
            WM_XBUTTONDOWN | WM_XBUTTONUP => Some(InputEvent::MouseButton {
                button: if high == 1 { 3 } else { 4 },
                down: wparam as u32 == WM_XBUTTONDOWN,
            }),
            WM_MOUSEWHEEL => Some(InputEvent::Wheel { delta: high as i16 as i32 }),
            WM_MOUSEHWHEEL => Some(InputEvent::HWheel { delta: high as i16 as i32 }),
            _ => None,
        };
        let suppress = STATE.with(|s| {
            let mut s = s.borrow_mut();
            let Some(st) = s.as_mut() else { return false };
            let forwarding = st.control.forwarding();
            if let Some(event) = event {
                if let InputEvent::MouseButton { button, down } = event {
                    let bit = 1u8 << button;
                    st.buttons = if down { st.buttons | bit } else { st.buttons & !bit };
                    if down {
                        st.hotkey.on_other_input();
                    }
                }
                if forwarding {
                    (st.sink)(event);
                }
            }
            return forwarding && st.control.suppress;
        });
        if suppress {
            return 1;
        }
    }
    // SAFETY: pass the event on to the next hook.
    unsafe { return CallNextHookEx(null_mut(), code, wparam, lparam) }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_INPUT {
        STATE.with(|s| {
            let s = s.borrow();
            let Some(st) = s.as_ref() else { return };
            if !st.control.forwarding() {
                return;
            }
            // SAFETY: lparam is the HRAWINPUT of this WM_INPUT message.
            if let Some((dx, dy)) = unsafe { read_raw_mouse(lparam as HRAWINPUT) } {
                if dx != 0 || dy != 0 {
                    (st.sink)(InputEvent::MouseMove { dx, dy });
                }
            }
        });
    }
    // SAFETY: default processing (also releases the raw input buffer).
    unsafe { return DefWindowProcW(hwnd, msg, wparam, lparam) }
}

unsafe fn read_raw_mouse(handle: HRAWINPUT) -> Option<(i32, i32)> {
    // SAFETY: buffer is a RAWINPUT, size passed in/out as the API requires.
    unsafe {
        let mut raw: RAWINPUT = zeroed();
        let mut size = size_of::<RAWINPUT>() as u32;
        let n = GetRawInputData(
            handle,
            RID_INPUT,
            &mut raw as *mut RAWINPUT as *mut _,
            &mut size,
            size_of::<RAWINPUTHEADER>() as u32,
        );
        if n == u32::MAX || raw.header.dwType != RIM_TYPEMOUSE {
            return None;
        }
        let mouse = raw.data.mouse;
        if mouse.usFlags & MOUSE_MOVE_ABSOLUTE != 0 {
            return None;
        }
        return Some((mouse.lLastX, mouse.lLastY));
    }
}
