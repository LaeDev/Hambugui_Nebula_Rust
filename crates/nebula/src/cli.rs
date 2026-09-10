//! The command line: clap definitions plus thin handlers over `nebula_core`.
//!
//! Port of upstream `src/index.ts`, extended with NeoForge, the auto-detecting
//! modpack importer and the publish targets. Command names, aliases and the
//! messages users grep for are kept as upstream wrote them.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};
use log::LevelFilter;
use nebula_core::publish::{PublishOptions, Target, gdrive, webdav};
use nebula_core::server::FORGE_MOD_LIST_WARNING;
use nebula_core::{
    Config, CreateServerOptions, Error, GenerateOptions, MinecraftVersion, Promotion, Report,
    Result, Root, distribution, import, meta, publish, remote, server, validate,
};

/// Where Nebula keeps per-user state: the Google Drive refresh token, and the
/// GUI's `settings.json`. One definition, in the GUI's settings module — the
/// CLI had its own copy that ignored `NEBULA_CONFIG_DIR`, so a test run with
/// that variable set still wrote its token to the real config directory.
use crate::gui::config_dir;

#[derive(Parser)]
#[command(
    name = "nebula",
    bin_name = "nebula",
    version,
    about = "Distribution generator for the Helios and Hambugui launchers.",
    subcommand_negates_reqs = true
)]
struct Cli {
    /// Log debug messages.
    #[arg(short, long, global = true)]
    verbose: bool,
    /// Log warnings and errors only.
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    quiet: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Base init command.
    #[command(visible_alias = "i", subcommand)]
    Init(InitCommand),
    /// Base generate command.
    #[command(visible_alias = "g", subcommand)]
    Generate(GenerateCommand),
    /// Validate a distribution.json against the launcher contract.
    Validate {
        /// Distribution index file name.
        #[arg(default_value = "distribution")]
        name: String,
        /// Treat http:// urls and missing SHA-256 digests as errors.
        #[arg(long)]
        strict: bool,
        /// Validate this file instead of <ROOT>/<name>.json.
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Get the latest version of Forge.
    LatestForge { version: String },
    /// Get the recommended version of Forge. Returns latest if there is no recommended build.
    RecommendedForge { version: String },
    /// Get the latest version of the Fabric loader.
    LatestFabric,
    /// Get the recommended (stable) version of the Fabric loader.
    RecommendedFabric,
    /// Get the latest version of NeoForge.
    LatestNeoforge { version: String },
    /// Get the recommended (non-beta) version of NeoForge.
    RecommendedNeoforge { version: String },
    /// Upload everything a generated distribution references.
    Publish {
        /// Distribution index file name.
        #[arg(default_value = "distribution")]
        name: String,
        /// Where to upload.
        #[arg(long, value_enum)]
        target: PublishTargetKind,
        /// Upload every file, ignoring the change tracking in publish-state.json.
        #[arg(long)]
        all: bool,
    },
    /// Authorise Nebula against a Google account and store the refresh token.
    GdriveAuth,
    /// Launch the graphical interface.
    Gui,
}

#[derive(Subcommand)]
enum InitCommand {
    /// Generate an empty standard file structure.
    Root,
}

#[derive(Subcommand)]
enum GenerateCommand {
    /// Generate a new server configuration.
    Server {
        /// Server id.
        id: String,
        /// Minecraft version.
        version: String,
        /// Forge version, or latest/recommended.
        #[arg(long, conflicts_with_all = ["fabric", "neoforge"])]
        forge: Option<String>,
        /// Fabric loader version, or latest/recommended.
        #[arg(long, conflicts_with_all = ["forge", "neoforge"])]
        fabric: Option<String>,
        /// NeoForge version, or latest/recommended.
        #[arg(long, conflicts_with_all = ["forge", "fabric"])]
        neoforge: Option<String>,
    },
    /// Generate a new server configuration from a CurseForge, Modrinth or Prism modpack.
    ServerImport {
        /// Server id.
        id: String,
        /// Modpack file, by name below <ROOT>/modpacks or as a path.
        file: PathBuf,
    },
    /// Generate a new server configuration from a CurseForge modpack.
    ServerCurseforge {
        /// Server id.
        id: String,
        /// The name of the modpack zip file.
        zip_name: PathBuf,
    },
    /// Generate a distribution index from the root file structure.
    Distro {
        /// Distribution index file name.
        #[arg(default_value = "distribution")]
        name: String,
        /// Install the generated distribution to your local Helios data folder.
        #[arg(long, visible_alias = "installLocal")]
        install_local: bool,
        /// Delete cached output after it is no longer required.
        #[arg(long, visible_alias = "discardOutput")]
        discard_output: bool,
        /// Invalidate and delete existing caches as they are encountered.
        #[arg(long, visible_alias = "invalidateCache")]
        invalidate_cache: bool,
        /// Emit untracked files without digests, as upstream Nebula did. Helios only.
        #[arg(long)]
        helios_untracked: bool,
    },
    /// Generate json schemas.
    Schemas,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum PublishTargetKind {
    Webdav,
    Gdrive,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let level = log_level(cli.verbose, cli.quiet);
    match cli.command {
        // The GUI installs its own `log::Log`, and only one may be set per
        // process, so env_logger stays out of its way. It still takes the same
        // verdict `-v`/`-q` gives the CLI.
        None | Some(Command::Gui) => crate::gui::run(level),
        Some(command) => {
            init_logger(level);
            dispatch(command)
        }
    }
}

fn log_level(verbose: bool, quiet: bool) -> LevelFilter {
    match (verbose, quiet) {
        (true, _) => LevelFilter::Debug,
        (_, true) => LevelFilter::Warn,
        _ => LevelFilter::Info,
    }
}

fn init_logger(level: LevelFilter) {
    env_logger::Builder::new()
        .filter_level(level)
        // RUST_LOG still wins, which is how you silence one noisy module.
        .parse_default_env()
        .format(|buf, record| {
            let style = buf.default_level_style(record.level());
            writeln!(
                buf,
                "[{style}{}{style:#}] [{}] {}",
                record.level(),
                record.target(),
                record.args()
            )
        })
        .init();
}

fn dispatch(command: Command) -> Result<()> {
    match command {
        Command::Init(InitCommand::Root) => init_root(),
        Command::Generate(command) => generate(command),
        Command::Validate { name, strict, file } => validate_command(&name, strict, file),
        Command::LatestForge { version } => forge_promotion(&version, Promotion::Latest),
        Command::RecommendedForge { version } => forge_promotion(&version, Promotion::Recommended),
        Command::LatestFabric => fabric_promotion(Promotion::Latest),
        Command::RecommendedFabric => fabric_promotion(Promotion::Recommended),
        Command::LatestNeoforge { version } => neoforge_promotion(&version, Promotion::Latest),
        Command::RecommendedNeoforge { version } => {
            neoforge_promotion(&version, Promotion::Recommended)
        }
        Command::Publish { name, target, all } => publish_command(&name, target, all),
        Command::GdriveAuth => gdrive::authorize(
            &gdrive::config_from_env(&config_dir())?,
            &gdrive::default_open_browser,
        ),
        // `run` routes the GUI before dispatch, so it can hand it the log
        // level; reaching it here would mean that stopped being true.
        Command::Gui => unreachable!("the GUI is dispatched by `run`"),
    }
}

/// `Config` plus the `Root` every handler builds from it.
fn root_from_env() -> Result<(Config, Root)> {
    let cfg = Config::from_env()?;
    let root = Root::new(&cfg.root, cfg.base_url.clone());
    log::debug!("Root set to {}", root.path.display());
    log::debug!("Base Url set to {}", root.base_url);
    Ok((cfg, root))
}

fn init_root() -> Result<()> {
    let (_, root) = root_from_env()?;
    log::debug!("Invoked init root.");
    // `init_root` logs "Successfully created new root at <root>" itself.
    distribution::init_root(&root)
}

fn generate(command: GenerateCommand) -> Result<()> {
    match command {
        GenerateCommand::Server {
            id,
            version,
            forge,
            fabric,
            neoforge,
        } => generate_server(&id, &version, forge, fabric, neoforge),
        GenerateCommand::ServerImport { id, file } => import_modpack(&id, &file, None),
        GenerateCommand::ServerCurseforge { id, zip_name } => {
            import_modpack(&id, &zip_name, Some(import::ImportFormat::CurseForge))
        }
        GenerateCommand::Distro {
            name,
            install_local,
            discard_output,
            invalidate_cache,
            helios_untracked,
        } => generate_distro(GenerateOptions {
            name,
            install_local,
            discard_output,
            invalidate_cache,
            helios_untracked,
            gui_fallback: installer_gui_fallback(),
        }),
        GenerateCommand::Schemas => {
            let (_, root) = root_from_env()?;
            log::debug!("Invoked generate schemas.");
            meta::generate_schemas(&root)?;
            log::info!("Successfully generated schemas");
            Ok(())
        }
    }
}

fn generate_server(
    id: &str,
    version: &str,
    forge: Option<String>,
    fabric: Option<String>,
    neoforge: Option<String>,
) -> Result<()> {
    let (_, root) = root_from_env()?;
    let mc: MinecraftVersion = version.parse()?;
    log::debug!("Generating server {id} for Minecraft {mc}.");

    let forge = promoted(forge, "Forge", |promotion| {
        remote::get_promoted_forge_version(&mc, promotion)
    })?;
    if forge.is_some() && mc >= MinecraftVersion::new(1, 20, Some(3)) {
        for line in FORGE_MOD_LIST_WARNING.lines() {
            log::error!(target: "ForgeSupport", "{line}");
        }
    }
    let fabric = promoted(fabric, "Fabric", remote::get_promoted_fabric_version)?;
    let neoforge = promoted(neoforge, "NeoForge", |promotion| {
        remote::get_promoted_neoforge_version(&mc, promotion)
    })?;

    server::create_server(
        &root,
        id,
        &mc,
        &CreateServerOptions {
            version: None,
            forge_version: forge,
            fabric_version: fabric,
            neoforge_version: neoforge,
        },
    )?;
    Ok(())
}

/// Whether a failed headless Forge/NeoForge install may re-run the installer
/// with its window open. Never in CI: `NEBULA_FORGE_GUI_FALLBACK=0` disables it
/// outright, and otherwise somebody has to be sitting at the terminal to see it.
fn installer_gui_fallback() -> bool {
    std::env::var("NEBULA_FORGE_GUI_FALLBACK").as_deref() != Ok("0")
        && std::io::stdin().is_terminal()
}

/// `latest` and `recommended` are looked up; anything else is a literal version.
fn promoted(
    raw: Option<String>,
    loader: &str,
    resolve: impl FnOnce(Promotion) -> Result<String>,
) -> Result<Option<String>> {
    let Some(raw) = raw else { return Ok(None) };
    let Ok(promotion) = raw.parse::<Promotion>() else {
        return Ok(Some(raw));
    };
    log::debug!("Resolving {raw} {loader} Version..");
    let version = resolve(promotion)?;
    log::debug!("{loader} version set to {version}");
    Ok(Some(version))
}

/// `expected` is set by `server-curseforge`, which only accepts that one format.
fn import_modpack(id: &str, file: &Path, expected: Option<import::ImportFormat>) -> Result<()> {
    let (_, root) = root_from_env()?;
    log::debug!(
        "Generating server {id} using modpack {} as a template.",
        file.display()
    );

    let path = import::resolve_input(&root, file)?;
    if let Some(expected) = expected {
        let format = import::detect_format(&path)?;
        if format != expected {
            return Err(Error::Invalid(format!(
                "{} is not a CurseForge modpack; use server-import",
                path.display()
            )));
        }
    }

    let report = import::import_modpack(&root, id, &path, &import::default_download)?;
    report_manual_downloads(&report.manual);
    log::info!(
        "{} modpack: {} downloaded, {} copied, {} skipped, {} manual.",
        report.format,
        report.downloaded,
        report.copied,
        report.skipped_unsupported,
        report.manual.len()
    );
    Ok(())
}

/// The upstream banner. It only makes sense on a terminal that scrolls, so it
/// lives here and not in `import_modpack`, which logs one line the GUI renders
/// as its own box. Written straight to stderr: no level prefix, no ANSI, and
/// `--quiet` cannot hide a list the user has to act on.
fn report_manual_downloads(manual: &[import::ManualDownload]) {
    if manual.is_empty() {
        return;
    }
    let rule = "============================================";
    let mut out = std::io::stderr().lock();
    let _ = writeln!(
        out,
        "{rule}
WARNING"
    );
    let _ = writeln!(
        out,
        "{} files declared by this modpack do not permit 3rd-party downloads.",
        manual.len()
    );
    let _ = writeln!(out, "They MUST be downloaded manually.");
    let _ = writeln!(
        out,
        "The mods and their download urls will be listed below."
    );
    let _ = writeln!(out, "{rule}");
    for file in manual {
        let _ = writeln!(out, "{} ({}) - {}", file.name, file.file_name, file.url);
    }
    let _ = writeln!(out, "{rule}");
    let _ = writeln!(out, "YOUR MODPACK IS NOT FULLY GENERATED!");
    let _ = writeln!(
        out,
        "MANUAL ACTION REQUIRED! SCROLL UP AND READ THE WARNING!"
    );
    let _ = writeln!(out, "{rule}");
}

fn generate_distro(opts: GenerateOptions) -> Result<()> {
    let (cfg, _) = root_from_env()?;
    log::debug!("Install option set to {}", opts.install_local);
    log::debug!("Discard Output option set to {}", opts.discard_output);
    log::debug!("Invalidate Cache option set to {}", opts.invalidate_cache);
    log::debug!("Invoked generate distro name {}.json.", opts.name);

    let result = distribution::generate_distribution(&cfg, &opts)?;
    if result.installed_to.is_some() {
        log::info!("Success!");
    }

    // Advisory: the file is already written, so a finding is a heads-up, not a
    // failure. `nebula validate --strict` is the gate.
    print_report(&validate::validate(&result.distribution, false)?);
    Ok(())
}

fn validate_command(name: &str, strict: bool, file: Option<PathBuf>) -> Result<()> {
    let path = match file {
        Some(path) => path,
        None => root_from_env()?.1.path.join(format!("{name}.json")),
    };
    log::debug!("Invoked validate with {}", path.display());

    let bytes = std::fs::read(&path).map_err(|source| Error::Io {
        context: format!("reading {}", path.display()),
        source,
    })?;
    let report = validate::validate_json(&bytes, strict)?;
    print_report(&report);

    match report.is_ok() {
        true => Ok(()),
        false => Err(Error::Invalid(format!(
            "{} does not satisfy the launcher contract ({} errors)",
            path.display(),
            report.errors()
        ))),
    }
}

/// One line per finding, then the counts. Never decides the exit code: the
/// caller knows whether findings are fatal.
fn print_report(report: &Report) {
    for finding in &report.findings {
        println!(
            "{} {} {}",
            finding.severity, finding.location, finding.message
        );
    }
    println!(
        "{} artifacts, {} with SHA-256, {} errors, {} warnings",
        report.artifacts,
        report.sha256,
        report.errors(),
        report.warnings()
    );
}

fn forge_promotion(version: &str, promotion: Promotion) -> Result<()> {
    let mc: MinecraftVersion = version.parse()?;
    log::debug!("Invoked {promotion}-forge with version {mc}.");
    let forge = remote::get_promoted_forge_version(&mc, promotion)?;
    match promotion {
        Promotion::Latest => log::info!("Latest version: Forge {forge} ({mc})"),
        Promotion::Recommended => log::info!("Recommended version: Forge {forge} ({mc})"),
    }
    Ok(())
}

fn fabric_promotion(promotion: Promotion) -> Result<()> {
    log::debug!("Invoked {promotion}-fabric.");
    let fabric = remote::get_promoted_fabric_version(promotion)?;
    match promotion {
        Promotion::Latest => log::info!("Latest version: Fabric {fabric}"),
        Promotion::Recommended => log::info!("Recommended version: Fabric {fabric}"),
    }
    Ok(())
}

fn neoforge_promotion(version: &str, promotion: Promotion) -> Result<()> {
    let mc: MinecraftVersion = version.parse()?;
    log::debug!("Invoked {promotion}-neoforge with version {mc}.");
    let neoforge = remote::get_promoted_neoforge_version(&mc, promotion)?;
    match promotion {
        Promotion::Latest => log::info!("Latest version: NeoForge {neoforge} ({mc})"),
        Promotion::Recommended => log::info!("Recommended version: NeoForge {neoforge} ({mc})"),
    }
    Ok(())
}

fn publish_command(name: &str, kind: PublishTargetKind, all: bool) -> Result<()> {
    let (_, root) = root_from_env()?;
    let path = root.path.join(format!("{name}.json"));
    let plan = publish::plan_from_distribution(&root, &path)?;

    let mut target: Box<dyn Target + Send> = match kind {
        PublishTargetKind::Webdav => Box::new(webdav::WebDavTarget::from_env(&root.base_url)?),
        PublishTargetKind::Gdrive => Box::new(gdrive::GoogleDriveTarget::new(
            gdrive::config_from_env(&config_dir())?,
        )?),
    };
    let opts = PublishOptions {
        only_changed: !all,
        // Drive hands back its own urls, so the uploaded copy must carry them.
        rewrite_urls: kind == PublishTargetKind::Gdrive,
    };

    let report = publish::publish(&root, &plan, target.as_mut(), &opts)?;
    println!(
        "uploaded {}, skipped {}, distribution URL: {}",
        report.uploaded, report.skipped, report.distribution_url
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn command_tree_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn loaders_are_mutually_exclusive() {
        let both = Cli::try_parse_from([
            "nebula", "g", "server", "T", "1.20.1", "--forge", "1", "--fabric", "2",
        ]);
        assert!(both.is_err());
        let one = Cli::try_parse_from([
            "nebula",
            "g",
            "server",
            "T",
            "1.20.1",
            "--neoforge",
            "recommended",
        ]);
        assert!(one.is_ok());
    }

    #[test]
    fn camel_case_flags_still_parse() {
        assert!(Cli::try_parse_from(["nebula", "g", "distro", "--installLocal"]).is_ok());
        assert!(Cli::try_parse_from(["nebula", "g", "distro", "--invalidateCache"]).is_ok());
    }

    #[test]
    fn verbosity_flags_pick_the_log_level() {
        // The GUI takes the same verdict, so `nebula gui -v` fills its log band
        // with the debug lines the CLI would have printed.
        assert_eq!(log_level(true, false), LevelFilter::Debug);
        assert_eq!(log_level(false, true), LevelFilter::Warn);
        assert_eq!(log_level(false, false), LevelFilter::Info);
    }
}
