use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use js_sys::{Date, JSON, Object, Reflect};
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;
use web_sys::{
    Document, Element, Event, EventTarget, HtmlCanvasElement, KeyboardEvent, Navigator,
    PointerEvent, WheelEvent, Window,
};

use crate::{
    BrowserApiPreference, BrowserGraphicsApi, SailingControl, browser_api_attempts,
    gpu_renderer::GpuRenderer, webgl_renderer::WebGlRenderer,
};

const STORAGE_KEY: &str = "north-seas-adventure.keyboard-bindings.v1";
const GRAPHICS_API_STORAGE_KEY: &str = "north-seas-adventure.graphics-api.v1";

#[derive(Clone, Copy)]
struct Action {
    id: &'static str,
    label: &'static str,
    default_key: &'static str,
    command: &'static str,
}

const ACTIONS: [Action; 12] = [
    Action {
        id: "cameraLeft",
        label: "Поворот влево",
        default_key: "ArrowLeft",
        command: "ArrowLeft",
    },
    Action {
        id: "cameraRight",
        label: "Поворот вправо",
        default_key: "ArrowRight",
        command: "ArrowRight",
    },
    Action {
        id: "cameraUp",
        label: "Наклон камеры вверх",
        default_key: "ArrowUp",
        command: "ArrowUp",
    },
    Action {
        id: "cameraDown",
        label: "Наклон камеры вниз",
        default_key: "ArrowDown",
        command: "ArrowDown",
    },
    Action {
        id: "steerLeft",
        label: "Рулевое весло влево",
        default_key: "a",
        command: "a",
    },
    Action {
        id: "steerRight",
        label: "Рулевое весло вправо",
        default_key: "d",
        command: "d",
    },
    Action {
        id: "speedUp",
        label: "Увеличить ход",
        default_key: "w",
        command: "w",
    },
    Action {
        id: "speedDown",
        label: "Уменьшить ход",
        default_key: "s",
        command: "s",
    },
    Action {
        id: "dialogue",
        label: "Режим диалога",
        default_key: "c",
        command: "c",
    },
    Action {
        id: "damage",
        label: "Повреждение судна",
        default_key: "z",
        command: "z",
    },
    Action {
        id: "distantSea",
        label: "Дальний вид моря",
        default_key: "f",
        command: "f",
    },
    Action {
        id: "reset",
        label: "Сброс камеры",
        default_key: "r",
        command: "r",
    },
];

#[derive(Clone, Copy)]
struct ActiveControl {
    pointer_id: i32,
    kind: SailingControl,
}

#[derive(Clone, Copy)]
struct PointerPosition {
    x: f64,
    y: f64,
}

struct WebState {
    mobile: bool,
    settings_open: bool,
    api_preference: BrowserApiPreference,
    listening_action: Option<usize>,
    bindings: Vec<String>,
    pressed_steering: HashSet<usize>,
    pointers: HashMap<i32, PointerPosition>,
    primary_pointer: Option<i32>,
    pinch_distance: Option<f64>,
    active_control: Option<ActiveControl>,
    last_tap: Option<(f64, f64, f64)>,
    animation_running: bool,
    last_animation_time: Option<f64>,
}

impl Default for WebState {
    fn default() -> Self {
        Self {
            mobile: false,
            settings_open: false,
            api_preference: BrowserApiPreference::Automatic,
            listening_action: None,
            bindings: ACTIONS
                .iter()
                .map(|action| action.default_key.to_owned())
                .collect(),
            pressed_steering: HashSet::new(),
            pointers: HashMap::new(),
            primary_pointer: None,
            pinch_distance: None,
            active_control: None,
            last_tap: None,
            animation_running: false,
            last_animation_time: None,
        }
    }
}

thread_local! {
    static WEB_STATE: RefCell<WebState> = RefCell::new(WebState::default());
    static BROWSER_RENDERER: RefCell<Option<BrowserRenderer>> = const { RefCell::new(None) };
    static EVENT_HANDLERS: RefCell<Vec<Closure<dyn FnMut(Event)>>> = const { RefCell::new(Vec::new()) };
    static ANIMATION_HANDLER: RefCell<Option<Closure<dyn FnMut(f64)>>> = const { RefCell::new(None) };
}

enum BrowserRenderer {
    WebGpu(GpuRenderer),
    WebGl(WebGlRenderer),
}

#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    let window = browser_window()?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("В браузере не найден документ."))?;
    let root = document
        .get_element_by_id("app")
        .ok_or_else(|| JsValue::from_str("В HTML не найден корневой элемент игры."))?;
    let navigator = window.navigator();
    let mobile = is_mobile(&navigator);

    WEB_STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.mobile = mobile;
        state.bindings = load_bindings(&window);
        state.api_preference = load_browser_api_preference(&window);
    });
    crate::set_mobile_controls(mobile);

    root.set_inner_html(&application_markup(mobile));
    render_game_settings(&document);
    install_handlers(&window, &document)?;
    resize_scene(&window, &document);
    let canvas = document
        .get_element_by_id("game-canvas")
        .ok_or_else(|| JsValue::from_str("В HTML не найден холст WebGPU."))?
        .dyn_into::<HtmlCanvasElement>()?;
    let api_preference = WEB_STATE.with(|state| state.borrow().api_preference);
    let renderer_document = document.clone();
    wasm_bindgen_futures::spawn_local(async move {
        match initialize_browser_renderer(&renderer_document, canvas, api_preference).await {
            Ok(renderer) => {
                BROWSER_RENDERER.with(|current| current.replace(Some(renderer)));
                if let Some(window) = web_sys::window() {
                    if let Some(document) = window.document() {
                        resize_scene(&window, &document);
                        hide_loading(&document);
                    }
                }
            }
            Err(error) => {
                show_graphics_error(&renderer_document, api_preference);
                web_sys::console::error_1(&JsValue::from_str(&error));
            }
        }
    });
    Ok(())
}

async fn initialize_browser_renderer(
    document: &Document,
    initial_canvas: HtmlCanvasElement,
    preference: BrowserApiPreference,
) -> Result<BrowserRenderer, String> {
    let attempts = browser_api_attempts(preference);
    let mut errors = Vec::with_capacity(attempts.len());
    let mut canvas = Some(initial_canvas);

    for (index, api) in attempts.iter().copied().enumerate() {
        let current_canvas = if index == 0 {
            canvas
                .take()
                .ok_or_else(|| "Не удалось получить холст игровой сцены.".to_owned())?
        } else {
            replace_game_canvas(document)?
        };
        let result = match api {
            BrowserGraphicsApi::WebGpu => GpuRenderer::new_web(current_canvas)
                .await
                .map(BrowserRenderer::WebGpu),
            BrowserGraphicsApi::WebGl => {
                WebGlRenderer::new(current_canvas).map(BrowserRenderer::WebGl)
            }
        };
        match result {
            Ok(renderer) => {
                if index > 0 {
                    web_sys::console::warn_1(&JsValue::from_str(
                        "WebGPU недоступен; сцена запущена через резервный WebGL.",
                    ));
                }
                return Ok(renderer);
            }
            Err(error) => errors.push(format!("{api:?}: {error}")),
        }
    }

    Err(format!(
        "Не удалось запустить игру через графические API браузера. {}",
        errors.join(" ")
    ))
}

fn replace_game_canvas(document: &Document) -> Result<HtmlCanvasElement, String> {
    let scene = document_element(document, "game-scene")
        .ok_or_else(|| "В игре не найден контейнер сцены.".to_owned())?;
    scene.set_inner_html(r#"<canvas id="game-canvas" aria-label="Игровая сцена"></canvas>"#);
    document_element(document, "game-canvas")
        .ok_or_else(|| "Не удалось создать новый холст игровой сцены.".to_owned())?
        .dyn_into::<HtmlCanvasElement>()
        .map_err(|error| format!("Новый холст не является элементом canvas: {error:?}"))
}

fn browser_window() -> Result<Window, JsValue> {
    web_sys::window().ok_or_else(|| JsValue::from_str("Окно браузера недоступно."))
}

fn is_mobile(navigator: &Navigator) -> bool {
    let user_agent = navigator.user_agent().unwrap_or_default();
    let platform = navigator.platform().unwrap_or_default();
    let touch_points = navigator.max_touch_points() as u32;
    let user_agent_data = Reflect::get(navigator.as_ref(), &JsValue::from_str("userAgentData"))
        .ok()
        .and_then(|value| Reflect::get(&value, &JsValue::from_str("mobile")).ok())
        .and_then(|value| value.as_bool());
    crate::detect_mobile_device(&user_agent, user_agent_data, &platform, touch_points)
}

fn application_markup(mobile: bool) -> String {
    format!(
        r#"<main class="game-shell" data-device="{}">
<section class="game-stage" aria-label="Игровая сцена">
<div id="game-scene" class="game-canvas" role="application" aria-label="Игровая сцена" tabindex="0"><canvas id="game-canvas" aria-label="Игровая сцена"></canvas></div>
<div id="loading" class="stage-overlay" role="status"><div class="load-mark"></div><h2>Поднимаем парус</h2><p>Загружается игровая сцена…</p></div>
</section>
<button id="settings-button" class="hud-settings-button" type="button" data-ui-action="toggle-settings" aria-label="Настройки" aria-expanded="false"><svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><path d="M4 7h16M4 17h16"/><circle cx="8" cy="7" r="2"/><circle cx="16" cy="17" r="2"/></svg></button>
<section id="game-settings" class="game-settings" aria-label="Настройки игры" hidden></section>
</main>"#,
        if mobile { "mobile" } else { "desktop" }
    )
}

fn document_element(document: &Document, id: &str) -> Option<Element> {
    document.get_element_by_id(id)
}

fn render_scene_into(document: &Document) {
    let Some(canvas) = document_element(document, "game-canvas")
        .and_then(|element| element.dyn_into::<HtmlCanvasElement>().ok())
    else {
        return;
    };
    let Some(scene) = document_element(document, "game-scene") else {
        return;
    };
    let rect = scene.get_bounding_client_rect();
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let ratio = web_sys::window()
        .map(|window| window.device_pixel_ratio().clamp(1.0, 3.0))
        .unwrap_or(1.0);
    let width = (rect.width() * ratio).round().max(1.0) as u32;
    let height = (rect.height() * ratio).round().max(1.0) as u32;
    if canvas.width() != width {
        canvas.set_width(width);
    }
    if canvas.height() != height {
        canvas.set_height(height);
    }
    let Ok(mesh) = crate::current_scene_mesh() else {
        return;
    };
    BROWSER_RENDERER.with(|renderer| {
        if let Some(renderer) = renderer.borrow_mut().as_mut() {
            let result = match renderer {
                BrowserRenderer::WebGpu(renderer) => renderer.render(&mesh, width, height),
                BrowserRenderer::WebGl(renderer) => renderer.render(&mesh, width, height),
            };
            if let Err(error) = result {
                web_sys::console::error_1(&JsValue::from_str(&error));
            }
        }
    });
}

fn hide_loading(document: &Document) {
    if let Some(loading) = document_element(document, "loading") {
        let _ = loading.set_attribute("hidden", "");
    }
}

fn show_graphics_loading(document: &Document) {
    if let Some(loading) = document_element(document, "loading") {
        let _ = loading.set_attribute("class", "stage-overlay");
        let _ = loading.remove_attribute("hidden");
        loading.set_inner_html(
            r#"<div class="load-mark"></div><h2>Поднимаем парус</h2><p>Загружается игровая сцена…</p>"#,
        );
    }
}

fn show_graphics_error(document: &Document, preference: BrowserApiPreference) {
    if let Some(loading) = document_element(document, "loading") {
        let _ = loading.set_attribute("class", "stage-overlay stage-overlay-error");
        loading.set_text_content(Some(preference.unavailable_message()));
    }
}

fn render_game_settings(document: &Document) {
    let Some(panel) = document_element(document, "game-settings") else {
        return;
    };
    let (open, listening, bindings, mobile, api_preference) = WEB_STATE.with(|state| {
        let state = state.borrow();
        (
            state.settings_open,
            state.listening_action,
            state.bindings.clone(),
            state.mobile,
            state.api_preference,
        )
    });
    let rows = ACTIONS
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let key = display_key(&bindings[index]);
            let value = if listening == Some(index) {
                "Нажмите клавишу…".to_owned()
            } else {
                escape_html(&key)
            };
            format!(
                r#"<div class="binding-row"><span>{}</span><button class="binding-key" type="button" data-ui-action="bind" data-bind-action="{}" aria-label="{}: {}">{}</button></div>"#,
                action.label,
                action.id,
                action.label,
                value,
                value
            )
        })
        .collect::<String>();
    let api_options = [
        (BrowserApiPreference::Automatic, "Автоматически"),
        (BrowserApiPreference::WebGpu, "WebGPU"),
        (BrowserApiPreference::WebGl, "WebGL"),
    ]
    .into_iter()
    .map(|(preference, label)| {
        let pressed = preference == api_preference;
        format!(
            r#"<button class="graphics-api-choice" type="button" data-ui-action="select-browser-api" data-browser-api="{}" aria-pressed="{}">{}</button>"#,
            preference.as_storage_value(),
            pressed,
            label
        )
    })
    .collect::<String>();
    let keyboard_settings = if mobile {
        String::new()
    } else {
        format!(
            r#"<section class="settings-section" aria-label="Настройки клавиатуры"><h3>Клавиатура</h3><div class="binding-list">{rows}</div><button class="bindings-reset" type="button" data-ui-action="reset-bindings">Сбросить привязки</button></section>"#
        )
    };
    panel.set_inner_html(&format!(
        r#"<div class="settings-heading"><h2>Настройки</h2><button class="settings-close" type="button" data-ui-action="close-settings" aria-label="Закрыть">×</button></div><section class="settings-section" aria-label="Настройки графики"><h3>Графический API</h3><div class="graphics-api-options" role="group" aria-label="Выбор графического API">{api_options}</div></section>{keyboard_settings}"#
    ));
    let settings_button = document_element(document, "settings-button");
    if open {
        let _ = panel.remove_attribute("hidden");
        if let Some(button) = settings_button {
            let _ = button.set_attribute("aria-expanded", "true");
        }
    } else {
        let _ = panel.set_attribute("hidden", "");
        if let Some(button) = settings_button {
            let _ = button.set_attribute("aria-expanded", "false");
        }
    }
}

fn display_key(key: &str) -> String {
    match key {
        "ArrowLeft" => "←".to_owned(),
        "ArrowRight" => "→".to_owned(),
        "ArrowUp" => "↑".to_owned(),
        "ArrowDown" => "↓".to_owned(),
        " " => "Пробел".to_owned(),
        "Escape" => "Esc".to_owned(),
        _ => key.to_owned(),
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn load_bindings(window: &Window) -> Vec<String> {
    let mut bindings = ACTIONS
        .iter()
        .map(|action| action.default_key.to_owned())
        .collect::<Vec<_>>();
    let Some(storage) = window.local_storage().ok().flatten() else {
        return bindings;
    };
    let Some(serialized) = storage.get_item(STORAGE_KEY).ok().flatten() else {
        return bindings;
    };
    let Ok(value) = JSON::parse(&serialized) else {
        return bindings;
    };
    for (index, action) in ACTIONS.iter().enumerate() {
        let Ok(key) = Reflect::get(&value, &JsValue::from_str(action.id)) else {
            continue;
        };
        if let Some(key) = key.as_string().filter(|key| !key.is_empty()) {
            bindings[index] = key;
        }
    }
    bindings
}

fn save_bindings(window: &Window, bindings: &[String]) {
    let Some(storage) = window.local_storage().ok().flatten() else {
        return;
    };
    let values = Object::new();
    for (index, action) in ACTIONS.iter().enumerate() {
        if let Some(key) = bindings.get(index) {
            let _ = Reflect::set(
                &values,
                &JsValue::from_str(action.id),
                &JsValue::from_str(key),
            );
        }
    }
    if let Ok(serialized) = JSON::stringify(&values)
        && let Some(serialized) = serialized.as_string()
    {
        let _ = storage.set_item(STORAGE_KEY, &serialized);
    }
}

fn load_browser_api_preference(window: &Window) -> BrowserApiPreference {
    let Ok(Some(storage)) = window.local_storage() else {
        return BrowserApiPreference::Automatic;
    };
    let Ok(Some(value)) = storage.get_item(GRAPHICS_API_STORAGE_KEY) else {
        return BrowserApiPreference::Automatic;
    };
    BrowserApiPreference::from_storage_value(&value).unwrap_or_else(|| {
        web_sys::console::error_1(&JsValue::from_str(
            "В сохранённых настройках указан неизвестный графический API; используется автоматический режим.",
        ));
        BrowserApiPreference::Automatic
    })
}

fn save_browser_api_preference(
    window: &Window,
    preference: BrowserApiPreference,
) -> Result<(), String> {
    let storage = window
        .local_storage()
        .map_err(|error| format!("Не удалось открыть локальное хранилище: {error:?}"))?
        .ok_or_else(|| "Браузер не предоставил локальное хранилище.".to_owned())?;
    storage
        .set_item(GRAPHICS_API_STORAGE_KEY, preference.as_storage_value())
        .map_err(|error| format!("Не удалось сохранить графический API: {error:?}"))
}

fn listen(
    target: &EventTarget,
    event_name: &str,
    handler: impl FnMut(Event) + 'static,
) -> Result<(), JsValue> {
    let closure = Closure::wrap(Box::new(handler) as Box<dyn FnMut(Event)>);
    target.add_event_listener_with_callback(event_name, closure.as_ref().unchecked_ref())?;
    EVENT_HANDLERS.with(|handlers| handlers.borrow_mut().push(closure));
    Ok(())
}

fn install_handlers(window: &Window, document: &Document) -> Result<(), JsValue> {
    let scene = document_element(document, "game-scene")
        .ok_or_else(|| JsValue::from_str("В игре не найден холст сцены."))?;
    let root = document_element(document, "app")
        .ok_or_else(|| JsValue::from_str("В игре не найден корневой элемент."))?;
    let window_target: &EventTarget = window.unchecked_ref();
    let scene_target: &EventTarget = scene.unchecked_ref();
    let root_target: &EventTarget = root.unchecked_ref();

    listen(root_target, "click", move |event| {
        handle_click(event);
    })?;

    let scene_for_pointer = scene.clone();
    listen(scene_target, "pointerdown", move |event| {
        if let Some(event) = event.dyn_ref::<PointerEvent>() {
            handle_pointer_down(event, &scene_for_pointer);
        }
    })?;
    let scene_for_move = scene.clone();
    listen(scene_target, "pointermove", move |event| {
        if let Some(event) = event.dyn_ref::<PointerEvent>() {
            handle_pointer_move(event, &scene_for_move);
        }
    })?;
    let scene_for_end = scene.clone();
    listen(scene_target, "pointerup", move |event| {
        if let Some(event) = event.dyn_ref::<PointerEvent>() {
            handle_pointer_end(event, &scene_for_end, true);
        }
    })?;
    let scene_for_cancel = scene.clone();
    listen(scene_target, "pointercancel", move |event| {
        if let Some(event) = event.dyn_ref::<PointerEvent>() {
            handle_pointer_end(event, &scene_for_cancel, false);
        }
    })?;
    let scene_for_wheel = scene.clone();
    listen(scene_target, "wheel", move |event| {
        if let Some(event) = event.dyn_ref::<WheelEvent>() {
            event.prevent_default();
            crate::wheel_input(event.delta_y() as f32);
            if let Some(document) = web_sys::window().and_then(|window| window.document()) {
                render_scene_into(&document);
            }
        }
    })?;
    listen(window_target, "keydown", move |event| {
        if let Some(event) = event.dyn_ref::<KeyboardEvent>() {
            handle_key_down(event);
        }
    })?;
    listen(window_target, "keyup", move |event| {
        if let Some(event) = event.dyn_ref::<KeyboardEvent>() {
            handle_key_up(event);
        }
    })?;
    listen(window_target, "blur", move |_| {
        WEB_STATE.with(|state| state.borrow_mut().pressed_steering.clear());
        crate::rudder_input(0.0);
        start_sailing();
    })?;
    let window_for_resize = window.clone();
    let document_for_resize = document.clone();
    listen(window_target, "resize", move |_| {
        resize_scene(&window_for_resize, &document_for_resize);
    })?;
    let window_for_orientation = window.clone();
    let document_for_orientation = document.clone();
    listen(window_target, "orientationchange", move |_| {
        resize_scene(&window_for_orientation, &document_for_orientation);
    })?;
    let _ = scene_for_wheel;

    ANIMATION_HANDLER.with(|handler| {
        if handler.borrow().is_none() {
            *handler.borrow_mut() = Some(Closure::wrap(Box::new(animate) as Box<dyn FnMut(f64)>));
        }
    });
    Ok(())
}

fn handle_click(event: Event) {
    let Some(target) = event
        .target()
        .and_then(|target| target.dyn_into::<Element>().ok())
    else {
        return;
    };
    let Ok(Some(button)) = target.closest("[data-ui-action]") else {
        return;
    };
    let Some(action) = button.get_attribute("data-ui-action") else {
        return;
    };
    event.prevent_default();
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    match action.as_str() {
        "toggle-settings" => {
            WEB_STATE.with(|state| {
                let mut state = state.borrow_mut();
                state.settings_open = !state.settings_open;
                if state.settings_open {
                    state.pressed_steering.clear();
                }
            });
            let close = WEB_STATE.with(|state| state.borrow().settings_open);
            if close {
                crate::rudder_input(0.0);
            }
            render_game_settings(&document);
        }
        "close-settings" => {
            WEB_STATE.with(|state| {
                let mut state = state.borrow_mut();
                state.settings_open = false;
                state.listening_action = None;
            });
            render_game_settings(&document);
        }
        "select-browser-api" => {
            if let Some(value) = button.get_attribute("data-browser-api")
                && let Some(preference) = BrowserApiPreference::from_storage_value(&value)
            {
                select_browser_api(window, document, preference);
            }
        }
        "reset-bindings" => {
            let bindings = ACTIONS
                .iter()
                .map(|action| action.default_key.to_owned())
                .collect::<Vec<_>>();
            WEB_STATE.with(|state| state.borrow_mut().bindings = bindings.clone());
            save_bindings(&window, &bindings);
            render_game_settings(&document);
        }
        "bind" => {
            if let Some(id) = button.get_attribute("data-bind-action")
                && let Some(index) = ACTIONS.iter().position(|action| action.id == id)
            {
                WEB_STATE.with(|state| state.borrow_mut().listening_action = Some(index));
                render_game_settings(&document);
            }
        }
        _ => {}
    }
}

fn select_browser_api(window: Window, document: Document, preference: BrowserApiPreference) {
    let changed = WEB_STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.api_preference == preference {
            false
        } else {
            state.api_preference = preference;
            true
        }
    });
    if !changed {
        return;
    }

    if let Err(error) = save_browser_api_preference(&window, preference) {
        web_sys::console::error_1(&JsValue::from_str(&error));
    }
    render_game_settings(&document);
    BROWSER_RENDERER.with(|renderer| {
        renderer.borrow_mut().take();
    });

    let canvas = match replace_game_canvas(&document) {
        Ok(canvas) => canvas,
        Err(error) => {
            show_graphics_error(&document, preference);
            web_sys::console::error_1(&JsValue::from_str(&error));
            return;
        }
    };
    show_graphics_loading(&document);
    let renderer_document = document.clone();
    wasm_bindgen_futures::spawn_local(async move {
        match initialize_browser_renderer(&renderer_document, canvas, preference).await {
            Ok(renderer) => {
                BROWSER_RENDERER.with(|current| current.replace(Some(renderer)));
                resize_scene(&window, &renderer_document);
                hide_loading(&renderer_document);
            }
            Err(error) => {
                show_graphics_error(&renderer_document, preference);
                web_sys::console::error_1(&JsValue::from_str(&error));
            }
        }
    });
}

fn handle_key_down(event: &KeyboardEvent) {
    if event.alt_key() || event.ctrl_key() || event.meta_key() {
        return;
    }
    let key = event.key();
    let listening = WEB_STATE.with(|state| state.borrow().listening_action);
    if let Some(index) = listening {
        event.prevent_default();
        event.stop_propagation();
        if key == "Escape" {
            WEB_STATE.with(|state| state.borrow_mut().listening_action = None);
        } else {
            update_binding(index, &key);
        }
        if let Some(document) = web_sys::window().and_then(|window| window.document()) {
            render_game_settings(&document);
        }
        return;
    }

    let action_index = WEB_STATE.with(|state| {
        let state = state.borrow();
        if state.mobile || state.settings_open {
            return None;
        }
        state
            .bindings
            .iter()
            .position(|binding| binding.eq_ignore_ascii_case(&key))
    });
    let Some(index) = action_index else {
        return;
    };
    event.prevent_default();
    match index {
        4 | 5 => {
            WEB_STATE.with(|state| {
                state.borrow_mut().pressed_steering.insert(index);
            });
            update_rudder_from_keyboard();
            start_sailing();
        }
        6 | 7 => {
            if !event.repeat() {
                crate::speed_input(if index == 6 { 1 } else { -1 });
                render_from_window();
                start_sailing();
            }
        }
        _ => {
            if !event.repeat() {
                crate::key_input(ACTIONS[index].command);
                render_from_window();
            }
        }
    }
}

fn handle_key_up(event: &KeyboardEvent) {
    let key = event.key();
    let action_index = WEB_STATE.with(|state| {
        state
            .borrow()
            .bindings
            .iter()
            .position(|binding| binding.eq_ignore_ascii_case(&key))
    });
    let Some(index @ (4 | 5)) = action_index else {
        return;
    };
    event.prevent_default();
    WEB_STATE.with(|state| {
        state.borrow_mut().pressed_steering.remove(&index);
    });
    update_rudder_from_keyboard();
    start_sailing();
}

fn update_binding(index: usize, key: &str) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let bindings = WEB_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(previous_key) = state.bindings.get(index).cloned() else {
            return state.bindings.clone();
        };
        if let Some(conflict) =
            state
                .bindings
                .iter()
                .enumerate()
                .find_map(|(candidate, binding)| {
                    (candidate != index && binding.eq_ignore_ascii_case(key)).then_some(candidate)
                })
        {
            state.bindings[conflict] = previous_key;
        }
        state.bindings[index] = key.to_owned();
        state.listening_action = None;
        state.bindings.clone()
    });
    save_bindings(&window, &bindings);
}

fn update_rudder_from_keyboard() {
    let rudder = WEB_STATE.with(|state| {
        let state = state.borrow();
        if state.pressed_steering.contains(&4) {
            -1.0
        } else if state.pressed_steering.contains(&5) {
            1.0
        } else {
            0.0
        }
    });
    crate::rudder_input(rudder);
    render_from_window();
}

fn handle_pointer_down(event: &PointerEvent, scene: &Element) {
    if event.pointer_type() == "mouse"
        && let Some(scene) = scene.dyn_ref::<web_sys::HtmlElement>()
    {
        let _ = scene.focus();
    }
    let pointer_id = event.pointer_id();
    let position = PointerPosition {
        x: f64::from(event.client_x()),
        y: f64::from(event.client_y()),
    };
    WEB_STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.pointers.insert(pointer_id, position);
        if state.primary_pointer.is_none() {
            state.primary_pointer = Some(pointer_id);
        }
        if state.pointers.len() >= 2 {
            if state
                .active_control
                .is_some_and(|active| matches!(active.kind, SailingControl::Rudder))
            {
                crate::rudder_input(0.0);
            }
            state.active_control = None;
            state.pinch_distance = distance_between_pointers(&state.pointers);
        }
    });
    let _ = scene.set_pointer_capture(pointer_id);

    if !WEB_STATE.with(|state| state.borrow().mobile) {
        return;
    }
    let rect = scene.get_bounding_client_rect();
    let local_x = f64::from(event.client_x()) - rect.left();
    let local_y = f64::from(event.client_y()) - rect.top();
    let Some(kind) = sailing_control_at(local_x as f32, local_y as f32) else {
        return;
    };
    WEB_STATE.with(|state| {
        state.borrow_mut().active_control = Some(ActiveControl { pointer_id, kind });
    });
    event.prevent_default();
    match kind {
        SailingControl::Rudder => set_rudder_from_pointer(pointer_id, local_x),
        SailingControl::SpeedUp => {
            crate::speed_input(1);
            render_from_window();
            start_sailing();
        }
        SailingControl::SpeedDown => {
            crate::speed_input(-1);
            render_from_window();
            start_sailing();
        }
    }
}

fn handle_pointer_move(event: &PointerEvent, scene: &Element) {
    let pointer_id = event.pointer_id();
    let position = PointerPosition {
        x: f64::from(event.client_x()),
        y: f64::from(event.client_y()),
    };
    let (previous, active_control, count, primary, pinch_before, pinch_after) =
        WEB_STATE.with(|state| {
            let mut state = state.borrow_mut();
            let previous = state.pointers.insert(pointer_id, position);
            let active = state.active_control;
            let count = state.pointers.len();
            let primary = state.primary_pointer;
            let before = state.pinch_distance;
            let after = if count >= 2 {
                distance_between_pointers(&state.pointers)
            } else {
                None
            };
            if count >= 2 {
                state.pinch_distance = after;
            }
            (previous, active, count, primary, before, after)
        });

    if let Some(active) = active_control
        && active.pointer_id == pointer_id
    {
        if matches!(active.kind, SailingControl::Rudder) {
            let scene_left = scene.get_bounding_client_rect().left();
            set_rudder_from_pointer(pointer_id, f64::from(event.client_x()) - scene_left);
        }
        event.prevent_default();
        return;
    }
    if count >= 2 {
        if let (Some(previous), Some(next)) = (pinch_before, pinch_after) {
            crate::wheel_input((previous - next) as f32);
            render_from_window();
        }
        event.prevent_default();
        return;
    }
    if primary == Some(pointer_id)
        && let Some(previous) = previous
    {
        crate::drag_input(
            (position.x - previous.x) as f32,
            (position.y - previous.y) as f32,
        );
        render_from_window();
    }
    let _ = scene;
}

fn handle_pointer_end(event: &PointerEvent, _scene: &Element, is_up: bool) {
    let pointer_id = event.pointer_id();
    let active = WEB_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let active = state
            .active_control
            .filter(|active| active.pointer_id == pointer_id);
        if active.is_some() {
            state.active_control = None;
        }
        state.pointers.remove(&pointer_id);
        if state.primary_pointer == Some(pointer_id) {
            state.primary_pointer = state.pointers.keys().next().copied();
        }
        if state.pointers.len() < 2 {
            state.pinch_distance = None;
        }
        active
    });
    if let Some(active) = active {
        if matches!(active.kind, SailingControl::Rudder) {
            crate::rudder_input(0.0);
            render_from_window();
        }
        start_sailing();
        event.prevent_default();
        return;
    }

    if is_up && event.pointer_type() != "mouse" {
        let now = Date::now();
        let current = (
            now,
            f64::from(event.client_x()),
            f64::from(event.client_y()),
        );
        let is_double_tap = WEB_STATE.with(|state| {
            let mut state = state.borrow_mut();
            let double = state.last_tap.is_some_and(|(time, x, y)| {
                now - time < 330.0 && (current.1 - x).hypot(current.2 - y) < 32.0
            });
            state.last_tap = if double { None } else { Some(current) };
            double
        });
        if is_double_tap {
            crate::key_input("C");
            render_from_window();
        }
    }
}

fn sailing_control_at(x: f32, y: f32) -> Option<SailingControl> {
    crate::CAMERA_STATE.with(|state| state.borrow().sailing_control_at(x, y))
}

fn set_rudder_from_pointer(pointer_id: i32, local_x: f64) {
    let active = WEB_STATE.with(|state| state.borrow().active_control);
    if !active.is_some_and(|active| {
        active.pointer_id == pointer_id && matches!(active.kind, SailingControl::Rudder)
    }) {
        return;
    }
    crate::CAMERA_STATE.with(|state| state.borrow_mut().set_rudder_from_screen_x(local_x as f32));
    render_from_window();
    start_sailing();
}

fn distance_between_pointers(pointers: &HashMap<i32, PointerPosition>) -> Option<f64> {
    let mut points = pointers.values();
    let first = points.next()?;
    let second = points.next()?;
    Some((first.x - second.x).hypot(first.y - second.y))
}

fn resize_scene(window: &Window, document: &Document) {
    let Some(scene) = document_element(document, "game-scene") else {
        return;
    };
    let rect = scene.get_bounding_client_rect();
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    crate::resize_scene(rect.width() as f32, rect.height() as f32);
    render_scene_into(document);
    let _ = window;
}

fn render_from_window() {
    if let Some(document) = web_sys::window().and_then(|window| window.document()) {
        render_scene_into(&document);
    }
}

fn start_sailing() {
    let should_start = WEB_STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.animation_running {
            false
        } else {
            state.animation_running = true;
            true
        }
    });
    if !should_start {
        return;
    }
    request_animation_frame();
}

fn request_animation_frame() {
    let Some(window) = web_sys::window() else {
        return;
    };
    ANIMATION_HANDLER.with(|handler| {
        let handler = handler.borrow();
        if let Some(handler) = handler.as_ref() {
            let _ = window.request_animation_frame(handler.as_ref().unchecked_ref());
        }
    });
}

fn animate(time: f64) {
    let elapsed = WEB_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let elapsed = state
            .last_animation_time
            .map(|previous| ((time - previous) / 1000.0).clamp(0.0, 0.1) as f32)
            .unwrap_or(1.0 / 60.0);
        state.last_animation_time = Some(time);
        elapsed
    });
    let moving = crate::advance_sailing(elapsed);
    render_from_window();
    if moving {
        request_animation_frame();
    } else {
        WEB_STATE.with(|state| {
            let mut state = state.borrow_mut();
            state.animation_running = false;
            state.last_animation_time = None;
        });
    }
}
