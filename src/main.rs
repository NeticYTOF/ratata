use anyhow::{Context, Result, anyhow};
use clap::{CommandFactory, Parser, Subcommand};
use ratata::{
    Config, DownloadJob, Media, MediaFormat, MediaKind, download, extract, http, sanitize_filename,
};
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(
    name = "ratata",
    version,
    about = "Download videos, audio and images from social sites — fast and simple.",
    after_help = "Examples:\n  ratata https://www.youtube.com/watch?v=...\n  ratata https://www.instagram.com/reel/...\n  ratata info https://vimeo.com/...\n\nRun ratata --help-all for advanced options."
)]
struct Cli {
    /// Optional command; omit it to download the URLs you pass.
    #[command(subcommand)]
    command: Option<Command>,

    /// URLs to download
    urls: Vec<String>,

    /// Show every advanced option
    #[arg(long, global = true)]
    help_all: bool,

    /// Save into this folder or file
    #[arg(short, long, global = true)]
    output: Option<String>,

    /// Choose a specific format
    #[arg(short, long, global = true, hide = true)]
    format: Option<String>,

    /// Turn off the progress screen
    #[arg(long, global = true, hide = true)]
    no_tui: bool,

    /// Connections per file
    #[arg(long, global = true, hide = true)]
    segments: Option<usize>,

    /// Retries per connection
    #[arg(long, global = true, hide = true)]
    retries: Option<u8>,

    /// Audio only
    #[arg(long, global = true, hide = true)]
    audio_only: bool,

    /// List what is available
    #[arg(long, global = true, hide = true)]
    list_formats: bool,

    /// Download every quality or image
    #[arg(long, global = true, hide = true)]
    all_formats: bool,

    /// Netscape cookie file for private posts
    #[arg(long, global = true, hide = true)]
    cookies: Option<String>,

    /// Replace files with the same name
    #[arg(long, global = true, hide = true)]
    overwrite: bool,

    /// Extra diagnostics
    #[arg(long, global = true, hide = true)]
    verbose: bool,
}

#[derive(Subcommand, Debug)]
enum Command {
    #[command(alias = "dl", alias = "get", about = "Download one or more URLs")]
    Download { urls: Vec<String> },
    #[command(about = "Show details and available formats for a URL")]
    Info { url: String },
    #[command(about = "List available formats for a URL")]
    Formats { url: String },
    #[command(about = "Manage saved settings")]
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigAction {
    Show,
    Init,
    Path,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("\n{}", friendly_error(&error));
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    if cli.help_all {
        let mut command = Cli::command();
        for name in [
            "format",
            "no-tui",
            "segments",
            "retries",
            "audio-only",
            "list-formats",
            "all-formats",
            "cookies",
            "overwrite",
            "verbose",
        ] {
            command = command.mut_arg(name, |arg| arg.hide(false));
        }
        command.print_help()?;
        return Ok(());
    }

    let mut config = Config::load().context("Loading your saved settings failed")?;
    if let Some(n) = cli.segments {
        config.segments = n.clamp(1, 64);
    }
    if let Some(n) = cli.retries {
        config.retries = n;
    }
    if cli.no_tui {
        config.tui = false;
    }
    if cli.overwrite {
        config.overwrite = true;
    }
    if cli.verbose {
        config.keyring = true;
    }
    if let Some(path) = cli.cookies.as_deref() {
        config.cookies = Some(
            ratata::expand_tilde_path(path)
                .to_string_lossy()
                .into_owned(),
        );
    }

    let mut urls = Vec::new();
    match cli.command {
        Some(Command::Config { action }) => {
            match action {
                ConfigAction::Show => println!("{}", toml::to_string_pretty(&config)?),
                ConfigAction::Init => println!("Saved settings to {}", config.save()?.display()),
                ConfigAction::Path => println!("{}", Config::path()?.display()),
            }
            return Ok(());
        }
        Some(Command::Info { url }) | Some(Command::Formats { url }) => {
            let client = http::build_client(&config)?;
            let media = extract::extract(&url, &client, &config).await?;
            print_media(&media);
            return Ok(());
        }
        Some(Command::Download { urls: given }) => urls.extend(given),
        None => urls.extend(cli.urls),
    }

    if urls.is_empty() {
        if io::stdin().is_terminal() {
            urls.push(prompt_url()?);
        } else {
            Cli::command().print_help()?;
            return Ok(());
        }
    }

    let client = http::build_client(&config)?;
    let mut jobs = Vec::new();
    let mut next_id = 0usize;
    for url in urls.iter() {
        let media = extract::extract(url, &client, &config).await?;
        if cli.list_formats {
            print_media(&media);
            continue;
        }
        let choices: Vec<(MediaFormat, Option<String>)> = if cli.all_formats {
            media
                .formats
                .iter()
                .cloned()
                .map(|format| {
                    let suffix =
                        format!("{}_{}", format.kind.label(), sanitize_filename(&format.id));
                    (format, Some(suffix))
                })
                .collect()
        } else {
            vec![(
                choose_format(&media, cli.format.as_deref(), cli.audio_only)?,
                None,
            )]
        };
        for (format, suffix) in choices {
            let destination = destination_for(
                &media,
                &format,
                &config,
                cli.output.as_deref(),
                suffix.as_deref(),
            )?;
            jobs.push(DownloadJob {
                id: next_id,
                media: media.clone(),
                format,
                destination,
            });
            next_id += 1;
        }
    }
    if jobs.is_empty() {
        return Ok(());
    }
    let client = Arc::new(client);
    if config.tui && !cli.no_tui {
        ratata::ui::run(jobs, config, client).await?;
    } else {
        download::run_plain(jobs, &config, client).await?;
    }
    Ok(())
}

fn prompt_url() -> Result<String> {
    print!("Paste a link to download: ");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let url = line.trim();
    if url.is_empty() {
        return Err(anyhow!(
            "No link entered. Run ratata --help to see examples."
        ));
    }
    Ok(url.to_owned())
}

fn friendly_error(error: &anyhow::Error) -> String {
    let message = format!("{error:#}");
    let lower = message.to_ascii_lowercase();
    let suggestion = if lower.contains("unsupported url") {
        "Check that the link is complete, or paste a direct media link such as .mp4 or .m3u8."
    } else if lower.contains("no media formats") || lower.contains("no matching format") {
        "The post may be private or restricted. Try ratata info LINK, or use --cookies with a cookie file."
    } else if lower.contains("403") || lower.contains("401") || lower.contains("forbidden") {
        "That site refused the request. Try again later, or pass --cookies cookies.txt if you are logged in."
    } else if lower.contains("dns") || lower.contains("connect") || lower.contains("timed out") {
        "Check your internet connection and try again."
    } else {
        "Run ratata --help for examples, or ratata --help-all for advanced options."
    };
    format!("Ratata hit a problem: {message}\n\nTry: {suggestion}")
}

fn choose_format(media: &Media, requested: Option<&str>, audio_only: bool) -> Result<MediaFormat> {
    let chosen = if audio_only {
        media.best_audio().or_else(|| media.best_video())
    } else {
        match requested {
            None | Some("best") => media.best_video().or_else(|| media.best_image()),
            Some("worst") => media
                .formats
                .iter()
                .filter(|f| f.kind == MediaKind::Video)
                .min_by_key(|f| f.quality()),
            Some(id) => media.formats.iter().find(|f| f.id == id),
        }
    };
    chosen
        .cloned()
        .ok_or_else(|| anyhow!("No matching format for this link"))
}

fn destination_for(
    media: &Media,
    format: &MediaFormat,
    config: &Config,
    output: Option<&str>,
    suffix: Option<&str>,
) -> Result<PathBuf> {
    let explicit_file = output
        .map(PathBuf::from)
        .filter(|p| p.extension().is_some());
    let base = if let Some(file) = explicit_file {
        ratata::expand_tilde_path(&file.to_string_lossy())
    } else {
        let dir = output
            .map(ratata::expand_tilde_path)
            .unwrap_or(config.output_dir()?);
        std::fs::create_dir_all(&dir)?;
        dir.join(format!(
            "{}{}.{}",
            sanitize_filename(&media.title),
            suffix.map(|s| format!("_{s}")).unwrap_or_default(),
            format.ext
        ))
    };
    if config.overwrite || !base.exists() {
        return Ok(base);
    }
    let stem = base
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "ratata-download".into());
    let ext = base
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_else(|| format.ext.clone());
    for n in 1..1000 {
        let candidate = base.with_file_name(format!("{stem}_{n}.{ext}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(anyhow!("No free filename for {}", base.display()))
}

fn print_media(media: &Media) {
    println!("\n  {} — {}", media.platform, media.title);
    if let Some(author) = &media.author {
        println!("    by {author}");
    }
    if let Some(duration) = media.duration {
        println!("    duration {duration:.1}s");
    }
    println!("\n    ID           TYPE      EXT      RES       FPS      SIZE");
    for format in &media.formats {
        println!("    {}", format.describe());
    }
    println!();
}
