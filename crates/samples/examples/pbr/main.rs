use std::mem::size_of;
use std::num::NonZeroU32;
use std::time::Duration;

use gf_base::{
    asset::gltf::{load_gltf, LoadOption, MaterialKey, self, SInto},
    downcast_mut,
    glam::{Mat3, Mat4},
    image::GenericImageView,
    snafu::{OptionExt, ResultExt},
    texture::{self},
    wgpu::{
        self,
        util::{BufferInitDescriptor, DeviceExt, DrawIndexedIndirect, RenderEncoder},
        BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
        BindGroupLayoutEntry, DepthStencilState, Operations, PipelineLayoutDescriptor,
        RenderPassDepthStencilAttachment, ShaderStages, TextureDescriptor,
        VertexFormat::*,
    },
    winit, App, BaseState, Error, GLTFErrSnafu, ImageLoadErrSnafu, NoneErrSnafu, StateDynObj,
    SurfaceErrSnafu,
};

struct State {
    gbuffer_pipeline: wgpu::RenderPipeline,
    index: wgpu::Buffer,
    vertices: wgpu::Buffer,
    normal: wgpu::Buffer,
    uv0: wgpu::Buffer,
    tangent: wgpu::Buffer,
    bi_tangent: wgpu::Buffer,
    obj_count: usize,
    obj_buf: wgpu::Buffer,
    indirect_buf: wgpu::Buffer,
    tex_bind_group: wgpu::BindGroup,

    emissive_pipeline: wgpu::RenderPipeline,
    light_buf: wgpu::Buffer,
    light_bind_group: wgpu::BindGroup,
    cube_buf: wgpu::Buffer,
    cube_ind: wgpu::Buffer,
    cube_ind_count: usize,

    deferred_pipeline: wgpu::RenderPipeline,
    quad_buf: wgpu::Buffer,
    quad_uv_buf: wgpu::Buffer,
    gbuffer_bind_group: wgpu::BindGroup,
    position_gb: texture::Texture,
    normal_gb: texture::Texture,
    albedo_gb: texture::Texture,
}

impl StateDynObj for State {}

#[repr(C, align(16))]
#[derive(Copy, Clone, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct PerObjData {
    base_color: u32,
    sampler: u32,
    normal: u32,
    normal_sampler: u32,
    metallic: u32,
    metallic_sampler: u32,
    pub transform: [[f32; 4]; 4],
    _padding: [u32; 2],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct PerObjB {
    pub transform: Mat4,
    pub normal_matrix: Mat3,
    _padding: [f32; 3],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct MeshMaterial {
    base_color: u32,
    sampler: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct LightBuffer {
    position: [f32; 3],
    _padding: u32,
    color: [f32; 3],
    _padding2: u32,
}

fn init(base_state: &mut BaseState) -> Result<(), Error> {
    let device = &base_state.device;
    let queue = &base_state.queue;

    base_state.camera.position = gf_base::glam::Vec3::from([0.0, 4.0, 5.0]);

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
        "{}/../../assets/gltf/glTF-Sample-Models/2.0/FlightHelmet/glTF/FlightHelmet.gltf",
        env!("CARGO_MANIFEST_DIR")
    );

    let path = format!(
        "{}/../../assets/gltf/glTF-Sample-Models/2.0/DamagedHelmet/glTF/DamagedHelmet.gltf",
        env!("CARGO_MANIFEST_DIR")
    );

    // let path = format!(
    //     "{}/../../assets/gltf/glTF-Sample-Models/2.0/BarramundiFish/glTF/BarramundiFish.gltf",
    //     env!("CARGO_MANIFEST_DIR")
    // );

    // let path = format!(
    //     "{}/../../assets/gltf/glTF-Sample-Models/2.0/Box With Spaces/glTF/Box With Spaces.gltf",
    //     env!("CARGO_MANIFEST_DIR")
    // );

    //TODO transform has a problem
    // let path = format!(
    //     "{}/../../assets/gltf/ikea_sacabambaspis/scene.gltf",
    //     env!("CARGO_MANIFEST_DIR")
    // );

    // let path = format!(
    //     "{}/../../assets/gltf/ikea_sacabambaspis/sacabambaspis.glb",
    //     env!("CARGO_MANIFEST_DIR")
    // );

    let (scene_view, scene_buffer) =
        load_gltf(&path, LoadOption { gen_tbn: true }).context(GLTFErrSnafu)?;

    //Load scene data to wgpu buffer
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
        contents: bytemuck::cast_slice(scene_buffer.texcoord.get(0).context(NoneErrSnafu)?),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let tangent_buf = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("tangent"),
        contents: bytemuck::cast_slice(&scene_buffer.tangent),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let bi_tangent_buf = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("bi_tangent"),
        contents: bytemuck::cast_slice(&scene_buffer.bi_tangent),
        usage: wgpu::BufferUsages::VERTEX,
    });

    //Collect Indirect commands&generate per object buffer
    let mut indirect = Vec::new();
    let mut obj_count = 0;
    let mut per_obj_data = vec![];
    for node in &scene_view.nodes {
        let node = node.1;
        for mesh in &node.meshes {
            indirect.push(DrawIndexedIndirect {
                vertex_count: mesh.index.count as u32,
                instance_count: 1,
                base_index: (mesh.index.indices.start / mesh.index.type_size) as u32,
                vertex_offset: (mesh.positions.start / mesh.vertex_type_size) as i32,
                base_instance: 0,
            });
            let gltf_mat = &scene_view.materials[mesh.mat.context(NoneErrSnafu)?];
            let base_color = gltf_mat
                .get(&MaterialKey::BaseColor)
                .context(NoneErrSnafu)?;
            let normal_tex = gltf_mat.get(&MaterialKey::Normal).context(NoneErrSnafu)?;
            let metallic_tex = gltf_mat
                .get(&MaterialKey::MetallicRoughness)
                .context(NoneErrSnafu)?;

            let per_obj = PerObjData {
                transform: node.per_node_info.transform.to_cols_array_2d(),
                base_color: base_color.image_id.context(NoneErrSnafu)? as u32,
                sampler: base_color.sampler as u32,
                normal: normal_tex.image_id.context(NoneErrSnafu)? as u32,
                normal_sampler: normal_tex.sampler as u32,
                metallic: metallic_tex.image_id.context(NoneErrSnafu)? as u32,
                metallic_sampler: metallic_tex.sampler as u32,
                _padding: [0; 2],
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

    //scene texture&sampler
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
                label: img_info.name.as_deref(),
                size: wgpu::Extent3d {
                    width: img_dimensions.0,
                    height: img_dimensions.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: img_info.target_format,
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
        let desc: wgpu::SamplerDescriptor<'_> = sampler.t_into();
        let wgpu_sampler = device.create_sampler(&desc);
        samplers.push(wgpu_sampler);
    }
    if samplers.is_empty() {
        let desc = wgpu::SamplerDescriptor {
            label: Some("Default Sampler"),
            ..Default::default()
        };
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
    let tangent_layout = wgpu::VertexBufferLayout {
        array_stride: Float32x4.size(),
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![
            3 => Float32x4,
        ],
    };
    let bi_tangent_layout = wgpu::VertexBufferLayout {
        array_stride: Float32x3.size(),
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![
            4 => Float32x3,
        ],
    };

    let object_layout = wgpu::VertexBufferLayout {
        array_stride: size_of::<PerObjData>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &wgpu::vertex_attr_array![
            8 => Uint32,
            9 => Uint32,
            10 => Uint32,
            11 => Uint32,
            12 => Uint32,
            13 => Uint32,

            14 => Float32x4,
            15 => Float32x4,
            16 => Float32x4,
            17 => Float32x4,
        ],
    };
    // bind group layout
    let tex_bind_group_layout: wgpu::BindGroupLayout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
    let light_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
            label: None,
        });

    //GBuffer Attachments
    let (position_gb, normal_gb, albedo_gb, gbuffer_bind_group_layout, gbuffer_bind_group) =
        prepare_gbuffer_resource(base_state)?;

    //Gbuffer pipeline

    let gbuffer_shader = device.create_shader_module(wgpu::include_wgsl!("gbuffer_shader.wgsl"));

    let gbuffer_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("GBuffer Pipeline Layout"),
        bind_group_layouts: &[&base_state.camera_bind_group_layout, &tex_bind_group_layout],
        push_constant_ranges: &[],
    });

    let gbuffer_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("GBuffer Render Layout"),
        layout: Some(&gbuffer_pipeline_layout),
        vertex: wgpu::VertexState {
            module: &gbuffer_shader,
            entry_point: "vs_main",
            buffers: &[
                vertex_layout.clone(),
                normal_layout,
                uv0_layout.clone(),
                tangent_layout,
                object_layout,
            ],
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
            module: &gbuffer_shader,
            entry_point: "fs_main",
            targets: &[
                //Position
                Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba16Float,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                }),
                //Normal
                Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba16Float,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                }),
                Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba16Float,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
            ],
        }),
        multiview: None,
    });

    //Deferred render
    let quad_vertices = [
        [-1.0f32, 1.0, 0.0],
        [-1.0, -1.0, 0.0],
        [1.0, 1.0, 0.0],
        [1.0, -1.0, 0.0],
        [1.0, 1.0, 0.0],
        [-1.0, -1.0, 0.0],
    ];
    let quad_uv = [
        [0.0f32, 0.0],
        [0.0, 1.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [1.0, 0.0],
        [0.0, 1.0],
    ];

    let quad_buf = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("GBuffer Quad"),
        contents: bytemuck::cast_slice(&quad_vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let quad_uv_buf = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("GBuffer Quad UV"),
        contents: bytemuck::cast_slice(&quad_uv),
        usage: wgpu::BufferUsages::VERTEX,
    });

    let deferred_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: Some("Deferred Pipeline layout"),
        bind_group_layouts: &[
            &base_state.camera_bind_group_layout,
            &gbuffer_bind_group_layout,
            &light_bind_group_layout,
        ],
        push_constant_ranges: &[],
    });

    let deferred_shader = device.create_shader_module(wgpu::include_wgsl!("deferred_shader.wgsl"));

    let deferred_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Deferred Pipeline"),
        layout: Some(&deferred_pipeline_layout),
        vertex: wgpu::VertexState {
            module: &deferred_shader,
            entry_point: "vs_main",
            buffers: &[vertex_layout.clone(), uv0_layout],
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
            module: &deferred_shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format: base_state.config.format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
    });

    //Light debug
    // Generate light data
    let light_data = LightBuffer {
        position: [-0., 1.5, 0.3],
        // position: [-0.4, 0.1, -0.3],
        // position: [0., 0.5, -0.3],
        color: [1.0; 3],
        _padding: 0,
        _padding2: 0,
    };
    let light_buf = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("light buffer"),
        contents: bytemuck::cast_slice(&[light_data]),
        usage: wgpu::BufferUsages::UNIFORM,
    });

    let light_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        layout: &light_bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: light_buf.as_entire_binding(),
        }],
        label: None,
    });

    let cube_path = format!("{}/../../assets/gltf/cube.glb", env!("CARGO_MANIFEST_DIR"));

    let (scene_view, scene_buffer) =
        load_gltf(cube_path, Default::default()).context(GLTFErrSnafu)?;
    let cube_pos = &scene_buffer.positions[scene_view.nodes.get(&0).context(NoneErrSnafu)?.meshes
        [0]
    .positions
    .clone()];
    let cube_buffer = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("light debug cube pos"),
        contents: cube_pos,
        usage: wgpu::BufferUsages::VERTEX,
    });
    let cube_ind = &scene_buffer.index[scene_view.nodes.get(&0).context(NoneErrSnafu)?.meshes[0]
        .index
        .indices
        .clone()];
    let cube_ind_buffer = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("light debug cube index"),
        contents: cube_ind,
        usage: wgpu::BufferUsages::INDEX,
    });

    let emissive_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Light Pipeline Layout"),
        bind_group_layouts: &[
            &base_state.camera_bind_group_layout,
            &light_bind_group_layout,
        ],
        push_constant_ranges: &[],
    });
    let light_debug_shader = device.create_shader_module(wgpu::include_wgsl!("light_debug.wgsl"));

    let emissive_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("emissive Pipeline"),
        layout: Some(&emissive_pipeline_layout),
        vertex: wgpu::VertexState {
            module: &light_debug_shader,
            entry_point: "vs_main",
            buffers: &[vertex_layout],
        },
        fragment: Some(wgpu::FragmentState {
            module: &light_debug_shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format: base_state.config.format,
                blend: Some(wgpu::BlendState {
                    alpha: wgpu::BlendComponent::REPLACE,
                    color: wgpu::BlendComponent::REPLACE,
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            // Setting this to anything other than Fill requires Features::NON_FILL_POLYGON_MODE
            polygon_mode: wgpu::PolygonMode::Fill,
            // Requires Features::DEPTH_CLIP_CONTROL
            unclipped_depth: false,
            // Requires Features::CONSERVATIVE_RASTERIZATION
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
        // If the pipeline will be used with a multiview render pass, this
        // indicates how many array layers the attachments will have.
        multiview: None,
    });

    let state = Box::new(State {
        gbuffer_pipeline,
        deferred_pipeline,
        vertices: vert_buf,
        normal: normal_buf,
        uv0: uv0_buf,
        index: index_buf,
        obj_count: obj_count as usize,
        obj_buf,
        indirect_buf,
        tex_bind_group,
        light_buf,
        light_bind_group,
        emissive_pipeline,
        cube_buf: cube_buffer,
        cube_ind: cube_ind_buffer,
        cube_ind_count: scene_view.nodes.get(&0).context(NoneErrSnafu)?.meshes[0]
            .index
            .count,
        tangent: tangent_buf,
        bi_tangent: bi_tangent_buf,
        position_gb,
        normal_gb,
        albedo_gb,
        gbuffer_bind_group,
        quad_buf,
        quad_uv_buf,
    });
    base_state.extra_state = Some(state);

    Ok(())
}

fn prepare_gbuffer_resource(
    base_state: &BaseState,
) -> Result<
    (
        texture::Texture,
        texture::Texture,
        texture::Texture,
        BindGroupLayout,
        wgpu::BindGroup,
    ),
    Error,
> {
    let device = &base_state.device;

    let pos_tex_desc = wgpu::TextureDescriptor {
        label: Some("Gbuffer Position"),
        size: wgpu::Extent3d {
            width: base_state.size.width,
            height: base_state.size.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT // 3.
        | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    };
    let mut position_tex = texture::Texture::create_texture(device, pos_tex_desc, true);
    let normal_tex_desc = wgpu::TextureDescriptor {
        label: Some("Gbuffer Normal"),
        size: wgpu::Extent3d {
            width: base_state.size.width,
            height: base_state.size.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT // 3.
        | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    };
    let normal_tex = texture::Texture::create_texture(device, normal_tex_desc, false);
    let albedo_tex_desc = wgpu::TextureDescriptor {
        label: Some("Gbuffer Albedo"),
        size: wgpu::Extent3d {
            width: base_state.size.width,
            height: base_state.size.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT // 3.
        | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    };
    let mut albedo_tex = texture::Texture::create_texture(device, albedo_tex_desc, true);
    let gbuffer_bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some("GBuffer Bind Group Layout"),
        entries: &[
            //Sampler
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            //Position Buffer
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            //Normal Buffer
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            //Albedo Buffer
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            // Albedo Sampler
            BindGroupLayoutEntry {
                binding: 4,
                visibility: ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let gbuffer_bind_group = device.create_bind_group(&BindGroupDescriptor {
        label: Some("GBuffer Bind Group"),
        layout: &gbuffer_bind_group_layout,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Sampler(
                    &position_tex.sampler.take().context(NoneErrSnafu)?,
                ),
            },
            BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&position_tex.view),
            },
            BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&normal_tex.view),
            },
            BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&albedo_tex.view),
            },
            BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Sampler(
                    &albedo_tex.sampler.take().context(NoneErrSnafu)?,
                ),
            },
        ],
    });
    Ok((
        position_tex,
        normal_tex,
        albedo_tex,
        gbuffer_bind_group_layout,
        gbuffer_bind_group,
    ))
}

//TODO
fn tick(_state: &mut BaseState, _dt: Duration) -> Result<(), Error> {
    Ok(())
}

fn render(base_state: &mut BaseState, _dt: Duration) -> Result<(), Error> {
    let state_long_live = base_state.extra_state.as_mut().context(NoneErrSnafu)?;
    let state = downcast_mut::<State>(state_long_live).context(NoneErrSnafu)?;
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

    //Render Gbuffers
    {
        let mut gbuffer_render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Render Pass"),
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment {
                    view: &state.position_gb.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: &state.normal_gb.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: &state.albedo_gb.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                }),
            ],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: &base_state.depth.view,
                depth_ops: Some(Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        gbuffer_render_pass.set_pipeline(&state.gbuffer_pipeline);
        gbuffer_render_pass.set_bind_group(0, &base_state.camera_bind_group, &[]);
        gbuffer_render_pass.set_bind_group(1, &state.tex_bind_group, &[]);

        gbuffer_render_pass.set_vertex_buffer(0, state.vertices.slice(..));
        gbuffer_render_pass.set_vertex_buffer(1, state.normal.slice(..));
        gbuffer_render_pass.set_vertex_buffer(2, state.uv0.slice(..));
        gbuffer_render_pass.set_vertex_buffer(3, state.tangent.slice(..));
        gbuffer_render_pass.set_vertex_buffer(4, state.obj_buf.slice(..));

        gbuffer_render_pass.set_index_buffer(state.index.slice(..), wgpu::IndexFormat::Uint32);

        gbuffer_render_pass.multi_draw_indexed_indirect(
            &state.indirect_buf,
            0,
            state.obj_count as u32,
        );
    }

    //Deferred render
    {
        let mut deferred_render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        deferred_render_pass.set_pipeline(&state.deferred_pipeline);
        deferred_render_pass.set_bind_group(0, &base_state.camera_bind_group, &[]);
        deferred_render_pass.set_bind_group(1, &state.gbuffer_bind_group, &[]);
        deferred_render_pass.set_bind_group(2, &state.light_bind_group, &[]);

        deferred_render_pass.set_vertex_buffer(0, state.quad_buf.slice(..));
        deferred_render_pass.set_vertex_buffer(1, state.quad_uv_buf.slice(..));

        deferred_render_pass.draw(0..6, 0..1);
    }

    // Render light debug cube
    {
        let mut emissive_render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: &base_state.depth.view,
                depth_ops: Some(Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        emissive_render_pass.set_pipeline(&state.emissive_pipeline);
        emissive_render_pass.set_bind_group(0, &base_state.camera_bind_group, &[]);
        emissive_render_pass.set_bind_group(1, &state.light_bind_group, &[]);
        emissive_render_pass.set_vertex_buffer(0, state.cube_buf.slice(..));
        emissive_render_pass.set_index_buffer(state.cube_ind.slice(..), wgpu::IndexFormat::Uint32);
        emissive_render_pass.draw_indexed(0..state.cube_ind_count as u32, 0, 0..1);
    }

    // submit will accept anything that implements IntoIter
    base_state.queue.submit(std::iter::once(encoder.finish()));
    output.present();
    Ok(())
}

fn resize(
    base_state: &mut BaseState,
    _new_size: winit::dpi::PhysicalSize<u32>,
) -> Result<(), Error> {
    let (position_gb, normal_gb, albedo_gb, gbuffer_bind_group_layout, gbuffer_bind_group) =
        prepare_gbuffer_resource(base_state)?;
    let state_long_live = base_state.extra_state.as_mut().context(NoneErrSnafu)?;
    let state = downcast_mut::<State>(state_long_live).context(NoneErrSnafu)?;

    state.gbuffer_bind_group = gbuffer_bind_group;
    state.position_gb = position_gb;
    state.normal_gb = normal_gb;
    state.albedo_gb = albedo_gb;
    Ok(())
}

fn main() {
    let mut app = App::builder()
        .config((
            wgpu::Backends::VULKAN | wgpu::Backends::METAL,
            wgpu::Features::MULTI_DRAW_INDIRECT
                | wgpu::Features::INDIRECT_FIRST_INSTANCE
                | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING
                | wgpu::Features::TEXTURE_BINDING_ARRAY,
        ))
        .init_fn(init)
        .tick_fn(tick)
        .render_fn(render)
        .resize_fn(resize)
        .build();

    app.run();
}
