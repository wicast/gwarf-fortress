use std::time::Duration;

use gf_base::asset::gltf::load_gltf;
use gf_base::{default_configs, downcast_mut, run, BaseState, StateDynObj};
use gf_base::wgpu;

use wgpu::util::DeviceExt;

#[derive(Default)]
struct State {
    render_pipeline: Option<wgpu::RenderPipeline>,
    vertices: Option<wgpu::Buffer>,
    index: Option<wgpu::Buffer>,
    index_count: usize,
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


impl StateDynObj for State {}

fn init(base_state: &mut BaseState) {
    let device = &base_state.device;
    let mut state = downcast_mut::<State>(&mut base_state.extra_state).unwrap();

    let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));

    let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Render Pipeline Layout"),
        bind_group_layouts: &[&base_state.camera_bind_group_layout],
        push_constant_ranges: &[],
    });

    let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Render Pipeline Layout"),
        layout: Some(&render_pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
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
            module: &shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                // 4.
                format: base_state.config.format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
    });

    // let path = "/Users/wicast/Third-part/glTF-Sample-Models/2.0/FlightHelmet/glTF-KTX-BasisU/FlightHelmet.gltf";
    let path = format!("{}/assets/simple_two.glb", std::env::current_dir().unwrap().display());
    let path = format!("{}/assets/simple_plane.glb", std::env::current_dir().unwrap().display());
    let mesh = load_gltf(path).unwrap();

    let mut vertices = vec![];
    for m in mesh.positions {
        vertices.push(Vertex {
            position: m,
            color: [0.5, 0.0, 0.5],
        })
    }
    let mut indices = vec![];
    for i in mesh.indices {
        indices.push(i);
    }
    state.index_count = indices.len();

    state.render_pipeline = Some(render_pipeline);
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Vertex Buffer"),
        contents: bytemuck::cast_slice(vertices.as_slice()),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Index Buffer"),
        contents: bytemuck::cast_slice(indices.as_slice()),
        usage: wgpu::BufferUsages::INDEX,
    });
    state.vertices = Some(vertex_buffer);
    state.index = Some(index_buffer);
}

fn render(base_state: &mut BaseState, dt: Duration) -> Result<(), wgpu::SurfaceError> {
    let output = base_state.surface.get_current_texture()?;
    let view = output
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    let mut encoder = base_state
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

        let state = downcast_mut::<State>(&mut base_state.extra_state).unwrap();
        let pipeline = state.render_pipeline.as_ref().unwrap();
        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, &base_state.camera_bind_group, &[]);
        render_pass.set_vertex_buffer(0, state.vertices.as_ref().unwrap().slice(..));
        render_pass.set_index_buffer(
            state.index.as_ref().unwrap().slice(..),
            wgpu::IndexFormat::Uint16,
        );
        render_pass.draw_indexed(0..state.index_count as u32, 0, 0..1);
    }

    // submit will accept anything that implements IntoIter
    base_state.queue.submit(std::iter::once(encoder.finish()));
    output.present();
    Ok(())
}

//WIP!!
fn main() {
    pollster::block_on(run(
        Box::<State>::default(),
        default_configs,
        init,
        |state, dt| {},
        render,
    ))
}
