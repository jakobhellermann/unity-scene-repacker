use std::borrow::Cow;
use std::io::Cursor;

use anyhow::{Context, Result};
use rabex::files::serializedfile::builder::SerializedFileBuilder;
use rabex::objects::ClassId;
use rabex::objects::pptr::PPtr;
use rabex::serde_typetree;
use rabex::typetree::TypeTreeProvider;
use rabex_env::Environment;
use rabex_env::rewrite::{FieldPath, FieldRewriter, FieldValue};
use rabex_env::unity::types::MonoScript;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Deserializer};
use serde_derive::Deserialize;

/// Write a value into a field of every MonoBehaviour of a script.
#[derive(Debug, Clone, Deserialize)]
pub struct RewriteRule {
    /// Without `.dll`
    pub assembly: String,
    /// `Namespace.Class`
    pub script: String,
    #[serde(deserialize_with = "field_path")]
    pub field: FieldPath,
    #[serde(deserialize_with = "field_value")]
    pub value: FieldValue,
}

fn field_path<'de, D: Deserializer<'de>>(deserializer: D) -> Result<FieldPath, D::Error> {
    let path = <Cow<str>>::deserialize(deserializer)?;
    path.parse().map_err(serde::de::Error::custom)
}

fn field_value<'de, D: Deserializer<'de>>(deserializer: D) -> Result<FieldValue, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Value {
        Bool(bool),
        Int(i64),
    }
    Ok(match Value::deserialize(deserializer)? {
        Value::Bool(bool) => FieldValue::Bool(bool),
        Value::Int(int) => FieldValue::Int(int),
    })
}

/// Rewrite the MonoBehaviours of `builder` that `rules` apply to.
pub(crate) fn apply<P: TypeTreeProvider>(
    env: &Environment,
    builder: &mut SerializedFileBuilder<'_, P>,
    rules: &[RewriteRule],
) -> Result<()> {
    if rules.is_empty() {
        return Ok(());
    }
    let mut fields_by_script: FxHashMap<(&str, &str), Vec<(FieldPath, FieldValue)>> =
        FxHashMap::default();
    for rule in rules {
        fields_by_script
            .entry((&rule.assembly, &rule.script))
            .or_default()
            .push((rule.field.clone(), rule.value));
    }

    let script_types = builder
        .serialized
        .m_ScriptTypes
        .as_deref()
        .unwrap_or_default();
    let mut rewriters = FxHashMap::default();
    for (type_id, ty) in builder.serialized.m_Types.iter().enumerate() {
        if ty.m_ClassID != ClassId::MonoBehaviour {
            continue;
        }
        let Ok(script_index) = usize::try_from(ty.m_ScriptTypeIndex) else {
            continue;
        };
        let script = read_script(env, builder, script_types[script_index].into())?;
        let full_name = script.full_name();
        let Some(fields) = fields_by_script.get(&(script.assembly_name_base(), &*full_name)) else {
            continue;
        };
        let tree = match &ty.m_Type {
            Some(tree) => tree,
            None => env.generate_typetree_for_script(&script)?,
        };
        let rewriter =
            FieldRewriter::new(tree, fields).with_context(|| format!("rewriting {full_name}"))?;
        rewriters.insert(type_id as i32, rewriter);
    }

    for (info, data) in builder.objects.values_mut() {
        if let Some(rewriter) = rewriters.get(&info.m_TypeID) {
            rewriter
                .apply(data)
                .with_context(|| format!("rewriting object {}", info.m_PathID))?;
        }
    }
    Ok(())
}

fn read_script<P: TypeTreeProvider>(
    env: &Environment,
    builder: &SerializedFileBuilder<'_, P>,
    script: PPtr,
) -> Result<MonoScript> {
    let Some(index) = script.m_FileID.get_externals_index() else {
        let (info, data) = builder
            .objects
            .get(&script.m_PathID)
            .with_context(|| format!("script {script:?} is not in the file"))?;
        let tree = builder
            .serialized
            .get_typetree_for(info, builder.typetree_provider)?;
        return Ok(serde_typetree::from_reader_endianed(
            &mut Cursor::new(&**data),
            &tree,
            builder.serialized.m_Header.m_Endianess,
        )?);
    };
    let external = builder
        .serialized
        .m_Externals
        .get(index)
        .with_context(|| format!("script {script:?} references a missing external"))?;
    env.load_serialized(&external.pathName)?
        .object_at::<MonoScript>(script.m_PathID)?
        .read()
}
