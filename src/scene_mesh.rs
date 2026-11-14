use tiny_skia_path::{PathSegment, Point};
use usvg::{FillRule, Node, Paint, Tree};

use crate::{CameraState, render_svg};

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
}

#[derive(Debug, Default)]
pub struct SceneMesh {
    pub width: f32,
    pub height: f32,
    pub vertices: Vec<Vertex>,
}

#[derive(Clone, Copy)]
struct Point2 {
    x: f32,
    y: f32,
}

struct Contour {
    points: Vec<Point2>,
    closed: bool,
}

pub fn build_scene_mesh(state: CameraState) -> Result<SceneMesh, String> {
    let width = state.viewport_width.clamp(320.0, 3000.0);
    let height = state.viewport_height.clamp(320.0, 3000.0);
    let svg = render_svg(state);
    mesh_from_svg(&svg, width, height)
}

fn mesh_from_svg(svg: &str, width: f32, height: f32) -> Result<SceneMesh, String> {
    let options = usvg::Options::default();
    let tree = Tree::from_str(svg, &options)
        .map_err(|error| format!("Не удалось разобрать игровую сцену: {error}"))?;
    let mut mesh = SceneMesh {
        width,
        height,
        vertices: Vec::with_capacity(4096),
    };
    draw_group(&tree.root(), 1.0, &mut mesh.vertices)?;
    if mesh.vertices.is_empty() {
        return Err("Игровая сцена не содержит графических элементов.".to_owned());
    }
    Ok(mesh)
}

fn draw_group(
    group: &usvg::Group,
    parent_opacity: f32,
    vertices: &mut Vec<Vertex>,
) -> Result<(), String> {
    let opacity = parent_opacity * group.opacity().get();
    for node in group.children() {
        match node {
            Node::Group(group) => draw_group(group, opacity, vertices)?,
            Node::Path(path) if path.is_visible() => {
                draw_path(path, opacity, vertices)?;
            }
            Node::Path(_) | Node::Image(_) | Node::Text(_) => {}
        }
    }
    Ok(())
}

fn draw_path(
    path: &usvg::Path,
    group_opacity: f32,
    vertices: &mut Vec<Vertex>,
) -> Result<(), String> {
    let contours = flatten_path(path);
    let bounds = path.abs_bounding_box();

    if let Some(fill) = path.fill() {
        for contour in &contours {
            let triangles = triangulate(&contour.points, fill.rule()).map_err(|error| {
                format!(
                    "{error} Контур: {} точек, границы ({:.1}, {:.1})—({:.1}, {:.1}).",
                    contour.points.len(),
                    bounds.x(),
                    bounds.y(),
                    bounds.right(),
                    bounds.bottom(),
                )
            })?;
            for triangle in triangles {
                for point in triangle {
                    let color = paint_color(
                        fill.paint(),
                        bounds.x(),
                        bounds.y(),
                        bounds.width(),
                        bounds.height(),
                        point,
                        fill.opacity().get() * group_opacity,
                    );
                    vertices.push(Vertex {
                        position: [point.x, point.y],
                        color,
                    });
                }
            }
        }
    }

    if let Some(stroke) = path.stroke() {
        let color = match stroke.paint() {
            Paint::Color(color) => [
                f32::from(color.red) / 255.0,
                f32::from(color.green) / 255.0,
                f32::from(color.blue) / 255.0,
                stroke.opacity().get() * group_opacity,
            ],
            Paint::LinearGradient(gradient) => {
                let stop = gradient
                    .stops()
                    .first()
                    .ok_or("У градиента нет цветовых точек.")?;
                let color = stop.color();
                [
                    f32::from(color.red) / 255.0,
                    f32::from(color.green) / 255.0,
                    f32::from(color.blue) / 255.0,
                    stop.opacity().get() * stroke.opacity().get() * group_opacity,
                ]
            }
            Paint::RadialGradient(gradient) => {
                let stop = gradient
                    .stops()
                    .first()
                    .ok_or("У градиента нет цветовых точек.")?;
                let color = stop.color();
                [
                    f32::from(color.red) / 255.0,
                    f32::from(color.green) / 255.0,
                    f32::from(color.blue) / 255.0,
                    stop.opacity().get() * stroke.opacity().get() * group_opacity,
                ]
            }
            Paint::Pattern(_) => return Err("Штриховка SVG-узлов не поддерживается.".to_owned()),
        };
        let width = stroke.width().get() * path.abs_transform().get_scale().0.max(0.01);
        for contour in contours {
            stroke_contour(&contour, width, stroke.linecap(), color, vertices);
        }
    }
    Ok(())
}

fn flatten_path(path: &usvg::Path) -> Vec<Contour> {
    let mut contours = Vec::new();
    let mut points = Vec::new();
    let mut current = Point2 { x: 0.0, y: 0.0 };
    let mut contour_start = Point2 { x: 0.0, y: 0.0 };
    let transform = path.abs_transform();

    for segment in path.data().segments() {
        match segment {
            PathSegment::MoveTo(point) => {
                push_contour(&mut contours, &mut points, false);
                current = from_point(point);
                contour_start = current;
                points.push(transform_point(transform, current));
            }
            PathSegment::LineTo(point) => {
                current = from_point(point);
                points.push(transform_point(transform, current));
            }
            PathSegment::QuadTo(control, end) => {
                let control = from_point(control);
                let end = from_point(end);
                let start = current;
                for step in 1..=8 {
                    let t = step as f32 / 8.0;
                    let inverse = 1.0 - t;
                    let point = Point2 {
                        x: inverse * inverse * start.x
                            + 2.0 * inverse * t * control.x
                            + t * t * end.x,
                        y: inverse * inverse * start.y
                            + 2.0 * inverse * t * control.y
                            + t * t * end.y,
                    };
                    points.push(transform_point(transform, point));
                }
                current = end;
            }
            PathSegment::CubicTo(first, second, end) => {
                let first = from_point(first);
                let second = from_point(second);
                let end = from_point(end);
                let start = current;
                for step in 1..=12 {
                    let t = step as f32 / 12.0;
                    let inverse = 1.0 - t;
                    let point = Point2 {
                        x: inverse.powi(3) * start.x
                            + 3.0 * inverse.powi(2) * t * first.x
                            + 3.0 * inverse * t * t * second.x
                            + t.powi(3) * end.x,
                        y: inverse.powi(3) * start.y
                            + 3.0 * inverse.powi(2) * t * first.y
                            + 3.0 * inverse * t * t * second.y
                            + t.powi(3) * end.y,
                    };
                    points.push(transform_point(transform, point));
                }
                current = end;
            }
            PathSegment::Close => {
                current = contour_start;
                push_contour(&mut contours, &mut points, true);
            }
        }
    }
    push_contour(&mut contours, &mut points, false);
    contours
}

fn from_point(point: Point) -> Point2 {
    Point2 {
        x: point.x,
        y: point.y,
    }
}

fn transform_point(transform: usvg::Transform, point: Point2) -> Point2 {
    let mut point = Point::from_xy(point.x, point.y);
    transform.map_point(&mut point);
    from_point(point)
}

fn push_contour(contours: &mut Vec<Contour>, points: &mut Vec<Point2>, closed: bool) {
    if points.len() >= 2 {
        contours.push(Contour {
            points: std::mem::take(points),
            closed,
        });
    } else {
        points.clear();
    }
}

fn triangulate(points: &[Point2], fill_rule: FillRule) -> Result<Vec<[Point2; 3]>, String> {
    let mut contour = points.to_vec();
    remove_duplicate_points(&mut contour);
    remove_collinear_points(&mut contour);
    if contour.len() < 3 {
        return Ok(Vec::new());
    }
    if fill_rule == FillRule::EvenOdd && contour.len() < 3 {
        return Ok(Vec::new());
    }

    let area = signed_area(&contour);
    if area.abs() < 0.001 {
        return Ok(Vec::new());
    }
    let orientation = area.signum();
    let mut indices = (0..contour.len()).collect::<Vec<_>>();
    let mut triangles = Vec::with_capacity(contour.len() - 2);

    while indices.len() > 3 {
        let mut ear = None;
        for position in 0..indices.len() {
            let previous = indices[(position + indices.len() - 1) % indices.len()];
            let current = indices[position];
            let next = indices[(position + 1) % indices.len()];
            let a = contour[previous];
            let b = contour[current];
            let c = contour[next];
            let cross = cross(a, b, c) * orientation;
            if cross <= 0.0001 {
                continue;
            }
            if indices.iter().copied().any(|candidate| {
                candidate != previous
                    && candidate != current
                    && candidate != next
                    && point_in_triangle(contour[candidate], a, b, c)
            }) {
                continue;
            }
            ear = Some((position, [a, b, c]));
            break;
        }
        let Some((position, triangle)) = ear else {
            return Err("Не удалось триангулировать контур игровой сцены.".to_owned());
        };
        triangles.push(triangle);
        indices.remove(position);
    }

    triangles.push([
        contour[indices[0]],
        contour[indices[1]],
        contour[indices[2]],
    ]);
    Ok(triangles)
}

fn remove_duplicate_points(points: &mut Vec<Point2>) {
    points.dedup_by(|left, right| distance_squared(*left, *right) < 0.0001);
    if points.len() > 1 && distance_squared(points[0], *points.last().unwrap()) < 0.0001 {
        points.pop();
    }
}

fn remove_collinear_points(points: &mut Vec<Point2>) {
    while points.len() > 3 {
        let mut removed = false;
        for index in 0..points.len() {
            let previous = points[(index + points.len() - 1) % points.len()];
            let current = points[index];
            let next = points[(index + 1) % points.len()];
            let between = (current.x - previous.x) * (current.x - next.x)
                + (current.y - previous.y) * (current.y - next.y);
            if cross(previous, current, next).abs() <= 0.001 && between <= 0.001 {
                points.remove(index);
                removed = true;
                break;
            }
        }
        if !removed {
            break;
        }
    }
}

fn signed_area(points: &[Point2]) -> f32 {
    let mut area = 0.0;
    for index in 0..points.len() {
        let next = (index + 1) % points.len();
        area += points[index].x * points[next].y - points[next].x * points[index].y;
    }
    area * 0.5
}

fn cross(a: Point2, b: Point2, c: Point2) -> f32 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

fn point_in_triangle(point: Point2, a: Point2, b: Point2, c: Point2) -> bool {
    let first = cross(a, b, point);
    let second = cross(b, c, point);
    let third = cross(c, a, point);
    (first >= -0.001 && second >= -0.001 && third >= -0.001)
        || (first <= 0.001 && second <= 0.001 && third <= 0.001)
}

fn distance_squared(a: Point2, b: Point2) -> f32 {
    (a.x - b.x).powi(2) + (a.y - b.y).powi(2)
}

fn stroke_contour(
    contour: &Contour,
    width: f32,
    line_cap: usvg::LineCap,
    color: [f32; 4],
    vertices: &mut Vec<Vertex>,
) {
    let points = &contour.points;
    if points.len() < 2 || width <= 0.0 {
        return;
    }
    let segment_count = if contour.closed {
        points.len()
    } else {
        points.len() - 1
    };
    let half = width * 0.5;
    for index in 0..segment_count {
        let start = points[index];
        let end = points[(index + 1) % points.len()];
        let delta_x = end.x - start.x;
        let delta_y = end.y - start.y;
        let length = delta_x.hypot(delta_y);
        if length <= 0.001 {
            continue;
        }
        let normal = Point2 {
            x: -delta_y / length * half,
            y: delta_x / length * half,
        };
        push_triangle(
            [
                offset(start, normal),
                offset(start, negate(normal)),
                offset(end, normal),
            ],
            color,
            vertices,
        );
        push_triangle(
            [
                offset(start, negate(normal)),
                offset(end, negate(normal)),
                offset(end, normal),
            ],
            color,
            vertices,
        );
    }

    for point in points.iter().copied() {
        add_round_join(point, half, color, vertices);
    }
    if !contour.closed && line_cap == usvg::LineCap::Round {
        add_round_join(points[0], half, color, vertices);
        add_round_join(*points.last().unwrap(), half, color, vertices);
    }
}

fn add_round_join(center: Point2, radius: f32, color: [f32; 4], vertices: &mut Vec<Vertex>) {
    if radius <= 0.0 {
        return;
    }
    let steps = 8;
    for step in 0..steps {
        let first_angle = step as f32 / steps as f32 * std::f32::consts::TAU;
        let second_angle = (step + 1) as f32 / steps as f32 * std::f32::consts::TAU;
        push_triangle(
            [
                center,
                Point2 {
                    x: center.x + first_angle.cos() * radius,
                    y: center.y + first_angle.sin() * radius,
                },
                Point2 {
                    x: center.x + second_angle.cos() * radius,
                    y: center.y + second_angle.sin() * radius,
                },
            ],
            color,
            vertices,
        );
    }
}

fn push_triangle(points: [Point2; 3], color: [f32; 4], vertices: &mut Vec<Vertex>) {
    vertices.extend(points.map(|point| Vertex {
        position: [point.x, point.y],
        color,
    }));
}

fn offset(point: Point2, amount: Point2) -> Point2 {
    Point2 {
        x: point.x + amount.x,
        y: point.y + amount.y,
    }
}

fn negate(point: Point2) -> Point2 {
    Point2 {
        x: -point.x,
        y: -point.y,
    }
}

fn paint_color(
    paint: &Paint,
    min_x: f32,
    min_y: f32,
    width: f32,
    height: f32,
    point: Point2,
    opacity: f32,
) -> [f32; 4] {
    match paint {
        Paint::Color(color) => [
            f32::from(color.red) / 255.0,
            f32::from(color.green) / 255.0,
            f32::from(color.blue) / 255.0,
            opacity,
        ],
        Paint::LinearGradient(gradient) => {
            let start_x = min_x + gradient.x1() * width;
            let start_y = min_y + gradient.y1() * height;
            let end_x = min_x + gradient.x2() * width;
            let end_y = min_y + gradient.y2() * height;
            let delta_x = end_x - start_x;
            let delta_y = end_y - start_y;
            let length_squared = delta_x * delta_x + delta_y * delta_y;
            let amount = if length_squared <= f32::EPSILON {
                0.0
            } else {
                (((point.x - start_x) * delta_x + (point.y - start_y) * delta_y) / length_squared)
                    .clamp(0.0, 1.0)
            };
            sample_gradient(gradient.stops(), amount, opacity)
        }
        Paint::RadialGradient(gradient) => {
            let stop = gradient.stops().first().copied();
            stop.map_or([1.0, 1.0, 1.0, opacity], |stop| {
                let color = stop.color();
                [
                    f32::from(color.red) / 255.0,
                    f32::from(color.green) / 255.0,
                    f32::from(color.blue) / 255.0,
                    stop.opacity().get() * opacity,
                ]
            })
        }
        Paint::Pattern(_) => [0.0, 0.0, 0.0, 0.0],
    }
}

fn sample_gradient(stops: &[usvg::Stop], amount: f32, opacity: f32) -> [f32; 4] {
    let Some(first) = stops.first().copied() else {
        return [1.0, 1.0, 1.0, opacity];
    };
    let Some(last) = stops.last().copied() else {
        return [1.0, 1.0, 1.0, opacity];
    };
    let next = stops
        .iter()
        .copied()
        .find(|stop| stop.offset().get() >= amount)
        .unwrap_or(last);
    let previous = stops
        .iter()
        .copied()
        .take_while(|stop| stop.offset().get() < amount)
        .last()
        .unwrap_or(first);
    let span = next.offset().get() - previous.offset().get();
    let mix = if span <= f32::EPSILON {
        0.0
    } else {
        ((amount - previous.offset().get()) / span).clamp(0.0, 1.0)
    };
    let first_color = previous.color();
    let second_color = next.color();
    let interpolate = |first: f32, second: f32| first + (second - first) * mix;
    [
        interpolate(f32::from(first_color.red), f32::from(second_color.red)) / 255.0,
        interpolate(f32::from(first_color.green), f32::from(second_color.green)) / 255.0,
        interpolate(f32::from(first_color.blue), f32::from(second_color.blue)) / 255.0,
        interpolate(previous.opacity().get(), next.opacity().get()) * opacity,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_mesh_contains_finite_colored_triangles() {
        let mesh = build_scene_mesh(CameraState {
            viewport_width: 402.0,
            viewport_height: 874.0,
            mobile_controls: true,
            ..CameraState::default()
        })
        .unwrap();

        assert!(mesh.vertices.len() > 300);
        assert_eq!(mesh.vertices.len() % 3, 0);
        assert!(mesh.vertices.iter().all(|vertex| {
            vertex.position.iter().all(|value| value.is_finite())
                && vertex.color.iter().all(|value| value.is_finite())
        }));
    }

    #[test]
    fn landscape_scene_mesh_contains_finite_colored_triangles() {
        let mesh = build_scene_mesh(CameraState {
            viewport_width: 1200.0,
            viewport_height: 700.0,
            ..CameraState::default()
        })
        .unwrap();

        assert!(mesh.vertices.len() > 300);
        assert_eq!(mesh.vertices.len() % 3, 0);
        assert!(mesh.vertices.iter().all(|vertex| {
            vertex.position.iter().all(|value| value.is_finite())
                && vertex.color.iter().all(|value| value.is_finite())
        }));
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn native_api_settings_panel_builds_as_a_scene_mesh() {
        let mut camera = CameraState::default();
        camera.enable_native_api_settings(true);
        camera.api_settings_open = true;
        camera.graphics_api_preference = crate::GraphicsApiPreference::Vulkan;

        let mesh = build_scene_mesh(camera).expect("сетка панели графических настроек");
        assert!(mesh.vertices.len() > 300);
        assert!(mesh.vertices.iter().all(|vertex| {
            vertex.position.iter().all(|value| value.is_finite())
                && vertex.color.iter().all(|value| value.is_finite())
        }));
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn unavailable_api_error_screen_builds_as_a_scene_mesh() {
        let mut camera = CameraState::default();
        camera.enable_native_api_settings(true);
        camera.api_settings_open = true;
        camera.api_settings_error = true;
        camera.graphics_api_preference = crate::GraphicsApiPreference::Vulkan;

        let mesh = build_scene_mesh(camera).expect("сетка экрана ошибки графического API");
        assert!(mesh.vertices.len() > 50);
        assert!(mesh.vertices.iter().all(|vertex| {
            vertex.position.iter().all(|value| value.is_finite())
                && vertex.color.iter().all(|value| value.is_finite())
        }));
    }

    #[test]
    fn concave_contours_are_triangulated() {
        let points = [
            Point2 { x: 0.0, y: 0.0 },
            Point2 { x: 4.0, y: 0.0 },
            Point2 { x: 4.0, y: 1.0 },
            Point2 { x: 1.0, y: 1.0 },
            Point2 { x: 1.0, y: 4.0 },
            Point2 { x: 0.0, y: 4.0 },
        ];
        let triangles = triangulate(&points, FillRule::NonZero).unwrap();
        assert_eq!(triangles.len(), points.len() - 2);
    }
}
