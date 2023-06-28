use std::time::Duration;

use gf_base::wgpu;
use gf_base::{downcast_mut, run, BaseState, StateDynObj};

use wgpu::util::DeviceExt;

#[derive(Default)]
struct State {
    render_pipeline: Option<wgpu::RenderPipeline>,
    vertices: Option<wgpu::Buffer>,
    index: Option<wgpu::Buffer>,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    color: [f32; 3],
}

impl Vertex {
    const ATTRIBS: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];

    fn desc() -> wgpu::VertexBufferLayout<'static> {
        use std::mem;

        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

const VERTICES: &[Vertex] = &[
    Vertex {
        position: [-0.0868241, 0.49240386, 0.0],
        color: [0.5, 0.0, 0.5],
    }, // A
    Vertex {
        position: [-0.49513406, 0.06958647, 0.0],
        color: [0.5, 0.0, 0.5],
    }, // B
    Vertex {
        position: [-0.21918549, -0.44939706, 0.0],
        color: [0.5, 0.0, 0.5],
    }, // C
    Vertex {
        position: [0.35966998, -0.3473291, 0.0],
        color: [0.5, 0.0, 0.5],
    }, // D
    Vertex {
        position: [0.44147372, 0.2347359, 0.0],
        color: [0.5, 0.0, 0.5],
    }, // E
];
const INDICES: &[u16] = &[0, 1, 4, 1, 2, 4, 2, 3, 4];

impl StateDynObj for State {}

fn init(state: &mut BaseState) {
    let device = &state.device;
    let shader = state
        .device
        .create_shader_module(wgpu::include_wgsl!("shader.wgsl"));
    let vert_shader = &shader;
    let frag_shader = &shader;
    let vert_shader = unsafe {
        state
            .device
            .create_shader_module_spirv(&wgpu::include_spirv_raw!("shader.spv"))
    };
    let frag_shader = unsafe {
        state
            .device
            .create_shader_module_spirv(&wgpu::include_spirv_raw!("shader.spv"))
    };

    let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Render Pipeline Layout"),
        bind_group_layouts: &[],
        push_constant_ranges: &[],
    });

    let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Render Pipeline Layout"),
        layout: Some(&render_pipeline_layout),
        vertex: wgpu::VertexState {
            module: &vert_shader,
            entry_point: "vs_main",
            buffers: &[Vertex::desc()],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        fragment: Some(wgpu::FragmentState {
            module: &frag_shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                // 4.
                format: state.config.format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
    });

    let mut state = downcast_mut::<State>(&mut state.extra_state).unwrap();
    state.render_pipeline = Some(render_pipeline);
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Vertex Buffer"),
        contents: bytemuck::cast_slice(VERTICES),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Index Buffer"),
        contents: bytemuck::cast_slice(INDICES),
        usage: wgpu::BufferUsages::INDEX,
    });
    state.vertices = Some(vertex_buffer);
    state.index = Some(index_buffer);
}

fn render(state: &mut BaseState, dt: Duration) -> Result<(), wgpu::SurfaceError> {
    let output = state.surface.get_current_texture()?;
    let view = output
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    let mut encoder = state
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });

    {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 1.0,
                    }),
                    store: true,
                },
            })],
            depth_stencil_attachment: None,
        });

        let state = downcast_mut::<State>(&mut state.extra_state).unwrap();
        let pipeline = state.render_pipeline.as_ref().unwrap();
        render_pass.set_pipeline(pipeline);
        render_pass.set_vertex_buffer(0, state.vertices.as_ref().unwrap().slice(..));
        render_pass.set_index_buffer(
            state.index.as_ref().unwrap().slice(..),
            wgpu::IndexFormat::Uint16,
        );
        render_pass.draw_indexed(0..INDICES.len() as u32, 0, 0..1);
    }

    // submit will accept anything that implements IntoIter
    state.queue.submit(std::iter::once(encoder.finish()));
    output.present();
    Ok(())
}

fn main() {
    pollster::block_on(run(
        Box::<State>::default(),
        || {
            (
                wgpu::Backends::all(),
                wgpu::Features::SPIRV_SHADER_PASSTHROUGH,
            )
        },
        init,
        |_state, dt| {
            // let state = cast_mut::<State>(&mut state.extra_state).unwrap();
            // println!("state: {}", state.i)
        },
        render,
    ))
}
