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

struct PerObjInput {
    @location(8) base_color: u32,
    @location(9) base_color_sampler: u32,

    @location(14) modle_mat_0: vec4<f32>,
    @location(15) modle_mat_1: vec4<f32>,
    @location(16) modle_mat_2: vec4<f32>,
    @location(17) modle_mat_3: vec4<f32>,
    @location(18) normal_mat_0: vec3<f32>,
    @location(19) normal_mat_1: vec3<f32>,
    @location(20) normal_mat_2: vec3<f32>,
}


struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv0: vec2<f32>,
    @location(1) world_pos: vec3<f32>,
    @location(2) normal: vec3<f32>,
    @location(8) base_color: u32,
    @location(9) base_color_sampler: u32,
};

@vertex
fn vs_main(
    vert: VertexInput,
    obj: PerObjInput,
    normal: Normal,
    uv: UV,
) -> VertexOutput {
    var out: VertexOutput;

    let modle_mat = mat4x4<f32>(obj.modle_mat_0, obj.modle_mat_1, obj.modle_mat_2, obj.modle_mat_3);
    let normal_mat = mat3x3<f32>(obj.normal_mat_0, obj.normal_mat_1, obj.normal_mat_2);

    out.world_pos = (modle_mat * vec4<f32>(vert.position, 1.0)).xyz;
    out.clip_position = camera.view_proj * vec4<f32>(out.world_pos, 1.0);
    out.normal = normal_mat * normal.normal;

    out.uv0 = uv.uv0;
    out.base_color = obj.base_color;
    out.base_color_sampler = obj.base_color_sampler;
    return out;
}

// Fragment shader

@group(1) @binding(0)
var base_color_texes: binding_array<texture_2d<f32>>;
@group(1) @binding(1)
var samplers: binding_array<sampler>;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let ambient_strength= 0.01;
    let ambient = light.color * ambient_strength;

    let light_direction = normalize(light.position - in.world_pos);
    let diffuse_strength = max(dot(in.normal, light_direction), 0.0);
    let diffuse = light.color * diffuse_strength;

    let view_direction = normalize(camera.view_pos.xyz - in.world_pos);
    let half_direction = normalize(view_direction+light_direction);
    let shiness = 256.;
    let spec_strength = pow(max(dot(in.normal, half_direction), 0.), shiness);

    let specular = spec_strength * light.color;
    
    let tex_color = textureSampleLevel(
        base_color_texes[in.base_color],
        samplers[in.base_color_sampler],
        in.uv0,
        0.0
    ).rgb;

    let out_color = tex_color * (ambient+ diffuse + specular);

    return vec4<f32>(out_color, 1.0);
}