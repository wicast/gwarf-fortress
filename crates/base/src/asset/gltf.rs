use std::{collections::BTreeMap, path::Path};

use cgmath::{Rad, Vector3, Matrix4, Vector4};
use goth_gltf::{default_extensions, Gltf, NodeTransform, Primitive};
use snafu::Snafu;

use crate::{asset::read_u32, Mesh};

use super::read_f32x3;

#[derive(Debug, Snafu)]
pub enum Error {
    FileNotFound,
    GltfLoadFailed,
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
    // todo!()
}

fn node_transform_to_matrix(n_transform: &NodeTransform) -> Matrix4<f32> {
    match n_transform {
        NodeTransform::Matrix(m) =>{
            let c0: [f32;4] = m[0..3].try_into().unwrap();
            let c1: [f32;4] = m[4..7].try_into().unwrap();
            let c2: [f32;4] = m[8..11].try_into().unwrap();
            let c3: [f32;4] = m[12..15].try_into().unwrap();
             Matrix4::from_cols(c0.into(), c1.into(), c2.into(), c3.into())
            },
        NodeTransform::Set {
            translation,
            rotation,
            scale,
        } => {
            let m = cgmath::Matrix4::from_translation(Vector3 {
                x: translation[0],
                y: translation[1],
                z: translation[2],
            });
            let m = cgmath::Matrix4::from_nonuniform_scale(scale[0], scale[1], scale[2]) * m;
            let m = cgmath::Matrix4::from_angle_x(Rad(rotation[0])) * m;
            let m = cgmath::Matrix4::from_angle_x(Rad(rotation[1])) * m;
            cgmath::Matrix4::from_angle_x(Rad(rotation[2])) * m
        }
    }
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
        let transform = node_transform_to_matrix(&node.transform());
        let mesh_id = &node.mesh.ok_or(Error::GltfLoadFailed)?;
        let mesh = &gltf_info.meshes[*mesh_id];
        for primitive in &mesh.primitives {
            let pos_count = positions.len() as u32;

            let primitive_reader = PrimitiveReader::new(&gltf_info, &buffer_map, primitive);
            let position = primitive_reader
                .get_positions()
                .ok_or(Error::GltfLoadFailed)?;

            //TODO deal position in shader
            for pos in position {
                let n_pos: Vector4<f32> = transform * Vector4 { x: pos[0], y: pos[1], z: pos[2], w: 1.0 };
                positions.push([n_pos[0],n_pos[1],n_pos[2]]);
            }

            let mut index = primitive_reader.get_index().ok_or(Error::GltfLoadFailed)?;
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
        transform: vec![],
    })
}
