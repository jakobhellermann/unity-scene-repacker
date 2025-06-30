use anyhow::{Result, bail};
use elsa::sync::FrozenMap;
use rabex::typetree::TypeTreeNode;

use super::TypeTreeGenerator;

pub struct TypeTreeGeneratorCache {
    generator: Option<TypeTreeGenerator>,
    cache: FrozenMap<(String, String), Box<TypeTreeNode>>,
}
impl TypeTreeGeneratorCache {
    pub fn new(generator: TypeTreeGenerator) -> Self {
        TypeTreeGeneratorCache {
            generator: Some(generator),
            cache: FrozenMap::default(),
        }
    }
    pub fn prefilled(cache: FrozenMap<(String, String), Box<TypeTreeNode>>) -> Self {
        TypeTreeGeneratorCache {
            generator: None,
            cache,
        }
    }

    pub fn generate(&self, assembly_name: &str, full_name: &str) -> Result<&TypeTreeNode> {
        let key = (assembly_name.to_owned(), full_name.to_owned());
        match self.cache.get(&key) {
            Some(value) => Ok(value),
            None => {
                let Some(generator) = &self.generator else {
                    bail!("Missing {assembly_name} / {full_name} in monobehaviour typetree export");
                };
                let value = generator.generate_typetree_raw(assembly_name, full_name)?;
                Ok(self.cache.insert(key, Box::new(value)))
            }
        }
    }
}
