//! The command line: clap definitions plus thin handlers over `nebula_core`.
//!
//! Port of upstream `src/index.ts`, extended with NeoForge, the auto-detecting
//! modpack importer and the publish targets. Command names, aliases and the
//! messages users grep for are kept as upstream wrote them.

use std::collections::HashMap;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};
use log::LevelFilter;
use nebula_core::catalog::index::InstalledMod;
use nebula_core::catalog::{
    self, Catalog, Endpoints, InstallChoice, InstallPlan, InstallReport, ManualDownload,
    PlannedFile, ProviderId, ReleaseType, SearchQuery, SortKey,
};
use nebula_core::publish::{PublishOptions, Target, gdrive, webdav};
use nebula_core::scan::Namespace;
use nebula_core::server::FORGE_MOD_LIST_WARNING;
use nebula_core::{
    Config, CreateServerOptions, Error, GenerateOptions, MinecraftVersion, Promotion, Report,
    Result, Root, ServerDir, distribution, import, meta, publish, remote, server, validate,
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
    /// Search, install, update, move and remove the mods of one server.
    Mods {
        /// Server directory name below <ROOT>/servers, ex. Test-1.20.1.
        server: String,
        #[command(subcommand)]
        command: ModsCommand,
    },
    /// Base server command.
    #[command(subcommand)]
    Server(ServerCommand),
    /// Launch the graphical interface.
    Gui,
}

#[derive(Subcommand)]
enum ModsCommand {
    /// Search a provider for mods that fit this server's Minecraft version and loader.
    Search {
        /// Search text.
        query: String,
        /// Which provider to search.
        #[arg(long, value_enum, default_value_t = ProviderArg::Modrinth)]
        provider: ProviderArg,
        /// Result ordering.
        #[arg(long, value_enum, default_value_t = SortArg::Relevance)]
        sort: SortArg,
        /// How many results to show.
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// Skip this many results.
        #[arg(long, default_value_t = 0)]
        offset: u32,
    },
    /// Install a mod and the dependencies it requires.
    Install {
        /// Project id or slug.
        project: String,
        /// Install this exact version id instead of the newest suitable one.
        #[arg(long)]
        version: Option<String>,
        /// Which namespace directory the files land in.
        #[arg(long, value_enum, default_value_t = NamespaceArg::Required)]
        namespace: NamespaceArg,
        /// Install the mod alone. A missing required dependency can stop the game from starting.
        #[arg(long)]
        no_deps: bool,
        /// Which provider to install from.
        #[arg(long, value_enum, default_value_t = ProviderArg::Modrinth)]
        provider: ProviderArg,
    },
    /// List every mod on this server, managed or hand-placed.
    List,
    /// Check every managed mod for a newer version.
    Update {
        /// Install the updates instead of only listing them.
        #[arg(long)]
        apply: bool,
    },
    /// Delete one mod file and its index entry.
    Remove {
        /// Mod file name, ex. sodium-fabric-0.5.8.jar.
        file: String,
        /// Required only when another installed mod needs this one.
        #[arg(long)]
        yes: bool,
    },
    /// Move one mod between namespaces, which is how it is enabled or disabled.
    Move {
        /// Mod file name.
        file: String,
        /// Target namespace.
        #[arg(value_enum)]
        namespace: NamespaceArg,
    },
}

#[derive(Subcommand)]
enum ServerCommand {
    /// Delete a server directory and everything in it.
    Delete {
        /// Server directory name below <ROOT>/servers.
        dir: String,
        /// Required. Without it the command prints what would be deleted and fails.
        #[arg(long)]
        yes: bool,
    },
    /// Copy a server directory under a new server id.
    Duplicate {
        /// Server directory name below <ROOT>/servers.
        dir: String,
        /// The new server id; the Minecraft version suffix is kept.
        new_id: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum ProviderArg {
    Modrinth,
    Curseforge,
}

impl From<ProviderArg> for ProviderId {
    fn from(value: ProviderArg) -> Self {
        match value {
            ProviderArg::Modrinth => ProviderId::Modrinth,
            ProviderArg::Curseforge => ProviderId::CurseForge,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum SortArg {
    Relevance,
    Downloads,
    Newest,
}

impl From<SortArg> for SortKey {
    fn from(value: SortArg) -> Self {
        match value {
            SortArg::Relevance => SortKey::Relevance,
            SortArg::Downloads => SortKey::Downloads,
            SortArg::Newest => SortKey::Newest,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum NamespaceArg {
    Required,
    #[value(name = "optionalon")]
    OptionalOn,
    #[value(name = "optionaloff")]
    OptionalOff,
}

impl From<NamespaceArg> for Namespace {
    fn from(value: NamespaceArg) -> Self {
        match value {
            NamespaceArg::Required => Namespace::Required,
            NamespaceArg::OptionalOn => Namespace::OptionalOn,
            NamespaceArg::OptionalOff => Namespace::OptionalOff,
        }
    }
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
        Command::Mods { server, command } => {
            let (cfg, root) = root_from_env()?;
            mods_command(&cfg, &root, &server, command)
        }
        Command::Server(command) => {
            let (_, root) = root_from_env()?;
            server_command(&root, command)
        }
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

// ---------------------------------------------------------------------------
// Mod catalog
// ---------------------------------------------------------------------------

/// The mod subcommands. Split from `dispatch` so the tests can drive it
/// against a temporary root without touching the process environment.
fn mods_command(cfg: &Config, root: &Root, server: &str, command: ModsCommand) -> Result<()> {
    let dir = find_server(root, server)?;
    let catalog = Catalog::new(Endpoints::default(), cfg.curseforge_api_key.clone());
    match command {
        ModsCommand::Search {
            query,
            provider,
            sort,
            limit,
            offset,
        } => mods_search(&catalog, &dir, &query, provider, sort, limit, offset),
        ModsCommand::Install {
            project,
            version,
            namespace,
            no_deps,
            provider,
        } => mods_install(
            &catalog,
            &dir,
            &project,
            version.as_deref(),
            namespace.into(),
            no_deps,
            provider.into(),
        ),
        ModsCommand::List => mods_list(&dir),
        ModsCommand::Update { apply } => mods_update(&catalog, &dir, apply),
        ModsCommand::Remove { file, yes } => mods_remove(&dir, &file, yes),
        ModsCommand::Move { file, namespace } => {
            let namespace: Namespace = namespace.into();
            catalog::manage::move_mod(&dir, &file, namespace)?;
            println!("{file}을(를) {}(으)로 옮겼습니다", namespace.dir_name());
            Ok(())
        }
    }
}

/// The server directory, or an error that names the ones that do exist.
fn find_server(root: &Root, dir_name: &str) -> Result<ServerDir> {
    let dir = ServerDir::new(root.clone(), dir_name);
    if dir.path.is_dir() {
        return Ok(dir);
    }
    let names = catalog::manage::server_names(root);
    let known = match names.is_empty() {
        true => "없음".to_owned(),
        false => names.join(", "),
    };
    Err(Error::Invalid(format!(
        "서버 폴더 {dir_name}을(를) 찾을 수 없습니다 — 있는 서버: {known}"
    )))
}

#[allow(clippy::too_many_arguments)]
fn mods_search(
    catalog: &Catalog,
    dir: &ServerDir,
    query: &str,
    provider: ProviderArg,
    sort: SortArg,
    limit: u32,
    offset: u32,
) -> Result<()> {
    let ctx = catalog::server_context(dir)?;
    let provider: ProviderId = provider.into();
    let page = catalog::install::require_provider(catalog, provider)?.search(&SearchQuery {
        query: query.to_owned(),
        sort: sort.into(),
        limit,
        offset,
        mc: ctx.mc,
        loader: ctx.loader,
    })?;
    println!(
        "{provider} · 검색 결과 {}개 중 {}개 (Minecraft {} · {:?})",
        page.total,
        page.hits.len(),
        ctx.mc,
        ctx.loader
    );
    for hit in &page.hits {
        let author = hit.author.as_deref().unwrap_or("작성자 미상");
        println!(
            "  {}  {} ({}) · 다운로드 {} · {author}",
            hit.project_id, hit.title, hit.slug, hit.downloads
        );
        // One line per result: a provider description is free text and a
        // multi-paragraph one would run into the next row (§8.4).
        let summary = hit.description.lines().next().unwrap_or_default().trim();
        if !summary.is_empty() {
            println!("      {summary}");
        }
    }
    Ok(())
}

fn mods_install(
    catalog: &Catalog,
    dir: &ServerDir,
    project: &str,
    version: Option<&str>,
    namespace: Namespace,
    no_deps: bool,
    provider: ProviderId,
) -> Result<()> {
    let ctx = catalog::server_context(dir)?;
    let plan = catalog::install::plan_install(catalog, dir, provider, project, version)?;
    print_plan(&plan);

    if !plan.incompatible.is_empty() {
        return Err(Error::Invalid(format!(
            "{}와(과) 함께 설치할 수 없는 모드가 있습니다: {}",
            plan.root.title,
            plan.incompatible.join(", ")
        )));
    }
    if !plan.unresolvable.is_empty() {
        return Err(Error::Invalid(format!(
            "의존성을 찾을 수 없습니다: {} — 설치를 중단했습니다",
            plan.unresolvable.join(", ")
        )));
    }

    // Required dependencies are kept unless `--no-deps` says otherwise; a
    // dropped required dependency is what D6 exists to prevent, so the risk
    // is spelled out at the moment it is taken (§8.4), not only in --help.
    if no_deps && !plan.required.is_empty() {
        let omitted: Vec<&str> = plan
            .required
            .iter()
            .map(|dep| dep.file.title.as_str())
            .collect();
        let mut out = std::io::stderr().lock();
        let _ = writeln!(out, "필수 의존성을 빼고 설치합니다: {}", omitted.join(", "));
        let _ = writeln!(out, "필수 의존성을 빼면 게임이 시작되지 않을 수 있습니다");
    }
    let choice = InstallChoice {
        deps: match no_deps {
            true => Default::default(),
            false => plan
                .required
                .iter()
                .map(|dep| dep.file.project_id.clone())
                .collect(),
        },
    };
    let report = catalog::install::apply(catalog, dir, &plan, &choice, namespace, &progress_line)?;
    print_manual(&report.manual, &ctx.container.join(namespace.dir_name()));
    print_report_counts(&report);
    Ok(())
}

/// Deleting a mod another one requires is the one §8.3 case that needs a
/// confirmation, so `--yes` is demanded only then.
fn mods_remove(dir: &ServerDir, file: &str, yes: bool) -> Result<()> {
    let index = catalog::index::ModIndex::load(dir)?;
    let dependents = catalog::manage::dependents_of(&index, file);
    if !dependents.is_empty() {
        println!("{file}을(를) 필요로 하는 모드: {}", dependents.join(", "));
        if !yes {
            return Err(Error::Invalid(format!(
                "{file}을(를) 지우지 않았습니다 — 정말 지우려면 --yes를 붙이세요"
            )));
        }
    }
    catalog::manage::remove_mod(dir, file)?;
    println!("{file}을(를) 삭제했습니다");
    Ok(())
}

fn mods_list(dir: &ServerDir) -> Result<()> {
    let rows = catalog::manage::list_installed(dir)?;
    println!("설치된 모드 {}개", rows.len());
    for row in &rows {
        println!(
            "  {}  {}  {}{}",
            row.file_name,
            row.title(),
            row.namespace.dir_name(),
            markers(row)
        );
        if let Some(reason) = &row.unshippable {
            println!(
                "      배포본을 만들 수 없는 파일입니다 ({reason}) — 지우거나 올바른 jar로 바꾸세요"
            );
        }
    }
    Ok(())
}

/// The same markers §8.3 draws as chips.
fn markers(row: &InstalledMod) -> String {
    let mut marks = Vec::new();
    if row.missing {
        marks.push("파일 없음");
    }
    if row.unshippable.is_some() {
        marks.push("배포 불가");
    }
    if row.is_unmanaged() {
        marks.push("직접 추가");
    }
    match marks.is_empty() {
        true => String::new(),
        false => format!("  {}", marks.join(" · ")),
    }
}

/// The human version for an installed row: the jar's own metadata first,
/// then the index's `version_id`, else "unknown" — same ladder as the GUI's
/// `installed_version` (crates/nebula/src/gui/mods.rs), duplicated here
/// because that one is private to the GUI module.
fn installed_version(row: &InstalledMod) -> String {
    row.local
        .as_ref()
        .map(|local| local.version.clone())
        .filter(|version| !version.trim().is_empty())
        .or_else(|| row.entry.as_ref().map(|entry| entry.version_id.clone()))
        .unwrap_or_else(|| "버전 미상".to_owned())
}

fn mods_update(catalog: &Catalog, dir: &ServerDir, apply: bool) -> Result<()> {
    let report = catalog::update::check_updates(catalog, dir)?;
    let candidates = report.candidates;
    if !report.adopted.is_empty() {
        println!(
            "직접 추가 {}개를 관리 목록에 연결했습니다: {}",
            report.adopted.len(),
            report.adopted.join(", ")
        );
    }
    if candidates.is_empty() {
        println!("모두 최신입니다");
        return Ok(());
    }
    println!("업데이트 {}개", candidates.len());
    let installed = catalog::manage::list_installed(dir)?;
    let installed_versions: HashMap<&str, String> = installed
        .iter()
        .map(|row| (row.file_name.as_str(), installed_version(row)))
        .collect();
    for candidate in &candidates {
        let from = installed_versions
            .get(candidate.file_name.as_str())
            .cloned()
            .unwrap_or_else(|| candidate.from.clone());
        println!(
            "  {} : {} → {} ({})",
            candidate.file_name,
            from,
            candidate.to.number,
            release_label(candidate.to.release)
        );
    }
    if !apply {
        println!("--apply를 붙이면 설치합니다");
        return Ok(());
    }

    let results = catalog::update::apply_updates(catalog, dir, &candidates, &progress_line)?;
    let failed: Vec<(&String, String)> = results
        .iter()
        .filter_map(|(name, outcome)| outcome.as_ref().err().map(|e| (name, e.to_string())))
        .collect();
    println!(
        "업데이트 {} · 실패 {}",
        results.len() - failed.len(),
        failed.len()
    );
    for (name, message) in &failed {
        println!("  실패 {name}: {message}");
    }
    match failed.is_empty() {
        true => Ok(()),
        false => Err(Error::Invalid(format!(
            "모드 {}개를 업데이트하지 못했습니다",
            failed.len()
        ))),
    }
}

fn print_plan(plan: &InstallPlan) {
    println!("{}", plan_line(&plan.root));
    for dep in &plan.required {
        println!("  필수 의존성 {}{}", dep.file.title, installed_suffix(dep));
    }
    for dep in &plan.optional {
        println!("  선택 의존성 {}{}", dep.file.title, installed_suffix(dep));
    }
}

/// §8.4's confirm line: `{title} {number} · {정식|베타|알파} · {file} · {date}`,
/// matching the GUI. `version_number` is CurseForge's real file number;
/// `version_name` is a free-text title that usually just repeats the mod
/// name, so it is only a fallback for a provider that leaves the number
/// empty. The date is dropped when the provider didn't send one.
fn plan_line(file: &PlannedFile) -> String {
    let number = if file.version_number.trim().is_empty() {
        &file.version_name
    } else {
        &file.version_number
    };
    let mut line = format!(
        "{} {} · {} · {}",
        file.title,
        number,
        release_label(file.release),
        file.file.file_name
    );
    if !file.published.trim().is_empty() {
        let date = file.published.get(..10).unwrap_or(&file.published);
        line.push_str(" · ");
        line.push_str(date);
    }
    line
}

fn installed_suffix(dep: &nebula_core::catalog::PlannedDep) -> String {
    match &dep.already_installed {
        Some(file) => format!(" (설치됨: {file})"),
        None => String::new(),
    }
}

fn print_report_counts(report: &InstallReport) {
    println!(
        "설치 {} · 건너뜀 {} · 직접 다운로드 {}",
        report.installed.len(),
        report.skipped_already_installed.len(),
        report.manual.len()
    );
}

/// A blocked CurseForge file is not a failure; it is a list of links plus the
/// folder the user drops the jar into (§8.4).
fn print_manual(manual: &[ManualDownload], target: &Path) {
    if manual.is_empty() {
        return;
    }
    let mut out = std::io::stderr().lock();
    let _ = writeln!(out, "직접 내려받아야 하는 파일 {}개", manual.len());
    for file in manual {
        let _ = writeln!(out, "  {} ({}) - {}", file.name, file.file_name, file.url);
    }
    let _ = writeln!(out, "  받은 파일을 여기에 넣으세요: {}", target.display());
    let _ = writeln!(
        out,
        "  파일 이름은 .jar로 끝나야 하며, 완료되지 않은 다운로드(.crdownload 등)는 \
         배포본 생성을 막습니다"
    );
}

/// Progress goes to stderr as plain lines, so `nebula mods … list > out.txt`
/// keeps its output clean and `--quiet` cannot hide a running download.
fn progress_line(index: usize, total: usize, file: &str) {
    let _ = writeln!(
        std::io::stderr().lock(),
        "다운로드 중 {index}/{total} · {file}"
    );
}

fn release_label(release: ReleaseType) -> &'static str {
    match release {
        ReleaseType::Release => "정식",
        ReleaseType::Beta => "베타",
        ReleaseType::Alpha => "알파",
    }
}

// ---------------------------------------------------------------------------
// Server management
// ---------------------------------------------------------------------------

fn server_command(root: &Root, command: ServerCommand) -> Result<()> {
    match command {
        ServerCommand::Delete { dir, yes } => {
            let server = find_server(root, &dir)?;
            if !yes {
                println!("이 폴더가 영구 삭제됩니다: {}", server.path.display());
                println!("배포본을 다시 생성하면 이 서버가 사라집니다.");
                return Err(Error::Invalid(format!(
                    "{dir}을(를) 지우지 않았습니다 — 정말 지우려면 --yes를 붙이세요"
                )));
            }
            catalog::manage::delete_server(root, &dir)?;
            println!("서버 {dir}을(를) 삭제했습니다");
            Ok(())
        }
        ServerCommand::Duplicate { dir, new_id } => {
            let new_dir = catalog::manage::duplicate_server(root, &dir, &new_id)?;
            println!("서버 {dir}을(를) {new_dir}(으)로 복제했습니다");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn command_tree_is_well_formed() {
        Cli::command().debug_assert();
    }

    fn sample_planned_file() -> PlannedFile {
        PlannedFile {
            provider: ProviderId::CurseForge,
            project_id: "1".to_owned(),
            slug: Some("sodium".to_owned()),
            title: "Sodium".to_owned(),
            text_provider: ProviderId::CurseForge,
            version_id: "42".to_owned(),
            version_name: "Sodium for 1.20.1".to_owned(),
            version_number: "0.5.8".to_owned(),
            published: "2024-03-01T12:34:56Z".to_owned(),
            release: ReleaseType::Release,
            file: nebula_core::catalog::VersionFile {
                file_name: "sodium-0.5.8.jar".to_owned(),
                url: Some("https://example.invalid/sodium.jar".to_owned()),
                size: 123,
                primary: true,
                sha1: None,
                sha512: None,
                md5: None,
                fingerprint: None,
                file_id: Some("42".to_owned()),
            },
        }
    }

    #[test]
    fn plan_line_prefers_version_number_and_appends_date() {
        let file = sample_planned_file();
        assert_eq!(
            plan_line(&file),
            "Sodium 0.5.8 · 정식 · sodium-0.5.8.jar · 2024-03-01"
        );
    }

    #[test]
    fn plan_line_falls_back_to_version_name_when_number_missing() {
        let mut file = sample_planned_file();
        file.version_number.clear();
        assert_eq!(
            plan_line(&file),
            "Sodium Sodium for 1.20.1 · 정식 · sodium-0.5.8.jar · 2024-03-01"
        );
    }

    #[test]
    fn plan_line_omits_date_when_published_missing() {
        let mut file = sample_planned_file();
        file.published.clear();
        assert_eq!(plan_line(&file), "Sodium 0.5.8 · 정식 · sodium-0.5.8.jar");
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
    fn every_mod_and_server_subcommand_parses() {
        for args in [
            vec!["nebula", "mods", "Test-1.20.1", "search", "sodium"],
            vec![
                "nebula",
                "mods",
                "Test-1.20.1",
                "search",
                "sodium",
                "--provider",
                "curseforge",
                "--sort",
                "downloads",
                "--limit",
                "5",
                "--offset",
                "10",
            ],
            vec!["nebula", "mods", "Test-1.20.1", "install", "sodium"],
            vec![
                "nebula",
                "mods",
                "Test-1.20.1",
                "install",
                "sodium",
                "--version",
                "abc123",
                "--namespace",
                "optionaloff",
                "--no-deps",
                "--provider",
                "curseforge",
            ],
            vec!["nebula", "mods", "Test-1.20.1", "list"],
            vec!["nebula", "mods", "Test-1.20.1", "update"],
            vec!["nebula", "mods", "Test-1.20.1", "update", "--apply"],
            vec!["nebula", "mods", "Test-1.20.1", "remove", "a.jar"],
            vec!["nebula", "mods", "Test-1.20.1", "remove", "a.jar", "--yes"],
            vec![
                "nebula",
                "mods",
                "Test-1.20.1",
                "move",
                "a.jar",
                "optionalon",
            ],
            vec!["nebula", "server", "delete", "Test-1.20.1", "--yes"],
            vec!["nebula", "server", "delete", "Test-1.20.1"],
            vec!["nebula", "server", "duplicate", "Test-1.20.1", "Copy"],
        ] {
            assert!(Cli::try_parse_from(&args).is_ok(), "{args:?}");
        }

        // The documented defaults.
        let cli = Cli::try_parse_from(["nebula", "mods", "T-1.20.1", "search", "q"]).unwrap();
        let Some(Command::Mods {
            server,
            command:
                ModsCommand::Search {
                    provider,
                    sort,
                    limit,
                    offset,
                    ..
                },
        }) = cli.command
        else {
            panic!("mods search did not parse into its own variant");
        };
        assert_eq!(server, "T-1.20.1");
        assert_eq!(provider, ProviderArg::Modrinth);
        assert_eq!(sort, SortArg::Relevance);
        assert_eq!(limit, 20);
        assert_eq!(offset, 0);

        let cli = Cli::try_parse_from(["nebula", "mods", "T-1.20.1", "install", "jei"]).unwrap();
        let Some(Command::Mods {
            command: ModsCommand::Install {
                namespace, no_deps, ..
            },
            ..
        }) = cli.command
        else {
            panic!("mods install did not parse into its own variant");
        };
        assert_eq!(namespace, NamespaceArg::Required);
        assert!(!no_deps);

        // The namespace values are the directory names, not clap's kebab-case.
        assert!(
            Cli::try_parse_from(["nebula", "mods", "T", "move", "a.jar", "optional-on"]).is_err()
        );
        assert!(
            Cli::try_parse_from(["nebula", "mods", "T", "search", "q", "--provider", "curse"])
                .is_err()
        );
        assert!(Cli::try_parse_from(["nebula", "server", "duplicate", "T-1.20.1"]).is_err());
    }

    /// One end-to-end run of the read-only subcommand against a real root.
    #[test]
    fn mods_list_reads_a_temporary_root() {
        let dir = tempfile::tempdir().unwrap();
        let base_url = Config::normalize_base_url("https://cdn.example.invalid/").unwrap();
        let root = Root::new(dir.path(), base_url.clone());
        let created = server::create_server(
            &root,
            "Test",
            &"1.20.1".parse::<MinecraftVersion>().unwrap(),
            &CreateServerOptions {
                fabric_version: Some("0.14.18".into()),
                ..Default::default()
            },
        )
        .unwrap();
        std::fs::write(
            created
                .mod_container
                .unwrap()
                .join("required")
                .join("hand.jar"),
            b"not a zip",
        )
        .unwrap();

        let cfg = Config {
            root: dir.path().to_path_buf(),
            base_url,
            java_executable: None,
            helios_data_folder: None,
            curseforge_api_key: None,
        };
        mods_command(&cfg, &root, "Test-1.20.1", ModsCommand::List).unwrap();

        // An unknown server names the ones that do exist.
        let error = mods_command(&cfg, &root, "Nope-1.20.1", ModsCommand::List).unwrap_err();
        assert!(error.to_string().contains("Test-1.20.1"), "{error}");

        // `server delete` without `--yes` prints and refuses.
        let error = server_command(
            &root,
            ServerCommand::Delete {
                dir: "Test-1.20.1".into(),
                yes: false,
            },
        )
        .unwrap_err();
        assert!(matches!(error, Error::Invalid(_)), "{error}");
        assert!(created.dir.path.is_dir(), "nothing may be deleted");

        server_command(
            &root,
            ServerCommand::Delete {
                dir: "Test-1.20.1".into(),
                yes: true,
            },
        )
        .unwrap();
        assert!(!created.dir.path.exists());
    }

    /// §8.3: deleting a mod another one needs is the one case that asks.
    #[test]
    fn mods_remove_asks_before_breaking_a_dependency() {
        use nebula_core::catalog::ProviderId;
        use nebula_core::catalog::index::{IndexEntry, IndexHashes, ModIndex};

        let dir = tempfile::tempdir().unwrap();
        let base_url = Config::normalize_base_url("https://cdn.example.invalid/").unwrap();
        let root = Root::new(dir.path(), base_url.clone());
        let created = server::create_server(
            &root,
            "Test",
            &"1.20.1".parse::<MinecraftVersion>().unwrap(),
            &CreateServerOptions {
                fabric_version: Some("0.14.18".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let required = created.mod_container.unwrap().join("required");
        std::fs::write(required.join("api.jar"), b"api").unwrap();
        std::fs::write(required.join("sodium.jar"), b"sodium").unwrap();

        let row = |project: &str| {
            IndexEntry::new(
                ProviderId::Modrinth,
                project,
                "v1",
                IndexHashes::default(),
                Namespace::Required,
            )
        };
        let mut index = ModIndex::default();
        index.mods.insert("api.jar".into(), row("fabric-api"));
        let mut sodium = row("sodium");
        sodium.dependencies = vec!["fabric-api".into()];
        index.mods.insert("sodium.jar".into(), sodium);
        index.save(&created.dir).unwrap();

        let cfg = Config {
            root: dir.path().to_path_buf(),
            base_url,
            java_executable: None,
            helios_data_folder: None,
            curseforge_api_key: None,
        };
        let remove = |file: &str, yes: bool| {
            mods_command(
                &cfg,
                &root,
                "Test-1.20.1",
                ModsCommand::Remove {
                    file: file.to_owned(),
                    yes,
                },
            )
        };

        let error = remove("api.jar", false).unwrap_err();
        assert!(matches!(error, Error::Invalid(_)), "{error}");
        assert!(required.join("api.jar").is_file(), "nothing may be deleted");

        // Nothing depends on Sodium, so that one never asks.
        remove("sodium.jar", false).unwrap();
        remove("api.jar", true).unwrap();
        assert!(!required.join("api.jar").exists());
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
