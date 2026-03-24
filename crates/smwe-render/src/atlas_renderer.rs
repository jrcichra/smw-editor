use egui_glow::glow::*;

/// Simple textured quad renderer for atlas-based tile display.
///
/// Each vertex is (x, y, u, v) in level-pixel space for position and 0..1 for UV.
/// The shader applies pan offset and zoom, then samples from the atlas texture.
pub struct AtlasRenderer {
    program: Program,
    vao: VertexArray,
    vbo: Buffer,
    vertex_count: i32,
    atlas_tex_loc: Option<UniformLocation>,
    offset_loc: Option<UniformLocation>,
    screen_loc: Option<UniformLocation>,
    zoom_loc: Option<UniformLocation>,
    destroyed: bool,
}

const VS: &str = r#"#version 400
layout(location = 0) in vec2 pos;
layout(location = 1) in vec2 uv;
uniform vec2 offset;
uniform vec2 screen_size;
uniform float zoom;
out vec2 v_uv;
void main() {
    v_uv = uv;
    vec2 sp = (pos + offset) * zoom;
    vec2 ndc = sp / screen_size * 2.0 - 1.0;
    ndc.y = -ndc.y;
    gl_Position = vec4(ndc, 0.0, 1.0);
}
"#;

const FS: &str = r#"#version 400
in vec2 v_uv;
uniform sampler2D atlas;
out vec4 out_color;
void main() {
    out_color = texture(atlas, v_uv);
    if (out_color.a < 0.01) discard;
}
"#;

/// A single quad vertex: position (level pixels) + UV (atlas 0..1).
#[repr(C)]
#[derive(Copy, Clone)]
pub struct QuadVertex {
    pub x: f32,
    pub y: f32,
    pub u: f32,
    pub v: f32,
}

impl AtlasRenderer {
    pub fn new(gl: &Context) -> Self {
        let program = compile_program(gl, VS, FS);
        let (vao, vbo) = unsafe {
            let vao = gl.create_vertex_array().expect("Failed to create atlas VAO");
            let vbo = gl.create_buffer().expect("Failed to create atlas VBO");
            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(ARRAY_BUFFER, Some(vbo));
            // position: vec2 at location 0
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, FLOAT, false, 16, 0);
            // uv: vec2 at location 1
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 2, FLOAT, false, 16, 8);
            gl.bind_vertex_array(None);
            (vao, vbo)
        };
        unsafe {
            Self {
                program,
                vao,
                vbo,
                vertex_count: 0,
                atlas_tex_loc: gl.get_uniform_location(program, "atlas"),
                offset_loc: gl.get_uniform_location(program, "offset"),
                screen_loc: gl.get_uniform_location(program, "screen_size"),
                zoom_loc: gl.get_uniform_location(program, "zoom"),
                destroyed: false,
            }
        }
    }

    pub fn destroy(&mut self, gl: &Context) {
        if self.destroyed {
            return;
        }
        unsafe {
            gl.delete_program(self.program);
            gl.delete_vertex_array(self.vao);
            gl.delete_buffer(self.vbo);
        }
        self.destroyed = true;
    }

    /// Upload quad vertices for rendering.
    pub fn set_quads(&mut self, gl: &Context, verts: &[QuadVertex]) {
        if self.destroyed {
            return;
        }
        self.vertex_count = verts.len() as i32;
        unsafe {
            gl.bind_vertex_array(Some(self.vao));
            gl.bind_buffer(ARRAY_BUFFER, Some(self.vbo));
            gl.buffer_data_u8_slice(ARRAY_BUFFER, verts.align_to().1, DYNAMIC_DRAW);
        }
    }

    /// Render all uploaded quads with the given atlas texture.
    pub fn paint(
        &self, gl: &Context, atlas_tex: Texture, screen_w: f32, screen_h: f32, offset_x: f32, offset_y: f32, zoom: f32,
    ) {
        if self.destroyed || self.vertex_count == 0 {
            return;
        }
        unsafe {
            gl.use_program(Some(self.program));
            gl.active_texture(TEXTURE0);
            gl.bind_texture(TEXTURE_2D, Some(atlas_tex));
            gl.uniform_1_i32(self.atlas_tex_loc.as_ref(), 0);
            gl.uniform_2_f32(self.offset_loc.as_ref(), offset_x, offset_y);
            gl.uniform_2_f32(self.screen_loc.as_ref(), screen_w, screen_h);
            gl.uniform_1_f32(self.zoom_loc.as_ref(), zoom);
            gl.bind_vertex_array(Some(self.vao));
            gl.draw_arrays(TRIANGLES, 0, self.vertex_count);
        }
    }
}

fn compile_program(gl: &Context, vs: &str, fs: &str) -> Program {
    unsafe {
        let prog = gl.create_program().expect("shader program");
        let v = gl.create_shader(VERTEX_SHADER).expect("create VS");
        gl.shader_source(v, vs);
        gl.compile_shader(v);
        assert!(gl.get_shader_compile_status(v), "VS: {}", gl.get_shader_info_log(v));
        gl.attach_shader(prog, v);
        let f = gl.create_shader(FRAGMENT_SHADER).expect("create FS");
        gl.shader_source(f, fs);
        gl.compile_shader(f);
        assert!(gl.get_shader_compile_status(f), "FS: {}", gl.get_shader_info_log(f));
        gl.attach_shader(prog, f);
        gl.link_program(prog);
        assert!(gl.get_program_link_status(prog), "{}", gl.get_program_info_log(prog));
        gl.detach_shader(prog, v);
        gl.detach_shader(prog, f);
        gl.delete_shader(v);
        gl.delete_shader(f);
        prog
    }
}
