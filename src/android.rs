use std::time::{Duration, Instant};

use android_activity::{
    AndroidApp, InputStatus, MainEvent, PollEvent,
    input::{InputEvent, MotionAction, MotionEvent},
};
use ndk::native_window::NativeWindow;
use raw_window_handle::{AndroidDisplayHandle, HasWindowHandle, RawDisplayHandle};

use crate::{CameraState, SailingControl, build_scene_mesh, native_renderer::NativeRenderer};

#[derive(Default)]
struct TouchState {
    previous_position: Option<(f32, f32)>,
    tap_start: Option<(f32, f32)>,
    last_tap: Option<(Instant, f32, f32)>,
    pinch_distance: Option<f32>,
    multi_touch: bool,
    dragging: bool,
    active_control: Option<SailingControl>,
}

struct AndroidRenderer {
    renderer: NativeRenderer,
    window: NativeWindow,
}

impl AndroidRenderer {
    fn new(window: NativeWindow) -> Result<Self, String> {
        let handle = window
            .window_handle()
            .map_err(|error| format!("Не удалось получить Android-окно: {error}"))?;
        let renderer = NativeRenderer::new_auto(
            Some(RawDisplayHandle::Android(AndroidDisplayHandle::new())),
            handle.as_raw(),
            window.width().max(1) as u32,
            window.height().max(1) as u32,
        )?;
        eprintln!("Графический адаптер: {}", renderer.renderer_name());
        Ok(Self { renderer, window })
    }

    fn draw(&mut self, camera: &mut CameraState) -> Result<(), String> {
        let width = self.window.width().max(1) as u32;
        let height = self.window.height().max(1) as u32;
        camera.viewport_width = width as f32;
        camera.viewport_height = height as f32;
        let mesh = build_scene_mesh(*camera)?;
        self.renderer.render(&mesh, width, height)
    }
}

#[unsafe(no_mangle)]
pub fn android_main(app: AndroidApp) {
    let mut camera = CameraState::default();
    camera.set_mobile_controls(true);
    let mut touch = TouchState::default();
    let mut renderer: Option<AndroidRenderer> = None;
    let mut redraw = true;
    let mut running = true;
    let mut last_simulation = Instant::now();
    let mut last_sailing_draw = Instant::now();

    while running {
        app.poll_events(Some(Duration::from_millis(16)), |event| match event {
            PollEvent::Main(MainEvent::InitWindow { .. }) => {
                renderer = app.native_window().and_then(|window| {
                    AndroidRenderer::new(window)
                        .map_err(|error| {
                            eprintln!("Не удалось создать поверхность Android: {error}");
                        })
                        .ok()
                });
                redraw = true;
            }
            PollEvent::Main(MainEvent::TerminateWindow { .. }) => {
                renderer = None;
            }
            PollEvent::Main(MainEvent::WindowResized { .. })
            | PollEvent::Main(MainEvent::RedrawNeeded { .. }) => redraw = true,
            PollEvent::Main(MainEvent::InputAvailable { .. }) => {
                if let Ok(mut input_events) = app.input_events_iter() {
                    loop {
                        let received_input = input_events.next(|event| match event {
                            InputEvent::MotionEvent(motion) => {
                                if handle_input(motion, &mut camera, &mut touch) {
                                    redraw = true;
                                    InputStatus::Handled
                                } else {
                                    InputStatus::Unhandled
                                }
                            }
                            _ => InputStatus::Unhandled,
                        });
                        if !received_input {
                            break;
                        }
                    }
                }
            }
            PollEvent::Main(MainEvent::Destroy) => running = false,
            _ => {}
        });

        let now = Instant::now();
        let elapsed = now.duration_since(last_simulation).as_secs_f32();
        last_simulation = now;
        if camera.speed > 0 {
            camera.advance_sailing(elapsed);
            if now.duration_since(last_sailing_draw) >= Duration::from_millis(33) {
                redraw = true;
                last_sailing_draw = now;
            }
        }

        if redraw {
            if let Some(renderer) = renderer.as_mut() {
                if let Err(error) = renderer.draw(&mut camera) {
                    eprintln!("Не удалось отобразить Android-сцену: {error}");
                }
            }
            redraw = false;
        }
    }
}

fn handle_input(
    motion: &MotionEvent<'_>,
    camera: &mut CameraState,
    touch: &mut TouchState,
) -> bool {
    let action = motion.action();
    let count = motion.pointer_count();
    if count == 0 {
        return false;
    }

    if count > 1 || matches!(action, MotionAction::PointerDown | MotionAction::PointerUp) {
        if touch.active_control == Some(SailingControl::Rudder) {
            camera.set_rudder(0.0);
        }
        touch.active_control = None;
        touch.multi_touch = true;
        touch.previous_position = None;
        touch.tap_start = None;
        if action == MotionAction::Move && count >= 2 {
            let next_distance = distance_between(motion);
            if let Some(previous_distance) = touch.pinch_distance {
                camera.apply_wheel(previous_distance - next_distance);
            }
            touch.pinch_distance = Some(next_distance);
            return true;
        }
        if action == MotionAction::PointerDown && count >= 2 {
            touch.pinch_distance = Some(distance_between(motion));
            return true;
        }
        if action == MotionAction::PointerUp || count < 2 {
            touch.pinch_distance = None;
        }
        return matches!(action, MotionAction::PointerDown | MotionAction::PointerUp);
    }

    let pointer = motion.pointer_at_index(motion.pointer_index());
    let position = (pointer.x(), pointer.y());

    match action {
        MotionAction::Down => {
            if let Some(control) = camera.sailing_control_at(position.0, position.1) {
                touch.active_control = Some(control);
                match control {
                    SailingControl::Rudder => camera.set_rudder_from_screen_x(position.0),
                    SailingControl::SpeedUp => camera.change_speed(1),
                    SailingControl::SpeedDown => camera.change_speed(-1),
                }
                touch.previous_position = Some(position);
                return true;
            }
            touch.active_control = None;
            touch.previous_position = Some(position);
            touch.tap_start = Some(position);
            touch.dragging = false;
            true
        }
        MotionAction::Move => {
            if let Some(control) = touch.active_control {
                if control == SailingControl::Rudder {
                    camera.set_rudder_from_screen_x(position.0);
                }
                touch.previous_position = Some(position);
                return true;
            }
            if let Some(previous) = touch.previous_position {
                let delta = (position.0 - previous.0, position.1 - previous.1);
                if let Some(start) = touch.tap_start {
                    touch.dragging |= distance(start, position) > 12.0;
                }
                camera.apply_drag(delta.0, delta.1);
                touch.previous_position = Some(position);
                true
            } else {
                false
            }
        }
        MotionAction::Up => {
            if let Some(control) = touch.active_control.take() {
                if control == SailingControl::Rudder {
                    camera.set_rudder(0.0);
                }
                touch.previous_position = None;
                touch.tap_start = None;
                touch.dragging = false;
                touch.multi_touch = false;
                touch.pinch_distance = None;
                return true;
            }
            if !touch.multi_touch && !touch.dragging {
                if let Some((time, x, y)) = touch.last_tap {
                    if time.elapsed() < Duration::from_millis(330)
                        && distance((x, y), position) < 32.0
                    {
                        camera.apply_key("C");
                        touch.last_tap = None;
                    } else {
                        touch.last_tap = Some((Instant::now(), position.0, position.1));
                    }
                } else {
                    touch.last_tap = Some((Instant::now(), position.0, position.1));
                }
            }
            touch.previous_position = None;
            touch.tap_start = None;
            touch.dragging = false;
            touch.multi_touch = false;
            touch.pinch_distance = None;
            true
        }
        _ => false,
    }
}

fn distance_between(event: &MotionEvent<'_>) -> f32 {
    let first = event.pointer_at_index(0);
    let second = event.pointer_at_index(1);
    distance((first.x(), first.y()), (second.x(), second.y()))
}

fn distance(first: (f32, f32), second: (f32, f32)) -> f32 {
    (first.0 - second.0).hypot(first.1 - second.1)
}
