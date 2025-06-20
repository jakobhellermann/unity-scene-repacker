use std::borrow::Cow;

use rabex::files::serialzedfile::TypeTreeProvider;
use rabex::objects::ClassId;
use rabex::tpk::UnityVersion;
use rabex::typetree::TypeTreeNode;

pub struct TypeTreeCache<T> {
    inner: T,
    typetree_cache: elsa::FrozenMap<ClassId, Box<Option<TypeTreeNode>>>,
}
impl<T: TypeTreeProvider> TypeTreeCache<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            typetree_cache: Default::default(),
        }
    }
}

impl<T: TypeTreeProvider> TypeTreeProvider for TypeTreeCache<T> {
    fn get_typetree_node(
        &self,
        class_id: ClassId,
        target_version: UnityVersion,
    ) -> Option<Cow<'_, TypeTreeNode>> {
        match self.typetree_cache.get(&class_id) {
            Some(value) => value.as_ref().map(Cow::Borrowed),
            None => {
                let tt = self
                    .inner
                    .get_typetree_node(class_id, target_version)
                    .map(Cow::into_owned);
                self.typetree_cache
                    .insert(class_id, Box::new(tt))
                    .as_ref()
                    .map(Cow::Borrowed)
            }
        }
    }
}
