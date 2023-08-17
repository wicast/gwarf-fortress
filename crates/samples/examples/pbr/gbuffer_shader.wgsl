//Uniforms
struct Camera {
    view_pos: vec4<f32>,
    view_proj: mat4x4<f32>,
}
@group(0) @binding(0)
var<uniform> camera: Camera;

//Vertex Buffers
struct VertexInput {
    @location(0) position: vec3<f32>,
};
struct Normal {
    @location(1) normal: vec3<f32>,
}
struct UV {
    @location(2) uv0: vec2<f32>,
}
struct Tangent {
    @location(3) tangent: vec4<f32>,
}

struct PerObjInput {
    @location(8) base_color: u32,
    @location(9) base_color_sampler: u32,
    @location(10) normal_map: u32,
    @location(11) normal_sampler: u32,
    @location(12) metallic_map: u32,
    @location(13) metallic_sampler: u32,

    @location(14) model_mat_0: vec4<f32>,
    @location(15) model_mat_1: vec4<f32>,
    @location(16) model_mat_2: vec4<f32>,
    @location(17) model_mat_3: vec4<f32>,
}

//Vertex Output
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) pos: vec3<f32>,
    @location(1) uv0: vec2<f32>,

    @location(8) base_color: u32,
    @location(9) base_color_sampler: u32,
    @location(10) normal_map: u32,
    @location(11) normal_sampler: u32,
    @location(12) metallic_map: u32,
    @location(13) metallic_sampler: u32,
    @location(14) a_normal: vec3<f32>,
    @location(15) a_tangent: vec3<f32>,
    @location(16) a_bi_tangent: vec3<f32>,

    @location(30) debug_vec3: vec3<f32>,
};

@vertex
fn vs_main(
    vert: VertexInput,
    obj: PerObjInput,
    normal: Normal,
    uv: UV,
    tangent: Tangent,
) -> VertexOutput {
    var out: VertexOutput;

    let model_mat = mat4x4<f32>(obj.model_mat_0, obj.model_mat_1, obj.model_mat_2, obj.model_mat_3);

    let a_normal = normalize((model_mat * vec4<f32>(normal.normal, 1.0)).xyz);
    var a_tangent = normalize((model_mat * tangent.tangent)).xyz;
    let a_bi_tangent = normalize(cross(a_normal, a_tangent) * tangent.tangent.w);

    let obj_pos = (model_mat * vec4<f32>(vert.position, 1.0));
    out.clip_position = camera.view_proj * obj_pos;
    out.pos = obj_pos.xyz;
    out.uv0 = uv.uv0;

    out.base_color = obj.base_color;
    out.base_color_sampler = obj.base_color_sampler;
    out.normal_map = obj.normal_map;
    out.normal_sampler = obj.normal_sampler;
    out.metallic_map = obj.metallic_map;
    out.metallic_sampler = obj.metallic_sampler;
    out.a_tangent = a_tangent;
    out.a_normal = a_normal;
    out.a_bi_tangent = a_bi_tangent;

    return out;
}

//Fragment Uniform
@group(1) @binding(0)
var textures: binding_array<texture_2d<f32>>;
@group(1) @binding(1)
var samplers: binding_array<sampler>;

//Fragment out
struct FragOut {
    @location(0) pos: vec4<f32>,
    @location(1) normal: vec4<f32>,
    @location(2) albedo: vec4<f32>,
}

@fragment
fn fs_main(in: VertexOutput) -> FragOut {
    var out: FragOut;

    let tex_color = textureSampleLevel(
        textures[in.base_color],
        samplers[in.base_color_sampler],
        in.uv0,
        0.0
    ).rgb;

    var normal = textureSampleLevel(
        textures[in.normal_map],
        samplers[in.normal_sampler],
        in.uv0,
        0.0
    ).rgb;

    var metallic = textureSampleLevel(
        textures[in.metallic_map],
        samplers[in.metallic_sampler],
        in.uv0,
        0.0
    ).rgb;

    let tbn = mat3x3<f32>(in.a_tangent, in.a_bi_tangent, in.a_normal);

    out.pos = vec4<f32>(in.pos, 1.0);
    // component w for metallic
    out.albedo = vec4<f32>(tex_color, metallic.b);
    normal = (normal * 2.0 - 1.0);
    normal = tbn * normal;
    // component w for roughness
    out.normal = vec4<f32>(normalize(normal), metallic.g);
    return out;
}