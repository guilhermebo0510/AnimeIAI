// Posterizar: cada canal vai para o nível mais perto entre `niveis` níveis igualmente espaçados.
struct Params { size: vec2<u32>, frame: u32, count: u32, p: array<vec4<f32>, 4> }
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var dst: texture_storage_2d<rgba32float, write>;
@group(0) @binding(2) var<uniform> u: Params;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= u.size.x || id.y >= u.size.y) { return; }
    let c = textureLoad(src, vec2<i32>(id.xy), 0);
    let n = max(round(u.p[0].x) - 1.0, 1.0);
    let q = round(c.rgb * n) / n;
    textureStore(dst, vec2<i32>(id.xy), vec4<f32>(q, c.a));
}
