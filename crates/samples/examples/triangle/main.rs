use std::time::Duration;

use gf_base::{downcast_mut, run, BaseState, StateDynObj, default_configs};
use gf_base::wgpu;


#[derive(Default)]
struct State {
    render_pipeline: Option<wgpu::RenderPipeline>,
}

impl StateDynObj for State {}

fn init(state: &mut BaseState) {
    let device = &state.device;
    let shader = state
        .device
        .create_shader_module(wgpu::include_wgsl!("shader.wgsl"));

    let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Render Pipeline Layout"),
        bind_group_layouts: &[],
        push_constant_ranges: &[],
    });

    let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Render Pipeline Layout"),
        layout: Some(&render_pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            buffers: &[]
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
        multisample: wgpu::MultisampleState { count: 1, mask: !0, alpha_to_coverage_enabled: false },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState { // 4.
                format: state.config.format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })]
        }),
        multiview: None,
    });

    let mut state = downcast_mut::<State>(&mut state.extra_state).unwrap();
    state.render_pipeline = Some(render_pipeline);
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
        let pipeline = &state.render_pipeline.as_ref();
        render_pass.set_pipeline(pipeline.unwrap());
        render_pass.draw(0..3, 0..1);
    }

    // submit will accept anything that implements IntoIter
    state.queue.submit(std::iter::once(encoder.finish()));
    output.present();
    Ok(())
}

fn main() {
    pollster::block_on(run(
        Box::new(State::default()),
        default_configs,
        init,
        |state, dt| {
            // let state = cast_mut::<State>(&mut state.extra_state).unwrap();
            // println!("state: {}", state.i)
        },
        render,
    ))
}
