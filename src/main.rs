use north_seas_adventure::{CameraState, render_svg};

fn main() {
    let output_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/north-seas-preview.svg".to_owned());
    let svg = render_svg(CameraState::default());
    std::fs::write(&output_path, svg)
        .unwrap_or_else(|error| panic!("Не удалось сохранить изображение: {error}"));
    println!("Тестовая сцена сохранена: {output_path}");
}
