/// Basic wip format for type tree dumps for a set of assemblies / types
use anyhow::Result;
use elsa::sync::FrozenMap;
use rabex::typetree::TypeTreeNode;

use typetree_generator_api::reconstruct_typetree_node;

/*
 * n_assemblies x
 *  len name
 *  n_types x:
 *    len full_name
 *    n_flat_nodes x:
 *      len name
 *      len type
 *      u8  index
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

    let export = lz4_flex::decompress_size_prepended(export)?;
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
                let name = read_str(&mut reader)?;
                let ty = read_str(&mut reader)?;
                let index = reader.read_u8()?;
                let full_name = reader.read_i32::<LE>()?;
                flat_nodes.push((name, ty, index, full_name));
            }

            let node = reconstruct_typetree_node(&flat_nodes);
            monobehaviour_type_cache.insert((assembly_name.clone(), full_name), Box::new(node));
        }
    }

    Ok(monobehaviour_type_cache)
}
