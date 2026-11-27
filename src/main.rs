use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use ratata::{
    Config, DownloadJob, Media, MediaFormat, MediaKind, download, extract, http, sanitize_filename,
};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(
    name = "ratata",
    version,
    about = "Blossom-fast terminal downloader for social media"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    #[arg(short, long, global = true, help = "Output directory or file")]
    output: Option<String>,
    #[arg(short, long, global = true, help = "Format id, or best/worst")]
    format: Option<String>,
    #[arg(long, global = true, help = "Disable the flower-tunnel UI")]
    no_tui: bool,
    #[arg(long, global = true, help = "Parallel HTTP connections per file")]
    segments: Option<usize>,
    #[arg(long, global = true, help = "Retry attempts per segment")]
    retries: Option<u8>,
    #[arg(long, global = true, help = "Pick audio only")]
    audio_only: bool,
    #[arg(long, global = true, help = "List formats instead of downloading")]
    list_formats: bool,
    #[arg(long, global = true, help = "Overwrite existing files")]
    overwrite: bool,
    #[arg(long, global = true, help = "Verbose diagnostics")]
    verbose: bool,
}

#[derive(Subcommand, Debug)]
enum Command {
    #[command(alias = "dl", alias = "get")]
    Download {
        urls: Vec<String>,
    },
    Info {
        url: String,
    },
    Formats {
        url: String,
    },
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
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut config = Config::load().context("loading configuration")?;
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

    match cli.command {
        Command::Config { action } => {
            match action {
                ConfigAction::Show => println!("{}", toml::to_string_pretty(&config)?),
                ConfigAction::Init => {
                    let path = config.save()?;
                    println!("Wrote {}", path.display());
                }
                ConfigAction::Path => println!("{}", Config::path()?.display()),
            }
            return Ok(());
        }
        Command::Info { url } | Command::Formats { url } => {
            let client = http::build_client(&config)?;
            let media = extract::extract(&url, &client, &config).await?;
            print_media(&media);
            return Ok(());
        }
        Command::Download { urls } => {
            if urls.is_empty() {
                return Err(anyhow!("provide at least one URL"));
            }
            let client = http::build_client(&config)?;
            let mut jobs = Vec::new();
            for (index, url) in urls.iter().enumerate() {
                let media = extract::extract(url, &client, &config).await?;
                if cli.list_formats {
                    print_media(&media);
                    continue;
                }
                let format = choose_format(&media, cli.format.as_deref(), cli.audio_only)?;
                let destination = destination_for(&media, &format, &config, cli.output.as_deref())?;
                jobs.push(DownloadJob {
                    id: index,
                    media,
                    format,
                    destination,
                });
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
        }
    }
    Ok(())
}

fn choose_format(media: &Media, requested: Option<&str>, audio_only: bool) -> Result<MediaFormat> {
    let chosen = if audio_only {
        media.best_audio().or_else(|| media.best_video())
    } else {
        match requested {
            None => media.best_video().or_else(|| media.best_image()),
            Some("best") => media.best_video().or_else(|| media.best_image()),
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
        .ok_or_else(|| anyhow!("no matching format in {}", media.platform))
}

fn destination_for(
    media: &Media,
    format: &MediaFormat,
    config: &Config,
    output: Option<&str>,
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
            "{}.{}",
            sanitize_filename(&media.title),
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
    Err(anyhow!("no free filename for {}", base.display()))
}

fn print_media(media: &Media) {
    println!("\n  ✿ {} — {}", media.platform, media.title);
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
