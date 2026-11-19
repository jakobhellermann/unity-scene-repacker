use std::borrow::Cow;
use std::io::Cursor;

use anyhow::{Context, Result, bail};
use rabex::files::serializedfile::builder::SerializedFileBuilder;
use rabex::objects::ClassId;
use rabex::objects::pptr::{PPtr, PathId};
use rabex::serde_typetree;
use rabex::typetree::TypeTreeProvider;
use rabex_env::Environment;
use rabex_env::rewrite::{FieldPath, FieldRewriter, FieldValue};
use rabex_env::unity::types::MonoScript;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Deserializer};
use serde_derive::Deserialize;

/// Change every MonoBehaviour of a script.
#[derive(Debug, Clone, Deserialize)]
pub struct RewriteRule {
    /// Without `.dll`
    pub assembly: String,
    /// `Namespace.Class`
    pub script: String,
    #[serde(flatten)]
    pub action: RewriteAction,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum RewriteAction {
    /// Write `value` into `field`.
    Field {
        #[serde(deserialize_with = "field_path")]
        field: FieldPath,
        #[serde(deserialize_with = "field_value")]
        value: FieldValue,
    },
    /// Bind to another script, which reads the serialized fields by name.
    Remap { remap: ScriptName },
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScriptName {
    /// Without `.dll`
    pub assembly: String,
    /// `Namespace.Class`
    pub script: String,
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
    let mut remaps: FxHashMap<(&str, &str), &ScriptName> = FxHashMap::default();
    for rule in rules {
        let key = (rule.assembly.as_str(), rule.script.as_str());
        match &rule.action {
            RewriteAction::Field { field, value } => fields_by_script
                .entry(key)
                .or_default()
                .push((field.clone(), *value)),
            RewriteAction::Remap { remap } => {
                if remaps.insert(key, remap).is_some() {
                    bail!(
                        "{}:{} is remapped more than once",
                        rule.assembly,
                        rule.script
                    );
                }
            }
        }
    }

    rewrite_fields(env, builder, &fields_by_script)?;
    // after the fields, which match the scripts by their original names
    remap_scripts(builder, &remaps)
}

fn rewrite_fields<P: TypeTreeProvider>(
    env: &Environment,
    builder: &mut SerializedFileBuilder<'_, P>,
    fields_by_script: &FxHashMap<(&str, &str), Vec<(FieldPath, FieldValue)>>,
) -> Result<()> {
    if fields_by_script.is_empty() {
        return Ok(());
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

/// Renames the MonoScripts of `builder`
fn remap_scripts<P: TypeTreeProvider>(
    builder: &mut SerializedFileBuilder<'_, P>,
    remaps: &FxHashMap<(&str, &str), &ScriptName>,
) -> Result<()> {
    if remaps.is_empty() {
        return Ok(());
    }
    let script_ids: Vec<PathId> = builder
        .objects
        .values()
        .filter(|(info, _)| info.m_ClassID == ClassId::MonoScript)
        .map(|(info, _)| info.m_PathID)
        .collect();

    for path_id in script_ids {
        let mut script = read_local_script(builder, path_id)?;
        let full_name = script.full_name();
        let Some(to) = remaps.get(&(script.assembly_name_base(), &*full_name)) else {
            continue;
        };

        let (namespace, class) = to.script.rsplit_once('.').unwrap_or(("", &to.script));
        let (namespace, class) = (namespace.to_owned(), class.to_owned());

        // older unity versions include the extension
        let extension = match script.m_AssemblyName.ends_with(".dll") {
            true => ".dll",
            false => "",
        };
        let assembly = format!("{}{extension}", to.assembly);

        script.m_Name = class.clone();
        script.m_ClassName = class;
        script.m_Namespace = namespace;
        script.m_AssemblyName = assembly;

        builder.replace_object_at(path_id, &script)?;
    }
    Ok(())
}

fn read_script<P: TypeTreeProvider>(
    env: &Environment,
    builder: &SerializedFileBuilder<'_, P>,
    script: PPtr,
) -> Result<MonoScript> {
    let Some(index) = script.m_FileID.get_externals_index() else {
        return read_local_script(builder, script.m_PathID);
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

fn read_local_script<P: TypeTreeProvider>(
    builder: &SerializedFileBuilder<'_, P>,
    path_id: PathId,
) -> Result<MonoScript> {
    let (info, data) = builder
        .objects
        .get(&path_id)
        .with_context(|| format!("script {path_id} is not in the file"))?;
    let tree = builder
        .serialized
        .get_typetree_for(info, builder.typetree_provider)?;

    Ok(serde_typetree::from_reader_endianed(
        &mut Cursor::new(data.as_ref()),
        &tree,
        builder.serialized.m_Header.m_Endianess,
    )?)
}
