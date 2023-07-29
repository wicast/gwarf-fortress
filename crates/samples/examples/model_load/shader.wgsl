struct Camera {
    view_pos: vec4<f32>,
    view_proj: mat4x4<f32>,
}
@group(0) @binding(0)
var<uniform> camera: Camera;

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
    @location(8) tran_row_0: vec4<f32>,
    @location(9) tran_row_1: vec4<f32>,
    @location(10) tran_row_2: vec4<f32>,
    @location(11) tran_row_3: vec4<f32>,
    @location(12) base_color: u32,
    @location(13) base_color_sampler: u32,
}


struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv0: vec2<f32>,
    @location(1) base_color: u32,
    @location(2) base_color_sampler: u32,
};

@vertex
fn vs_main(
    vert: VertexInput,
    obj: PerObjInput,
    normal: Normal,
    uv: UV,
) -> VertexOutput {
    var out: VertexOutput;
    let obj_transform: mat4x4<f32> = mat4x4<f32>(obj.tran_row_0, obj.tran_row_1, obj.tran_row_2, obj.tran_row_3);
    out.clip_position = camera.view_proj * obj_transform * vec4<f32>(vert.position, 1.0);
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
    let out_color = textureSampleLevel(
        base_color_texes[in.base_color],
        samplers[in.base_color_sampler],
        in.uv0,
        0.0
    ).rgb;
    return vec4<f32>(out_color, 1.0);
}