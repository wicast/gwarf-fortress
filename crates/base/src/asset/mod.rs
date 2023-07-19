use goth_gltf::ComponentType;

pub mod gltf;

//TODO use Result
pub fn read_f32x3(
    slice: &[u8],
    byte_stride: Option<usize>,
    accessor: &goth_gltf::Accessor,
) -> Option<Vec<[f32; 3]>> {
    Some(
        match (accessor.component_type, accessor.normalized, byte_stride) {
            (ComponentType::Float, false, None | Some(12)) => {
                //TODO cast failed
                let slice: &[f32] = bytemuck::cast_slice(slice);
                slice
                    .chunks(3)
                    //TODO unwrap
                    .map(|slice| <[f32; 3]>::try_from(slice).unwrap())
                    .collect()
            }
            (ComponentType::Float, false, Some(stride)) => {
                let stride = stride;
                slice
                    .chunks(stride)
                    .map(|slice| {
                        <[f32; 3]>::try_from(bytemuck::cast_slice(&slice[0..12])).unwrap()
                    })
                    .collect()
            }
            (ComponentType::Short, true, Some(stride)) => {
                let slice: &[i16] = bytemuck::cast_slice(slice);
                // Cow::Owned(
                //     slice
                //         .chunks(stride / 2)
                //         .map(|slice| {
                //             Vec3::from(std::array::from_fn(|i| signed_short_to_float(slice[i])))
                //         })
                //         .collect(),
                // )
                todo!()
            }
            (ComponentType::UnsignedShort, false, Some(8)) => {
                let slice: &[u16] = bytemuck::cast_slice(slice);
                // Cow::Owned(
                //     slice
                //         .chunks(4)
                //         .map(move |slice| Vec3::from(std::array::from_fn(|i| slice[i] as f32)))
                //         .collect(),
                // )
                todo!()
            }
            (ComponentType::UnsignedShort, true, Some(8)) => {
                let slice: &[u16] = bytemuck::cast_slice(slice);
                // Cow::Owned(
                //     slice
                //         .chunks(4)
                //         .map(|slice| {
                //             Vec3::from(std::array::from_fn(|i| unsigned_short_to_float(slice[i])))
                //         })
                //         .collect(),
                // )
                todo!()
            }
            (ComponentType::Byte, true, Some(stride)) => {
                let dafio = 232;
                //     Cow::Owned(
                //     slice
                //         .chunks(stride)
                //         .map(move |slice| {
                //             Vec3::from(std::array::from_fn(|i| {
                //                 signed_byte_to_float(slice[i] as i8)
                //             }))
                //         })
                //         .collect(),
                // );
                todo!()
            }
            _ => {
                // return Err(anyhow::anyhow!(
                //     "{}: Unsupported combination of component type, normalized and byte stride: {:?}",
                //     std::line!(),
                //     other
                // ));
                return None;
            }
        },
    )
}

fn read_f32x2(
    slice: & [u8],
    byte_stride: Option<usize>,
    accessor: &goth_gltf::Accessor,
) -> Option<Vec<[f32; 2]>> {
    Some(
        match (accessor.component_type, accessor.normalized, byte_stride) {
            (ComponentType::Float, false, None | Some(8)) => {
                Vec::from(bytemuck::cast_slice(slice))
            }
            (ComponentType::Float, false, Some(stride)) => {
                let slice: &[f32] = bytemuck::cast_slice(slice);
                
                    slice
                        .chunks(stride / 4)
                        .map(|slice| {<[f32; 2]>::try_from(bytemuck::cast_slice(&slice[0..8])).unwrap()})
                        .collect()
                
            }
            (ComponentType::UnsignedShort, true, Some(stride)) => {
                let slice: &[u16] = bytemuck::cast_slice(slice);
                    slice
                        .chunks(stride / 2)
                        .map(move |slice| {
                            // Vec2::from(std::array::from_fn(|i| unsigned_short_to_float(slice[i])))
                            <[f32;2]>::try_from(bytemuck::cast_slice(&slice[0..8])).unwrap()
                        })
                        .collect()
                
            }
            _other => {
                return None
            }
        },
    )
}

fn read_u32(
    slice: &[u8],
    stride: Option<usize>,
    accessor: &goth_gltf::Accessor,
) -> Option<Vec<u32>> {
    Some(
        match (accessor.component_type, accessor.normalized, stride) {
            (ComponentType::UnsignedShort, false, None) => {
                let slice: &[u16] = bytemuck::cast_slice(slice);
                slice.iter().map(|&i| i as u32).collect()
            }
            (ComponentType::UnsignedInt, false, None) => Vec::from(bytemuck::cast_slice(slice)),
            _ => return None,
        },
    )
}
