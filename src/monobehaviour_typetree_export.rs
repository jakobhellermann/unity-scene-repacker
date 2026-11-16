use std::iter::Peekable;

/// Basic wip format for type tree dumps for a set of assemblies / types
use anyhow::Result;
use elsa::sync::FrozenMap;
use rabex::typetree::TypeTreeNode;

/*
 * n_assemblies x
 *  len name
 *  n_types x:
 *    len full_name
 *    n_flat_nodes x:
 *      len type
 *      len name
 *      u8  level
 *      i32 flags
 */
pub fn read(export: &[u8]) -> Result<FrozenMap<(String, String), Box<TypeTreeNode>>> {
    use byteorder::{LE, ReadBytesExt};

    fn read_str(reader: &mut impl std::io::Read) -> Result<String> {
        let len = reader.read_u32::<LE>()?;
        let mut data = vec![0; len as usize];
        reader.read_exact(&mut data)?;
        let str = String::from_utf8(data)?;
        Ok(str)
    }

    let export = lz4_flex::block::decompress_size_prepended(export)?;
    let mut reader = &mut &*export;
    let monobehaviour_type_cache = FrozenMap::default();

    let n_assemblies = reader.read_u32::<LE>()?;
    for _ in 0..n_assemblies {
        let assembly_name = read_str(&mut reader)?;
        let n_types = reader.read_u32::<LE>()?;
        for _ in 0..n_types {
            let full_name = read_str(&mut reader)?;
            let n_flat_nodes = reader.read_u32::<LE>()?;
            let mut flat_nodes = Vec::with_capacity(n_flat_nodes as usize);
            for _ in 0..n_flat_nodes {
                let ty = read_str(&mut reader)?;
                let name = read_str(&mut reader)?;
                let level = reader.read_u8()?;
                let flags = reader.read_i32::<LE>()?;
                flat_nodes.push((ty, name, level, flags));
            }

            let node = reconstruct_typetree_node(&flat_nodes);
            monobehaviour_type_cache.insert((assembly_name.clone(), full_name), Box::new(node));
        }
    }

    Ok(monobehaviour_type_cache)
}

/// Rebuilds a tree from pre-order `(type, name, level, flags)` nodes.
fn reconstruct_typetree_node(flat: &[(String, String, u8, i32)]) -> TypeTreeNode {
    fn build<'a>(
        iter: &mut Peekable<impl Iterator<Item = &'a (String, String, u8, i32)>>,
    ) -> TypeTreeNode {
        let (ty, name, level, flags) = iter.next().unwrap();
        let mut children = Vec::new();
        while iter
            .peek()
            .is_some_and(|(_, _, child_level, _)| child_level > level)
        {
            children.push(build(iter));
        }
        TypeTreeNode {
            m_Type: ty.clone(),
            m_Name: name.clone(),
            m_MetaFlag: Some(*flags),
            children,
            ..Default::default()
        }
    }
    build(&mut flat.iter().peekable())
}
