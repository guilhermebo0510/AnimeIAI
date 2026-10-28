// Retícula: grade girada de células; em cada uma, um ponto de raio proporcional à escuridão da cor (área do ponto =
// escuridão). O ponto tem a cor do desenho; fora dele, transparente. Borda do ponto antisserrilhada (1 px).
struct Params { size: vec2<u32>, frame: u32, count: u32, p: array<vec4<f32>, 4> }
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var dst: texture_storage_2d<rgba32float, write>;
@group(0) @binding(2) var<uniform> u: Params;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= u.size.x || id.y >= u.size.y) { return; }
    let c = textureLoad(src, vec2<i32>(id.xy), 0);
    let cell = max(u.p[0].x, 2.0);
    let ang = radians(u.p[0].y);
    let p = vec2<f32>(id.xy) + vec2<f32>(0.5, 0.5);
    let r = vec2<f32>(cos(ang) * p.x + sin(ang) * p.y, -sin(ang) * p.x + cos(ang) * p.y) / cell;
    let local = (fract(r) - vec2<f32>(0.5, 0.5)) * cell;
    let dark = 1.0 - dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let radius = sqrt(dark / 3.14159265) * cell;
    let cover = clamp(radius - length(local) + 0.5, 0.0, 1.0) * c.a;
    textureStore(dst, vec2<i32>(id.xy), vec4<f32>(c.rgb, cover));
}
