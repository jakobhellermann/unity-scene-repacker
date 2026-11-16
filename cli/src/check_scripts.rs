use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Args;
use clap_complete::ArgValueCompleter;
use paris::{success, warn};
use rabex::tpk::TpkTypeTreeBlob;
use rabex::typetree::typetree_cache::sync::TypeTreeCache;
use rabex_env::Environment;
use rabex_env::resolver::GameFiles;
use unity_scene_repacker::script_check::{
    FieldDiff, ScriptIssue, ScriptReport, check_bundle_scripts,
};

use crate::{GameArgs, completion, resolve_game_dir};

#[derive(Args, Debug)]
pub struct CheckScriptsArgs {
    /// The repacked bundle
    bundle: PathBuf,
    /// The game the bundle was repacked from
    #[clap(flatten)]
    game: GameArgs,
    #[clap(flatten)]
    target: TargetGameArgs,
}

#[derive(Args, Debug)]
#[group(required = true, multiple = false)]
#[clap(next_help_heading = "Target game options")]
pub struct TargetGameArgs {
    /// Data directory of the game the bundle will be loaded in
    #[arg(long)]
    target_game_dir: Option<PathBuf>,
    /// App ID or search term for the steam game the bundle will be loaded in
    #[arg(long, add = ArgValueCompleter::new(completion::complete_steam_game))]
    target_steam_game: Option<String>,
}

pub fn run(args: CheckScriptsArgs) -> Result<()> {
    let source_dir = resolve_game_dir(args.game.game_dir, args.game.steam_game)?;
    let target_dir = resolve_game_dir(args.target.target_game_dir, args.target.target_steam_game)?;

    let open = |dir: &Path| -> Result<Environment> {
        Ok(Environment::new(
            GameFiles::probe(dir)?,
            TypeTreeCache::new(TpkTypeTreeBlob::embedded()),
        ))
    };
    let source = open(&source_dir)?;
    let target = open(&target_dir)?;

    let bundle = std::fs::read(&args.bundle)
        .with_context(|| format!("couldn't read bundle '{}'", args.bundle.display()))?;
    let reports = check_bundle_scripts(&source, &target, &bundle)?;

    let mut missing = Vec::new();
    let mut differ = Vec::new();
    let mut ok = 0;
    for report in &reports {
        match &report.issue {
            None => ok += 1,
            Some(ScriptIssue::FieldsDiffer(diffs)) => differ.push((report, diffs)),
            Some(issue) => missing.push((report, issue)),
        }
    }

    let script_name = |report: &ScriptReport| match &report.script {
        Some(script) => format!(
            "{} ({})",
            script.full_name,
            script.assembly.trim_end_matches(".dll")
        ),
        None => "<no script>".to_owned(),
    };
    let width = reports
        .iter()
        .map(|r| r.behaviours.to_string().len())
        .max()
        .unwrap_or(1);

    let color = std::io::stdout().is_terminal();
    let paint = |code: &str, text: &str| match color {
        true => format!("\x1b[{code}m{text}\x1b[0m"),
        false => text.to_owned(),
    };

    if !differ.is_empty() {
        warn!("<b>{} load with different fields</>", scripts(differ.len()));
        for (report, diffs) in &differ {
            println!("    {:>width$}x {}", report.behaviours, script_name(report));
            let mut lines = diffs
                .iter()
                .map(|diff| match diff {
                    FieldDiff::Removed { path, ty } => {
                        (0, "-", "31", field_path(path), short_type(ty))
                    }
                    FieldDiff::TypeChanged { path, from, to } => (
                        1,
                        "~",
                        "33",
                        field_path(path),
                        format!("{} -> {}", short_type(from), short_type(to)),
                    ),
                    FieldDiff::Added { path, ty } => {
                        (2, "+", "32", field_path(path), short_type(ty))
                    }
                })
                .collect::<Vec<_>>();
            lines.sort();
            let path_width = lines
                .iter()
                .map(|(_, _, _, path, _)| path.len())
                .max()
                .unwrap_or(0);
            for (_, sign, code, path, ty) in lines {
                println!(
                    "    {:width$}   {} {}",
                    "",
                    paint(code, &format!("{sign} {path:path_width$}")),
                    paint("2", &ty),
                );
            }
        }
        println!();
    }

    if !missing.is_empty() {
        report_error(&format!(
            "<b>{} missing in target</>",
            scripts(missing.len())
        ));
        for (report, issue) in &missing {
            let reason = match issue {
                ScriptIssue::NoScript => "",
                ScriptIssue::NotInSource => "not in the source game's assemblies",
                ScriptIssue::MissingAssembly => "assembly missing",
                ScriptIssue::MissingClass => "class missing",
                ScriptIssue::FieldsDiffer(_) => unreachable!(),
            };
            println!(
                "    {:>width$}x {}  {}",
                report.behaviours,
                script_name(report),
                paint("2", reason)
            );
        }
        println!();
    }

    if !missing.is_empty() {
        report_error(&format!(
            "{} of {} won't load in the target game, {} load with different fields",
            missing.len(),
            scripts(reports.len()),
            differ.len()
        ));
        std::process::exit(1);
    }
    if !differ.is_empty() {
        warn!(
            "All scripts exist in the target game, {} of {} with different fields",
            differ.len(),
            scripts(reports.len())
        );
        return Ok(());
    }
    success!(
        "All {} exist in the target game with the same fields",
        scripts(ok)
    );
    Ok(())
}

fn field_path(path: &str) -> String {
    path.replace(".Array.data", "[]")
}

fn short_type(ty: &str) -> String {
    ty.replace('$', "")
}

fn scripts(count: usize) -> String {
    match count {
        1 => "1 script".to_owned(),
        n => format!("{n} scripts"),
    }
}

/// Like `paris::error!`, but on stdout with the rest of the report
fn report_error(message: &str) {
    paris::output::format_stdout(format!("<red><cross></> {message}"), "\n");
}
