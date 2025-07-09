use anyhow::{Result, bail};
use elsa::sync::FrozenMap;
use rabex::typetree::TypeTreeNode;
use typetree_generator_api::TypeTreeGenerator;

pub struct TypeTreeGeneratorCache {
    generator: Option<TypeTreeGenerator>,
    cache: FrozenMap<(String, String), Box<TypeTreeNode>>,
    base_node: TypeTreeNode,
}
impl TypeTreeGeneratorCache {
    pub fn new(generator: TypeTreeGenerator, base_node: TypeTreeNode) -> Self {
        TypeTreeGeneratorCache {
            generator: Some(generator),
            cache: FrozenMap::default(),
            base_node,
        }
    }
    pub fn prefilled(
        cache: FrozenMap<(String, String), Box<TypeTreeNode>>,
        base_node: TypeTreeNode,
    ) -> Self {
        TypeTreeGeneratorCache {
            generator: None,
            cache,
            base_node,
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
                let value = generator
                    .generate_typetree_raw(self.base_node.clone(), assembly_name, full_name)?
                    .unwrap_or_else(|| self.base_node.clone());
                Ok(self.cache.insert(key, Box::new(value)))
            }
        }
    }
}
