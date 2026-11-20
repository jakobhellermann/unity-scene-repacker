use std::borrow::Cow;
use std::io::Cursor;

use anyhow::{Context, Result, bail};
use rabex::files::serializedfile::builder::SerializedFileBuilder;
use rabex::objects::ClassId;
use rabex::objects::pptr::{PPtr, PathId};
use rabex::serde_typetree;
use rabex::typetree::TypeTreeProvider;
use rabex_env::Environment;
use rabex_env::rewrite::{FieldPath, Fields, Scalar};
use rabex_env::unity::types::MonoScript;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Deserializer};
use serde_derive::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum RewriteRule {
    /// Change every MonoBehaviour of a script.
    Script {
        /// Without `.dll`
        assembly: String,
        /// `Namespace.Class`
        script: String,
        #[serde(flatten)]
        action: RewriteAction,
    },
    /// Bind every script of an assembly to the same-named script in another assembly.
    Assembly(AssemblyRemap),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssemblyRemap {
    /// Without `.dll`
    pub assembly: String,
    /// Without `.dll`
    pub remap_assembly: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum RewriteAction {
    /// Write `value` into `field`.
    Field {
        #[serde(deserialize_with = "field_path")]
        field: FieldPath,
        #[serde(deserialize_with = "field_value")]
        value: Scalar,
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

fn field_value<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Scalar, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Value {
        Bool(bool),
        Int(i64),
    }
    Ok(match Value::deserialize(deserializer)? {
        Value::Bool(bool) => Scalar::Bool(bool),
        Value::Int(int) => Scalar::Int(int),
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

    let mut fields_by_script: FxHashMap<(&str, &str), Vec<(FieldPath, Scalar)>> =
        FxHashMap::default();
    let mut remaps = Remaps::default();
    for rule in rules {
        match rule {
            RewriteRule::Script {
                assembly,
                script,
                action: RewriteAction::Field { field, value },
            } => fields_by_script
                .entry((assembly, script))
                .or_default()
                .push((field.clone(), *value)),
            RewriteRule::Script {
                assembly,
                script,
                action: RewriteAction::Remap { remap },
            } => {
                if remaps.scripts.insert((assembly, script), remap).is_some() {
                    bail!("{assembly}:{script} is remapped more than once");
                }
            }
            RewriteRule::Assembly(AssemblyRemap {
                assembly,
                remap_assembly,
            }) => {
                if remaps.assemblies.insert(assembly, remap_assembly).is_some() {
                    bail!("{assembly} is remapped more than once");
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
    fields_by_script: &FxHashMap<(&str, &str), Vec<(FieldPath, Scalar)>>,
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
        let paths: Vec<FieldPath> = fields.iter().map(|(path, _)| path.clone()).collect();
        let values: Vec<Scalar> = fields.iter().map(|(_, value)| *value).collect();
        let fields = Fields::at(tree, &paths).with_context(|| format!("rewriting {full_name}"))?;
        rewriters.insert(type_id as i32, (fields, values));
    }

    for (info, data) in builder.objects.values_mut() {
        if let Some((fields, values)) = rewriters.get(&info.m_TypeID) {
            fields
                .rewrite(data, |current| {
                    current.copy_from_slice(values);
                    Ok(())
                })
                .with_context(|| format!("rewriting object {}", info.m_PathID))?;
        }
    }
    Ok(())
}

#[derive(Default)]
struct Remaps<'a> {
    scripts: FxHashMap<(&'a str, &'a str), &'a ScriptName>,
    /// Applies to the scripts without a remap in `scripts`
    assemblies: FxHashMap<&'a str, &'a str>,
}

/// Renames the MonoScripts of `builder`
fn remap_scripts<P: TypeTreeProvider>(
    builder: &mut SerializedFileBuilder<'_, P>,
    remaps: &Remaps,
) -> Result<()> {
    if remaps.scripts.is_empty() && remaps.assemblies.is_empty() {
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
        let (assembly, class_name) = match remaps
            .scripts
            .get(&(script.assembly_name_base(), &*full_name))
        {
            Some(to) => (&*to.assembly, Some(&*to.script)),
            None => match remaps.assemblies.get(script.assembly_name_base()) {
                Some(&to) => (to, None),
                None => continue,
            },
        };

        // older unity versions include the extension
        let extension = match script.m_AssemblyName.ends_with(".dll") {
            true => ".dll",
            false => "",
        };
        let assembly = format!("{assembly}{extension}");

        if let Some(to) = class_name {
            let (namespace, class) = to.rsplit_once('.').unwrap_or(("", to));
            script.m_Name = class.to_owned();
            script.m_ClassName = class.to_owned();
            script.m_Namespace = namespace.to_owned();
        }
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
