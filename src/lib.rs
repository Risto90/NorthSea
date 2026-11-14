use std::fmt::Write;

#[cfg(target_os = "android")]
mod android;
#[cfg(target_arch = "wasm32")]
mod web;
mod gpu_renderer;
#[cfg(target_os = "linux")]
mod desktop;
mod scene_mesh;

pub use scene_mesh::{SceneMesh, Vertex, build_scene_mesh};

const WIDTH: f32 = 1200.0;
const HEIGHT: f32 = 700.0;
const SIDE_COUNT: usize = 10;
const COMPASS_ICON_SVG: &str = include_str!("../artifacts/north-seas-adventure/public/compass.svg");

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraState {
    pub yaw: f32,
    pub ship_heading: f32,
    pub rudder: f32,
    pub speed: u8,
    pub travel_distance: f32,
    pub free_tilt: f32,
    pub zoom: f32,
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub dialogue: bool,
    pub distant_sea: bool,
    pub damaged: bool,
    pub mobile_controls: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SailingControl {
    Rudder,
    SpeedUp,
    SpeedDown,
}

#[derive(Clone, Copy)]
struct ControlRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl ControlRect {
    fn contains(self, x: f32, y: f32) -> bool {
        (self.x..=self.x + self.width).contains(&x) && (self.y..=self.y + self.height).contains(&y)
    }
}

#[derive(Clone, Copy)]
struct SailingControlLayout {
    rudder: ControlRect,
    speed_up: ControlRect,
    speed_down: ControlRect,
}

impl CameraState {
    pub const fn new() -> Self {
        Self {
            yaw: 0.25,
            ship_heading: 0.0,
            rudder: 0.0,
            speed: 0,
            travel_distance: 0.0,
            free_tilt: 31.0,
            zoom: 1.0,
            viewport_width: WIDTH,
            viewport_height: HEIGHT,
            dialogue: false,
            distant_sea: false,
            damaged: false,
            mobile_controls: false,
        }
    }

    pub fn set_mobile_controls(&mut self, enabled: bool) {
        self.mobile_controls = enabled;
    }

    pub fn sailing_control_at(&self, x: f32, y: f32) -> Option<SailingControl> {
        if !self.mobile_controls || self.dialogue {
            return None;
        }
        let layout = sailing_control_layout(self.viewport_width, self.viewport_height);
        if layout.rudder.contains(x, y) {
            Some(SailingControl::Rudder)
        } else if layout.speed_up.contains(x, y) {
            Some(SailingControl::SpeedUp)
        } else if layout.speed_down.contains(x, y) {
            Some(SailingControl::SpeedDown)
        } else {
            None
        }
    }

    pub fn set_rudder_from_screen_x(&mut self, x: f32) {
        let rudder = sailing_control_layout(self.viewport_width, self.viewport_height).rudder;
        let center = rudder.x + rudder.width * 0.5;
        self.rudder = ((x - center) / (rudder.width * 0.5)).clamp(-1.0, 1.0);
    }

    pub fn set_rudder(&mut self, value: f32) {
        self.rudder = value.clamp(-1.0, 1.0);
    }

    pub fn change_speed(&mut self, delta: i32) {
        self.speed = (i32::from(self.speed) + delta).clamp(0, 3) as u8;
    }

    pub fn advance_sailing(&mut self, delta_seconds: f32) -> bool {
        if !delta_seconds.is_finite() || delta_seconds <= 0.0 {
            return self.speed > 0;
        }
        let elapsed = delta_seconds.min(0.1);
        self.ship_heading = (self.ship_heading
            + self.rudder * (0.12 + f32::from(self.speed) * 0.15) * elapsed)
            .rem_euclid(std::f32::consts::TAU);
        if self.speed > 0 {
            self.travel_distance = (self.travel_distance + f32::from(self.speed) * elapsed * 30.0)
                .rem_euclid(100_000.0);
        }
        self.speed > 0
    }

    pub fn apply_key(&mut self, key: &str) {
        match key {
            "ArrowLeft" => self.yaw -= 0.12,
            "ArrowRight" => self.yaw += 0.12,
            "ArrowUp" => self.free_tilt = (self.free_tilt + 2.0).clamp(18.0, 43.0),
            "ArrowDown" => self.free_tilt = (self.free_tilt - 2.0).clamp(18.0, 43.0),
            "c" | "C" => self.dialogue = !self.dialogue,
            "z" | "Z" => self.damaged = !self.damaged,
            "f" | "F" => {
                self.distant_sea = !self.distant_sea;
                self.zoom = if self.distant_sea { 0.72 } else { 1.0 };
            }
            "r" | "R" => *self = Self::default(),
            _ => {}
        }
    }

    pub fn apply_wheel(&mut self, delta: f32) {
        self.zoom = (self.zoom - delta.signum() * 0.08).clamp(0.55, 1.8);
        self.distant_sea = self.zoom < 0.68;
    }

    pub fn apply_drag(&mut self, delta_x: f32, delta_y: f32) {
        self.yaw -= delta_x * 0.008;
        self.free_tilt = (self.free_tilt + delta_y * 0.08).clamp(18.0, 43.0);
    }

    fn tilt(self) -> f32 {
        if self.dialogue {
            88.0
        } else if self.distant_sea {
            0.0
        } else {
            self.free_tilt
        }
    }
}

impl Default for CameraState {
    fn default() -> Self {
        Self::new()
    }
}

pub fn detect_mobile_device(
    user_agent: &str,
    mobile_hint: Option<bool>,
    platform: &str,
    max_touch_points: u32,
) -> bool {
    if mobile_hint == Some(true) {
        return true;
    }
    let user_agent = user_agent.to_ascii_lowercase();
    if ["android", "iphone", "ipad", "ipod"]
        .iter()
        .any(|marker| user_agent.contains(marker))
    {
        return true;
    }
    platform.eq_ignore_ascii_case("MacIntel") && max_touch_points > 1
}

#[derive(Clone, Copy)]
struct Point2 {
    x: f32,
    y: f32,
}

#[derive(Clone, Copy)]
struct Point3 {
    x: f32,
    y: f32,
    z: f32,
}

struct Slice {
    z: f32,
    outline: Vec<Point2>,
    top_color: &'static str,
    side_color: &'static str,
}

struct LayeredModel {
    slices: Vec<Slice>,
}

struct Face {
    points: [Point3; 3],
    color: &'static str,
    depth: f32,
}

pub fn render_svg(state: CameraState) -> String {
    let width = state.viewport_width.clamp(320.0, 3000.0);
    let height = state.viewport_height.clamp(320.0, 3000.0);
    let base_scale = (width / WIDTH).min(height / HEIGHT);
    let mut svg = String::with_capacity(24000);
    write!(
        svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width:.0} {height:.0}" role="img" aria-label="Морская сцена">
<defs>
  <linearGradient id="sea" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#263a40"/><stop offset="1" stop-color="#14262c"/></linearGradient>
  <linearGradient id="fog" x1="0" y1="0" x2="1" y2="0"><stop stop-color="#c3c3b5" stop-opacity="0"/><stop offset=".5" stop-color="#c3c3b5" stop-opacity=".11"/><stop offset="1" stop-color="#c3c3b5" stop-opacity="0"/></linearGradient>
</defs>
<rect width="{width:.0}" height="{height:.0}" fill="url(#sea)"/> "##
    )
    .expect("Не удалось сформировать начало SVG.");

    draw_sea(
        &mut svg,
        state.distant_sea,
        width,
        height,
        state.travel_distance,
        state.ship_heading,
    );

    let camera_tilt = state.tilt().to_radians();
    let scale = base_scale * 2.0 * state.zoom * if state.distant_sea { 0.72 } else { 1.18 };
    let ship_yaw = state.yaw + state.ship_heading;
    let center = Point2 {
        x: width * 0.5,
        y: height * 0.46,
    };

    if state.dialogue {
        draw_model(
            &mut svg,
            &make_torfinn(),
            center,
            (width / 180.0).min(height / 280.0) * 1.15 * state.zoom,
            state.yaw,
            camera_tilt,
            state.damaged,
        );
    } else {
        if state.speed > 0 {
            draw_ship_wake(&mut svg, center, scale, ship_yaw, camera_tilt, state.speed);
        }
        draw_model(
            &mut svg,
            &make_ship(),
            center,
            scale,
            ship_yaw,
            camera_tilt,
            state.damaged,
        );
        draw_ship_details(
            &mut svg,
            center,
            scale,
            ship_yaw,
            camera_tilt,
            state.damaged,
        );
    }

    draw_navigation_hud(&mut svg, width, height, state);
    svg.push_str("</svg>");
    svg
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static CAMERA_STATE: std::cell::RefCell<CameraState> =
        std::cell::RefCell::new(CameraState::default());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn key_input(key: &str) {
    CAMERA_STATE.with(|state| state.borrow_mut().apply_key(key));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn wheel_input(delta: f32) {
    CAMERA_STATE.with(|state| state.borrow_mut().apply_wheel(delta));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn drag_input(delta_x: f32, delta_y: f32) {
    CAMERA_STATE.with(|state| state.borrow_mut().apply_drag(delta_x, delta_y));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn set_mobile_controls(enabled: bool) {
    CAMERA_STATE.with(|state| state.borrow_mut().set_mobile_controls(enabled));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn rudder_input(value: f32) {
    CAMERA_STATE.with(|state| state.borrow_mut().set_rudder(value));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn speed_input(delta: i32) {
    CAMERA_STATE.with(|state| state.borrow_mut().change_speed(delta));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn advance_sailing(delta_seconds: f32) -> bool {
    CAMERA_STATE.with(|state| state.borrow_mut().advance_sailing(delta_seconds))
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn resize_scene(width: f32, height: f32) {
    CAMERA_STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.viewport_width = width;
        state.viewport_height = height;
    });
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn render_scene() -> String {
    CAMERA_STATE.with(|state| render_svg(*state.borrow()))
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn current_scene_mesh() -> Result<SceneMesh, String> {
    CAMERA_STATE.with(|state| build_scene_mesh(*state.borrow()))
}

#[cfg(target_os = "linux")]
pub fn run_desktop() {
    desktop::run();
}

fn draw_navigation_hud(svg: &mut String, width: f32, height: f32, state: CameraState) {
    let shortest_side = width.min(height);
    let margin = (shortest_side * 0.03).clamp(10.0, 22.0);
    let compass_size = (shortest_side * 0.18).clamp(64.0, 116.0);
    let compass_radius = compass_size * 0.5;
    let compass_x = margin + compass_radius;
    let compass_y = height - margin - compass_radius;
    let map_width = (width * 0.31).min(height * 0.45).clamp(100.0, 184.0);
    let map_height = map_width * 130.0 / 190.0;
    let map_x = width - margin - map_width;
    let map_y = height - margin - map_height;

    write!(
        svg,
        r##"<g id="ship-seal" transform="translate({ship_x:.1} {ship_y:.1})">
<circle r="22" fill="#102126" fill-opacity=".84" stroke="#d8c39d" stroke-opacity=".68"/>
<circle r="18" fill="none" stroke="#d8c39d" stroke-opacity=".3"/>
<path d="M-12 3h24l-4 6h-16zM-7 0l7-8 7 8M0-8v15M-16 11q5-3 10 0t10 0 10 0" fill="none" stroke="#e1cfaa" stroke-width="1.4" stroke-linejoin="round"/>
</g>
<g id="navigation-compass" transform="translate({compass_x:.1} {compass_y:.1})">
<circle r="{compass_radius:.1}" fill="#18282b" fill-opacity=".9" stroke="#d4bd94" stroke-opacity=".72" stroke-width="1.4"/>
<g transform="rotate({compass_angle:.2}) scale({icon_scale:.3}) translate(-10 -10)" color="#d9c49a">{compass_icon}</g>
</g>
<g id="nautical-map" transform="translate({map_x:.1} {map_y:.1})">
<rect width="{map_width:.1}" height="{map_height:.1}" rx="2" fill="#352c20" stroke="#d8c097" stroke-width="1.5"/>
<rect x="4" y="4" width="{map_inner_width:.1}" height="{map_inner_height:.1}" fill="#b9a477" stroke="#201d18" stroke-opacity=".7" stroke-width=".7"/>
<g transform="translate(4 4) scale({map_scale:.5})">
<path d="M0 0h78l-8 13 9 9-11 7 4 12-16 4-4 13-14 4-5 14-15 3-2 13L0 99z" fill="#66705a" stroke="#443a2a" stroke-width="2"/>
<path d="M0 98l16-11 17 3 7-13 15-4 5-13 14-4 4-12 11-7-9-9 8-13" fill="none" stroke="#d9c69a" stroke-width="3"/>
<path d="M95 28c21 9 19 19 39 15s24 6 56-1M81 76c26-6 30 8 52 5s28 9 56 3M96 107c17-8 31 0 47-6s27 2 47-6" fill="none" stroke="#70828a" stroke-width="1.4" stroke-opacity=".7"/>
<g transform="translate({map_ship_x:.1} {map_ship_y:.1}) rotate({map_heading:.2})">
<path d="M0-10 4 5 0 2-4 5z" fill="#8e3327" stroke="#30271d" stroke-width="1"/>
<circle r="2" fill="#e8d5aa" stroke="#30271d" stroke-width=".8"/>
</g>
<circle cx="38" cy="59" r="2.5" fill="#d6b06c" stroke="#342a1c"/>
<circle cx="55" cy="44" r="2" fill="#d6b06c" stroke="#342a1c"/>
<path d="M166 21v16m-5-11 5-5 5 5m-2-11v6m-2-6 2 6 2-6" fill="none" stroke="#403324" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/>
</g>
</g>"##,
        ship_x = margin + 22.0,
        ship_y = margin + 22.0,
        compass_radius = compass_radius,
        compass_angle = -(state.ship_heading + state.yaw).to_degrees(),
        icon_scale = compass_radius * 0.72 * 24.0 / 200.0,
        compass_icon = COMPASS_ICON_SVG,
        map_inner_width = map_width - 8.0,
        map_inner_height = map_height - 8.0,
        map_scale = (map_width - 8.0) / 190.0,
        map_heading = -state.ship_heading.to_degrees(),
        map_ship_x = 125.0 + (state.travel_distance * 0.006).sin() * 18.0,
        map_ship_y = 68.0 + (state.travel_distance * 0.006).cos() * 12.0,
    )
    .expect("Не удалось сформировать морские приборы.");

    if state.mobile_controls {
        draw_sailing_controls(svg, width, height, state);
    }
}

fn sailing_control_layout(width: f32, screen_height: f32) -> SailingControlLayout {
    let shortest_side = width.min(screen_height);
    let margin = (shortest_side * 0.03).clamp(10.0, 22.0);
    let compass_size = (shortest_side * 0.18).clamp(64.0, 116.0);
    let map_width = (width * 0.31).min(screen_height * 0.45).clamp(100.0, 184.0);
    let left = margin + compass_size + 6.0;
    let right = width - margin - map_width - 6.0;
    let available = (right - left).max(112.0);
    let total_width = available.min(168.0);
    let control_height = (shortest_side * 0.13).clamp(42.0, 54.0);
    let gap = 6.0;
    let rudder_width = (total_width - gap) * 0.63;
    let speed_width = total_width - rudder_width - gap;
    let x = left + (available - total_width) * 0.5;
    let screen_control_y = (screen_height - margin - control_height).max(0.0);
    let speed_x = x + rudder_width + gap;
    let speed_half_height = control_height * 0.5;
    SailingControlLayout {
        rudder: ControlRect {
            x,
            y: screen_control_y,
            width: rudder_width,
            height: control_height,
        },
        speed_up: ControlRect {
            x: speed_x,
            y: screen_control_y,
            width: speed_width,
            height: speed_half_height,
        },
        speed_down: ControlRect {
            x: speed_x,
            y: screen_control_y + speed_half_height,
            width: speed_width,
            height: control_height - speed_half_height,
        },
    }
}

fn draw_sailing_controls(svg: &mut String, width: f32, height: f32, state: CameraState) {
    let layout = sailing_control_layout(width, height);
    let rudder = layout.rudder;
    let speed = layout.speed_up;
    let speed_down = layout.speed_down;
    let pivot_x = rudder.width * 0.5;
    let pivot_y = rudder.height * 0.5;

    write!(
        svg,
        r##"<g id="mobile-sailing-controls">
<g data-sailing-control="rudder" role="button" aria-label="Рулевое весло">
<rect x="{rudder_x:.1}" y="{rudder_y:.1}" width="{rudder_width:.1}" height="{rudder_height:.1}" rx="8" fill="#122126" fill-opacity=".94" stroke="#c6ac7c" stroke-width="1.3"/>
<path d="M{left_arrow_x:.1} {arrow_y:.1}l6 -4v8zM{right_arrow_x:.1} {arrow_y:.1}l-6 -4v8z" fill="#d6c292" stroke="#392d20" stroke-width=".7"/>
<g transform="translate({rudder_x:.1} {rudder_y:.1})">
<g transform="translate({pivot_x:.1} {pivot_y:.1}) rotate({rudder_degrees:.1})">
<path d="M-26 0h52" stroke="#5c3e23" stroke-width="7" stroke-linecap="round"/>
<path d="M-26 0h52" stroke="#bd9561" stroke-width="4.2" stroke-linecap="round"/>
<path d="M-31 -3l-5 3 5 3zM31 -3l5 3-5 3z" fill="#d6c292" stroke="#392d20" stroke-width=".7"/>
</g>
<circle cx="{pivot_x:.1}" cy="{pivot_y:.1}" r="5" fill="#cfb77f" stroke="#34291d" stroke-width="1.2"/>
</g>
</g>
<g data-sailing-control="speed-up" role="button" aria-label="Увеличить ход">
<rect x="{speed_x:.1}" y="{speed_y:.1}" width="{speed_width:.1}" height="{speed_half:.1}" rx="6" fill="#122126" fill-opacity=".94" stroke="#c6ac7c" stroke-width="1.2"/>
<path d="M{speed_center:.1} {speed_up_center:.1}l-5 5h10z" fill="#e0c995" stroke="#392d20" stroke-width=".7"/>
</g>
<g data-sailing-control="speed-down" role="button" aria-label="Уменьшить ход">
<rect x="{speed_x:.1}" y="{speed_down_y:.1}" width="{speed_width:.1}" height="{speed_half:.1}" rx="6" fill="#122126" fill-opacity=".94" stroke="#c6ac7c" stroke-width="1.2"/>
<path d="M{speed_center:.1} {speed_down_center:.1}l-5-5h10z" fill="#e0c995" stroke="#392d20" stroke-width=".7"/>
</g>
</g>"##,
        rudder_x = rudder.x,
        rudder_y = rudder.y,
        rudder_width = rudder.width,
        rudder_height = rudder.height,
        left_arrow_x = rudder.x + 5.0,
        right_arrow_x = rudder.x + rudder.width - 5.0,
        arrow_y = rudder.y + pivot_y,
        rudder_degrees = state.rudder * 28.0,
        speed_x = speed.x,
        speed_y = speed.y,
        speed_width = speed.width,
        speed_half = speed.height,
        speed_center = speed.x + speed.width * 0.5,
        speed_up_center = speed.y + speed.height * 0.5,
        speed_down_y = speed_down.y,
        speed_down_center = speed_down.y + speed_down.height * 0.5,
        pivot_x = pivot_x,
        pivot_y = pivot_y,
    )
    .expect("Не удалось сформировать экранные органы управления.");

    for level in 0..3 {
        let fill = if level < state.speed {
            "#ddc08b"
        } else {
            "#4a5047"
        };
        write!(
            svg,
            r#"<rect x="{:.1}" y="{:.1}" width="7" height="3" rx="1" fill="{}"/>"#,
            speed.x + speed.width - 10.0,
            speed.y + 4.0 + level as f32 * 5.0,
            fill,
        )
        .expect("Не удалось сформировать индикатор скорости.");
    }
}

fn make_ship() -> LayeredModel {
    let dimensions = [
        (0.0, 94.0, 13.0, "#57391f", "#57391f"),
        (8.0, 116.0, 22.0, "#754b29", "#8b5e33"),
        (16.0, 128.0, 30.0, "#9a6837", "#57391f"),
        (24.0, 122.0, 31.0, "#57391f", "#8b5e33"),
        (32.0, 111.0, 24.0, "#9a6837", "#57391f"),
        (40.0, 80.0, 15.0, "#57391f", "#8b5e33"),
    ];

    LayeredModel {
        slices: dimensions
            .into_iter()
            .map(|(z, length, width, top_color, side_color)| Slice {
                z,
                outline: ship_outline(length, width),
                top_color,
                side_color,
            })
            .collect(),
    }
}

fn make_torfinn() -> LayeredModel {
    let dimensions = [
        (0.0, 7.0, 8.0, "#3d403b", "#57391f"),
        (7.0, 9.0, 9.0, "#444640", "#57391f"),
        (15.0, 10.0, 8.0, "#4a4c45", "#343631"),
        (24.0, 15.0, 10.0, "#84402b", "#343631"),
        (34.0, 17.0, 10.0, "#343631", "#57391f"),
        (43.0, 18.0, 9.0, "#343631", "#8b5e33"),
        (51.0, 7.0, 7.0, "#84402b", "#57391f"),
        (59.0, 9.0, 8.0, "#b58e63", "#57391f"),
        (67.0, 11.0, 9.0, "#936f43", "#57391f"),
        (74.0, 7.0, 7.0, "#936f43", "#343631"),
    ];

    LayeredModel {
        slices: dimensions
            .into_iter()
            .map(|(z, half_x, half_y, top_color, side_color)| Slice {
                z,
                outline: ellipse_outline(half_x, half_y, SIDE_COUNT),
                top_color,
                side_color,
            })
            .collect(),
    }
}

fn ship_outline(length: f32, width: f32) -> Vec<Point2> {
    vec![
        Point2 { x: -length, y: 0.0 },
        Point2 {
            x: -length * 0.72,
            y: -width * 0.55,
        },
        Point2 {
            x: length * 0.42,
            y: -width,
        },
        Point2 { x: length, y: 0.0 },
        Point2 {
            x: length * 0.42,
            y: width,
        },
        Point2 {
            x: -length * 0.72,
            y: width * 0.55,
        },
    ]
}

fn ellipse_outline(half_x: f32, half_y: f32, sides: usize) -> Vec<Point2> {
    (0..sides)
        .map(|index| {
            let angle = index as f32 / sides as f32 * std::f32::consts::TAU;
            Point2 {
                x: angle.cos() * half_x,
                y: angle.sin() * half_y,
            }
        })
        .collect()
}

fn draw_model(
    svg: &mut String,
    model: &LayeredModel,
    center: Point2,
    scale: f32,
    yaw: f32,
    tilt: f32,
    damaged: bool,
) {
    let camera = Point2 {
        x: yaw.sin(),
        y: yaw.cos(),
    };
    let mut faces = Vec::new();

    for slice in &model.slices {
        let center_point = Point3 {
            x: 0.0,
            y: 0.0,
            z: slice.z,
        };
        for edge_index in 0..slice.outline.len() {
            let next = (edge_index + 1) % slice.outline.len();
            faces.push(make_face(
                [
                    center_point,
                    Point3 {
                        x: slice.outline[edge_index].x,
                        y: slice.outline[edge_index].y,
                        z: slice.z,
                    },
                    Point3 {
                        x: slice.outline[next].x,
                        y: slice.outline[next].y,
                        z: slice.z,
                    },
                ],
                slice.top_color,
                yaw,
                tilt,
            ));
        }
    }

    for layer_index in 0..model.slices.len().saturating_sub(1) {
        let lower = &model.slices[layer_index];
        let upper = &model.slices[layer_index + 1];
        let winding = if signed_area(&lower.outline) >= 0.0 {
            1.0
        } else {
            -1.0
        };

        for edge_index in 0..lower.outline.len() {
            if damaged && layer_index == 2 && edge_index == 2 {
                continue;
            }

            let next = (edge_index + 1) % lower.outline.len();
            let a = lower.outline[edge_index];
            let b = lower.outline[next];
            let edge_x = b.x - a.x;
            let edge_y = b.y - a.y;
            let facing = edge_y * winding * camera.x - edge_x * winding * camera.y;
            if facing <= 0.0 {
                continue;
            }

            let upper_a = upper.outline[edge_index];
            let upper_b = upper.outline[next];
            let lower_a_3d = Point3 {
                x: a.x,
                y: a.y,
                z: lower.z,
            };
            let lower_b_3d = Point3 {
                x: b.x,
                y: b.y,
                z: lower.z,
            };
            let upper_a_3d = Point3 {
                x: upper_a.x,
                y: upper_a.y,
                z: upper.z,
            };
            let upper_b_3d = Point3 {
                x: upper_b.x,
                y: upper_b.y,
                z: upper.z,
            };
            let color = if damaged && layer_index == 1 && edge_index == 3 {
                "#9d3929"
            } else {
                lower.side_color
            };
            faces.push(make_face(
                [lower_a_3d, lower_b_3d, upper_b_3d],
                color,
                yaw,
                tilt,
            ));
            faces.push(make_face(
                [lower_a_3d, upper_b_3d, upper_a_3d],
                color,
                yaw,
                tilt,
            ));
        }
    }

    faces.sort_by(|left, right| left.depth.total_cmp(&right.depth));
    for face in faces {
        let points = face
            .points
            .map(|point| project(point, center, scale, yaw, tilt));
        write!(
            svg,
            r#"<polygon points="{:.1},{:.1} {:.1},{:.1} {:.1},{:.1}" fill="{}"/>"#,
            points[0].x,
            points[0].y,
            points[1].x,
            points[1].y,
            points[2].x,
            points[2].y,
            face.color
        )
        .expect("Не удалось сформировать грань SVG.");
    }
}

fn make_face(points: [Point3; 3], color: &'static str, yaw: f32, tilt: f32) -> Face {
    let depth = points
        .iter()
        .map(|point| camera_depth(*point, yaw, tilt))
        .sum::<f32>()
        / 3.0;
    Face {
        points,
        color,
        depth,
    }
}

fn project(point: Point3, center: Point2, scale: f32, yaw: f32, tilt: f32) -> Point2 {
    let (sin_yaw, cos_yaw) = yaw.sin_cos();
    let rotated_x = point.x * cos_yaw - point.y * sin_yaw;
    let rotated_y = point.x * sin_yaw + point.y * cos_yaw;
    Point2 {
        x: center.x + rotated_x * scale,
        y: center.y + (-rotated_y * tilt.cos() - point.z * tilt.sin()) * scale,
    }
}

fn draw_ship_wake(svg: &mut String, center: Point2, scale: f32, yaw: f32, tilt: f32, speed: u8) {
    let wake_length = 95.0 + f32::from(speed) * 38.0;
    for side in [-1.0, 1.0] {
        let hull = project(
            Point3 {
                x: -72.0,
                y: side * 15.0,
                z: 2.0,
            },
            center,
            scale,
            yaw,
            tilt,
        );
        let wake = project(
            Point3 {
                x: -wake_length,
                y: side * 28.0,
                z: 1.0,
            },
            center,
            scale,
            yaw,
            tilt,
        );
        let control = project(
            Point3 {
                x: -wake_length * 0.62,
                y: side * 38.0,
                z: 1.0,
            },
            center,
            scale,
            yaw,
            tilt,
        );
        write!(
            svg,
            r##"<path d="M{:.1} {:.1}Q{:.1} {:.1} {:.1} {:.1}" fill="none" stroke="#d7ddd0" stroke-opacity=".38" stroke-width="{:.1}" stroke-linecap="round"/>"##,
            hull.x,
            hull.y,
            control.x,
            control.y,
            wake.x,
            wake.y,
            1.2 + f32::from(speed) * 0.45,
        )
        .expect("Не удалось сформировать след корабля.");
    }
}

fn camera_depth(point: Point3, yaw: f32, tilt: f32) -> f32 {
    let rotated_y = point.x * yaw.sin() + point.y * yaw.cos();
    rotated_y * tilt.sin() + point.z * tilt.cos()
}

fn signed_area(points: &[Point2]) -> f32 {
    let mut area = 0.0;
    for index in 0..points.len() {
        let next = (index + 1) % points.len();
        area += points[index].x * points[next].y - points[next].x * points[index].y;
    }
    area * 0.5
}

fn draw_sea(
    svg: &mut String,
    distant: bool,
    width: f32,
    height: f32,
    travel_distance: f32,
    ship_heading: f32,
) {
    let spacing = height / if distant { 11.0 } else { 16.0 };
    let horizontal_shift = (travel_distance * ship_heading.cos() * 0.2).rem_euclid(width.max(1.0));
    let vertical_shift = (travel_distance * ship_heading.sin() * 0.2).rem_euclid(spacing);
    for row in 0..17 {
        let y = height * 0.16 + (row as f32 - 2.0) * spacing + vertical_shift;
        let opacity = if row % 3 == 0 { 0.17 } else { 0.09 };
        for copy in 0..=1 {
            let x_shift = horizontal_shift - copy as f32 * width;
            write!(
                svg,
                r##"<path d="M{start:.1} {y:.1} Q{quarter:.1} {control:.1} {half:.1} {y:.1} T{end:.1} {y:.1}" fill="none" stroke="#9aa9a0" stroke-opacity="{opacity}" stroke-width="2"/>"##,
                start = -x_shift,
                quarter = width * 0.25 - x_shift,
                control = y - height * 0.012,
                half = width * 0.5 - x_shift,
                end = width - x_shift,
            )
            .expect("Не удалось сформировать линии моря.");
        }
    }
    write!(
        svg,
        r##"<path d="M0 {top:.1} C{q1:.1} {control:.1} {q2:.1} {top:.1} {half:.1} {top:.1} S{q3:.1} {top:.1} {width:.1} {top:.1} L{width:.1} {bottom:.1} C{q4:.1} {bottom:.1} {q5:.1} {bottom:.1} {half:.1} {bottom:.1} S{q6:.1} {bottom:.1} 0 {bottom:.1}Z" fill="url(#fog)"/>"##,
        top = height * 0.24,
        control = height * 0.20,
        q1 = width * 0.22,
        q2 = width * 0.34,
        half = width * 0.5,
        q3 = width * 0.78,
        bottom = height * 0.33,
        q4 = width * 0.80,
        q5 = width * 0.68,
        q6 = width * 0.22,
    )
    .expect("Не удалось сформировать полосу тумана.");
}

fn draw_ship_details(
    svg: &mut String,
    center: Point2,
    scale: f32,
    yaw: f32,
    tilt: f32,
    damaged: bool,
) {
    let sail_color = if damaged { "#9d3929" } else { "#a98a64" };
    let sail = [
        Point3 {
            x: -35.0,
            y: -17.0,
            z: 43.0,
        },
        Point3 {
            x: 35.0,
            y: -17.0,
            z: 43.0,
        },
        Point3 {
            x: 0.0,
            y: 24.0,
            z: 43.0,
        },
    ]
    .map(|point| project(point, center, scale, yaw, tilt));
    write!(
        svg,
        r##"<polygon points="{:.1},{:.1} {:.1},{:.1} {:.1},{:.1}" fill="{}" fill-opacity=".95" stroke="#34271d" stroke-width="2"/>"##,
        sail[0].x,
        sail[0].y,
        sail[1].x,
        sail[1].y,
        sail[2].x,
        sail[2].y,
        sail_color
    )
    .expect("Не удалось сформировать парус.");

    if tilt > 0.08 {
        let mast_bottom = project(
            Point3 {
                x: 0.0,
                y: 0.0,
                z: 40.0,
            },
            center,
            scale,
            yaw,
            tilt,
        );
        let mast_top = project(
            Point3 {
                x: 0.0,
                y: 0.0,
                z: 112.0,
            },
            center,
            scale,
            yaw,
            tilt,
        );
        write!(
            svg,
            r##"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="#34271d" stroke-width="3"/>"##,
            mast_bottom.x, mast_bottom.y, mast_top.x, mast_top.y
        )
        .expect("Не удалось сформировать мачту.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_tilt_stays_limited_outside_dialogues() {
        let mut camera = CameraState::default();
        camera.apply_key("ArrowUp");
        for _ in 0..100 {
            camera.apply_key("ArrowUp");
        }
        assert_eq!(camera.free_tilt, 43.0);
        assert_eq!(camera.tilt(), 43.0);
        camera.apply_key("C");
        assert_eq!(camera.tilt(), 88.0);
    }

    #[test]
    fn far_sea_view_is_strictly_overhead() {
        let mut camera = CameraState::default();
        camera.apply_key("F");
        assert!(camera.distant_sea);
        assert_eq!(camera.tilt(), 0.0);
    }

    #[test]
    fn damage_changes_the_rendered_scene() {
        let camera = CameraState::default();
        let intact = render_svg(camera);
        let damaged = render_svg(CameraState {
            damaged: true,
            ..camera
        });
        assert_ne!(intact, damaged);
        assert!(damaged.contains("#9d3929"));
    }

    #[test]
    fn ship_slices_have_matching_contours() {
        let ship = make_ship();
        let point_count = ship.slices[0].outline.len();
        assert!(ship.slices.len() >= 4);
        assert!(
            ship.slices
                .iter()
                .all(|slice| slice.outline.len() == point_count)
        );
    }

    #[test]
    fn rendered_scene_tracks_portrait_and_landscape_viewports() {
        let portrait = render_svg(CameraState {
            viewport_width: 402.0,
            viewport_height: 874.0,
            ..CameraState::default()
        });
        let landscape = render_svg(CameraState {
            viewport_width: 874.0,
            viewport_height: 402.0,
            ..CameraState::default()
        });

        assert!(portrait.contains("viewBox=\"0 0 402 874\""));
        assert!(landscape.contains("viewBox=\"0 0 874 402\""));
    }

    #[test]
    fn navigation_hud_is_shared_with_every_render_target() {
        let scene = render_svg(CameraState::default());
        assert!(scene.contains("id=\"navigation-compass\""));
        assert!(scene.contains("id=\"nautical-map\""));
        assert!(scene.contains("id=\"ship-seal\""));
    }

    #[test]
    fn mobile_controls_are_visible_and_match_their_touch_regions() {
        let mut camera = CameraState {
            viewport_width: 402.0,
            viewport_height: 874.0,
            mobile_controls: true,
            ..CameraState::default()
        };
        let controls = sailing_control_layout(402.0, 874.0);
        assert_eq!(
            camera.sailing_control_at(
                controls.rudder.x + controls.rudder.width * 0.5,
                controls.rudder.y + controls.rudder.height * 0.5,
            ),
            Some(SailingControl::Rudder)
        );
        assert_eq!(
            camera.sailing_control_at(
                controls.speed_up.x + controls.speed_up.width * 0.5,
                controls.speed_up.y + controls.speed_up.height * 0.5,
            ),
            Some(SailingControl::SpeedUp)
        );
        assert_eq!(
            camera.sailing_control_at(
                controls.speed_down.x + controls.speed_down.width * 0.5,
                controls.speed_down.y + controls.speed_down.height * 0.5,
            ),
            Some(SailingControl::SpeedDown)
        );
        let mobile_scene = render_svg(camera);
        assert!(mobile_scene.contains("id=\"mobile-sailing-controls\""));
        assert!(mobile_scene.contains("aria-label=\"Рулевое весло\""));

        camera.set_mobile_controls(false);
        assert!(!render_svg(camera).contains("id=\"mobile-sailing-controls\""));
        camera.set_mobile_controls(true);
        camera.apply_key("C");
        assert!(
            camera
                .sailing_control_at(controls.rudder.x, controls.rudder.y)
                .is_none()
        );
    }

    #[test]
    fn speed_moves_the_ship_and_rudder_changes_its_heading() {
        let mut camera = CameraState::default();
        camera.set_rudder(1.0);
        camera.change_speed(8);
        assert_eq!(camera.speed, 3);
        assert!(camera.advance_sailing(0.1));
        assert!(camera.ship_heading > 0.0);
        assert!(camera.travel_distance > 0.0);

        camera.change_speed(-8);
        let distance = camera.travel_distance;
        assert!(!camera.advance_sailing(0.1));
        assert_eq!(camera.speed, 0);
        assert_eq!(camera.travel_distance, distance);
        camera.change_speed(-1);
        assert_eq!(camera.speed, 0);
    }

    #[test]
    fn historical_compass_icon_rotates_without_a_colored_needle() {
        let initial = render_svg(CameraState::default());
        let mut camera = CameraState::default();
        camera.ship_heading = 0.5;
        let turned = render_svg(camera);
        let initial_compass = initial
            .split("id=\"navigation-compass\"")
            .nth(1)
            .unwrap()
            .split("id=\"nautical-map\"")
            .next()
            .unwrap();
        let turned_compass = turned
            .split("id=\"navigation-compass\"")
            .nth(1)
            .unwrap()
            .split("id=\"nautical-map\"")
            .next()
            .unwrap();
        assert!(initial_compass.contains("rotate(-14.32) scale("));
        assert!(turned_compass.contains("rotate(-42.97) scale("));
        assert!(initial.contains(COMPASS_ICON_SVG));
        assert!(initial_compass.contains("class=\"lucide lucide-compass\""));
        assert!(initial_compass.contains("width=\"20\" height=\"20\""));
        assert!(!initial_compass.contains("#a33b2b"));
        assert!(!initial_compass.contains("needle"));
        assert_ne!(initial_compass, turned_compass);
    }

    #[test]
    fn mobile_browser_is_detected_from_standard_device_signals() {
        assert!(detect_mobile_device(
            "Mozilla/5.0 (Linux; Android 14)",
            None,
            "Linux armv8l",
            5,
        ));
        assert!(detect_mobile_device("Mozilla/5.0", Some(true), "Linux", 0,));
        assert!(detect_mobile_device("Mozilla/5.0", None, "MacIntel", 5,));
        assert!(!detect_mobile_device(
            "Mozilla/5.0 (Windows NT 10.0)",
            Some(false),
            "Win32",
            0,
        ));
    }
}
