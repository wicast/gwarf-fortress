use std::time::Duration;

use gf_base::snafu::{OptionExt, ResultExt};
use gf_base::{downcast_mut, App, BaseState, StateDynObj, SurfaceErrSnafu};
use gf_base::{wgpu, Error, NoneErrSnafu};

struct State {
    render_pipeline: wgpu::RenderPipeline,
}

impl StateDynObj for State {}

fn init(base_state: &mut BaseState) -> Result<(), Error> {
    let device = &base_state.device;

    let shader = base_state
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
            buffers: &[],
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

    let state = Box::new(State { render_pipeline });
    base_state.extra_state = Some(state);

    Ok(())
}

fn render(base_state: &mut BaseState, dt: Duration) -> Result<(), Error> {
    let output = base_state
        .surface
        .get_current_texture()
        .context(SurfaceErrSnafu)?;
    let view = output
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    let mut encoder = base_state
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });

    {
        let state_long_live = base_state.extra_state.as_mut().context(NoneErrSnafu)?;
        let state = downcast_mut::<State>(state_long_live).context(NoneErrSnafu)?;
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
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        let pipeline = &state.render_pipeline;
        render_pass.set_pipeline(pipeline);
        render_pass.draw(0..3, 0..1);
    }

    // submit will accept anything that implements IntoIter
    base_state.queue.submit(std::iter::once(encoder.finish()));
    output.present();
    Ok(())
}

fn main() {
    let mut app = App::builder().init_fn(init).render_fn(render).build();
    app.run();
}
