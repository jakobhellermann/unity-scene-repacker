use anyhow::Result;
use byteorder::{ByteOrder, LittleEndian, ReadBytesExt};
use rabex::files::serializedfile::Endianness;
use rabex::objects::PPtr;
use rabex::objects::pptr::{FileId, PathId};
use rabex::typetree::TypeTreeNode;
use rustc_hash::FxHashMap;
use std::io::{Cursor, Read, Seek};

#[inline(never)]
pub fn trace_pptrs<B: ByteOrder>(
    tt: &TypeTreeNode,
    reader: &mut (impl Read + Seek),
) -> Result<Vec<PPtr>> {
    let mut pptrs = Vec::new();

    visit::<_, LittleEndian>(reader, tt, &mut |tt, reader| {
        if tt.m_Type.starts_with("PPtr<") && tt.m_Name != "m_Father" {
            let file_id = reader.read_i32::<B>()?;
            let path_id = reader.read_i64::<B>()?;
            if tt.requires_align() {
                reader.align4()?;
            }

            let pptr = PPtr {
                m_FileID: file_id,
                m_PathID: path_id,
            };
            if !pptr.is_null() {
                pptrs.push(pptr);
            }
            Ok(true)
        } else {
            Ok(false)
        }
    })?;

    Ok(pptrs)
}

#[inline(never)]
pub fn replace_pptrs_inplace_endianed(
    value: &mut [u8],
    ty: &TypeTreeNode,
    path_id_remap: &FxHashMap<PathId, PathId>,
    file_id_remap: &FxHashMap<FileId, FileId>,
    endianness: Endianness,
) -> Result<()> {
    match endianness {
        Endianness::Little => {
            replace_pptrs_inplace::<LittleEndian>(value, ty, path_id_remap, file_id_remap)
        }
        Endianness::Big => {
            replace_pptrs_inplace::<LittleEndian>(value, ty, path_id_remap, file_id_remap)
        }
    }
}

pub fn replace_pptrs_inplace<B: ByteOrder>(
    value: &mut [u8],
    ty: &TypeTreeNode,
    path_id_remap: &FxHashMap<PathId, PathId>,
    file_id_remap: &FxHashMap<FileId, FileId>,
) -> Result<()> {
    visit::<_, B>(&mut Cursor::new(value), ty, &mut |tt, reader| {
        if tt.m_Type.starts_with("PPtr<") {
            let pos = reader.position() as usize;

            let file_id = reader.read_i32::<B>()?;
            let path_id = reader.read_i64::<B>()?;
            if tt.requires_align() {
                reader.align4()?;
            }

            if file_id == 0 {
                if let Some(&replacement) = path_id_remap.get(&path_id) {
                    B::write_i64(&mut reader.get_mut()[pos + 4..], replacement);
                }
            } else if let Some(&replacement) = file_id_remap.get(&file_id) {
                B::write_i32(&mut reader.get_mut()[pos..], replacement);
            }

            Ok(true)
        } else {
            Ok(false)
        }
    })?;

    Ok(())
}

pub fn visit<R, B>(
    reader: &mut R,
    tt: &TypeTreeNode,
    f: &mut impl FnMut(&TypeTreeNode, &mut R) -> Result<bool, std::io::Error>,
) -> Result<(), std::io::Error>
where
    R: Read + Seek,
    B: ByteOrder,
{
    let size = match tt.m_Type.as_str() {
        "bool" => 1,
        "UInt8" => 1,
        "UInt16" | "unsigned short" => 2,
        "UInt32" | "unsigned int" | "Type*" => 4,
        "UInt64" | "unsigned long long" | "FileSize" => 8,
        "SInt8" => 1,
        "SInt16" | "short" => 2,
        "SInt32" | "int" => 4,
        "SInt64" | "long long" => 8,
        "float" => 4,
        "double" => 8,
        "char" => 1,
        "string" => {
            let length = reader.read_u32::<B>()?;
            reader.seek_relative(length as i64)?;
            reader.align4()?;
            return Ok(());
        }
        "map" => {
            let length = reader.read_u32::<B>()?;

            let pair = &tt.children[0].children[1];
            let key_type = &pair.children[0];
            let value_type = &pair.children[1];

            if tt.requires_align() || pair.requires_align() {
                reader.align4()?;
            }

            for _ in 0..length {
                visit::<_, B>(reader, key_type, f)?;
                visit::<_, B>(reader, value_type, f)?;
            }

            return Ok(());
        }
        "TypelessData" => {
            todo!()
        }
        "ReferencedObject" | "ReferencedObjectData" | "ManagedReferencesRegistry" => {
            todo!()
        }
        _ => {
            if let [child] = tt.children.as_slice()
                && child.m_Type == "Array"
            {
                let item_type = &child.children[1];
                let length = reader.read_u32::<B>()?;
                for _ in 0..length {
                    visit::<_, B>(reader, item_type, f)?;
                }
                if tt.requires_align() || child.requires_align() {
                    reader.align4()?;
                }

                return Ok(());
            }

            if f(tt, reader)? {
                return Ok(());
            }

            for child in &tt.children {
                visit::<_, B>(reader, child, f)?;
            }

            return Ok(());
        }
    };

    reader.seek_relative(size)?;
    if tt.requires_align() {
        reader.align4()?;
    }

    Ok(())
}

trait SeekExt: Seek {
    fn align(&mut self, align: usize) -> Result<(), std::io::Error> {
        let pos = self.stream_position()?;
        let new_pos = (pos + align as u64 - 1) & !(align as u64 - 1);
        let diff = new_pos - pos;
        if diff > 0 {
            self.seek(std::io::SeekFrom::Current(diff as i64))?;
        }
        Ok(())
    }

    fn align4(&mut self) -> Result<(), std::io::Error> {
        self.align(4)
    }
}

impl<R: Read + Seek + ?Sized> SeekExt for R {}
