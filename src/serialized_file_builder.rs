use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{Seek, Write};

use anyhow::Result;
use byteorder::LittleEndian;
use rabex::files::serialzedfile::{
    FileIdentifier, Guid, ObjectInfo, SerializedFileHeader, SerializedType, TypeTreeProvider,
};
use rabex::files::{SerializedFile, serialzedfile};
use rabex::objects::ClassId;
use rabex::serde_typetree;
use rabex::tpk::UnityVersion;

use crate::unity::ClassIdType;

pub struct SerializedFileBuilder<'a, P> {
    unity_version: UnityVersion,
    common_offset_map: &'a HashMap<&'a str, u32>,
    typetree_provider: &'a P,
    next_path_id: i64,
    objects: Vec<(ObjectInfo, Cow<'a, [u8]>)>,

    types: Vec<SerializedType>,
    types_cache: HashMap<ClassId, i32>,
}

impl<'a, P: TypeTreeProvider> SerializedFileBuilder<'a, P> {
    pub fn new(
        version: UnityVersion,
        typetree_provider: &'a P,
        common_offset_map: &'a HashMap<&'a str, u32>,
    ) -> Self {
        Self {
            unity_version: version,
            typetree_provider,
            common_offset_map,
            next_path_id: 0,
            objects: Vec::new(),
            types: Vec::new(),
            types_cache: HashMap::default(),
        }
    }

    pub fn add_object<T: serde::Serialize + ClassIdType>(&mut self, object: &T) -> Result<()> {
        let tt = self
            .typetree_provider
            .get_typetree_node(T::CLASS_ID, self.unity_version)
            .unwrap();

        let data = serde_typetree::to_vec::<_, LittleEndian>(&object, &tt)?;

        let type_index = *self.types_cache.entry(T::CLASS_ID).or_insert_with(|| {
            let ty = self
                .typetree_provider
                .get_typetree_node(T::CLASS_ID, self.unity_version)
                .unwrap();
            let type_index = self.types.len();
            self.types
                .push(SerializedType::simple(T::CLASS_ID, Some(ty.into_owned())));
            type_index as i32
        });

        self.objects.push((
            ObjectInfo {
                m_PathID: self.next_path_id,
                m_TypeID: type_index,
                m_ClassID: T::CLASS_ID,
                ..Default::default()
            },
            Cow::Owned(data),
        ));

        self.next_path_id += 1;

        Ok(())
    }

    pub fn write<W: Write + Seek>(self, writer: W) -> Result<()> {
        let file = SerializedFile {
            m_Header: SerializedFileHeader {
                m_MetadataSize: 0,
                m_FileSize: 0,
                m_Version: 22,
                m_DataOffset: 0,
                m_Endianess: serialzedfile::Endianness::Little,
                m_Reserved: [0, 0, 0],
                unknown: 0,
            },
            m_UnityVersion: Some(self.unity_version),
            m_TargetPlatform: Some(24),
            m_EnableTypeTree: true,
            m_bigIDEnabled: None,
            m_Types: self.types,
            m_Objects: Default::default(),
            m_Objects_lookup: Default::default(),
            m_ScriptTypes: Some(vec![]),
            m_Externals: vec![FileIdentifier {
                tempEmpty: Some("".to_owned()),
                guid: Some(Guid([0, 0, 0, 0, 0, 0, 0, 0, 14, 0, 0, 0, 0, 0, 0, 0])),
                typeId: Some(0),
                pathName: "Library/unity default resources".into(),
            }],
            m_RefTypes: Some(vec![]),
            m_UserInformation: Some("".into()),
        };
        serialzedfile::write_serialized_with(
            writer,
            &file,
            self.common_offset_map,
            self.objects.into_iter(),
        )?;

        Ok(())
    }
}
