struct ViewUniform {
    transform: mat4x4<f32>,
    size: vec2<f32>,
    scale: f32,
}

struct TimeUniform {
    time: f32,
    speed: f32,
}

@group(0) @binding(0)
var<uniform> view_uniform: ViewUniform;

@group(1) @binding(0)
var<uniform> time_uniform: TimeUniform;

struct Vertex {
    @location(0) position: vec2<f32>,
}

struct NoteInstance {
    @location(1) n_position: vec2<f32>,
    @location(2) size: vec2<f32>,
    @location(3) color: vec3<f32>,
    @location(4) radius: f32,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,

    @location(0) src_position: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) color: vec3<f32>,
    @location(3) radius: f32,
    @location(4) note_pos: vec2<f32>,
    @location(5) seed: f32,
}

// Quad padding (px) so the outer glow has room to render.
const GLOW_PAD: f32 = 10.0;

@vertex
fn vs_main(vertex: Vertex, note: NoteInstance) -> VertexOutput {
    let speed = time_uniform.speed;

    let size = vec2<f32>(note.size.x * view_uniform.scale, note.size.y * abs(speed));

    // In an ideal world this should not be hard-coded
    let keyboard_h = view_uniform.size.y / 5.0;
    let keyboard_y = view_uniform.size.y - keyboard_h;

    var pos = vec2<f32>(note.n_position.x * view_uniform.scale, keyboard_y);

    if speed > 0.0 {
        // If notes are falling from top to down, we need to adjust the position,
        // as their start is on bottom of the quad rather than top
        pos.y -= size.y;
    }

    // Offset position by playback time
    pos.y -= (note.n_position.y - time_uniform.time) * speed;

    let pad = GLOW_PAD * view_uniform.scale;
    let qpos = pos - vec2<f32>(pad, pad);
    let qsize = size + vec2<f32>(pad, pad) * 2.0;

    let transform = mat4x4<f32>(
        vec4<f32>(qsize.x, 0.0,     0.0, 0.0),
        vec4<f32>(0.0,     qsize.y, 0.0, 0.0),
        vec4<f32>(0.0,     0.0,     1.0, 0.0),
        vec4<f32>(qpos.x,  qpos.y,  0.0, 1.0)
    );

    var out: VertexOutput;
    out.position = view_uniform.transform * transform * vec4<f32>(vertex.position, 0.0, 1.0);
    out.note_pos = pos;
    out.seed = fract(note.n_position.x * 0.1031 + note.n_position.y * 0.7919);

    out.src_position = vertex.position;
    out.size = size;
    out.color = note.color;
    out.radius = note.radius * view_uniform.scale;

    return out;
}

fn dist(
    frag_coord: vec2<f32>,
    position: vec2<f32>,
    size: vec2<f32>,
    radius: f32,
) -> f32 {
    let inner_size: vec2<f32> = size - vec2<f32>(radius, radius) * 2.0;
    let top_left: vec2<f32> = position + vec2<f32>(radius, radius);
    let bottom_right: vec2<f32> = top_left + inner_size;

    let top_left_distance: vec2<f32> = top_left - frag_coord;
    let bottom_right_distance: vec2<f32> = frag_coord - bottom_right;

    let dist: vec2<f32> = vec2<f32>(
        max(max(top_left_distance.x, bottom_right_distance.x), 0.0),
        max(max(top_left_distance.y, bottom_right_distance.y), 0.0),
    );

    return sqrt(dist.x * dist.x + dist.y * dist.y);
}

// Signed distance to a rounded box (negative inside).
fn sd_round_box(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(r, r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// Glass note: smoked translucent body, diagonal light streaks,
// bright rim, soft outer glow.
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let half = in.size * 0.5;
    let r = min(in.radius, min(half.x, half.y));
    let local = in.position.xy - in.note_pos; // px from note top-left
    let sd = sd_round_box(local - half, half, r);

    let c = in.color;
    let light = mix(c, vec3<f32>(1.0), 0.55);

    // Body: deep tint + streaks that slide along with the note.
    let s = local.x * 0.9 + local.y * 0.38 + in.seed * 200.0;
    let s1 = pow(0.5 + 0.5 * sin(s * 0.11), 28.0);
    let s2 = pow(0.5 + 0.5 * sin(s * 0.047 + 1.7), 40.0) * 0.6;
    let sheen = 1.0 - smoothstep(0.0, half.x * 1.2, local.x); // brighter left edge
    let body_rgb = c * 0.32 + light * (s1 + s2) * 0.55 + light * sheen * 0.08;
    let body_a = 0.72;

    // Rim: thin bright outline straddling the edge.
    let rim = exp(-(sd / 1.3) * (sd / 1.3));

    // Outer glow.
    let pad = GLOW_PAD * view_uniform.scale;
    let glow = select(0.0, exp(-sd / (pad * 0.35)) * 0.45, sd > 0.0);

    let inside = 1.0 - smoothstep(-0.75, 0.75, sd);

    let in_rgb = mix(body_rgb, light, rim);
    let in_a = max(body_a, rim);
    let out_rgb = mix(c, light, rim);
    let out_a = max(rim, glow);

    let rgb = mix(out_rgb, in_rgb, inside);
    let a = clamp(mix(out_a, in_a, inside), 0.0, 1.0);
    return vec4<f32>(rgb, a);
}
