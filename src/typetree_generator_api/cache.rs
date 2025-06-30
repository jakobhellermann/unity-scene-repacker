use elsa::sync::FrozenMap;
use rabex::typetree::TypeTreeNode;

use super::{Error, TypeTreeGenerator};

pub struct TypeTreeGeneratorCache {
    generator: TypeTreeGenerator,
    pub cache: FrozenMap<(String, String), Box<TypeTreeNode>>,
}
impl TypeTreeGeneratorCache {
    pub fn new(generator: TypeTreeGenerator) -> Self {
        TypeTreeGeneratorCache {
            generator,
            cache: FrozenMap::default(),
        }
    }

    pub fn generate(&self, assembly_name: &str, full_name: &str) -> Result<&TypeTreeNode, Error> {
        let key = (assembly_name.to_owned(), full_name.to_owned());
        match self.cache.get(&key) {
            Some(value) => Ok(value),
            None => {
                let value = self
                    .generator
                    .generate_typetree_raw(assembly_name, full_name)?;
                Ok(self.cache.insert(key, Box::new(value)))
            }
        }
    }
}
