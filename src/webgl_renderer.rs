use bytemuck::cast_slice;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use web_sys::{
    HtmlCanvasElement, WebGlBuffer, WebGlProgram, WebGlRenderingContext, WebGlShader,
    WebGlUniformLocation,
};

use crate::scene_mesh::{SceneMesh, Vertex};

const VERTEX_SHADER: &str = r#"
attribute vec2 a_position;
attribute vec4 a_color;
uniform vec2 u_resolution;
varying vec4 v_color;

void main() {
    vec2 clip = (a_position / u_resolution) * 2.0 - 1.0;
    gl_Position = vec4(clip.x, -clip.y, 0.0, 1.0);
    v_color = a_color;
}
"#;

const FRAGMENT_SHADER: &str = r#"
precision mediump float;
varying vec4 v_color;

void main() {
    gl_FragColor = v_color;
}
"#;

pub(crate) struct WebGlRenderer {
    context: WebGlRenderingContext,
    program: WebGlProgram,
    vertex_buffer: WebGlBuffer,
    resolution: WebGlUniformLocation,
    vertex_count: i32,
}

impl WebGlRenderer {
    pub(crate) fn new(canvas: HtmlCanvasElement) -> Result<Self, String> {
        let context = canvas
            .get_context("webgl")
            .map_err(js_error)?
            .ok_or_else(|| "Браузер не предоставил контекст WebGL.".to_owned())?
            .dyn_into::<WebGlRenderingContext>()
            .map_err(|error| js_error(error.into()))?;

        let vertex_shader = compile_shader(
            &context,
            WebGlRenderingContext::VERTEX_SHADER,
            VERTEX_SHADER,
        )?;
        let fragment_shader = compile_shader(
            &context,
            WebGlRenderingContext::FRAGMENT_SHADER,
            FRAGMENT_SHADER,
        )?;
        let program = context
            .create_program()
            .ok_or_else(|| "Браузер не создал программу WebGL.".to_owned())?;
        context.attach_shader(&program, &vertex_shader);
        context.attach_shader(&program, &fragment_shader);
        context.link_program(&program);
        context.delete_shader(Some(&vertex_shader));
        context.delete_shader(Some(&fragment_shader));
        if !context
            .get_program_parameter(&program, WebGlRenderingContext::LINK_STATUS)
            .as_bool()
            .unwrap_or(false)
        {
            return Err(context
                .get_program_info_log(&program)
                .unwrap_or_else(|| "Не удалось связать шейдеры WebGL.".to_owned()));
        }

        let position = context.get_attrib_location(&program, "a_position");
        let color = context.get_attrib_location(&program, "a_color");
        if position < 0 || color < 0 {
            return Err("В программе WebGL отсутствуют атрибуты вершин сцены.".to_owned());
        }
        let resolution = context
            .get_uniform_location(&program, "u_resolution")
            .ok_or_else(|| "В программе WebGL отсутствует размер сцены.".to_owned())?;
        let vertex_buffer = context
            .create_buffer()
            .ok_or_else(|| "Браузер не создал буфер вершин WebGL.".to_owned())?;

        context.use_program(Some(&program));
        context.bind_buffer(WebGlRenderingContext::ARRAY_BUFFER, Some(&vertex_buffer));
        context.enable_vertex_attrib_array(position as u32);
        context.vertex_attrib_pointer_with_i32(
            position as u32,
            2,
            WebGlRenderingContext::FLOAT,
            false,
            std::mem::size_of::<Vertex>() as i32,
            std::mem::offset_of!(Vertex, position) as i32,
        );
        context.enable_vertex_attrib_array(color as u32);
        context.vertex_attrib_pointer_with_i32(
            color as u32,
            4,
            WebGlRenderingContext::FLOAT,
            false,
            std::mem::size_of::<Vertex>() as i32,
            std::mem::offset_of!(Vertex, color) as i32,
        );

        Ok(Self {
            context,
            program,
            vertex_buffer,
            resolution,
            vertex_count: 0,
        })
    }

    pub(crate) fn render(
        &mut self,
        mesh: &SceneMesh,
        surface_width: u32,
        surface_height: u32,
    ) -> Result<(), String> {
        self.vertex_count = i32::try_from(mesh.vertices.len())
            .map_err(|_| "Сетка сцены слишком велика для WebGL.".to_owned())?;
        if self.vertex_count == 0 {
            return Err("Сетка сцены не содержит вершин для WebGL.".to_owned());
        }

        self.context.bind_buffer(
            WebGlRenderingContext::ARRAY_BUFFER,
            Some(&self.vertex_buffer),
        );
        self.context.buffer_data_with_u8_array(
            WebGlRenderingContext::ARRAY_BUFFER,
            cast_slice(&mesh.vertices),
            WebGlRenderingContext::DYNAMIC_DRAW,
        );
        self.context.viewport(
            0,
            0,
            surface_width.max(1) as i32,
            surface_height.max(1) as i32,
        );
        self.context.use_program(Some(&self.program));
        self.context.uniform2f(
            Some(&self.resolution),
            mesh.width.max(1.0),
            mesh.height.max(1.0),
        );
        self.context.clear_color(0.08, 0.15, 0.17, 1.0);
        self.context.clear(WebGlRenderingContext::COLOR_BUFFER_BIT);
        self.context
            .draw_arrays(WebGlRenderingContext::TRIANGLES, 0, self.vertex_count);
        Ok(())
    }
}

fn compile_shader(
    context: &WebGlRenderingContext,
    shader_type: u32,
    source: &str,
) -> Result<WebGlShader, String> {
    let shader = context
        .create_shader(shader_type)
        .ok_or_else(|| "Браузер не создал шейдер WebGL.".to_owned())?;
    context.shader_source(&shader, source);
    context.compile_shader(&shader);
    if context
        .get_shader_parameter(&shader, WebGlRenderingContext::COMPILE_STATUS)
        .as_bool()
        .unwrap_or(false)
    {
        Ok(shader)
    } else {
        Err(context
            .get_shader_info_log(&shader)
            .unwrap_or_else(|| "Не удалось скомпилировать шейдер WebGL.".to_owned()))
    }
}

fn js_error(error: JsValue) -> String {
    error
        .as_string()
        .unwrap_or_else(|| format!("Ошибка браузерного API: {error:?}"))
}
