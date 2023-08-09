//Vertex Buffer
struct VertexInput {
    @location(0) position: vec3<f32>,
};
struct UV {
    @location(2) uv: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    
};

@vertex
fn vs_main(
    vert: VertexInput,
    uv: UV,
) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(vert.position, 1.0);
    out.uv = uv.uv;
    return out;
}

// Fragment Uniform
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

@group(1) @binding(0)
var s: sampler;
@group(1) @binding(1)
var pos_gb: texture_2d<f32>;
@group(1) @binding(2)
var normal_gb: texture_2d<f32>;
@group(1) @binding(3)
var albedo_gb: texture_2d<f32>;
@group(1) @binding(4)
var s1: sampler;

struct FragOut {
    @location(0) color: vec4<f32>
}

@fragment
fn fs_main(in: VertexOutput) -> FragOut {
    var out: FragOut;
    let pos = textureSample(pos_gb, s, in.uv).rgb;
    let normal = textureSample(normal_gb, s, in.uv).rgb;
    let albedo = textureSample(albedo_gb, s1, in.uv);

    let ambient_strength = 0.005;
    let ambient = light.color * ambient_strength;

    let light_direction = normalize(light.position - pos);
    let diffuse_strength = max(dot(normal, light_direction), 0.0);
    let diffuse = light.color * diffuse_strength;

    let view_direction = normalize(camera.view_pos.rgb - pos);
    let half_direction = normalize(view_direction + light_direction);
    let shiness = 32.;
    let spec_strength = pow(max(dot(normal, half_direction), 0.), shiness);

    let specular = spec_strength * light.color;

    let out_color = albedo.rgb * (ambient + diffuse + specular);

    out.color = vec4<f32>(out_color, 1.0);
    return out;
}