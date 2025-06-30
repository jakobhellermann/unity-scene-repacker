#![allow(dead_code, unused_variables)]

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use rabex::UnityVersion;
use rabex::typetree::TypeTreeNode;

pub mod cache;

pub struct TypeTreeGenerator {}

pub enum GeneratorBackend {
    AssetStudio,
    AssetsTools,
    AssetRipper,
}

#[derive(Debug)]
pub enum Error {
    CreationError,
    Status(i32),
    UTF8(std::str::Utf8Error),
    IO(std::io::Error),
}
impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Error::IO(value)
    }
}
impl From<std::str::Utf8Error> for Error {
    fn from(value: std::str::Utf8Error) -> Self {
        Error::UTF8(value)
    }
}
impl Error {
    fn from_code(status_code: i32) -> Result<(), Error> {
        if status_code == 0 {
            Ok(())
        } else {
            Err(Error::Status(status_code))
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::CreationError => f.write_str("Could not create generator instance"),
            Error::Status(status) => write!(f, "Generator returned status code {status}"),
            Error::UTF8(utf8_error) => write!(f, "Could not decode as utf8: {utf8_error}"),
            Error::IO(error) => write!(f, "IO Error: {error}"),
        }
    }
}
impl std::error::Error for Error {}

impl TypeTreeGenerator {
    pub fn new(
        unity_version: UnityVersion,
        backend: GeneratorBackend,
    ) -> Result<TypeTreeGenerator, Error> {
        Ok(TypeTreeGenerator {})
    }

    pub fn load_dll_path(&self, path: impl AsRef<Path>) -> Result<(), Error> {
        let data = std::fs::read(&path).map_err(Error::IO)?;
        self.load_dll(&data)
    }

    pub fn load_dll(&self, dll: &[u8]) -> Result<(), Error> {
        todo!()
    }

    pub fn load_all_dll_in_dir(&self, path: impl AsRef<Path>) -> Result<(), Error> {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_type()?.is_file() && entry.path().extension().is_some_and(|x| x == "dll")
            {
                self.load_dll_path(entry.path()).unwrap();
            }
        }
        Ok(())
    }

    pub fn get_monobehaviour_definitions(&self) -> Result<HashMap<String, Vec<String>>, Error> {
        todo!()
    }

    pub fn generate_typetree_json(&self, assembly: &str, full_name: &str) -> Result<String, Error> {
        todo!()
    }

    pub fn generate_typetree_raw(
        &self,
        assembly: &str,
        full_name: &str,
    ) -> Result<TypeTreeNode, Error> {
        todo!()
    }
}

pub fn reconstruct_typetree_node<'a>(flat: &[(&'a str, &'a str, u8, i32)]) -> TypeTreeNode {
    let mut stack = Vec::new();

    let mut parent = 0;
    let mut prev = 0;

    let mut children: BTreeMap<usize, Vec<usize>> = Default::default();

    for (node, &(_, _, level, _)) in flat.iter().enumerate().skip(1) {
        if level > flat[prev].2 {
            stack.push(parent);
            parent = prev;
        } else if level < flat[prev].2 {
            while level <= flat[parent].2 {
                parent = stack.pop().unwrap();
            }
        }

        children.entry(parent).or_default().push(node);
        prev = node;
    }

    build_node_tree(0, flat, &children)
}

fn build_node_tree(
    index: usize,
    flat_nodes: &[(&str, &str, u8, i32)],
    children_map: &BTreeMap<usize, Vec<usize>>,
) -> TypeTreeNode {
    let &(ty, name, level, flags) = &flat_nodes[index];
    let child_indices = children_map.get(&index);

    let children = match child_indices {
        Some(indices) => indices
            .iter()
            .map(|&child_index| build_node_tree(child_index, flat_nodes, children_map))
            .collect(),
        None => Vec::new(),
    };

    TypeTreeNode {
        m_Type: ty.to_owned(),
        m_Name: name.to_owned(),
        m_Level: level,
        m_MetaFlag: Some(flags),
        children,
        ..Default::default()
    }
}

impl Drop for TypeTreeGenerator {
    fn drop(&mut self) {}
}
