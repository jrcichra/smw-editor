#version 400
layout(points) in;
in int v_tile_id[1];
in int v_params[1];

uniform vec2 screen_size;
uniform float zoom;

layout(triangle_strip, max_vertices = 4) out;
flat out int g_tile_id;
flat out int g_params;
out vec2 g_tex_coords;

void main() {
	vec2 position = gl_in[0].gl_Position.xy;

	g_tile_id = v_tile_id[0];
	g_params = v_params[0];
    // scale = tile size in screen pixels (e.g. 8 * zoom).
    // We expand the quad by 1 pixel on the far edges so the rasterizer never
    // leaves a gap between adjacent tiles at non-integer zoom levels.
    // g_tex_coords runs 0..scale across the un-expanded tile so the FS can
    // recover the correct 0-7 texel index via clamping.
    float scale = float(v_params[0] & 0xFF) * zoom;
    float expand = 1.0; // 1 extra pixel on right/bottom to close gaps

	vec2 pos;
	vec2 p;

	g_tex_coords = vec2(0.0, 0.0);
	pos = position;
	p = (pos / screen_size * 2.0 - 1.0) * vec2(1.0, -1.0);
	gl_Position = vec4(p, 0.0, 1.0);
	EmitVertex();

	g_tex_coords = vec2(scale + expand, 0.0);
	pos = position + vec2(scale + expand, 0.0);
	p = (pos / screen_size * 2.0 - 1.0) * vec2(1.0, -1.0);
	gl_Position = vec4(p, 0.0, 1.0);
	EmitVertex();

	g_tex_coords = vec2(0.0, scale + expand);
	pos = position + vec2(0.0, scale + expand);
	p = (pos / screen_size * 2.0 - 1.0) * vec2(1.0, -1.0);
	gl_Position = vec4(p, 0.0, 1.0);
	EmitVertex();

	g_tex_coords = vec2(scale + expand);
	pos = position + vec2(scale + expand);
	p = (pos / screen_size * 2.0 - 1.0) * vec2(1.0, -1.0);
	gl_Position = vec4(p, 0.0, 1.0);
	EmitVertex();

	EndPrimitive();
}
