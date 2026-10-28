// Contorno: gradiente de Sobel da luminância (pré-multiplicada pelo alfa, para a borda do desenho também contar);
// acima do limiar vira linha preta, com uma rampa de `suavidade` para a linha ficar antisserrilhada.
struct Params { size: vec2<u32>, frame: u32, count: u32, p: array<vec4<f32>, 4> }
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var dst: texture_storage_2d<rgba32float, write>;
@group(0) @binding(2) var<uniform> u: Params;

fn lum(x: i32, y: i32) -> f32 {
    let p = clamp(vec2<i32>(x, y), vec2<i32>(0, 0), vec2<i32>(u.size) - vec2<i32>(1, 1));
    let c = textureLoad(src, p, 0);
    return dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722)) * c.a + (1.0 - c.a);
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= u.size.x || id.y >= u.size.y) { return; }
    let x = i32(id.x);
    let y = i32(id.y);
    let gx = -lum(x - 1, y - 1) - 2.0 * lum(x - 1, y) - lum(x - 1, y + 1) + lum(x + 1, y - 1) + 2.0 * lum(x + 1, y) + lum(x + 1, y + 1);
    let gy = -lum(x - 1, y - 1) - 2.0 * lum(x, y - 1) - lum(x + 1, y - 1) + lum(x - 1, y + 1) + 2.0 * lum(x, y + 1) + lum(x + 1, y + 1);
    let g = length(vec2<f32>(gx, gy)) * 0.25;
    let a = smoothstep(u.p[0].x, u.p[0].x + u.p[0].y, g);
    textureStore(dst, vec2<i32>(id.xy), vec4<f32>(0.0, 0.0, 0.0, a));
}
