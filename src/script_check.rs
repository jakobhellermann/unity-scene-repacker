//! Check whether the scripts of a repacked bundle exist with the same fields in another game.
use std::collections::BTreeMap;
use std::io::{Cursor, ErrorKind};
use std::path::Path;

use anyhow::{Context, Result};
use rabex::files::SerializedFile;
use rabex::files::bundlefile::{BundleFileReader, ExtractionConfig};
use rabex::objects::{ClassId, PPtr};
use rabex::typetree::{TypeTreeNode, TypeTreeProvider as _};
use rabex_env::Environment;
use rabex_env::addressables::ArchivePath;
use rabex_env::resolver::EnvResolver as _;
use rabex_env::unity::types::MonoScript;
use rustc_hash::FxHashMap;

use crate::dependency_addressables::{self, ExternalPath};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScriptName {
    pub assembly: String,
    pub full_name: String,
}

#[derive(Debug)]
pub enum ScriptIssue {
    /// The MonoBehaviour has no script.
    NoScript,
    /// The source game can't produce a type tree for the script.
    NotInSource,
    MissingAssembly,
    MissingClass,
    FieldsDiffer(Vec<FieldDiff>),
}

#[derive(Debug)]
pub enum FieldDiff {
    Removed {
        path: String,
        ty: String,
    },
    Added {
        path: String,
        ty: String,
    },
    TypeChanged {
        path: String,
        from: String,
        to: String,
    },
}

#[derive(Debug)]
pub struct ScriptReport {
    pub script: Option<ScriptName>,
    pub behaviours: usize,
    pub issue: Option<ScriptIssue>,
}

/// Checks every script referenced by the MonoBehaviours in `bundle`, which was repacked from `source`, against `target`.
pub fn check_bundle_scripts(
    source: &Environment,
    target: &Environment,
    bundle: &[u8],
) -> Result<Vec<ScriptReport>> {
    let usages = collect_script_usages(source, bundle)?;

    let source_base = monobehaviour_base_fields(source)?;
    let target_base = monobehaviour_base_fields(target)?;

    let mut reports = usages
        .into_iter()
        .map(|(script, behaviours)| -> Result<_> {
            let issue = match &script {
                None => Some(ScriptIssue::NoScript),
                Some(script) => check_script(source, target, script, source_base, target_base)
                    .with_context(|| {
                        format!("checking {} ({})", script.full_name, script.assembly)
                    })?,
            };
            Ok(ScriptReport {
                script,
                behaviours,
                issue,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    reports.sort_by(|a, b| {
        b.behaviours
            .cmp(&a.behaviours)
            .then(a.script.cmp(&b.script))
    });

    Ok(reports)
}

fn collect_script_usages(
    source: &Environment,
    bundle: &[u8],
) -> Result<BTreeMap<Option<ScriptName>, usize>> {
    let bundle = BundleFileReader::from_reader(Cursor::new(bundle), &ExtractionConfig::default())?;

    let mut bundled_files = None;
    let mut usages = BTreeMap::new();
    for entry in bundle.serialized_files() {
        let data = bundle.read_entry(entry)?;
        let file = SerializedFile::from_reader(&mut Cursor::new(&data))
            .with_context(|| format!("reading {} in bundle", entry.path))?;

        let mut scripts = FxHashMap::<PPtr, Option<ScriptName>>::default();
        for info in file.objects() {
            if info.m_ClassID != ClassId::MonoBehaviour {
                continue;
            }
            let script = match file.script_type(info).and_then(PPtr::optional) {
                None => None,
                Some(pptr) => match scripts.get(&pptr) {
                    Some(script) => script.clone(),
                    None => {
                        let script = read_script(source, &file, pptr, &mut bundled_files)?;
                        scripts.insert(pptr, Some(script.clone()));
                        Some(script)
                    }
                },
            };
            *usages.entry(script).or_insert(0) += 1;
        }
    }
    Ok(usages)
}

fn read_script(
    source: &Environment,
    file: &SerializedFile,
    pptr: PPtr,
    bundled_files: &mut Option<FxHashMap<String, ExternalPath>>,
) -> Result<ScriptName> {
    let index = pptr
        .m_FileID
        .get_externals_index()
        .with_context(|| format!("script {pptr:?} is not in an external file"))?;
    let external = &file
        .m_Externals
        .get(index)
        .with_context(|| format!("script {pptr:?} points to a missing external"))?
        .pathName;

    let path = match ArchivePath::try_parse(Path::new(external))? {
        Some(archive) => {
            let bundled_files = match bundled_files {
                Some(files) => files,
                None => bundled_files.insert(dependency_addressables::bundled_game_files(source)?),
            };
            bundled_files
                .get(archive.bundle)
                .with_context(|| format!("{external} is not a bundled file of the source game"))?
        }
        None => external,
    };

    let script = source
        .load_serialized(path)?
        .object_at::<MonoScript>(pptr.m_PathID)?
        .read()
        .with_context(|| format!("reading script {} in {path}", pptr.m_PathID))?;
    Ok(ScriptName {
        assembly: script.assembly_name().into_owned(),
        full_name: script.full_name().into_owned(),
    })
}

fn check_script(
    source: &Environment,
    target: &Environment,
    script: &ScriptName,
    source_base: usize,
    target_base: usize,
) -> Result<Option<ScriptIssue>> {
    let Some(source_tt) = source.generate_typetree(&script.assembly, &script.full_name)? else {
        return Ok(Some(ScriptIssue::NotInSource));
    };

    match target
        .game_files
        .read_path(&Path::new("Managed").join(&script.assembly))
    {
        Ok(_) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Some(ScriptIssue::MissingAssembly)),
        Err(e) => return Err(e.into()),
    }
    let Some(target_tt) = target.generate_typetree(&script.assembly, &script.full_name)? else {
        return Ok(Some(ScriptIssue::MissingClass));
    };

    let source_fields = source_tt
        .children
        .get(source_base..)
        .context("source type tree is missing the MonoBehaviour fields")?;
    let target_fields = target_tt
        .children
        .get(target_base..)
        .context("target type tree is missing the MonoBehaviour fields")?;
    let mut diffs = Vec::new();
    diff_fields(source_fields, target_fields, "", &mut diffs);
    Ok((!diffs.is_empty()).then_some(ScriptIssue::FieldsDiffer(diffs)))
}

/// The number of leading fields generated type trees share with every MonoBehaviour
fn monobehaviour_base_fields(env: &Environment) -> Result<usize> {
    let node = env
        .tpk
        .get_typetree_node(ClassId::MonoBehaviour, env.unity_version()?)
        .context("no MonoBehaviour type tree for this unity version")?;
    Ok(node.children.len())
}

fn diff_fields(
    source: &[TypeTreeNode],
    target: &[TypeTreeNode],
    prefix: &str,
    diffs: &mut Vec<FieldDiff>,
) {
    for field in source {
        let path = format!("{prefix}{}", field.m_Name);
        match target.iter().find(|t| t.m_Name == field.m_Name) {
            None => diffs.push(FieldDiff::Removed {
                path,
                ty: field.m_Type.clone(),
            }),
            Some(t) if t.m_Type != field.m_Type => diffs.push(FieldDiff::TypeChanged {
                path,
                from: field.m_Type.clone(),
                to: t.m_Type.clone(),
            }),
            Some(t) => diff_fields(&field.children, &t.children, &format!("{path}."), diffs),
        }
    }
    for field in target {
        if !source.iter().any(|s| s.m_Name == field.m_Name) {
            diffs.push(FieldDiff::Added {
                path: format!("{prefix}{}", field.m_Name),
                ty: field.m_Type.clone(),
            });
        }
    }
}
