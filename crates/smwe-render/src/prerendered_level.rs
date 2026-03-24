use egui_glow::glow::*;

/// Pre-rendered level display.
///
/// Renders the level ONCE to a framebuffer texture, then displays a single
/// textured quad each frame. Block edits re-render to the FBO — no per-frame
/// VRAM/CGRAM tile decoding.
pub struct PrerenderedLevel {
    fbo: Framebuffer,
    fbo_texture: Texture,
    level_w: f32,
    level_h: f32,

    display_program: Program,
    display_vao: VertexArray,
    display_vbo: Buffer,
    tex_loc: Option<UniformLocation>,
    offset_loc: Option<UniformLocation>,
    screen_loc: Option<UniformLocation>,
    level_size_loc: Option<UniformLocation>,
    zoom_loc: Option<UniformLocation>,

    destroyed: bool,
}

const DISPLAY_VS: &str = r#"#version 400
layout(location = 0) in vec2 position;
uniform vec2 offset;
uniform vec2 screen_size;
uniform vec2 level_size;
uniform float zoom;
out vec2 v_uv;
void main() {
    // UV: normalize vertex position to 0..1 range for texture sampling
    v_uv = position / level_size;
    // Screen-space position with pan and zoom
    vec2 sp = (position + offset) * zoom;
    vec2 ndc = sp / screen_size * 2.0 - 1.0;
    ndc.y = -ndc.y;
    gl_Position = vec4(ndc, 0.0, 1.0);
}
"#;

const DISPLAY_FS: &str = r#"#version 400
in vec2 v_uv;
uniform sampler2D tex;
out vec4 out_color;
void main() {
    out_color = texture(tex, v_uv);
}
"#;

impl PrerenderedLevel {
    pub fn new(gl: &Context, level_w: u32, level_h: u32) -> Self {
        let (fbo, fbo_texture) = create_fbo(gl, level_w, level_h);
        let display_program = compile_shader(gl, DISPLAY_VS, DISPLAY_FS);
        let (display_vao, display_vbo) = create_level_quad(gl, level_w, level_h);

        unsafe {
            Self {
                fbo,
                fbo_texture,
                level_w: level_w as f32,
                level_h: level_h as f32,
                display_program,
                display_vao,
                display_vbo,
                tex_loc: gl.get_uniform_location(display_program, "tex"),
                offset_loc: gl.get_uniform_location(display_program, "offset"),
                screen_loc: gl.get_uniform_location(display_program, "screen_size"),
                level_size_loc: gl.get_uniform_location(display_program, "level_size"),
                zoom_loc: gl.get_uniform_location(display_program, "zoom"),
                destroyed: false,
            }
        }
    }

    pub fn destroy(&mut self, gl: &Context) {
        if self.destroyed {
            return;
        }
        unsafe {
            gl.delete_framebuffer(self.fbo);
            gl.delete_texture(self.fbo_texture);
            gl.delete_program(self.display_program);
            gl.delete_vertex_array(self.display_vao);
            gl.delete_buffer(self.display_vbo);
        }
        self.destroyed = true;
    }

    pub fn fbo(&self) -> Framebuffer {
        self.fbo
    }

    pub fn fbo_size(&self) -> (i32, i32) {
        (self.level_w as i32, self.level_h as i32)
    }

    /// Display the pre-rendered texture as a single quad.
    pub fn paint(&self, gl: &Context, screen_w: f32, screen_h: f32, offset_x: f32, offset_y: f32, zoom: f32) {
        if self.destroyed {
            return;
        }
        unsafe {
            gl.use_program(Some(self.display_program));
            gl.active_texture(TEXTURE0);
            gl.bind_texture(TEXTURE_2D, Some(self.fbo_texture));
            gl.uniform_1_i32(self.tex_loc.as_ref(), 0);
            gl.uniform_2_f32(self.offset_loc.as_ref(), offset_x, offset_y);
            gl.uniform_2_f32(self.screen_loc.as_ref(), screen_w, screen_h);
            gl.uniform_2_f32(self.level_size_loc.as_ref(), self.level_w, self.level_h);
            gl.uniform_1_f32(self.zoom_loc.as_ref(), zoom);
            gl.bind_vertex_array(Some(self.display_vao));
            gl.draw_arrays(TRIANGLE_STRIP, 0, 4);
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn create_fbo(gl: &Context, w: u32, h: u32) -> (Framebuffer, Texture) {
    unsafe {
        let fbo = gl.create_framebuffer().expect("Failed to create FBO");
        gl.bind_framebuffer(FRAMEBUFFER, Some(fbo));

        let tex = gl.create_texture().expect("Failed to create FBO texture");
        gl.bind_texture(TEXTURE_2D, Some(tex));
        gl.tex_image_2d(TEXTURE_2D, 0, RGBA8 as i32, w as i32, h as i32, 0, RGBA, UNSIGNED_BYTE, None);
        gl.tex_parameter_i32(TEXTURE_2D, TEXTURE_MIN_FILTER, NEAREST as i32);
        gl.tex_parameter_i32(TEXTURE_2D, TEXTURE_MAG_FILTER, NEAREST as i32);
        gl.tex_parameter_i32(TEXTURE_2D, TEXTURE_WRAP_S, CLAMP_TO_EDGE as i32);
        gl.tex_parameter_i32(TEXTURE_2D, TEXTURE_WRAP_T, CLAMP_TO_EDGE as i32);
        gl.framebuffer_texture_2d(FRAMEBUFFER, COLOR_ATTACHMENT0, TEXTURE_2D, Some(tex), 0);

        let status = gl.check_framebuffer_status(FRAMEBUFFER);
        assert_eq!(status, FRAMEBUFFER_COMPLETE, "FBO incomplete: {status:?}");

        gl.bind_framebuffer(FRAMEBUFFER, None);
        gl.bind_texture(TEXTURE_2D, None);
        (fbo, tex)
    }
}

fn compile_shader(gl: &Context, vs_src: &str, fs_src: &str) -> Program {
    unsafe {
        let prog = gl.create_program().expect("Failed to create shader program");

        let vs = gl.create_shader(VERTEX_SHADER).expect("create VS");
        gl.shader_source(vs, vs_src);
        gl.compile_shader(vs);
        assert!(gl.get_shader_compile_status(vs), "VS: {}", gl.get_shader_info_log(vs));
        gl.attach_shader(prog, vs);

        let fs = gl.create_shader(FRAGMENT_SHADER).expect("create FS");
        gl.shader_source(fs, fs_src);
        gl.compile_shader(fs);
        assert!(gl.get_shader_compile_status(fs), "FS: {}", gl.get_shader_info_log(fs));
        gl.attach_shader(prog, fs);

        gl.link_program(prog);
        assert!(gl.get_program_link_status(prog), "{}", gl.get_program_info_log(prog));

        gl.detach_shader(prog, vs);
        gl.detach_shader(prog, fs);
        gl.delete_shader(vs);
        gl.delete_shader(fs);

        prog
    }
}

fn create_level_quad(gl: &Context, w: u32, h: u32) -> (VertexArray, Buffer) {
    unsafe {
        let vao = gl.create_vertex_array().expect("Failed to create quad VAO");
        let vbo = gl.create_buffer().expect("Failed to create quad VBO");

        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(ARRAY_BUFFER, Some(vbo));

        // Quad covering the full level area in level-pixel coordinates.
        // UVs are computed in the vertex shader from these positions.
        let fw = w as f32;
        let fh = h as f32;
        let quad: [f32; 8] = [
            0.0, 0.0, // top-left
            fw, 0.0, // top-right
            0.0, fh, // bottom-left
            fw, fh, // bottom-right
        ];
        gl.buffer_data_u8_slice(ARRAY_BUFFER, quad.align_to().1, STATIC_DRAW);
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_f32(0, 2, FLOAT, false, 0, 0);

        gl.bind_vertex_array(None);
        (vao, vbo)
    }
}
