use std::ops::Range;
use std::{collections::BTreeMap, path::Path};

use base64::{DecodeError, Engine};
use glam::{Mat4, Quat, Vec3};
use goth_gltf::{default_extensions, ComponentType, NormalTextureInfo, OcclusionTextureInfo};
use goth_gltf::{Gltf, NodeTransform, Primitive, PrimitiveMode, TextureInfo};
use snafu::{Backtrace, OptionExt, ResultExt, Snafu};

use crate::asset::read_u32;

use super::{read_f32x2, read_f32x3};

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
    NoPositionFound {
        mesh_id: usize,
        backtrace: Backtrace,
    },
    NoIndexFound {
        mesh_id: usize,
    },

    FailedToGetU8Data,
}

#[derive(Debug, Default)]
pub struct SceneView {
    pub nodes: Vec<Node>,
    pub materials: Vec<Material>,
    //TODO
    pub samplers: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Node {
    pub name: Option<String>,
    pub meshes: Vec<TheMesh>,
    pub transform: [f32; 16],
}

#[derive(Debug, Default)]
pub struct TheMesh {
    pub positions: Range<usize>,
    pub normals: Option<Range<usize>>,
    pub uv0: Option<Range<usize>>,
    pub tangents: Option<Range<usize>>,
    pub index: Index,
    pub mode: PrimitiveMode,
    pub mat: Option<usize>,
}

#[derive(Debug, Default)]
pub struct Index {
    pub indices: Range<usize>,
    pub r#type: IndexType,
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

#[derive(Debug, Default)]
pub struct TextureData {
    pub data_range: Option<Range<usize>>,
    pub factor: [f32; 4],
    pub mime: String,
    pub tex_coord: usize,
    pub sampler: usize,
}

pub fn new_load_gltf<P: AsRef<Path>>(path: P) -> Result<(SceneView, Vec<u8>), Error> {
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

    let mut buffer_out = Vec::new();

    let mut scene_view_out = SceneView::default();
    let scene = gltf_info.scenes.get(0).context(DefaultSceneNotFoundSnafu)?;
    for node_id in &scene.nodes {
        let node: &goth_gltf::Node<default_extensions::Extensions> = &gltf_info.nodes[*node_id];
        //TODO node children
        let mesh_id = match node.mesh {
            Some(id) => id,
            None => continue,
        };
        let transform = node_transform_to_matrix(&node.transform()).to_cols_array();
        let mesh = &gltf_info.meshes[mesh_id];

        let mut meshes_out = Vec::new();
        for primitive in &mesh.primitives {
            let mut primitive_reader =
                PrimitiveBufferReader::new(&gltf_info, &mut buffer_out, &buffer_map);

            let index_accessor =
                &gltf_info.accessors[primitive.indices.context(NoIndexFoundSnafu { mesh_id })?];
            let index = Index {
                indices: primitive_reader
                    .get_raw_buffer(primitive.indices.context(NoIndexFoundSnafu { mesh_id })?)?,
                r#type: index_accessor.component_type.try_into()?,
            };
            let mesh_out = TheMesh {
                positions: primitive_reader.get_raw_buffer(
                    primitive
                        .attributes
                        .position
                        .context(NoPositionFoundSnafu { mesh_id })?,
                )?,
                normals: primitive
                    .attributes
                    .normal
                    .and_then(|normal| primitive_reader.get_raw_buffer(normal).ok()),
                uv0: primitive
                    .attributes
                    .texcoord_0
                    .and_then(|texcoord| primitive_reader.get_raw_buffer(texcoord).ok()),
                tangents: primitive
                    .attributes
                    .tangent
                    .and_then(|tangent| primitive_reader.get_raw_buffer(tangent).ok()),
                index,
                mode: primitive.mode,
                mat: primitive.material,
            };

            meshes_out.push(mesh_out);
        }

        let node_out = Node {
            transform,
            meshes: meshes_out,
            ..Default::default()
        };
        scene_view_out.nodes.push(node_out);
    }

    let mut image_loader = ImageLoader::new(&gltf_info, &path, &buffer_map, &mut buffer_out);
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

    Ok((scene_view_out, buffer_out))
}

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
    mat_range_map: BTreeMap<usize, Range<usize>>,
}
impl<'a, E: goth_gltf::Extensions, P: AsRef<Path>> ImageLoader<'a, E, P> {
    fn new(
        gltf_info: &'a Gltf<E>,
        path: &'a P,
        buffer_map: &'a BTreeMap<usize, &'a [u8]>,
        buffer_out: &'a mut Vec<u8>,
    ) -> Self {
        Self {
            gltf_info,
            path,
            buffer_map,
            buffer_out,
            mat_range_map: BTreeMap::new(),
        }
    }

    fn load_texture(
        &self,
        texture_info: &Option<SuperTextureInfo<E>>,
        color_factor: [f32; 4],
        key: MaterialKey,
        mat_out: &mut Material,
    ) -> Result<(), Error> {
        let tex_data = if let Some(texture_info) = texture_info {
            let texture = &self.gltf_info.textures[texture_info.index];
            let image_id = texture.source.unwrap();
            let image = &self.gltf_info.images[image_id];
            let data = self
                .mat_range_map
                .get(&image_id)
                .context(FailedGetBufferSnafu)?;

            let base_color_tex_data = TextureData {
                data_range: Some(data.clone()),
                factor: color_factor,
                mime: image.mime_type.clone().unwrap_or("image/png".to_string()),
                tex_coord: texture_info.tex_coord,
                sampler: texture.sampler.unwrap(),
            };
            Ok(base_color_tex_data)
        } else {
            Ok(TextureData {
                data_range: None,
                factor: color_factor,
                ..Default::default()
            })
        };
        mat_out.insert(key, tex_data?);
        Ok(())
    }

    fn prepare_images(&mut self) -> Result<(), Error> {
        for (i, image) in self.gltf_info.images.iter().enumerate() {
            let data = if let Some(ref uri) = image.uri {
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
            self.mat_range_map.insert(i, data);
        }
        Ok(())
    }
}

struct PrimitiveBufferReader<'a, E: goth_gltf::Extensions> {
    gltf_info: &'a goth_gltf::Gltf<E>,
    buffer_out: &'a mut Vec<u8>,
    buffer_map: &'a BTreeMap<usize, &'a [u8]>,
}

impl<'a, E: goth_gltf::Extensions> PrimitiveBufferReader<'a, E> {
    fn new(
        gltf_info: &'a goth_gltf::Gltf<E>,
        buffer_out: &'a mut Vec<u8>,
        buffer_map: &'a BTreeMap<usize, &'a [u8]>,
    ) -> Self {
        Self {
            gltf_info,
            buffer_out,
            buffer_map,
        }
    }

    fn get_raw_buffer(&mut self, access_id: usize) -> Result<Range<usize>, Error> {
        let accessor = self
            .gltf_info
            .accessors
            .get(access_id)
            .context(FailedGetBufferSnafu)?;

        let buffer_view_id = accessor.buffer_view.context(FailedGetBufferSnafu)?;
        let range = load_buffer_view_raw_data(
            self.gltf_info,
            accessor,
            buffer_view_id,
            self.buffer_map,
            self.buffer_out,
        )?;

        Ok(range)
    }
}

fn load_buffer_view_raw_data<'a, E: goth_gltf::Extensions>(
    gltf_info: &'a goth_gltf::Gltf<E>,
    accessor: &goth_gltf::Accessor,
    buffer_view_id: usize,
    buffer_map: &BTreeMap<usize, &'a [u8]>,
    buffer_out: &'a mut Vec<u8>,
) -> Result<Range<usize>, Error> {
    let offset = accessor.byte_offset;
    let type_size = accessor.component_type.byte_size();

    let buffer_view = &gltf_info.buffer_views[buffer_view_id];
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
    let buffer = buffer_map.get(&buffer_id).ok_or(Error::FailedGetBuffer)?;
    let length = possible_length
        .unwrap_or(buffer_view.byte_length)
        .min(buffer_view.byte_length);
    let buffer = &buffer[offset..offset + length];

    let buffer = if let Some(stride) = stride {
        buffer
            .iter()
            .enumerate()
            .filter(|(index, _i)| {
                if let Some(type_size) = type_size {
                    (index % stride) < type_size
                } else {
                    true
                }
            })
            .map(|i| *i.1)
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
            let rot = Quat::from_array(*rotation);
            let m = Mat4::from_translation(Vec3::from_slice(translation));
            let m = Mat4::from_quat(rot) * m;
            Mat4::from_scale(Vec3::from_slice(scale)) * m
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
        path.set_file_name(uri);
        std::fs::read(&path).context(FileReadFailedSnafu {
            path: path.to_string_lossy(),
        })
    }
}

//TODO deal with multiple node
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub uv0: Vec<[f32; 2]>,
    pub tangents: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

pub fn load_gltf<P: AsRef<Path>>(path: P) -> std::result::Result<Mesh, Error> {
    let gltf_bytes = std::fs::read(&path).context(FileReadFailedSnafu {
        path: path.as_ref().to_string_lossy(),
    })?;
    let (gltf_info, embedded_buffer) =
        Gltf::<default_extensions::Extensions>::from_bytes(&gltf_bytes)
            .context(JsonDeSerFailedSnafu)?;
    let mut buffer_map = new_buffer_map_with_embedded(embedded_buffer);
    //Load external data;
    let mut buffer_vec: Vec<Vec<u8>> = vec![];
    load_model_buffers(&gltf_info, &mut buffer_vec, path)?;
    insert_external_buffers(&buffer_vec, &mut buffer_map);

    let mut positions = vec![];
    let mut indices = vec![];
    let mut uv0 = vec![];

    let scene = gltf_info.scenes.get(0).context(DefaultSceneNotFoundSnafu)?;
    for node_id in &scene.nodes {
        let node: &goth_gltf::Node<default_extensions::Extensions> = &gltf_info.nodes[*node_id];
        let mesh_id = match node.mesh {
            Some(id) => id,
            None => continue,
        };
        let transform = node_transform_to_matrix(&node.transform());
        let mesh = &gltf_info.meshes[mesh_id];
        for primitive in &mesh.primitives {
            let pos_count = positions.len() as u32;

            let primitive_reader = PrimitiveReader::new(&gltf_info, &buffer_map, primitive);
            let position = primitive_reader
                .get_positions()
                .ok_or(Error::FailedToGetU8Data)?;

            //TODO deal position in shader
            for pos in position {
                let n_pos = transform * Vec3::from_slice(&pos).extend(1.0);
                positions.push(n_pos.truncate().to_array());
            }

            uv0.extend(primitive_reader.get_uv0().ok_or(Error::FailedToGetU8Data)?);

            let mut index: Vec<u32> = primitive_reader
                .get_index()
                .ok_or(Error::FailedToGetU8Data)?;
            index.iter_mut().for_each(|i: &mut u32| *i += pos_count);

            indices.extend(index);

            if let Some(mat_id) = primitive_reader.get_material_id() {
                let material = &gltf_info.materials[mat_id];
                let base_color_tex_info = &material
                    .pbr_metallic_roughness
                    .base_color_texture
                    .as_ref()
                    .unwrap();
                let base_color_tex_id = base_color_tex_info.index;
                let base_color_tex = &gltf_info.textures[base_color_tex_id];
                let base_color_img_id = base_color_tex.source.unwrap();
                let base_color_img = &gltf_info.images[base_color_img_id];
            }
        }
    }

    log::info!(
        "positions: {}, uv0:{}, indices:{}",
        positions.len(),
        uv0.len(),
        indices.len()
    );

    Ok(Mesh {
        positions,
        normals: vec![],
        colors: vec![],
        uv0,
        tangents: vec![],
        indices,
    })
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

struct PrimitiveReader<'a, E: goth_gltf::Extensions> {
    gltf_info: &'a goth_gltf::Gltf<E>,
    buffer_map: &'a BTreeMap<usize, &'a [u8]>,
    primitive: &'a Primitive,
}

impl<'a, E: goth_gltf::Extensions> PrimitiveReader<'a, E> {
    fn new(
        gltf_info: &'a goth_gltf::Gltf<E>,
        buffer_map: &'a BTreeMap<usize, &'a [u8]>,
        primitive: &'a Primitive,
    ) -> Self {
        Self {
            gltf_info,
            buffer_map,
            primitive,
        }
    }

    fn get_buffer_data_by_index(
        &self,
        id: usize,
    ) -> Option<(&goth_gltf::Accessor, Option<usize>, &[u8])> {
        let accessor = &self.gltf_info.accessors[id];
        let buffer_view_id = accessor.buffer_view?;
        let mut offset = accessor.byte_offset;

        let buffer_view = &self.gltf_info.buffer_views[buffer_view_id];
        offset += buffer_view.byte_offset;
        let length = accessor
            .byte_length(buffer_view)
            .min(buffer_view.byte_length);
        let stride = buffer_view.byte_stride;
        let buffer_id = buffer_view.buffer;
        let buffer = *self.buffer_map.get(&buffer_id)?;
        let out_buffer = &buffer[offset..offset + length];
        Some((accessor, stride, out_buffer))
    }

    fn get_positions(&self) -> Option<Vec<[f32; 3]>> {
        let id = self.primitive.attributes.position?;
        let (accessor, stride, slice) = self.get_buffer_data_by_index(id)?;
        read_f32x3(slice, stride, accessor)
    }

    fn get_index(&self) -> Option<Vec<u32>> {
        let id = self.primitive.indices?;
        let (accessor, stride, slice) = self.get_buffer_data_by_index(id)?;
        read_u32(slice, stride, accessor)
    }

    fn get_uv0(&self) -> Option<Vec<[f32; 2]>> {
        let id = self.primitive.attributes.texcoord_0?;
        let (accessor, stride, slice) = self.get_buffer_data_by_index(id)?;
        read_f32x2(slice, stride, accessor)
    }

    fn get_material_id(&self) -> Option<usize> {
        self.primitive.material
    }
}
