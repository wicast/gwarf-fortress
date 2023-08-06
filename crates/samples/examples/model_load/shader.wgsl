struct Camera {
    view_pos: vec4<f32>,
    view_proj: mat4x4<f32>,
}
@group(0) @binding(0)
var<uniform> camera: Camera;

struct Light {
    position: vec3<f32>,
    color: vec3<f32>,
}
@group(2) @binding(0)
var<uniform> light: Light;

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
    @location(3) tangent: vec3<f32>,
}

struct BiTangent {
    @location(4) bi_tangent: vec3<f32>,
}

struct PerObjInput {
    @location(8) base_color: u32,
    @location(9) base_color_sampler: u32,
    @location(10) normal: u32,
    @location(11) normal_sampler: u32,

    @location(14) model_mat_0: vec4<f32>,
    @location(15) model_mat_1: vec4<f32>,
    @location(16) model_mat_2: vec4<f32>,
    @location(17) model_mat_3: vec4<f32>,
    @location(18) normal_mat_0: vec3<f32>,
    @location(19) normal_mat_1: vec3<f32>,
    @location(20) normal_mat_2: vec3<f32>,
}


struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(1) uv0: vec2<f32>,
    @location(2) tangent_world_pos: vec3<f32>,
    @location(3) tangent_view_pos: vec3<f32>,
    @location(4) tangent_light_pos: vec3<f32>,
    @location(8) base_color: u32,
    @location(9) base_color_sampler: u32,
    @location(10) normal_map: u32,
    @location(11) normal_sampler: u32,
};

@vertex
fn vs_main(
    vert: VertexInput,
    obj: PerObjInput,
    normal: Normal,
    uv: UV,
    tangent: Tangent,
    bi_tangent: BiTangent,
) -> VertexOutput {
    var out: VertexOutput;

    let model_mat = mat4x4<f32>(obj.model_mat_0, obj.model_mat_1, obj.model_mat_2, obj.model_mat_3);
    let normal_mat = mat3x3<f32>(obj.normal_mat_0, obj.normal_mat_1, obj.normal_mat_2);

    let tbn = transpose(mat3x3<f32>(normalize(normal_mat * tangent.tangent), normalize(normal_mat * bi_tangent.bi_tangent), normalize(normal_mat * normal.normal)));

    let obj_pos = (model_mat * vec4<f32>(vert.position, 1.0));
    out.clip_position = camera.view_proj * obj_pos;
    out.tangent_world_pos = tbn * obj_pos.xyz;
    out.tangent_view_pos = tbn * camera.view_pos.xyz;
    out.tangent_light_pos = tbn * light.position;

    out.uv0 = uv.uv0;
    out.base_color = obj.base_color;
    out.base_color_sampler = obj.base_color_sampler;
    out.normal_map = obj.normal;
    out.normal_sampler = obj.normal_sampler;
    return out;
}

// Fragment shader

@group(1) @binding(0)
var textures: binding_array<texture_2d<f32>>;
@group(1) @binding(1)
var samplers: binding_array<sampler>;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
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
    normal = normalize(normal * 2.0 - 1.0);

    let ambient_strength = 0.01;
    let ambient = light.color * ambient_strength;

    let light_direction = normalize(in.tangent_light_pos - in.tangent_world_pos);
    let diffuse_strength = max(dot(normal, light_direction), 0.0);
    let diffuse = light.color * diffuse_strength;

    let view_direction = normalize(in.tangent_view_pos - in.tangent_world_pos);
    let half_direction = normalize(view_direction + light_direction);
    let shiness = 32.;
    let spec_strength = pow(max(dot(normal, half_direction), 0.), shiness);

    let specular = spec_strength * light.color;

    let out_color = tex_color * (ambient + diffuse + specular);

    return vec4<f32>(out_color, 1.0);
}