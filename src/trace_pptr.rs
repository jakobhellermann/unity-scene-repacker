use anyhow::{Context, Result};
use byteorder::LittleEndian;
use rabex::objects::pptr::PPtr;
use rabex::serde_typetree;
use rabex::typetree::TypeTreeNode;
use serde::de::{DeserializeSeed, IgnoredAny};
use std::io::{Read, Seek};

#[inline(never)]
pub fn trace_pptrs(tt: &TypeTreeNode, reader: &mut (impl Read + Seek)) -> Result<Vec<PPtr>> {
    let mut deserializer = serde_typetree::Deserializer::<_, LittleEndian>::from_reader(reader, tt);
    let mut output = Vec::new();
    CollectPPtrDeser {
        output: &mut output,
    }
    .deserialize(&mut deserializer)
    .context("Trying to scan for PPtr")?;
    return Ok(output);

    macro_rules! ignore {
        ($name:ident $ty:ty) => {
            fn $name<E: serde::de::Error>(self, _: $ty) -> Result<Self::Value, E> {
                Ok(())
            }
        };
    }

    struct CollectPPtrsVisitor<'a> {
        output: &'a mut Vec<PPtr>,
    }
    impl<'a, 'de> serde::de::Visitor<'de> for CollectPPtrsVisitor<'a> {
        type Value = ();

        ignore!(visit_bool bool);
        ignore!(visit_char char);
        ignore!(visit_u8 u8);
        ignore!(visit_u16 u16);
        ignore!(visit_u32 u32);
        ignore!(visit_u64 u64);
        ignore!(visit_i8 i8);
        ignore!(visit_i16 i16);
        ignore!(visit_i32 i32);
        ignore!(visit_i64 i64);
        ignore!(visit_f32 f32);
        ignore!(visit_f64 f64);
        ignore!(visit_string String);
        ignore!(visit_str & str);

        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut a: A,
        ) -> Result<Self::Value, A::Error> {
            while let Some(_) = a.next_element_seed(CollectPPtrDeser {
                output: self.output,
            })? {}

            Ok(())
        }

        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            let mut file_id = None;
            let mut path_id = None;
            let mut others = false;

            while let Some(key) = map.next_key::<String>()? {
                match key.as_str() {
                    "m_FileID" => file_id = Some(map.next_value::<i32>()?),
                    "m_PathID" => path_id = Some(map.next_value::<i64>()?),
                    "m_Father" => {
                        map.next_value::<IgnoredAny>()?;
                    }
                    _ => {
                        others = true;
                        // TODO: figure out int3_storage thing
                        let _ = map.next_value_seed(CollectPPtrDeser {
                            output: self.output,
                        });
                    }
                }
            }

            if let (Some(file_id), Some(path_id)) = (file_id, path_id) {
                assert!(!others);
                let pptr = PPtr {
                    m_FileID: file_id,
                    m_PathID: path_id,
                };
                if pptr != PPtr::default() {
                    self.output.push(pptr);
                }
            }

            Ok(())
        }

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("pptr visitor")
        }
    }

    struct CollectPPtrDeser<'a> {
        output: &'a mut Vec<PPtr>,
    }

    impl<'a, 'de> serde::de::DeserializeSeed<'de> for CollectPPtrDeser<'a> {
        type Value = ();

        fn deserialize<D: serde::Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> Result<Self::Value, D::Error> {
            deserializer.deserialize_any(CollectPPtrsVisitor {
                output: self.output,
            })
        }
    }
}
