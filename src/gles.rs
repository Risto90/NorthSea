use std::ffi::{CStr, c_char, c_void};
use std::mem::size_of;
use std::ptr;

use crate::scene_mesh::{SceneMesh, Vertex};

type GlEnum = u32;
type GlInt = i32;
type GlSize = i32;
type GlUInt = u32;
type GlFloat = f32;
type GlSizePtr = isize;

const GL_VERTEX_SHADER: GlEnum = 0x8B31;
const GL_FRAGMENT_SHADER: GlEnum = 0x8B30;
const GL_COMPILE_STATUS: GlEnum = 0x8B81;
const GL_LINK_STATUS: GlEnum = 0x8B82;
const GL_INFO_LOG_LENGTH: GlEnum = 0x8B84;
const GL_ARRAY_BUFFER: GlEnum = 0x8892;
const GL_STREAM_DRAW: GlEnum = 0x88E0;
const GL_FLOAT: GlEnum = 0x1406;
const GL_FALSE: u8 = 0;
const GL_TRIANGLES: GlEnum = 0x0004;
const GL_COLOR_BUFFER_BIT: GlEnum = 0x00004000;
const GL_BLEND: GlEnum = 0x0BE2;
const GL_SRC_ALPHA: GlEnum = 0x0302;
const GL_ONE_MINUS_SRC_ALPHA: GlEnum = 0x0303;
const GL_RENDERER: GlEnum = 0x1F01;

#[link(name = "GLESv2")]
unsafe extern "C" {
    fn glCreateShader(shader_type: GlEnum) -> GlUInt;
    fn glShaderSource(shader: GlUInt, count: GlSize, strings: *const *const c_char, lengths: *const GlInt);
    fn glCompileShader(shader: GlUInt);
    fn glGetShaderiv(shader: GlUInt, name: GlEnum, value: *mut GlInt);
    fn glGetShaderInfoLog(shader: GlUInt, capacity: GlSize, length: *mut GlSize, log: *mut c_char);
    fn glDeleteShader(shader: GlUInt);
    fn glCreateProgram() -> GlUInt;
    fn glAttachShader(program: GlUInt, shader: GlUInt);
    fn glLinkProgram(program: GlUInt);
    fn glGetProgramiv(program: GlUInt, name: GlEnum, value: *mut GlInt);
    fn glGetProgramInfoLog(program: GlUInt, capacity: GlSize, length: *mut GlSize, log: *mut c_char);
    fn glDeleteProgram(program: GlUInt);
    fn glGetAttribLocation(program: GlUInt, name: *const c_char) -> GlInt;
    fn glGetUniformLocation(program: GlUInt, name: *const c_char) -> GlInt;
    fn glGenBuffers(count: GlSize, buffers: *mut GlUInt);
    fn glDeleteBuffers(count: GlSize, buffers: *const GlUInt);
    fn glBindBuffer(target: GlEnum, buffer: GlUInt);
    fn glBufferData(target: GlEnum, size: GlSizePtr, data: *const c_void, usage: GlEnum);
    fn glEnableVertexAttribArray(index: GlUInt);
    fn glVertexAttribPointer(
        index: GlUInt,
        size: GlInt,
        kind: GlEnum,
        normalized: u8,
        stride: GlSize,
        pointer: *const c_void,
    );
    fn glViewport(x: GlInt, y: GlInt, width: GlSize, height: GlSize);
    fn glClearColor(red: GlFloat, green: GlFloat, blue: GlFloat, alpha: GlFloat);
    fn glClear(mask: GlEnum);
    fn glEnable(capability: GlEnum);
    fn glBlendFunc(source: GlEnum, destination: GlEnum);
    fn glUseProgram(program: GlUInt);
    fn glUniform2f(location: GlInt, first: GlFloat, second: GlFloat);
    fn glDrawArrays(mode: GlEnum, first: GlInt, count: GlSize);
    fn glGetString(name: GlEnum) -> *const u8;
}

pub(crate) struct GlesRenderer {
    program: GlUInt,
    buffer: GlUInt,
    position_attribute: GlUInt,
    color_attribute: GlUInt,
    viewport_uniform: GlInt,
}

impl GlesRenderer {
    pub(crate) fn new() -> Result<Self, String> {
        let vertex_shader = compile_shader(GL_VERTEX_SHADER, VERTEX_SHADER)?;
        let fragment_shader = match compile_shader(GL_FRAGMENT_SHADER, FRAGMENT_SHADER) {
            Ok(shader) => shader,
            Err(error) => {
                unsafe { glDeleteShader(vertex_shader) };
                return Err(error);
            }
        };
        let program = unsafe { glCreateProgram() };
        if program == 0 {
            unsafe {
                glDeleteShader(vertex_shader);
                glDeleteShader(fragment_shader);
            }
            return Err("Не удалось создать программу OpenGL ES.".to_owned());
        }

        unsafe {
            glAttachShader(program, vertex_shader);
            glAttachShader(program, fragment_shader);
            glLinkProgram(program);
            glDeleteShader(vertex_shader);
            glDeleteShader(fragment_shader);
        }

        let mut linked = 0;
        unsafe { glGetProgramiv(program, GL_LINK_STATUS, &mut linked) };
        if linked == 0 {
            let message = program_log(program);
            unsafe { glDeleteProgram(program) };
            return Err(format!("Не удалось связать программу OpenGL ES: {message}"));
        }

        let position_attribute = unsafe { glGetAttribLocation(program, c"a_position".as_ptr()) };
        let color_attribute = unsafe { glGetAttribLocation(program, c"a_color".as_ptr()) };
        let viewport_uniform = unsafe { glGetUniformLocation(program, c"u_viewport".as_ptr()) };
        if position_attribute < 0 || color_attribute < 0 || viewport_uniform < 0 {
            unsafe { glDeleteProgram(program) };
            return Err("В программе OpenGL ES отсутствуют атрибуты сцены.".to_owned());
        }

        let mut buffer = 0;
        unsafe { glGenBuffers(1, &mut buffer) };
        if buffer == 0 {
            unsafe { glDeleteProgram(program) };
            return Err("Не удалось создать буфер сцены OpenGL ES.".to_owned());
        }
        unsafe {
            glEnable(GL_BLEND);
            glBlendFunc(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA);
        }

        Ok(Self {
            program,
            buffer,
            position_attribute: position_attribute as GlUInt,
            color_attribute: color_attribute as GlUInt,
            viewport_uniform,
        })
    }

    pub(crate) fn draw(
        &mut self,
        mesh: &SceneMesh,
        surface_width: u32,
        surface_height: u32,
    ) -> Result<(), String> {
        let surface_width = surface_width.max(1);
        let surface_height = surface_height.max(1);
        if mesh.vertices.len() > GlSize::MAX as usize {
            return Err("В кадре слишком много вершин для OpenGL ES.".to_owned());
        }

        unsafe {
            glViewport(0, 0, surface_width as GlSize, surface_height as GlSize);
            glClearColor(0.08, 0.15, 0.17, 1.0);
            glClear(GL_COLOR_BUFFER_BIT);
            glUseProgram(self.program);
            glUniform2f(self.viewport_uniform, mesh.width, mesh.height);
            glBindBuffer(GL_ARRAY_BUFFER, self.buffer);
            glBufferData(
                GL_ARRAY_BUFFER,
                (mesh.vertices.len() * size_of::<Vertex>()) as GlSizePtr,
                mesh.vertices.as_ptr().cast(),
                GL_STREAM_DRAW,
            );

            let stride = size_of::<Vertex>() as GlSize;
            glEnableVertexAttribArray(self.position_attribute);
            glVertexAttribPointer(
                self.position_attribute,
                2,
                GL_FLOAT,
                GL_FALSE,
                stride,
                ptr::null(),
            );
            glEnableVertexAttribArray(self.color_attribute);
            glVertexAttribPointer(
                self.color_attribute,
                4,
                GL_FLOAT,
                GL_FALSE,
                stride,
                (2 * size_of::<GlFloat>()) as *const c_void,
            );
            glDrawArrays(GL_TRIANGLES, 0, mesh.vertices.len() as GlSize);
        }
        Ok(())
    }

    pub(crate) fn renderer_name() -> String {
        let name = unsafe { glGetString(GL_RENDERER) };
        if name.is_null() {
            return "OpenGL ES".to_owned();
        }
        unsafe { CStr::from_ptr(name.cast()) }
            .to_string_lossy()
            .into_owned()
    }
}

impl Drop for GlesRenderer {
    fn drop(&mut self) {
        unsafe {
            glDeleteBuffers(1, &self.buffer);
            glDeleteProgram(self.program);
        }
    }
}

fn compile_shader(shader_type: GlEnum, source: &str) -> Result<GlUInt, String> {
    let shader = unsafe { glCreateShader(shader_type) };
    if shader == 0 {
        return Err("Не удалось создать шейдер OpenGL ES.".to_owned());
    }
    let source = std::ffi::CString::new(source).expect("Исходный код шейдера содержит NUL.");
    let source_pointer = source.as_ptr();
    unsafe {
        glShaderSource(shader, 1, &source_pointer, ptr::null());
        glCompileShader(shader);
    }
    let mut compiled = 0;
    unsafe { glGetShaderiv(shader, GL_COMPILE_STATUS, &mut compiled) };
    if compiled == 0 {
        let message = shader_log(shader);
        unsafe { glDeleteShader(shader) };
        return Err(format!("Не удалось собрать шейдер OpenGL ES: {message}"));
    }
    Ok(shader)
}

fn shader_log(shader: GlUInt) -> String {
    let mut capacity = 0;
    unsafe { glGetShaderiv(shader, GL_INFO_LOG_LENGTH, &mut capacity) };
    if capacity <= 1 {
        return "драйвер не сообщил причину".to_owned();
    }
    let mut bytes = vec![0u8; capacity as usize];
    let mut written = 0;
    unsafe {
        glGetShaderInfoLog(
            shader,
            capacity,
            &mut written,
            bytes.as_mut_ptr().cast(),
        )
    };
    String::from_utf8_lossy(&bytes[..written.max(0) as usize]).into_owned()
}

fn program_log(program: GlUInt) -> String {
    let mut capacity = 0;
    unsafe { glGetProgramiv(program, GL_INFO_LOG_LENGTH, &mut capacity) };
    if capacity <= 1 {
        return "драйвер не сообщил причину".to_owned();
    }
    let mut bytes = vec![0u8; capacity as usize];
    let mut written = 0;
    unsafe {
        glGetProgramInfoLog(
            program,
            capacity,
            &mut written,
            bytes.as_mut_ptr().cast(),
        )
    };
    String::from_utf8_lossy(&bytes[..written.max(0) as usize]).into_owned()
}

const VERTEX_SHADER: &str = r#"
attribute vec2 a_position;
attribute vec4 a_color;
uniform vec2 u_viewport;
varying vec4 v_color;
void main() {
    vec2 normalized = (a_position / u_viewport) * 2.0 - 1.0;
    gl_Position = vec4(normalized.x, -normalized.y, 0.0, 1.0);
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
