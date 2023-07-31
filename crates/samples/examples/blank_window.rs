use std::time::Duration;

use gf_base::snafu::{OptionExt, ResultExt};
use gf_base::{default_configs, downcast_mut, run, BaseState, Error, StateDynObj, SurfaceErrSnafu};
use gf_base::{wgpu, NoneErrSnafu};

#[derive(Default)]
struct State {
    i: u32,
}

impl StateDynObj for State {}

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
        let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.1,
                        g: 0.2,
                        b: 0.3,
                        a: 1.0,
                    }),
                    store: true,
                },
            })],
            depth_stencil_attachment: None,
        });
    }

    // submit will accept anything that implements IntoIter
    base_state.queue.submit(std::iter::once(encoder.finish()));
    output.present();
    Ok(())
}

fn main() {
    pollster::block_on(run(
        default_configs,
        |base_state| {
            let state = Box::new(State { i: 3213312 });
            base_state.extra_state = Some(state);
            Ok(())
        },
        |base_state, dt| {
            let state_long_live = base_state.extra_state.as_mut().context(NoneErrSnafu)?;
            let state = downcast_mut::<State>(state_long_live).context(NoneErrSnafu)?;
            println!("state: {}", state.i);
            Ok(())
        },
        render,
    ))
}
