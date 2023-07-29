struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv0: vec2<f32>,
    @location(1) base_color: u32,
    @location(2) base_color_sampler: u32,
};

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