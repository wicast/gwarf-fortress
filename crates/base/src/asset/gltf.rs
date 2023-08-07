use std::ops::Range;
use std::{collections::BTreeMap, path::Path};

use base64::{DecodeError, Engine};
use glam::{Mat4, Quat, Vec3};
use goth_gltf::{
    default_extensions, ComponentType, NormalTextureInfo, OcclusionTextureInfo, Sampler,
};
use goth_gltf::{Gltf, NodeTransform, PrimitiveMode, TextureInfo};
use snafu::{Backtrace, OptionExt, ResultExt, Snafu};
use wgpu::TextureFormat;

#[derive(Debug, Snafu)]
pub enum Error {
    JsonDeSerFailed {
        source: nanoserde::DeJsonErr,
    },
    DefaultSceneNotFound,
    Base64MIMENotFound,
    Base64DecodeFailed {
        source: DecodeError,
    },
    FileReadFailed {
        path: String,
        source: std::io::Error,
    },
    UnsupportedIndexType,
    FailedGetBuffer,
    NoneErr {
        backtrace: Backtrace,
    },
    NoPositionFound {
        mesh_id: usize,
        backtrace: Backtrace,
    },
    NoIndexFound {
        mesh_id: usize,
    },
    IndexTypeError {
        mesh_id: usize,
    },
    UTF8Err {
        source: std::string::FromUtf8Error,
    },

    FailedToGetU8Data,
}

#[derive(Debug, Default)]
pub struct SceneView {
    pub nodes: Vec<Node>,
    pub materials: Vec<Material>,
    pub images: Vec<ImageData>,
    pub samplers: Vec<Sampler>,
}

#[derive(Debug, Default, Clone)]
pub struct GLTFBuffer {
    pub positions: Vec<u8>,
    pub tangent: Vec<u8>,
    pub normal: Vec<u8>,
    pub texcoord: Vec<Vec<u8>>,
    pub index: Vec<u8>,
    pub shared_data: Vec<u8>,
    pub bi_tangent: Vec<u8>,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PerNodeBuffer {
    pub transform: Mat4,
}

#[derive(Debug, Default)]
pub struct Node {
    pub id: usize,
    pub name: Option<String>,
    pub meshes: Vec<Mesh>,
    pub per_node_info: PerNodeBuffer,
    pub children: Vec<usize>,
}

#[derive(Debug, Default)]
pub struct Mesh {
    pub id: usize,
    pub index: Index,
    pub vertex_count: usize,
    pub vertex_type_size: usize,
    pub positions: Range<usize>,
    pub normals: Option<Range<usize>>,
    pub tangents: Option<Range<usize>>,
    pub bi_tangents: Option<Range<usize>>,
    pub uv0: Option<Range<usize>>,
    pub mode: PrimitiveMode,
    pub mat: Option<usize>,
}

#[derive(Debug, Default)]
pub struct Index {
    pub indices: Range<usize>,
    pub count: usize,
    pub type_size: usize,
}

#[derive(Debug, Default)]
pub enum IndexType {
    U16,
    #[default]
    U32,
}

impl From<IndexType> for wgpu::IndexFormat {
    fn from(val: IndexType) -> Self {
        match val {
            IndexType::U16 => wgpu::IndexFormat::Uint16,
            IndexType::U32 => wgpu::IndexFormat::Uint32,
        }
    }
}

impl TryFrom<ComponentType> for IndexType {
    type Error = Error;

    fn try_from(value: ComponentType) -> Result<Self, Self::Error> {
        match value {
            ComponentType::UnsignedShort => Ok(Self::U16),
            ComponentType::UnsignedInt => Ok(Self::U32),
            _ => Err(Error::UnsupportedIndexType),
        }
    }
}

pub type Material = BTreeMap<MaterialKey, TextureData>;

#[derive(Hash, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum MaterialKey {
    BaseColor,
    MetallicRoughness,
    Normal,
    Emissive,
    Occlusion,
    Other(String),
}

#[derive(Debug)]
pub struct ImageData {
    pub range: Range<usize>,
    pub mime: String,
    pub target_format: TextureFormat,
}

#[derive(Debug, Default)]
pub struct TextureData {
    pub image_id: Option<usize>,
    pub factor: [f32; 4],
    pub tex_coord: usize,
    pub sampler: usize,
    //TODO scale,etc
}

#[derive(Debug, Clone)]
struct SuperTextureInfo<E: goth_gltf::Extensions> {
    pub index: usize,
    pub tex_coord: usize,
    pub scale: Option<f32>,
    pub strength: Option<f32>,
    pub extensions: E::TextureInfoExtensions,
}

impl<E: goth_gltf::Extensions> From<&TextureInfo<E>> for SuperTextureInfo<E> {
    fn from(value: &TextureInfo<E>) -> Self {
        Self {
            index: value.index,
            tex_coord: value.tex_coord,
            scale: None,
            strength: None,
            extensions: value.extensions.clone(),
        }
    }
}

impl<E: goth_gltf::Extensions> From<&NormalTextureInfo<E>> for SuperTextureInfo<E> {
    fn from(value: &NormalTextureInfo<E>) -> Self {
        Self {
            index: value.index,
            tex_coord: value.tex_coord,
            scale: Some(value.scale),
            strength: None,
            extensions: value.extensions.clone(),
        }
    }
}

impl<E: goth_gltf::Extensions> From<&OcclusionTextureInfo<E>> for SuperTextureInfo<E> {
    fn from(value: &OcclusionTextureInfo<E>) -> Self {
        Self {
            index: value.index,
            tex_coord: value.tex_coord,
            scale: None,
            strength: Some(value.strength),
            extensions: value.extensions.clone(),
        }
    }
}

struct ImageLoader<'a, E: goth_gltf::Extensions, P: AsRef<Path>> {
    gltf_info: &'a Gltf<E>,
    path: &'a P,
    buffer_map: &'a BTreeMap<usize, &'a [u8]>,
    buffer_out: &'a mut Vec<u8>,
    image_out: &'a mut Vec<ImageData>,
    // tex_range_map: BTreeMap<usize, Range<usize>>,
}
impl<'a, E: goth_gltf::Extensions, P: AsRef<Path>> ImageLoader<'a, E, P> {
    fn new(
        gltf_info: &'a Gltf<E>,
        path: &'a P,
        buffer_map: &'a BTreeMap<usize, &'a [u8]>,
        buffer_out: &'a mut Vec<u8>,
        image_out: &'a mut Vec<ImageData>,
    ) -> Self {
        Self {
            gltf_info,
            path,
            buffer_map,
            buffer_out,
            image_out,
        }
    }

    fn load_texture(
        &mut self,
        texture_info: &Option<SuperTextureInfo<E>>,
        color_factor: [f32; 4],
        key: MaterialKey,
        mat_out: &mut Material,
    ) -> Result<(), Error> {
        let tex_data = if let Some(texture_info) = texture_info {
            let texture = &self.gltf_info.textures[texture_info.index];
            //HACK insert image target format
            if key == MaterialKey::Normal {
                if let Some(id) = texture.source {
                    let img = &mut self.image_out[id];
                    if img.mime != "ktx" {
                        img.target_format = wgpu::TextureFormat::Rgba8Unorm;
                    }
                }
            }

            TextureData {
                image_id: texture.source,
                factor: color_factor,
                tex_coord: texture_info.tex_coord,
                sampler: texture.sampler.unwrap_or_default(),
            }
        } else {
            TextureData {
                factor: color_factor,
                ..Default::default()
            }
        };
        mat_out.insert(key, tex_data);
        Ok(())
    }

    fn prepare_images(&mut self) -> Result<(), Error> {
        for image in &self.gltf_info.images {
            let range = if let Some(ref uri) = image.uri {
                let data = read_uri_data(uri, self.path)?;
                let start = self.buffer_out.len();
                self.buffer_out.extend(data);
                start..self.buffer_out.len()
            } else if let Some(view) = image.buffer_view {
                let view = &self.gltf_info.buffer_views[view];
                get_raw_data_via_buffer_view(0, view, self.buffer_map, None, None, self.buffer_out)?
            } else {
                return Err(Error::FailedGetBuffer);
            };
            self.image_out.push(ImageData {
                range,
                mime: image.mime_type.clone().unwrap_or("image/png".to_string()),
                target_format: wgpu::TextureFormat::Rgba8UnormSrgb,
            })
        }
        Ok(())
    }
}

struct PrimitiveBufferReader<'a, E: goth_gltf::Extensions> {
    gltf_info: &'a goth_gltf::Gltf<E>,
    buffer_map: &'a BTreeMap<usize, &'a [u8]>,
}

impl<'a, E: goth_gltf::Extensions> PrimitiveBufferReader<'a, E> {
    fn new(gltf_info: &'a goth_gltf::Gltf<E>, buffer_map: &'a BTreeMap<usize, &'a [u8]>) -> Self {
        Self {
            gltf_info,
            buffer_map,
        }
    }

    fn get_raw_buffer(
        &mut self,
        access_id: usize,
        buffer_out: &mut Vec<u8>,
    ) -> Result<(Range<usize>, usize, usize), Error> {
        let accessor = self
            .gltf_info
            .accessors
            .get(access_id)
            .context(FailedGetBufferSnafu)?;

        let buffer_view_id = accessor.buffer_view.context(FailedGetBufferSnafu)?;
        let buffer_view = &self.gltf_info.buffer_views[buffer_view_id];
        let range: Range<usize> =
            load_buffer_view_raw_data(accessor, buffer_view, self.buffer_map, buffer_out)?;

        Ok((
            range,
            accessor.count,
            accessor.component_type.byte_size() * accessor.accessor_type.num_components(),
        ))
    }
}

#[derive(Debug, Default)]
pub struct LoadOption {
    pub gen_tbn: bool,
}

pub fn load_gltf<P: AsRef<Path>>(
    path: P,
    option: LoadOption,
) -> Result<(SceneView, GLTFBuffer), Error> {
    let gltf_bytes = std::fs::read(&path).context(FileReadFailedSnafu {
        path: path.as_ref().to_string_lossy(),
    })?;
    let (gltf_info, embedded_buffer) =
        Gltf::<default_extensions::Extensions>::from_bytes(&gltf_bytes)
            .context(JsonDeSerFailedSnafu)?;
    //Prepare buffer data
    let mut buffer_map: BTreeMap<usize, &[u8]> = new_buffer_map_with_embedded(embedded_buffer);
    let mut buffer_vec: Vec<Vec<u8>> = vec![];
    load_model_buffers(&gltf_info, &mut buffer_vec, &path)?;
    insert_external_buffers(&buffer_vec, &mut buffer_map);

    let mut gltf_buffer_out = GLTFBuffer::default();

    let mut scene_view_out = SceneView::default();
    let scene = gltf_info.scenes.get(0).context(DefaultSceneNotFoundSnafu)?;
    for node_id in &scene.nodes {
        insert_node(
            &gltf_info,
            node_id,
            &buffer_map,
            &mut scene_view_out,
            &mut gltf_buffer_out,
            &Node::default(),
        )?
    }

    let mut image_loader = ImageLoader::new(
        &gltf_info,
        &path,
        &buffer_map,
        &mut gltf_buffer_out.shared_data,
        &mut scene_view_out.images,
    );
    image_loader.prepare_images()?;

    for mat in &gltf_info.materials {
        let mut mat_out = Material::new();
        let pbr = &mat.pbr_metallic_roughness;
        image_loader.load_texture(
            &pbr.base_color_texture.as_ref().map(Into::into),
            pbr.base_color_factor,
            MaterialKey::BaseColor,
            &mut mat_out,
        )?;
        image_loader.load_texture(
            &pbr.metallic_roughness_texture.as_ref().map(Into::into),
            [0., pbr.roughness_factor, pbr.metallic_factor, 0.],
            MaterialKey::MetallicRoughness,
            &mut mat_out,
        )?;

        if mat.normal_texture.is_some() {
            image_loader.load_texture(
                &mat.normal_texture.as_ref().map(Into::into),
                Default::default(),
                MaterialKey::Normal,
                &mut mat_out,
            )?;
        }
        if mat.occlusion_texture.is_some() {
            image_loader.load_texture(
                &mat.occlusion_texture.as_ref().map(Into::into),
                Default::default(),
                MaterialKey::Occlusion,
                &mut mat_out,
            )?;
        }
        //TODO emissive

        scene_view_out.materials.push(mat_out)
    }

    for sampler in &gltf_info.samplers {
        scene_view_out.samplers.push(sampler.clone());
    }

    if option.gen_tbn {
        for node in scene_view_out.nodes.iter_mut() {
            for mesh in node.meshes.iter_mut() {
                mesh.gen_tbn(&mut gltf_buffer_out);
            }
        }
    }

    Ok((scene_view_out, gltf_buffer_out))
}

impl Mesh {
    fn gen_tbn(&mut self, buffer: &mut GLTFBuffer) {
        let has_tangent = self.tangents.is_some();
        let indices: Vec<[u32; 1]> = check_and_cast(&buffer.index, &self.index.indices);
        let pos: Vec<[f32; 3]> = check_and_cast(&buffer.positions, &self.positions);
        let uv: Vec<[f32; 2]> = check_and_cast(&buffer.texcoord[0], self.uv0.as_ref().unwrap());

        let mut bi_tangent_buffer = vec![];
        let mut tangent_buffer = vec![];
        for c in indices.chunks(3) {
            let i0 = c[0][0] as usize;
            let i1 = c[1][0] as usize;
            let i2 = c[2][0] as usize;

            let pos0 = glam::Vec3::from_array(pos[i0]);
            let pos1 = glam::Vec3::from_array(pos[i1]);
            let pos2 = glam::Vec3::from_array(pos[i2]);

            let uv0 = glam::Vec2::from_array(uv[i0]);
            let uv1 = glam::Vec2::from_array(uv[i1]);
            let uv2 = glam::Vec2::from_array(uv[i2]);

            let delta_pos1 = pos1 - pos0;
            let delta_pos2 = pos2 - pos0;

            // This will give us a direction to calculate the
            // tangent and bitangent
            let delta_uv1 = uv1 - uv0;
            let delta_uv2 = uv2 - uv0;

            // Solving the following system of equations will
            // give us the tangent and bitangent.
            //     delta_pos1 = delta_uv1.x * T + delta_u.y * B
            //     delta_pos2 = delta_uv2.x * T + delta_uv2.y * B
            // Luckily, the place I found this equation provided
            // the solution!
            let r = 1.0 / (delta_uv1.x * delta_uv2.y - delta_uv1.y * delta_uv2.x);
            if !has_tangent {
                let tangent = (delta_pos1 * delta_uv2.y - delta_pos2 * delta_uv1.y) * r;
                tangent_buffer.push(tangent.extend(1.0).to_array());
            }
            // We flip the bitangent to enable right-handed normal
            // maps with wgpu texture coordinate system
            let bitangent = (delta_pos2 * delta_uv1.x - delta_pos1 * delta_uv2.x) * -r;

            bi_tangent_buffer.push(bitangent.to_array());
        }
        let bi_tangent_buf = &mut buffer.bi_tangent;
        let start = bi_tangent_buf.len();
        bi_tangent_buf.extend(bytemuck::cast_slice(&bi_tangent_buffer));
        self.bi_tangents = Some(start..bi_tangent_buf.len());

        if !has_tangent {
            let tangent_buf = &mut buffer.tangent;
            let start = tangent_buf.len();
            tangent_buf.extend(bytemuck::cast_slice(&tangent_buffer));
            self.tangents = Some(start..tangent_buf.len());
        }
    }
}

fn load_model_buffers<P: AsRef<Path>>(
    gltf_info: &Gltf<default_extensions::Extensions>,
    buffer_vec: &mut Vec<Vec<u8>>,
    path: P,
) -> Result<(), Error> {
    for (index, buffer) in gltf_info.buffers.iter().enumerate() {
        if buffer
            .extensions
            .ext_meshopt_compression
            .as_ref()
            .map(|ext| ext.fallback)
            .unwrap_or(false)
        {
            continue;
        }

        match &buffer.uri {
            Some(uri) => {
                buffer_vec.insert(index, read_uri_data(uri, &path)?);
            }
            None => continue,
        };
    }
    Ok(())
}

fn load_buffer_view_raw_data<E: goth_gltf::Extensions>(
    accessor: &goth_gltf::Accessor,
    buffer_view: &goth_gltf::BufferView<E>,
    buffer_map: &BTreeMap<usize, &[u8]>,
    buffer_out: &mut Vec<u8>,
) -> Result<Range<usize>, Error> {
    let offset = accessor.byte_offset;
    let type_size = accessor.component_type.byte_size() * accessor.accessor_type.num_components();

    let length = accessor.byte_length(buffer_view);

    get_raw_data_via_buffer_view(
        offset,
        buffer_view,
        buffer_map,
        Some(length),
        Some(type_size),
        buffer_out,
    )
}

fn get_raw_data_via_buffer_view<E: goth_gltf::Extensions>(
    mut offset: usize,
    buffer_view: &goth_gltf::BufferView<E>,
    buffer_map: &BTreeMap<usize, &[u8]>,
    possible_length: Option<usize>,
    type_size: Option<usize>,
    buffer_out: &mut Vec<u8>,
) -> Result<Range<usize>, Error> {
    offset += buffer_view.byte_offset;
    let stride = buffer_view.byte_stride;
    let buffer_id = buffer_view.buffer;
    // load buffer data
    let buffer = buffer_map.get(&buffer_id).context(FailedGetBufferSnafu)?;
    let length = possible_length
        .unwrap_or(buffer_view.byte_length)
        .min(buffer_view.byte_length);
    let buffer = &buffer[offset..offset + length];

    let buffer = if let Some(stride) = stride {
        let type_size = type_size.context(FailedGetBufferSnafu)?;
        buffer
            .chunks(stride)
            .map(|i| &i[0..type_size])
            .flat_map(|i| i.iter())
            .copied()
            .collect()
    } else {
        buffer.to_vec()
    };
    let buffer_start = buffer_out.len();
    buffer_out.extend(buffer);

    Ok(buffer_start..buffer_out.len())
}

fn new_buffer_map_with_embedded(buffer: Option<&[u8]>) -> BTreeMap<usize, &[u8]> {
    let mut buffer_map: BTreeMap<usize, &[u8]> = BTreeMap::new();
    if let Some(buffer) = buffer {
        buffer_map.insert(0_usize, buffer);
    }
    buffer_map
}

fn insert_external_buffers<'a>(
    buffer_vec: &'a [Vec<u8>],
    buffer_map: &mut BTreeMap<usize, &'a [u8]>,
) {
    for i in buffer_vec.iter().enumerate() {
        buffer_map.insert(i.0, i.1);
    }
}

fn node_transform_to_matrix(n_transform: &NodeTransform) -> Mat4 {
    match n_transform {
        NodeTransform::Matrix(m) => Mat4::from_cols_array(m),
        NodeTransform::Set {
            translation,
            rotation,
            scale,
        } => {
            let scale = Mat4::from_scale(Vec3::from_slice(scale));
            let rot = Quat::from_array(*rotation);
            let m = Mat4::from_quat(rot) * scale;
            let trans = Mat4::from_translation(Vec3::from_slice(translation));
            trans * m
        }
    }
}

fn read_uri_data(uri: &str, path: impl AsRef<Path>) -> Result<Vec<u8>, Error> {
    if uri.starts_with("data") {
        let (_mime_type, data) = uri.split_once(',').context(Base64MIMENotFoundSnafu)?;
        log::warn!("Loading buffers from embedded base64 is inefficient. Consider moving the buffers into a seperate file.");
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .context(Base64DecodeFailedSnafu)
    } else {
        let mut path = std::path::PathBuf::from(path.as_ref());
        path.set_file_name(urlencoding::decode(uri).context(UTF8ErrSnafu)?.into_owned());
        std::fs::read(&path).context(FileReadFailedSnafu {
            path: path.to_string_lossy(),
        })
    }
}

fn insert_node(
    gltf_info: &Gltf<default_extensions::Extensions>,
    node_id: &usize,
    buffer_map: &BTreeMap<usize, &[u8]>,
    scene_view_out: &mut SceneView,
    gltf_buffer_out: &mut GLTFBuffer,
    parent: &Node,
) -> Result<(), Error> {
    let node: &goth_gltf::Node<default_extensions::Extensions> = &gltf_info.nodes[*node_id];
    let mesh_id = match node.mesh {
        Some(id) => id,
        //TODO error?
        None => return Ok(()),
    };
    let transform = node_transform_to_matrix(&node.transform());
    let mesh = &gltf_info.meshes[mesh_id];

    let mut meshes_out = Vec::new();
    for primitive in &mesh.primitives {
        let mut primitive_reader = PrimitiveBufferReader::new(gltf_info, buffer_map);

        let index_accessor =
            &gltf_info.accessors[primitive.indices.context(NoIndexFoundSnafu { mesh_id })?];

        let raw_index_buffer = match index_accessor.component_type {
            ComponentType::UnsignedInt => primitive_reader.get_raw_buffer(
                primitive.indices.context(NoIndexFoundSnafu { mesh_id })?,
                &mut gltf_buffer_out.index,
            )?,
            ComponentType::UnsignedShort => {
                let mut output = vec![];
                primitive_reader.get_raw_buffer(
                    primitive.indices.context(IndexTypeSnafu { mesh_id })?,
                    &mut output,
                )?;
                let index: Vec<[u16; 1]> = check_and_cast(&output, &(0..output.len()));
                let new_indices: Vec<u32> = index.iter().map(|i| i[0] as u32).collect();
                let count = new_indices.len();
                let type_size = 4;
                let buffer_start = gltf_buffer_out.index.len();
                gltf_buffer_out
                    .index
                    .extend(bytemuck::cast_slice(&new_indices));
                (buffer_start..gltf_buffer_out.index.len(), count, type_size)
            }
            _ => return Err(Error::NoIndexFound { mesh_id }),
        };

        let index = Index {
            indices: raw_index_buffer.0,
            count: raw_index_buffer.1,
            type_size: raw_index_buffer.2,
        };
        let positions = primitive_reader.get_raw_buffer(
            primitive
                .attributes
                .position
                .context(NoPositionFoundSnafu { mesh_id })?,
            &mut gltf_buffer_out.positions,
        )?;
        let mesh_out = Mesh {
            id: mesh_id,
            vertex_count: positions.1,
            vertex_type_size: positions.2,
            positions: positions.0,
            normals: primitive
                .attributes
                .normal
                .and_then(|normal| {
                    primitive_reader
                        .get_raw_buffer(normal, &mut gltf_buffer_out.normal)
                        .ok()
                })
                .map(|i| i.0),
            uv0: primitive
                .attributes
                .texcoord_0
                .and_then(|texcoord| {
                    gltf_buffer_out.texcoord.resize(1, Default::default());
                    let tex_buffer = gltf_buffer_out.texcoord.get_mut(0)?;
                    primitive_reader.get_raw_buffer(texcoord, tex_buffer).ok()
                })
                .map(|i| i.0),
            tangents: primitive
                .attributes
                .tangent
                .and_then(|tangent| {
                    primitive_reader
                        .get_raw_buffer(tangent, &mut gltf_buffer_out.tangent)
                        .ok()
                })
                .map(|i| i.0),
            index,
            mode: primitive.mode,
            mat: primitive.material,
            bi_tangents: None,
        };

        meshes_out.push(mesh_out);
    }

    let transform = transform * parent.per_node_info.transform;
    let node_out = Node {
        id: *node_id,
        per_node_info: PerNodeBuffer { transform },
        meshes: meshes_out,
        ..Default::default()
    };

    for children in &node.children {
        insert_node(
            gltf_info,
            children,
            buffer_map,
            scene_view_out,
            gltf_buffer_out,
            &node_out,
        )?;
    }

    scene_view_out.nodes.insert(*node_id, node_out);

    Ok(())
}

pub fn check_and_cast<T: Copy + bytemuck::Pod, const N: usize>(
    scene_buffer: &[u8],
    range: &Range<usize>,
) -> Vec<[T; N]> {
    bytemuck::cast_slice(&scene_buffer[range.clone()])
        .chunks(N)
        .map(|slice| <[T; N]>::try_from(slice).unwrap())
        .collect()
}
