use std::{collections::BTreeMap, path::Path};

use goth_gltf::{default_extensions, Gltf, Primitive};
use snafu::Snafu;

use crate::{asset::read_u32, Mesh};

use super::read_f32x3;

#[derive(Debug, Snafu)]
pub enum Error {
    FileNotFound,
    GltfLoadFailed,
}

enum Buffer<'a> {
    Embedded(&'a [u8]),
    Uri(&'a goth_gltf::Buffer<default_extensions::Extensions>),
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

    fn get_buffer_inner(&self, id: usize) -> Option<(&goth_gltf::Accessor, Option<usize>, &[u8])> {
        let accessor = &self.gltf_info.accessors[id];
        let buffer_view_id = accessor.buffer_view?;
        let mut offset = accessor.byte_offset;

        let buffer_view = &self.gltf_info.buffer_views[buffer_view_id];
        offset += buffer_view.byte_offset;
        let length = buffer_view.byte_length;
        let stride = buffer_view.byte_stride;
        let buffer_id = buffer_view.buffer;
        let buffer = *self.buffer_map.get(&buffer_id)?;
        let out_buffer = &buffer[offset..offset + length];
        Some((accessor, stride, out_buffer))
    }

    fn get_positions(&self) -> Option<Vec<[f32; 3]>> {
        let position_id = self.primitive.attributes.position?;
        let (accessor, stride, buffer) = self.get_buffer_inner(position_id)?;
        read_f32x3(buffer, stride, accessor)
    }

    fn get_index(&self) -> Option<Vec<u32>> {
        let index_id = self.primitive.indices?;
        let (accessor, stride, slice) = self.get_buffer_inner(index_id)?;
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

fn insert_external_buffers(buffer_map: &mut BTreeMap<usize, &[u8]>, buffer_vec: &Vec<Vec<u8>>) {
    todo!()
}

pub fn load_gltf<P: AsRef<Path>>(path: P) -> std::result::Result<Mesh, Error> {
    let gltf_bytes = std::fs::read(&path).map_err(|_| Error::FileNotFound)?;
    let (gltf_info, embedded_buffer) =
        Gltf::<default_extensions::Extensions>::from_bytes(&gltf_bytes)
            .map_err(|_| Error::GltfLoadFailed)?;
    let mut buffer_map = new_buffer_map_with_embedded(embedded_buffer);
    // Load external data;
    let buffer_vec = vec![];

    insert_external_buffers(&mut buffer_map, &buffer_vec);

    let mut positions = vec![];
    let mut indices = vec![];

    let scene = gltf_info.scenes.get(0).ok_or(Error::GltfLoadFailed)?;
    for node_id in &scene.nodes {
        let node = &gltf_info.nodes[*node_id];
        let mesh_id = &node.mesh.ok_or(Error::GltfLoadFailed)?;
        let mesh = &gltf_info.meshes[*mesh_id];
        for primitive in &mesh.primitives {
            // println!("primitive: {:?}", primitive);
            // Mesh data
            let pos_count = positions.len() as u32;

            let primitive_reader = PrimitiveReader::new(&gltf_info, &buffer_map, primitive);
            let position = primitive_reader
                .get_positions()
                .ok_or(Error::GltfLoadFailed)?;

            let mut index = primitive_reader.get_index().ok_or(Error::GltfLoadFailed)?;
            index.iter_mut().for_each(|i| *i += pos_count);

            positions.extend(position);
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

// fn collect_buffer_view_map(
//     path: &std::path::Path,
//     gltf: &goth_gltf::Gltf<Extensions>,
//     glb_buffer: Option<&[u8]>,
// ) -> anyhow::Result<HashMap<usize, Vec<u8>>> {
//     use std::borrow::Cow;

//     let mut buffer_map = HashMap::new();

//     if let Some(glb_buffer) = glb_buffer {
//         buffer_map.insert(0, Cow::Borrowed(glb_buffer));
//     }

//     for (index, buffer) in gltf.buffers.iter().enumerate() {
//         if buffer
//             .extensions
//             .ext_meshopt_compression
//             .as_ref()
//             .map(|ext| ext.fallback)
//             .unwrap_or(false)
//         {
//             continue;
//         }

//         let uri = match &buffer.uri {
//             Some(uri) => uri,
//             None => continue,
//         };

//         if uri.starts_with("data") {
//             let (_mime_type, data) = uri
//                 .split_once(',')
//                 .ok_or_else(|| anyhow::anyhow!("Failed to get data uri split"))?;
//             log::warn!("Loading buffers from embedded base64 is inefficient. Consider moving the buffers into a seperate file.");
//             buffer_map.insert(
//                 index,
//                 Cow::Owned(base64::engine::general_purpose::STANDARD.decode(data)?),
//             );
//         } else {
//             let mut path = std::path::PathBuf::from(path);
//             path.set_file_name(uri);
//             buffer_map.insert(index, Cow::Owned(std::fs::read(&path).unwrap()));
//         }
//     }

//     let mut buffer_view_map = HashMap::new();

//     for (i, buffer_view) in gltf.buffer_views.iter().enumerate() {
//         if let Some(buffer) = buffer_map.get(&buffer_view.buffer) {
//             buffer_view_map.insert(
//                 i,
//                 buffer[buffer_view.byte_offset..buffer_view.byte_offset + buffer_view.byte_length]
//                     .to_vec(),
//             );
//         }
//     }

//     Ok(buffer_view_map)
// }
