use gf_base::{downcast_mut, run, BaseState, StateDynObj};
use gf_base::wgpu;


#[derive(Default)]
struct State {
    i: u32,
}

impl StateDynObj for State {}

fn render(state: &mut BaseState) -> Result<(), wgpu::SurfaceError> {
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
    state.queue.submit(std::iter::once(encoder.finish()));
    output.present();
    Ok(())
}

fn main() {
    pollster::block_on(run(
        Box::new(State::default()),
        |state| {
            let mut state = downcast_mut::<State>(&mut state.extra_state).unwrap();
            state.i = 3213312;
        },
        |state| {
            let state = downcast_mut::<State>(&mut state.extra_state).unwrap();
            // println!("state: {}", state.i)
        },
        render,
    ))
}
