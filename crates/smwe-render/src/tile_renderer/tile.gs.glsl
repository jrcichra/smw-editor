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
    float scale = float(v_params[0] & 0xFF) * zoom;
    // Snap the tile origin to whole pixels, then expand the far edge to the
    // next whole pixel. This guarantees zero sub-pixel gaps between tiles at
    // any zoom level, because floor(origin)+ceil(scale) == floor(next_origin)
    // when tiles are placed on an integer-pixel grid before zooming.
    vec2 origin = floor(position);
    float quad   = ceil(position.x + scale) - origin.x;
    float quadY  = ceil(position.y + scale) - origin.y;

	vec2 pos;
	vec2 p;

	g_tex_coords = vec2(0.0, 0.0);
	pos = origin + vec2(0.0, 0.0);
	p = (pos / screen_size * 2.0 - 1.0) * vec2(1.0, -1.0);
	gl_Position = vec4(p, 0.0, 1.0);
	EmitVertex();

	g_tex_coords = vec2(scale, 0.0);
	pos = origin + vec2(quad, 0.0);
	p = (pos / screen_size * 2.0 - 1.0) * vec2(1.0, -1.0);
	gl_Position = vec4(p, 0.0, 1.0);
	EmitVertex();

	g_tex_coords = vec2(0.0, scale);
	pos = origin + vec2(0.0, quadY);
	p = (pos / screen_size * 2.0 - 1.0) * vec2(1.0, -1.0);
	gl_Position = vec4(p, 0.0, 1.0);
	EmitVertex();

	g_tex_coords = vec2(scale);
	pos = origin + vec2(quad, quadY);
	p = (pos / screen_size * 2.0 - 1.0) * vec2(1.0, -1.0);
	gl_Position = vec4(p, 0.0, 1.0);
	EmitVertex();

	EndPrimitive();
}
