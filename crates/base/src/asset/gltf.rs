use std::{collections::BTreeMap, path::Path};

use base64::{DecodeError, Engine};
use glam::{Mat4, Quat, Vec3};
use goth_gltf::default_extensions::{self, Extensions};
use goth_gltf::{Gltf, NodeTransform, Primitive};
use snafu::{OptionExt, ResultExt, Snafu};

use crate::{asset::read_u32, Mesh};

use super::read_f32x3;

#[derive(Debug, Snafu)]
pub enum Error {
    JsonDeSerFailed { source: nanoserde::DeJsonErr },
    DefaultSceneNotFound,
    Base64MIMENotFound,
    Base64DecodeFailed { source: DecodeError },
    FileReadFailed { source: std::io::Error },
    FailedToGetU8Data
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
        let position_id = self.primitive.attributes.position?;
        let (accessor, stride, buffer) = self.get_buffer_data_by_index(position_id)?;
        read_f32x3(buffer, stride, accessor)
    }

    fn get_index(&self) -> Option<Vec<u32>> {
        let index_id = self.primitive.indices?;
        let (accessor, stride, slice) = self.get_buffer_data_by_index(index_id)?;
        read_u32(slice, stride, accessor)
    }
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

fn read_buffer(uri: &str, path: impl AsRef<Path>) -> Result<Vec<u8>, Error> {
    if uri.starts_with("data") {
        let (_mime_type, data) = uri.split_once(',').context(Base64MIMENotFoundSnafu)?;
        log::warn!("Loading buffers from embedded base64 is inefficient. Consider moving the buffers into a seperate file.");
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .context(Base64DecodeFailedSnafu)
    } else {
        let mut path = std::path::PathBuf::from(path.as_ref());
        path.set_file_name(uri);
        std::fs::read(&path).context(FileReadFailedSnafu)
    }
}

fn load_model_buffers<P: AsRef<Path>>(
    gltf_info: &Gltf<Extensions>,
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
                buffer_vec.insert(index, read_buffer(uri, &path)?);
            }
            None => continue,
        };
    }
    Ok(())
}

pub fn load_gltf<P: AsRef<Path>>(path: P) -> std::result::Result<Mesh, Error> {
    let gltf_bytes = std::fs::read(&path).context(FileReadFailedSnafu)?;
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

    let scene = gltf_info.scenes.get(0).context(DefaultSceneNotFoundSnafu)?;
    for node_id in &scene.nodes {
        let node = &gltf_info.nodes[*node_id];
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

            let mut index = primitive_reader.get_index().ok_or(Error::FailedToGetU8Data)?;
            index.iter_mut().for_each(|i: &mut u32| *i += pos_count);

            indices.extend(index);
        }
    }

    Ok(Mesh {
        positions,
        normals: vec![],
        colors: vec![],
        uvs: vec![],
        tangents: vec![],
        indices,
    })
}
