struct ViewUniform {
    transform: mat4x4<f32>,
    size: vec2<f32>,
    scale: f32,
}

@group(0) @binding(0)
var<uniform> view_uniform: ViewUniform;

struct Vertex {
    @location(0) position: vec2<f32>,
}

struct QuadInstance {
    @location(1) q_position: vec2<f32>,
    @location(2) size: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) params: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) quad_color: vec4<f32>,
    @location(2) params: vec4<f32>,
}

@vertex
fn vs_main(vertex: Vertex, quad: QuadInstance) -> VertexOutput {
    // Rotate the quad around its center by params.w (radians).
    let center = quad.q_position + quad.size * 0.5;
    let local = (vertex.position - 0.5) * quad.size;
    let cs = cos(quad.params.w);
    let sn = sin(quad.params.w);
    let rotated = vec2<f32>(local.x * cs - local.y * sn, local.x * sn + local.y * cs);
    let world = (center + rotated) * view_uniform.scale;

    var out: VertexOutput;
    out.position = view_uniform.transform * vec4<f32>(world, 0.0, 1.0);
    out.uv = vertex.position;
    out.quad_color = quad.color;
    out.params = quad.params;
    return out;
}

const WHITE: vec3<f32> = vec3<f32>(1.0, 1.0, 1.0);

// Soft halo: gaussian body + white-hot core, hard zero at the quad edge.
fn halo(uv: vec2<f32>, c: vec4<f32>) -> vec3<f32> {
    let d = length(uv - 0.5) * 2.0;
    let edge = 1.0 - smoothstep(0.7, 1.0, d);
    let body = exp(-d * d * 5.0) * edge;
    let core = exp(-d * d * 40.0);
    return (c.rgb * body + WHITE * core * 0.35) * c.a;
}

// Spark/ember: tight bright dot.
fn spark(uv: vec2<f32>, c: vec4<f32>) -> vec3<f32> {
    let d = length(uv - 0.5) * 2.0;
    let body = exp(-d * d * 10.0) * (1.0 - smoothstep(0.8, 1.0, d));
    let hot = exp(-d * d * 60.0);
    return (c.rgb * body + WHITE * hot * 0.9) * c.a;
}

// Shockwave: expanding ring that thins as it ages.
fn ring(uv: vec2<f32>, c: vec4<f32>, age: f32) -> vec3<f32> {
    let d = length(uv - 0.5) * 2.0;
    let r = mix(0.15, 0.95, sqrt(age));
    let w = mix(0.10, 0.015, age);
    let x = (d - r) / w;
    let v = exp(-x * x);
    return (c.rgb * v + WHITE * v * v * 0.4) * c.a;
}

// Light beam shooting up from the key into the waterfall.
fn beam(uv: vec2<f32>, c: vec4<f32>, seed: f32) -> vec3<f32> {
    let x = (uv.x - 0.5) * 2.0;
    let y = uv.y; // 0 = top, 1 = keyboard edge
    let shimmer = 0.85 + 0.15 * sin(y * 40.0 + seed * 6.28);
    let column = exp(-x * x * 9.0) * shimmer;
    let filament = exp(-x * x * 160.0);
    let fall = pow(y, 2.2);
    return (c.rgb * column + WHITE * filament * 0.6) * fall * c.a;
}

// ---- smoke ----------------------------------------------------------------
fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2<f32>(1.0, 0.0));
    let c = hash2(i + vec2<f32>(0.0, 1.0));
    let d = hash2(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn fbm(p_in: vec2<f32>) -> f32 {
    var p = p_in;
    var v = 0.0;
    var amp = 0.5;
    for (var i = 0; i < 4; i++) {
        v += amp * vnoise(p);
        p = p * 2.03 + vec2<f32>(1.7, 9.2);
        amp *= 0.5;
    }
    return v;
}

// Wispy puff: noise-carved density inside a soft mask, churning with age.
fn smoke(uv: vec2<f32>, c: vec4<f32>, age: f32, seed: f32) -> vec3<f32> {
    let p = uv - 0.5;
    let d = length(p) * 2.0;
    let mask = 1.0 - smoothstep(0.2, 1.0, d);
    let q = p * 3.0 + vec2<f32>(seed * 17.0, seed * 31.0);
    let warp = fbm(q + vec2<f32>(0.0, age * 1.5));
    let n = fbm(q * 1.4 + warp * 1.8 - vec2<f32>(0.0, age * 2.0));
    let density = smoothstep(0.35, 0.85, n) * mask;
    return c.rgb * density * c.a;
}

// Keyline: razor-thin hot edge, long warm glow climbing upward,
// short spill down onto the keys. `reach` = half quad height in px.
fn keyline(uv: vec2<f32>, c: vec4<f32>, reach: f32) -> vec3<f32> {
    let dy = (uv.y - 0.5) * 2.0 * reach; // px; negative = above the line
    let ady = abs(dy);
    let core = exp(-(dy / 1.6) * (dy / 1.6));
    let up = exp(-ady / (reach * 0.22)) * select(0.0, 1.0, dy < 0.0);
    let down = exp(-ady / 5.0) * select(0.0, 1.0, dy >= 0.0);
    let fade = 1.0 - smoothstep(0.7, 1.0, ady / reach);
    return (c.rgb * (up * 0.55 + down * 0.5) + mix(c.rgb, WHITE, 0.6) * core * 1.4) * fade * c.a;
}

// Ember: a glowing spindle. Hottest mid-body, fading at both ends,
// warm all the way through (no bright head).
fn streak(uv: vec2<f32>, c: vec4<f32>, seed: f32) -> vec3<f32> {
    let y = (uv.y - 0.5) * 2.0;
    let profile = pow(sin(3.14159 * uv.x), 1.4);          // 0 at ends, 1 mid
    let w = max(profile, 0.02);
    let across = exp(-(y / (0.55 * w)) * (y / (0.55 * w)));
    let core = exp(-(y / (0.18 * w)) * (y / (0.18 * w)));
    let flicker = 0.85 + 0.15 * sin(uv.x * 23.0 + seed * 40.0);
    let hot = mix(c.rgb, vec3<f32>(1.0, 0.85, 0.6), 0.5);  // warm, not white
    return (c.rgb * across * 0.7 + hot * core) * profile * flicker * c.a;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let k = i32(round(in.params.x));
    var rgb = vec3<f32>(0.0);
    switch k {
        case 1: { rgb = spark(in.uv, in.quad_color); }
        case 2: { rgb = ring(in.uv, in.quad_color, in.params.y); }
        case 3: { rgb = beam(in.uv, in.quad_color, in.params.z); }
        case 4: { rgb = smoke(in.uv, in.quad_color, in.params.y, in.params.z); }
        case 5: { rgb = keyline(in.uv, in.quad_color, in.params.z); }
        case 6: { rgb = streak(in.uv, in.quad_color, in.params.z); }
        default: { rgb = halo(in.uv, in.quad_color); }
    }
    return vec4<f32>(rgb, 0.0);
}
