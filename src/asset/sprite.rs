use crate::asset::{assert_ver, Asset, AssetDataError};
use crate::byteio::{ReadBytes, ReadString, WriteBytes, WriteString};
use std::convert::TryInto;
use std::io::{self, Seek, SeekFrom};

pub const VERSION: u32 = 800;
pub const VERSION_COLLISION: u32 = 800;
pub const VERSION_FRAME: u32 = 800;

pub struct Sprite {
    /// The asset name present in GML and the editor.
    pub name: String,

    /// The origin within the sprite.
    pub origin_x: i32,

    /// The origin within the sprite.
    pub origin_y: i32,

    /// The raw RGBA pixeldata for each frame.
    pub frames: Vec<Frame>,

    /// The collider associated with one or each frame.
    /// If `per_frame_colliders` is false, this contains 1 map.
    pub colliders: Vec<CollisionMap>,

    /// Whether each individual frame has its own collision map.
    pub per_frame_colliders: bool,
}

pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub data: Box<[u8]>,
}

pub struct CollisionMap {
    pub bbox_width: u32,
    pub bbox_height: u32,
    pub bbox_left: u32,
    pub bbox_right: u32,
    pub bbox_bottom: u32,
    pub bbox_top: u32,
    pub data: Box<[bool]>,
}

impl Asset for Sprite {
    fn deserialize<B>(bytes: B, strict: bool, _version: u32) -> Result<Self, AssetDataError>
    where
        B: AsRef<[u8]>,
        Self: Sized,
    {
        let mut reader = io::Cursor::new(bytes.as_ref());
        let name = reader.read_pas_string()?;

        if strict {
            let version = reader.read_u32_le()?;
            assert_ver(version, VERSION)?;
        } else {
            reader.seek(SeekFrom::Current(4))?;
        }

        let origin_x = reader.read_i32_le()?;
        let origin_y = reader.read_i32_le()?;
        let frame_count = reader.read_u32_le()?;
        let (frames, colliders, per_frame_colliders) = if frame_count != 0 {
            let mut frames = Vec::with_capacity(frame_count as usize);
            for _ in 0..frame_count {
                if strict {
                    let version = reader.read_u32_le()?;
                    assert_ver(version, VERSION_FRAME)?;
                } else {
                    reader.seek(SeekFrom::Current(4))?;
                }

                let frame_width = reader.read_u32_le()?;
                let frame_height = reader.read_u32_le()?;

                let pixeldata_len = reader.read_u32_le()? as usize;
                let pixeldata_pixels = frame_width as usize * frame_height as usize;

                // sanity check
                if pixeldata_len != (pixeldata_pixels * 4) {
                    panic!("Inconsistent pixel data length with dimensions");
                }

                // read pixeldata
                let pos = reader.position() as usize;
                reader.seek(SeekFrom::Current(pixeldata_len as i64))?;
                let data = reader
                    .get_ref()
                    .get(pos..pos + pixeldata_len)
                    .unwrap_or_else(|| unreachable!());

                frames.push(Frame {
                    width: frame_width,
                    height: frame_height,
                    data: data.to_vec().into_boxed_slice(),
                });
            }

            fn read_collision<T>(reader: &mut io::Cursor<T>, strict: bool) -> Result<CollisionMap, AssetDataError>
            where
                T: AsRef<[u8]>,
            {
                if strict {
                    let version = reader.read_u32_le()?;
                    assert_ver(version, VERSION_COLLISION)?;
                } else {
                    reader.seek(SeekFrom::Current(4))?;
                }

                let bbox_width = reader.read_u32_le()?;
                let bbox_height = reader.read_u32_le()?;
                let bbox_left = reader.read_u32_le()?;
                let bbox_right = reader.read_u32_le()?;
                let bbox_bottom = reader.read_u32_le()?;
                let bbox_top = reader.read_u32_le()?;

                let mask_size = bbox_width as usize * bbox_height as usize;
                let pos = reader.position() as usize;
                reader.seek(SeekFrom::Current(4 * mask_size as i64))?;
                let mask: Vec<bool> = reader
                    .get_ref() // inner data
                    .as_ref() // needed since data is AsRef<[u8]>
                    .get(pos..pos + (4 * mask_size)) // get mask data chunk
                    .unwrap_or_else(|| unreachable!()) // seek checked chunk size already...
                    .chunks_exact(4) // every 4 bytes
                    .map(|ch| {
                        // until we get const generics we need to do this to get an exact array
                        let chunk: &[u8; 4] = ch
                            .try_into()
                            .unwrap_or_else(|_| unsafe { std::hint::unreachable_unchecked() });
                        // nonzero value indicates collision pixel present
                        u32::from_le_bytes(*chunk) != 0
                    })
                    .collect();

                Ok(CollisionMap {
                    bbox_width,
                    bbox_height,
                    bbox_top,
                    bbox_bottom,
                    bbox_left,
                    bbox_right,
                    data: mask.into_boxed_slice(),
                })
            }

            let mut colliders: Vec<CollisionMap>;
            let per_frame_colliders = reader.read_u32_le()? != 0;
            if per_frame_colliders {
                colliders = Vec::with_capacity(frame_count as usize);
                for _ in 0..frame_count {
                    colliders.push(read_collision(&mut reader, strict)?);
                }
            } else {
                colliders = vec![read_collision(&mut reader, strict)?];
            }
            (frames, colliders, per_frame_colliders)
        } else {
            (Vec::new(), Vec::new(), false)
        };

        Ok(Sprite {
            name,
            origin_x,
            origin_y,
            frames,
            colliders,
            per_frame_colliders,
        })
    }

    fn serialize<W>(&self, writer: &mut W) -> io::Result<usize>
    where
        W: io::Write,
    {
        let mut result = writer.write_pas_string(&self.name)?;
        result += writer.write_u32_le(VERSION as u32)?;
        result += writer.write_i32_le(self.origin_x)?;
        result += writer.write_i32_le(self.origin_y)?;
        if self.frames.len() != 0 {
            result += writer.write_u32_le(self.frames.len() as u32)?;
            for frame in self.frames.iter() {
                result += writer.write_u32_le(VERSION_FRAME)?;
                result += writer.write_u32_le(frame.width)?;
                result += writer.write_u32_le(frame.height)?;
                result += writer.write_u32_le(frame.data.len() as u32)?;

                let pixeldata = frame.data.clone();
                result += writer.write(&pixeldata)?;
            }
            result += writer.write_u32_le(self.per_frame_colliders as u32)?;
            for collider in self.colliders.iter() {
                result += writer.write_u32_le(VERSION_COLLISION)?;
                result += writer.write_u32_le(collider.bbox_width)?;
                result += writer.write_u32_le(collider.bbox_height)?;
                result += writer.write_u32_le(collider.bbox_left)?;
                result += writer.write_u32_le(collider.bbox_right)?;
                result += writer.write_u32_le(collider.bbox_bottom)?;
                result += writer.write_u32_le(collider.bbox_top)?;
                for pixel in &*collider.data {
                    result += writer.write_u32_le(*pixel as u32)?;
                }
            }
        } else {
            result += writer.write_u32_le(0)?;
        }

        Ok(result)
    }
}
