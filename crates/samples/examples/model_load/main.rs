use std::mem::size_of;
use std::num::NonZeroU32;
use std::ops::Range;
use std::time::Duration;

use gf_base::{
    asset::gltf::{load_gltf, MaterialKey, PerNodeBuffer},
    downcast_mut,
    image::GenericImageView,
    run,
    snafu::{ErrorCompat, OptionExt, ResultExt},
    texture,
    wgpu::{
        self,
        util::{BufferInitDescriptor, DeviceExt, DrawIndexedIndirect},
        DepthStencilState, Operations, RenderPassDepthStencilAttachment, TextureDescriptor,
        VertexFormat::*,
    },
    BaseState, Error, GLTFErrSnafu, ImageLoadErrSnafu, NoneErrSnafu, StateDynObj, SurfaceErrSnafu,
};

struct State {
    render_pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    normal: wgpu::Buffer,
    uv0: wgpu::Buffer,
    index: wgpu::Buffer,
    obj_count: usize,
    obj_buf: wgpu::Buffer,
    indirect_buf: wgpu::Buffer,
    tex_bind_group: wgpu::BindGroup,
}

impl StateDynObj for State {}

#[repr(C, align(4))]
#[derive(Copy, Clone, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct PerObjData {
    node: PerNodeBuffer,
    mat: MeshMaterial,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct MeshMaterial {
    base_color: usize,
    sampler: usize,
}

fn init(base_state: &mut BaseState) -> Result<(), Error> {
    let device = &base_state.device;
    let queue = &base_state.queue;

    // Prepare buffers
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

    let (scene_view, scene_buffer) = load_gltf(&path).context(GLTFErrSnafu)?;

    let vert_buf = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("vertex"),
        contents: &scene_buffer.positions,
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buf = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("index"),
        contents: &scene_buffer.index,
        usage: wgpu::BufferUsages::INDEX,
    });
    let normal_buf = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("normal"),
        contents: bytemuck::cast_slice(&scene_buffer.normal),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let uv0_buf = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("uv0"),
        contents: bytemuck::cast_slice(&scene_buffer.texcoord[0]),
        usage: wgpu::BufferUsages::VERTEX,
    });

    let mut indirect = Vec::new();

    let mut obj_count = 0;
    let mut per_obj_data = vec![];
    for node in &scene_view.nodes {
        for mesh in &node.meshes {
            indirect.push(DrawIndexedIndirect {
                vertex_count: mesh.index.count as u32,
                instance_count: 1,
                base_index: (mesh.index.indices.start / mesh.index.type_size) as u32,
                vertex_offset: (mesh.positions.start / mesh.vertex_size) as i32,
                base_instance: obj_count,
            });
            let gltf_mat = &scene_view.materials[mesh.mat.context(NoneErrSnafu)?];
            let base_color = gltf_mat
                .get(&MaterialKey::BaseColor)
                .context(NoneErrSnafu)?;
            let mat = MeshMaterial {
                //TODO deal without image
                base_color: base_color.image_id.context(NoneErrSnafu)?,
                sampler: base_color.sampler,
            };
            let per_obj = PerObjData {
                node: node.per_node_info,
                mat,
            };
            per_obj_data.push(per_obj);

            obj_count += 1;
        }
    }

    let obj_buf = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("Per obj buffer"),
        contents: bytemuck::cast_slice(&per_obj_data),
        usage: wgpu::BufferUsages::VERTEX,
    });

    let mut texture_view_vec = vec![];
    for img_info in &scene_view.images {
        let color_source_data = &scene_buffer.shared_data[img_info.range.clone()];
        let dyn_img =
            gf_base::image::load_from_memory(color_source_data).context(ImageLoadErrSnafu)?;
        let img_dimensions = dyn_img.dimensions();
        let img_rgb = dyn_img.to_rgba8();

        let tex = device.create_texture_with_data(
            queue,
            &TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: img_dimensions.0,
                    height: img_dimensions.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            &img_rgb,
        );
        let tex_view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        texture_view_vec.push(tex_view);
    }

    let mut samplers = vec![];
    for sampler in &scene_view.samplers {
        let desc: wgpu::SamplerDescriptor<'_> = sampler.clone().into();
        let wgpu_sampler = device.create_sampler(&desc);
        samplers.push(wgpu_sampler);
    }

    let indirect: Vec<u8> = indirect
        .iter()
        .map(|i| i.as_bytes())
        .flat_map(|i| i.iter())
        .copied()
        .collect();

    let indirect_buf = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("indirect"),
        contents: &indirect,
        usage: wgpu::BufferUsages::INDIRECT,
    });

    // Vertex Layout
    let vertex_layout = wgpu::VertexBufferLayout {
        array_stride: Float32x3.size(),
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![
            0 => Float32x3,
        ],
    };
    let normal_layout = wgpu::VertexBufferLayout {
        array_stride: Float32x3.size(),
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![
            1 => Float32x3,
        ],
    };
    let uv0_layout = wgpu::VertexBufferLayout {
        array_stride: Float32x2.size(),
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![
            2 => Float32x2,
        ],
    };
    let object_layout = wgpu::VertexBufferLayout {
        array_stride: size_of::<PerObjData>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &wgpu::vertex_attr_array![
            8 => Float32x4,
            9 => Float32x4,
            10 => Float32x4,
            11 => Float32x4,
            12 => Uint32,
            13 => Uint32,
        ],
    };
    // bind group layout
    let tex_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: NonZeroU32::new(texture_view_vec.len() as u32),
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: NonZeroU32::new(samplers.len() as u32),
            },
        ],
    });

    let tex_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &tex_bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureViewArray(
                    texture_view_vec
                        .iter()
                        .collect::<Vec<&wgpu::TextureView>>()
                        .as_slice(),
                ),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::SamplerArray(
                    samplers.iter().collect::<Vec<&wgpu::Sampler>>().as_slice(),
                ),
            },
        ],
    });

    let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));

    let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Render Pipeline Layout"),
        bind_group_layouts: &[&base_state.camera_bind_group_layout, &tex_bind_group_layout],
        push_constant_ranges: &[],
    });

    let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Render Pipeline Layout"),
        layout: Some(&render_pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            buffers: &[vertex_layout, object_layout, normal_layout, uv0_layout],
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
        depth_stencil: Some(DepthStencilState {
            format: texture::Texture::DEPTH_FORMAT,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
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
    let state = Box::new(State {
        render_pipeline,
        vertices: vert_buf,
        normal: normal_buf,
        uv0: uv0_buf,
        index: index_buf,
        obj_count: obj_count as usize,
        obj_buf,
        indirect_buf,
        tex_bind_group,
    });
    base_state.extra_state = Some(state);

    Ok(())
}

fn render(base_state: &mut BaseState, _dt: Duration) -> Result<(), Error> {
    let output: wgpu::SurfaceTexture = base_state
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
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: &base_state.depth.view,
                depth_ops: Some(Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: false,
                }),
                stencil_ops: None,
            }),
        });

        let state_long_live = base_state.extra_state.as_mut().context(NoneErrSnafu)?;
        let state = downcast_mut::<State>(state_long_live).context(NoneErrSnafu)?;
        let pipeline = &state.render_pipeline;
        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, &base_state.camera_bind_group, &[]);
        render_pass.set_bind_group(1, &state.tex_bind_group, &[]);

        render_pass.set_vertex_buffer(0, state.vertices.slice(..));
        render_pass.set_vertex_buffer(1, state.obj_buf.slice(..));
        render_pass.set_vertex_buffer(2, state.normal.slice(..));
        render_pass.set_vertex_buffer(3, state.uv0.slice(..));

        render_pass.set_index_buffer(state.index.slice(..), wgpu::IndexFormat::Uint16);

        render_pass.multi_draw_indexed_indirect(&state.indirect_buf, 0, state.obj_count as u32);
    }

    // submit will accept anything that implements IntoIter
    base_state.queue.submit(std::iter::once(encoder.finish()));
    output.present();
    Ok(())
}

fn main() {
    pollster::block_on(run(
        || {
            (
                wgpu::Backends::all(),
                wgpu::Features::MULTI_DRAW_INDIRECT
                    | wgpu::Features::INDIRECT_FIRST_INSTANCE
                    | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING
                    | wgpu::Features::TEXTURE_BINDING_ARRAY,
            )
        },
        init,
        |_state, _dt| Ok(()),
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
