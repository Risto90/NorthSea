use std::collections::HashSet;
use std::ffi::{CString, c_char, c_int, c_long, c_uint, c_ulong, c_void};
use std::ptr::{self, NonNull};
use std::thread;
use std::time::{Duration, Instant};

use raw_window_handle::{RawDisplayHandle, RawWindowHandle, XlibDisplayHandle, XlibWindowHandle};

use crate::{
    CameraState, NativeSettingsAction, build_scene_mesh, native_renderer::NativeRenderer,
    native_settings::NativeSettings,
};

type Display = c_void;
type WindowId = c_ulong;
type Atom = c_ulong;

const KEY_PRESS: c_int = 2;
const KEY_RELEASE: c_int = 3;
const BUTTON_PRESS: c_int = 4;
const BUTTON_RELEASE: c_int = 5;
const MOTION_NOTIFY: c_int = 6;
const EXPOSE: c_int = 12;
const CONFIGURE_NOTIFY: c_int = 22;
const CLIENT_MESSAGE: c_int = 33;
const KEY_PRESS_MASK: c_long = 1;
const KEY_RELEASE_MASK: c_long = 1 << 1;
const BUTTON_PRESS_MASK: c_long = 1 << 2;
const BUTTON_RELEASE_MASK: c_long = 1 << 3;
const POINTER_MOTION_MASK: c_long = 1 << 6;
const EXPOSURE_MASK: c_long = 1 << 15;
const STRUCTURE_NOTIFY_MASK: c_long = 1 << 17;
const XK_ESCAPE: c_ulong = 0xff1b;
const XK_LEFT: c_ulong = 0xff51;
const XK_UP: c_ulong = 0xff52;
const XK_RIGHT: c_ulong = 0xff53;
const XK_DOWN: c_ulong = 0xff54;

#[link(name = "X11")]
unsafe extern "C" {
    fn XInitThreads() -> c_int;
    fn XOpenDisplay(name: *const c_char) -> *mut Display;
    fn XCloseDisplay(display: *mut Display) -> c_int;
    fn XDefaultScreen(display: *mut Display) -> c_int;
    fn XRootWindow(display: *mut Display, screen: c_int) -> WindowId;
    fn XCreateSimpleWindow(
        display: *mut Display,
        parent: WindowId,
        x: c_int,
        y: c_int,
        width: c_uint,
        height: c_uint,
        border_width: c_uint,
        border: c_ulong,
        background: c_ulong,
    ) -> WindowId;
    fn XDestroyWindow(display: *mut Display, window: WindowId) -> c_int;
    fn XStoreName(display: *mut Display, window: WindowId, name: *const c_char) -> c_int;
    fn XSelectInput(display: *mut Display, window: WindowId, event_mask: c_long) -> c_int;
    fn XMapWindow(display: *mut Display, window: WindowId) -> c_int;
    fn XFlush(display: *mut Display) -> c_int;
    fn XInternAtom(display: *mut Display, name: *const c_char, only_if_exists: c_int) -> Atom;
    fn XSetWMProtocols(
        display: *mut Display,
        window: WindowId,
        protocols: *mut Atom,
        count: c_int,
    ) -> c_int;
    fn XPending(display: *mut Display) -> c_int;
    fn XNextEvent(display: *mut Display, event: *mut XEvent) -> c_int;
    fn XLookupKeysym(event: *mut XKeyEvent, index: c_int) -> c_ulong;
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XKeyEvent {
    type_: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut Display,
    window: WindowId,
    root: WindowId,
    subwindow: WindowId,
    time: c_ulong,
    x: c_int,
    y: c_int,
    x_root: c_int,
    y_root: c_int,
    state: c_uint,
    keycode: c_uint,
    same_screen: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XButtonEvent {
    type_: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut Display,
    window: WindowId,
    root: WindowId,
    subwindow: WindowId,
    time: c_ulong,
    x: c_int,
    y: c_int,
    x_root: c_int,
    y_root: c_int,
    state: c_uint,
    button: c_uint,
    same_screen: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XMotionEvent {
    type_: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut Display,
    window: WindowId,
    root: WindowId,
    subwindow: WindowId,
    time: c_ulong,
    x: c_int,
    y: c_int,
    x_root: c_int,
    y_root: c_int,
    state: c_uint,
    is_hint: c_char,
    same_screen: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XConfigureEvent {
    type_: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut Display,
    event: WindowId,
    window: WindowId,
    x: c_int,
    y: c_int,
    width: c_int,
    height: c_int,
    border_width: c_int,
    above: WindowId,
    override_redirect: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
union XClientData {
    bytes: [c_char; 20],
    shorts: [i16; 10],
    longs: [c_long; 5],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XClientMessageEvent {
    type_: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut Display,
    window: WindowId,
    message_type: Atom,
    format: c_int,
    data: XClientData,
}

#[repr(C)]
#[derive(Clone, Copy)]
union XEvent {
    type_: c_int,
    key: XKeyEvent,
    button: XButtonEvent,
    motion: XMotionEvent,
    configure: XConfigureEvent,
    client: XClientMessageEvent,
    padding: [c_long; 24],
}

struct XWindow {
    display: NonNull<Display>,
    window: WindowId,
    screen: c_int,
    protocols_atom: Atom,
    close_protocol: Atom,
    width: u32,
    height: u32,
}

impl XWindow {
    fn new() -> Result<Self, String> {
        if unsafe { XInitThreads() } == 0 {
            return Err("Не удалось включить потокобезопасность X11.".to_owned());
        }
        let display = NonNull::new(unsafe { XOpenDisplay(ptr::null()) })
            .ok_or_else(|| "Не удалось открыть окно Linux: не найден X11-дисплей.".to_owned())?;
        let display_ptr = display.as_ptr();
        let screen = unsafe { XDefaultScreen(display_ptr) };
        let root = unsafe { XRootWindow(display_ptr, screen) };
        let width = 1200;
        let height = 700;
        let window =
            unsafe { XCreateSimpleWindow(display_ptr, root, 0, 0, width, height, 0, 0, 0) };
        if window == 0 {
            unsafe { XCloseDisplay(display_ptr) };
            return Err("Не удалось создать окно Linux.".to_owned());
        }

        let title = CString::new("Моря Туманного Севера").expect("Название окна содержит NUL.");
        let protocols_name = CString::new("WM_PROTOCOLS").expect("Имя протокола содержит NUL.");
        let protocol = CString::new("WM_DELETE_WINDOW").expect("Имя протокола содержит NUL.");
        let protocols_atom = unsafe { XInternAtom(display_ptr, protocols_name.as_ptr(), 0) };
        let close_protocol = unsafe { XInternAtom(display_ptr, protocol.as_ptr(), 0) };
        let event_mask = KEY_PRESS_MASK
            | KEY_RELEASE_MASK
            | BUTTON_PRESS_MASK
            | BUTTON_RELEASE_MASK
            | POINTER_MOTION_MASK
            | EXPOSURE_MASK
            | STRUCTURE_NOTIFY_MASK;
        unsafe {
            XStoreName(display_ptr, window, title.as_ptr());
            XSelectInput(display_ptr, window, event_mask);
            XSetWMProtocols(
                display_ptr,
                window,
                &close_protocol as *const Atom as *mut Atom,
                1,
            );
            XMapWindow(display_ptr, window);
            XFlush(display_ptr);
        }

        Ok(Self {
            display,
            window,
            screen,
            protocols_atom,
            close_protocol,
            width,
            height,
        })
    }

    fn raw_display_handle(&self) -> RawDisplayHandle {
        RawDisplayHandle::Xlib(XlibDisplayHandle::new(
            Some(self.display.cast()),
            self.screen,
        ))
    }

    fn raw_window_handle(&self) -> RawWindowHandle {
        RawWindowHandle::Xlib(XlibWindowHandle::new(self.window))
    }
}

impl Drop for XWindow {
    fn drop(&mut self) {
        unsafe {
            XDestroyWindow(self.display.as_ptr(), self.window);
            XCloseDisplay(self.display.as_ptr());
        }
    }
}

pub fn run() {
    if let Err(error) = run_inner() {
        eprintln!("Не удалось запустить игру: {error}");
    }
}

fn run_inner() -> Result<(), String> {
    let mut window = XWindow::new()?;
    let mut settings = NativeSettings::load_linux()?;
    let mut camera = CameraState::default();
    camera.enable_native_api_settings(true);
    camera.graphics_api_preference = settings.graphics_api();
    let mut renderer = match NativeRenderer::new_with_preference(
        Some(window.raw_display_handle()),
        window.raw_window_handle(),
        window.width,
        window.height,
        settings.graphics_api(),
    ) {
        Ok(renderer) => renderer,
        Err(error) => {
            eprintln!(
                "Выбранный графический API недоступен; игра не запускается через другой API: {error}"
            );
            camera.api_settings_error = true;
            camera.api_settings_open = true;
            NativeRenderer::new_diagnostic(
                Some(window.raw_display_handle()),
                window.raw_window_handle(),
                window.width,
                window.height,
            )
            .map_err(|diagnostic_error| format!("{error} {diagnostic_error}"))?
        }
    };
    if camera.api_settings_error {
        println!(
            "Показан экран ошибки; игровой режим API не запущен. Рендерер сообщения: {}",
            renderer.renderer_name()
        );
    } else {
        println!("Графический адаптер: {}", renderer.renderer_name());
    }

    let mut pressed = HashSet::new();
    let mut steer_left = false;
    let mut steer_right = false;
    let mut mouse_position = None;
    let mut running = true;
    let mut redraw = true;
    let mut previous_frame = Instant::now();

    while running {
        while unsafe { XPending(window.display.as_ptr()) } > 0 {
            let mut event = unsafe { std::mem::zeroed::<XEvent>() };
            unsafe { XNextEvent(window.display.as_ptr(), &mut event) };
            let event_type = unsafe { event.type_ };
            match event_type {
                KEY_PRESS => {
                    let mut key = unsafe { event.key };
                    let symbol = unsafe { XLookupKeysym(&mut key, 0) };
                    if symbol == XK_ESCAPE {
                        if camera.api_settings_open && !camera.api_settings_error {
                            camera.api_settings_open = false;
                            redraw = true;
                        } else {
                            running = false;
                        }
                    } else if pressed.insert(key.keycode) {
                        if !camera.api_settings_open
                            && let Some(name) = key_name(symbol)
                        {
                            match name {
                                "a" => steer_left = true,
                                "d" => steer_right = true,
                                "w" => camera.change_speed(1),
                                "s" => camera.change_speed(-1),
                                _ => camera.apply_key(name),
                            }
                            camera.set_rudder(match (steer_left, steer_right) {
                                (true, false) => -1.0,
                                (false, true) => 1.0,
                                _ => 0.0,
                            });
                            redraw = true;
                        }
                    }
                }
                KEY_RELEASE => {
                    let mut key = unsafe { event.key };
                    pressed.remove(&key.keycode);
                    match key_name(unsafe { XLookupKeysym(&mut key, 0) }) {
                        Some("a") => steer_left = false,
                        Some("d") => steer_right = false,
                        _ => {}
                    }
                    camera.set_rudder(match (steer_left, steer_right) {
                        (true, false) => -1.0,
                        (false, true) => 1.0,
                        _ => 0.0,
                    });
                    redraw = true;
                }
                BUTTON_PRESS => {
                    let button = unsafe { event.button };
                    if button.button == 1 {
                        let previous_preference = camera.graphics_api_preference;
                        let was_api_error = camera.api_settings_error;
                        match camera.native_settings_action_at(button.x as f32, button.y as f32) {
                            Some(NativeSettingsAction::TogglePanel) => {
                                mouse_position = None;
                                redraw = true;
                            }
                            Some(NativeSettingsAction::SelectApi(preference)) => {
                                mouse_position = None;
                                if preference != previous_preference || was_api_error {
                                    match NativeRenderer::new_with_preference(
                                        Some(window.raw_display_handle()),
                                        window.raw_window_handle(),
                                        window.width,
                                        window.height,
                                        preference,
                                    ) {
                                        Ok(next_renderer) => {
                                            renderer = next_renderer;
                                            camera.api_settings_error = false;
                                            camera.api_settings_open = false;
                                            match settings.save_graphics_api(preference) {
                                                Ok(()) => {
                                                    println!(
                                                        "Выбран графический API: {}",
                                                        renderer.renderer_name()
                                                    );
                                                }
                                                Err(error) => {
                                                    eprintln!("{error}");
                                                }
                                            }
                                        }
                                        Err(error) => {
                                            eprintln!(
                                                "Выбранный API недоступен; игровой режим остановлен, автоматического перехода не было: {error}"
                                            );
                                            if let Err(save_error) =
                                                settings.save_graphics_api(preference)
                                            {
                                                eprintln!("{save_error}");
                                            }
                                            camera.api_settings_error = true;
                                            camera.api_settings_open = true;
                                        }
                                    }
                                }
                                redraw = true;
                            }
                            None if camera.api_settings_open => {
                                mouse_position = None;
                            }
                            None => mouse_position = Some((button.x, button.y)),
                        }
                    }
                }
                BUTTON_RELEASE => {
                    if unsafe { event.button }.button == 1 {
                        mouse_position = None;
                    }
                }
                MOTION_NOTIFY => {
                    let motion = unsafe { event.motion };
                    if let Some((previous_x, previous_y)) = mouse_position {
                        camera.apply_drag(
                            (motion.x - previous_x) as f32,
                            (motion.y - previous_y) as f32,
                        );
                        mouse_position = Some((motion.x, motion.y));
                        redraw = true;
                    }
                }
                CONFIGURE_NOTIFY => {
                    let size = unsafe { event.configure };
                    window.width = size.width.max(1) as u32;
                    window.height = size.height.max(1) as u32;
                    camera.viewport_width = window.width as f32;
                    camera.viewport_height = window.height as f32;
                    redraw = true;
                }
                EXPOSE => redraw = true,
                CLIENT_MESSAGE => {
                    let client = unsafe { event.client };
                    if client.message_type == window.protocols_atom
                        && unsafe { client.data.longs[0] } as c_ulong == window.close_protocol
                    {
                        running = false;
                    }
                }
                _ => {}
            }
        }

        let now = Instant::now();
        let elapsed = (now - previous_frame).as_secs_f32().min(0.1);
        previous_frame = now;
        if !camera.api_settings_error && camera.advance_sailing(elapsed) {
            redraw = true;
        }
        if redraw {
            let mesh = build_scene_mesh(camera)?;
            renderer.render(&mesh, window.width, window.height)?;
            redraw = false;
        }
        thread::sleep(Duration::from_millis(4));
    }
    Ok(())
}

fn key_name(symbol: c_ulong) -> Option<&'static str> {
    match symbol {
        XK_LEFT => Some("ArrowLeft"),
        XK_RIGHT => Some("ArrowRight"),
        XK_UP => Some("ArrowUp"),
        XK_DOWN => Some("ArrowDown"),
        0x61 | 0x41 => Some("a"),
        0x64 | 0x44 => Some("d"),
        0x77 | 0x57 => Some("w"),
        0x73 | 0x53 => Some("s"),
        0x63 | 0x43 => Some("c"),
        0x7a | 0x5a => Some("z"),
        0x66 | 0x46 => Some("f"),
        0x72 | 0x52 => Some("r"),
        _ => None,
    }
}
