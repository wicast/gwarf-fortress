struct Camera {
    view_pos: vec4<f32>,
    view_proj: mat4x4<f32>,
}
@group(0) @binding(0)
var<uniform> camera: Camera;

struct VertexInput {
    @location(0) position: vec3<f32>,
};

struct PerObjInput {
    @location(1) tran_row_0: vec4<f32>,
    @location(2) tran_row_1: vec4<f32>,
    @location(3) tran_row_2: vec4<f32>,
    @location(4) tran_row_3: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

@vertex
fn vs_main(
    vert: VertexInput,
    obj: PerObjInput,
) -> VertexOutput {
    var out: VertexOutput;
    var obj_transform: mat4x4<f32> = mat4x4<f32>(obj.tran_row_0, obj.tran_row_1, obj.tran_row_2, obj.tran_row_3);
    out.clip_position = camera.view_proj* obj_transform * vec4<f32>(vert.position, 1.0);
    return out;
}

// Fragment shader

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(0.4,0.23, 0.2, 1.0);
}