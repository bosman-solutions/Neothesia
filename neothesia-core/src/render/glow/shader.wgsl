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
    var quad_position = quad.q_position * view_uniform.scale;
    var quad_size = quad.size * view_uniform.scale;

    var i_transform: mat4x4<f32> = mat4x4<f32>(
        vec4<f32>(quad_size.x, 0.0, 0.0, 0.0),
        vec4<f32>(0.0, quad_size.y, 0.0, 0.0),
        vec4<f32>(0.0, 0.0, 1.0, 0.0),
        vec4<f32>(quad_position, 0.0, 1.0)
    );

    var out: VertexOutput;
    out.position = view_uniform.transform * i_transform * vec4<f32>(vertex.position, 0.0, 1.0);
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

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let k = i32(round(in.params.x));
    var rgb = vec3<f32>(0.0);
    switch k {
        case 1: { rgb = spark(in.uv, in.quad_color); }
        case 2: { rgb = ring(in.uv, in.quad_color, in.params.y); }
        case 3: { rgb = beam(in.uv, in.quad_color, in.params.z); }
        default: { rgb = halo(in.uv, in.quad_color); }
    }
    return vec4<f32>(rgb, 0.0);
}
