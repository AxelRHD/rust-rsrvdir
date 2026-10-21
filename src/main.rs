//! rsrvdir – simple directory server with file info; one directory or several shares with a start page.

mod auth;
mod config;
mod listing;
mod server;
mod views;

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use crate::config::{FileConfig, Share, ShareConfig, THEMES};

#[derive(Parser)]
#[command(
    version,
    about = "Simple directory server with file info",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Directory to serve (ignored with --config)
    #[arg(default_value = ".")]
    dir: PathBuf,

    /// TOML file with several shares (start page lists them)
    #[arg(short, long, env = "RSRVDIR_CONFIG")]
    config: Option<PathBuf>,

    /// Port to listen on [default: 8080]
    #[arg(short, long, env = "RSRVDIR_PORT")]
    port: Option<u16>,

    /// Host/interface to bind [default: 0.0.0.0]
    #[arg(long, env = "RSRVDIR_HOST")]
    host: Option<String>,

    /// Color theme (auto, nord, squirrel, archlinux, monokai, zenburn) [default: auto]
    #[arg(long, env = "RSRVDIR_THEME")]
    theme: Option<String>,

    /// Inline Basic Auth (user:password)
    #[arg(long, conflicts_with = "auth_file")]
    auth: Option<String>,

    /// htpasswd file with bcrypt hashes
    #[arg(long, env = "RSRVDIR_AUTH_FILE")]
    auth_file: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// Create or update a user in an htpasswd file (prompts for the password)
    Htpasswd { file: PathBuf, user: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        // plain lines in the journal
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()))
        .init();
    let cli = Cli::parse();

    if let Some(Command::Htpasswd { file, user }) = &cli.command {
        let pw = rpassword::prompt_password("Password: ")?;
        if pw != rpassword::prompt_password("Repeat: ")? {
            bail!("passwords do not match");
        }
        auth::set_user(file, user, &pw)?;
        println!("{user} saved in {}", file.display());
        return Ok(());
    }

    let file = cli.config.as_deref().map(FileConfig::load).transpose()?;
    let (title, shares, cfg_listen, cfg_theme, cfg_auth_file) = match file {
        Some(f) => {
            if f.shares.is_empty() {
                bail!("config has no [[share]]");
            }
            let mut shares: Vec<Share> = Vec::new();
            for s in f.shares {
                if shares.iter().any(|x| x.name == s.name) {
                    bail!("share {:?} defined twice", s.name);
                }
                if s.name.is_empty() {
                    bail!("share name must not be empty in a config file");
                }
                shares.push(Share::new(s)?);
            }
            (
                f.title.unwrap_or_else(|| "rsrvdir".into()),
                shares,
                f.listen,
                f.theme,
                f.auth_file,
            )
        }
        None => {
            let share = Share::new(ShareConfig {
                name: String::new(),
                path: cli.dir.clone(),
                description: String::new(),
                allow: vec![],
                hide: vec![],
                sort: Default::default(),
                order: Default::default(),
                download: false,
                charset: None,
            })?;
            let title = share
                .root
                .file_name()
                .map_or("/".into(), |n| n.to_string_lossy().into_owned());
            (title, vec![share], None, None, None)
        }
    };

    let theme = cli.theme.or(cfg_theme).unwrap_or_else(|| "auto".into());
    if !THEMES.iter().any(|(t, _)| *t == theme) {
        bail!("unknown theme {theme:?}");
    }
    let creds = match (&cli.auth, cli.auth_file.as_ref().or(cfg_auth_file.as_ref())) {
        (Some(spec), _) => Some(auth::inline(spec)?),
        (None, Some(path)) => Some(auth::parse_htpasswd(path)?),
        (None, None) => None,
    };

    let mut addr: SocketAddr = cfg_listen
        .as_deref()
        .unwrap_or("0.0.0.0:8080")
        .parse()
        .context("listen must be ip:port")?;
    if let Some(h) = &cli.host {
        addr.set_ip(h.parse().context("--host must be an IP address")?);
    }
    if let Some(p) = cli.port {
        addr.set_port(p);
    }

    for s in &shares {
        tracing::info!(mount = %s.mount(), root = %s.root.display(), "share");
    }
    let auth_note = if creds.is_some() { " (Basic Auth)" } else { "" };
    tracing::info!("serving at http://{addr}{auth_note}");

    let app = server::router(
        server::App {
            title,
            theme,
            shares,
        },
        creds,
    );
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding {addr}"))?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown())
    .await?;
    Ok(())
}

async fn shutdown() {
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = term.recv() => {},
    }
}
