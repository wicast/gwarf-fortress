use std::ops::Range;
use std::time::Duration;

use gf_base::asset::gltf::load_gltf;
use gf_base::snafu::ErrorCompat;
use gf_base::wgpu;
use gf_base::wgpu::util::{BufferInitDescriptor, DrawIndexedIndirect};
use gf_base::{downcast_mut, run, BaseState, StateDynObj};

use wgpu::util::DeviceExt;

#[derive(Default)]
struct State {
    render_pipeline: Option<wgpu::RenderPipeline>,
    vertices: Option<wgpu::Buffer>,
    index: Option<wgpu::Buffer>,
    obj_count: usize,
    obj_buf: Option<wgpu::Buffer>,
    indirect_buf: Option<wgpu::Buffer>,
}

impl StateDynObj for State {}

fn init(base_state: &mut BaseState) {
    let device = &base_state.device;
    let state = downcast_mut::<State>(&mut base_state.extra_state).unwrap();

    let path = format!(
        "{}/../../assets/gltf/simple_two.glb",
        env!("CARGO_MANIFEST_DIR")
    );
    // let path = format!(
    //     "{}/../../assets/gltf/simple_plane.gltf",
    //     env!("CARGO_MANIFEST_DIR")
    // );
    let path = format!(
        "{}/../../assets/gltf/FlightHelmet/FlightHelmet.gltf",
        env!("CARGO_MANIFEST_DIR")
    );

    let (scene_view, scene_buffer) = match load_gltf(&path) {
        Ok(scene) => scene,
        Err(e) => {
            eprintln!("An error occurred: {}", e);
            if let Some(bt) = ErrorCompat::backtrace(&e) {
                eprintln!("{:?}", bt);
            }
            return;
        }
    };

    let vert_buf = device.create_buffer_init(&BufferInitDescriptor {
        label: None,
        contents: &scene_buffer.positions,
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buf = device.create_buffer_init(&BufferInitDescriptor {
        label: None,
        contents: &scene_buffer.index,
        usage: wgpu::BufferUsages::INDEX,
    });
    let obj_buf = device.create_buffer_init(&BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&scene_buffer.per_node),
        usage: wgpu::BufferUsages::VERTEX,
    });

    let mut indirect = Vec::new();

    let mut obj_count = 0;
    for node in &scene_view.nodes {
        for mesh in &node.meshes {
            println!("mesh.index.type_size:{}", mesh.index.type_size);
            indirect.push(DrawIndexedIndirect {
                vertex_count: mesh.index.count as u32,
                instance_count: 1,
                base_index: (mesh.index.indices.start / mesh.index.type_size) as u32,
                vertex_offset: (mesh.positions.start / mesh.vertex_size) as i32,
                base_instance: obj_count,
            });
            obj_count += 1;
        }
    }
    state.obj_count = obj_count as usize;

    let indirect: Vec<u8> = indirect
        .iter()
        .map(|i| i.as_bytes())
        .flat_map(|i| i.iter())
        .copied()
        .collect();

    let indirect_buf = device.create_buffer_init(&BufferInitDescriptor {
        label: None,
        contents: &indirect,
        usage: wgpu::BufferUsages::INDIRECT,
    });

    let vertex_layout = wgpu::VertexBufferLayout {
        //TODO calc stride
        array_stride: 3 * 4,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![
            0 => Float32x3,
        ],
    };

    let object_layout = wgpu::VertexBufferLayout {
        //TODO calc stride
        array_stride: 4 * 4 * 4,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &wgpu::vertex_attr_array![
            1 => Float32x4,
            2 => Float32x4,
            3 => Float32x4,
            4 => Float32x4,
        ],
    };

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
            buffers: &[vertex_layout, object_layout],
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

    state.render_pipeline = Some(render_pipeline);

    state.vertices = Some(vert_buf);
    state.index = Some(index_buf);
    state.obj_buf = Some(obj_buf);
    state.indirect_buf = Some(indirect_buf);
}

fn render(base_state: &mut BaseState, _dt: Duration) -> Result<(), wgpu::SurfaceError> {
    let output: wgpu::SurfaceTexture = base_state.surface.get_current_texture()?;
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
        render_pass.set_vertex_buffer(1, state.obj_buf.as_ref().unwrap().slice(..));
        render_pass.set_index_buffer(
            state.index.as_ref().unwrap().slice(..),
            wgpu::IndexFormat::Uint16,
        );
        render_pass.multi_draw_indexed_indirect(
            state.indirect_buf.as_ref().unwrap(),
            0,
            state.obj_count as u32,
        );
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
        || {
            (
                wgpu::Backends::all(),
                wgpu::Features::MULTI_DRAW_INDIRECT | wgpu::Features::INDIRECT_FIRST_INSTANCE,
            )
        },
        init,
        |_state, _dt| {},
        render,
    ))
}

#[test]
fn test_gltf_loader() {
    let path = format!(
        "{}/../../assets/gltf/simple_two.glb",
        env!("CARGO_MANIFEST_DIR")
    );

    let (scene_view, scene_buffer) = match load_gltf(&path) {
        Ok(scene) => scene,
        Err(e) => {
            eprintln!("An error occurred: {}", e);
            if let Some(bt) = ErrorCompat::backtrace(&e) {
                eprintln!("{:?}", bt);
            }
            panic!()
        }
    };

    let positions: Vec<[f32; 3]> = check_cast(
        &scene_buffer.positions,
        scene_view.nodes[1].meshes[0].positions.clone(),
    );

    match scene_view.nodes[1].meshes[0].index.r#type {
        gf_base::asset::gltf::IndexType::U16 => {
            let index: Vec<[u16; 1]> = check_cast(
                &scene_buffer.index,
                scene_view.nodes[1].meshes[0].index.indices.clone(),
            );
        }
        gf_base::asset::gltf::IndexType::U32 => {
            let index: Vec<[u32; 1]> = check_cast(
                &scene_buffer.index,
                scene_view.nodes[1].meshes[0].index.indices.clone(),
            );
        }
    }

    println!("simple two mat 0: {:?}", scene_view.materials[0]);

    let path = format!(
        "{}/../../assets/gltf/FlightHelmet/FlightHelmet.gltf",
        env!("CARGO_MANIFEST_DIR")
    );

    let (scene_view, scene_buffer) = match load_gltf(&path) {
        Ok(scene) => scene,
        Err(e) => {
            eprintln!("An error occurred: {}", e);
            if let Some(bt) = ErrorCompat::backtrace(&e) {
                eprintln!("{:?}", bt);
            }
            panic!()
        }
    };

    let positions: Vec<[f32; 3]> = check_cast(
        &scene_buffer.positions,
        scene_view.nodes[0].meshes[0].positions.clone(),
    );
    let positions: Vec<[f32; 3]> = check_cast(
        &scene_buffer.positions,
        scene_view.nodes[1].meshes[0].positions.clone(),
    );
    match scene_view.nodes[1].meshes[0].index.r#type {
        gf_base::asset::gltf::IndexType::U16 => {
            let index: Vec<[u16; 1]> = check_cast(
                &scene_buffer.index,
                scene_view.nodes[1].meshes[0].index.indices.clone(),
            );
        }
        gf_base::asset::gltf::IndexType::U32 => {
            let index: Vec<[u32; 1]> = check_cast(
                &scene_buffer.index,
                scene_view.nodes[1].meshes[0].index.indices.clone(),
            );
        }
    }

    // println!("{:?}", positions);

    println!("mat 0: {:?}", scene_view.materials[0]);

    let tex_info = scene_view.materials[0]
        .get(&gf_base::asset::gltf::MaterialKey::BaseColor)
        .unwrap();
    println!("{:?}", tex_info.mime);
    let img = gf_base::image::load_from_memory(
        &scene_buffer.shared_data[tex_info.data_range.clone().unwrap()],
    )
    .unwrap();

    // let path = format!(
    //     "{}/assets/gltf/simple_plane.gltf",
    //     std::env::current_dir().unwrap().display()
    // );

    // img.save("test.jpg").unwrap();
}

fn check_cast<T: Copy + bytemuck::Pod, const N: usize>(
    scene_buffer: &[u8],
    range: Range<usize>,
) -> Vec<[T; N]> {
    bytemuck::cast_slice(&scene_buffer[range])
        .chunks(N)
        .map(|slice| <[T; N]>::try_from(slice).unwrap())
        .collect()
}
