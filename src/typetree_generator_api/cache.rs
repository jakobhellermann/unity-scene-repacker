use std::collections::hash_map::Entry;

use rabex::typetree::TypeTreeNode;
use rustc_hash::FxHashMap;

use super::{Error, TypeTreeGenerator};

pub struct TypeTreeGeneratorCache {
    generator: TypeTreeGenerator,
    cache: FxHashMap<(String, String), TypeTreeNode>,
}
impl TypeTreeGeneratorCache {
    pub fn new(generator: TypeTreeGenerator) -> Self {
        TypeTreeGeneratorCache {
            generator,
            cache: FxHashMap::default(),
        }
    }

    pub fn generate(
        &mut self,
        assembly_name: &str,
        full_name: &str,
    ) -> Result<&TypeTreeNode, Error> {
        match self
            .cache
            .entry((assembly_name.to_owned(), full_name.to_owned()))
        {
            Entry::Occupied(occupied_entry) => Ok(occupied_entry.into_mut()),
            Entry::Vacant(vacant_entry) => {
                let value = self
                    .generator
                    .generate_typetree_raw(assembly_name, full_name)?;
                Ok(vacant_entry.insert(value))
            }
        }
    }
}
