use clap::{Parser, Subcommand};
use gateveriq::{
    core,
    service::{self, App, Config},
};
#[derive(Parser)]
#[command(version, about = "Gateveriq self-hosted verification service")]
struct Cli {
    #[arg(long, default_value = "config.toml")]
    config: String,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Create a configuration and private token key. Never overwrites files.
    Init,
    Serve,
    IntegrationCreate {
        #[arg(long)]
        name: String,
        #[arg(long, required = true, value_delimiter = ',')]
        actions: Vec<String>,
    },
    IntegrationRotate {
        #[arg(long)]
        id: String,
        #[arg(long,default_value_t=300,value_parser=clap::value_parser!(u32).range(0..=3600))]
        grace_seconds: u32,
    },
    IntegrationDisable {
        #[arg(long)]
        id: String,
    },
    Cleanup,
    /// Solve a challenge JSON file; optionally submit to a public API URL.
    Solve {
        #[arg(long)]
        challenge: String,
        #[arg(long)]
        submit_url: Option<String>,
    },
}
fn private_write(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(content.as_bytes())
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    if let Command::Init = cli.command {
        let config_path = std::path::Path::new(&cli.config);
        if config_path.exists() {
            return Err("configuration already exists".into());
        }
        let parent = config_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(std::path::Path::new("."));
        let parent = std::fs::canonicalize(parent)?;
        let key = parent.join("token.key");
        let config = Config {
            database: parent.join("gateveriq.sqlite3").to_string_lossy().into(),
            token_key_file: key.to_string_lossy().into(),
            public_listen: "127.0.0.1:9080".parse()?,
            internal_listen: "127.0.0.1:9081".parse()?,
            difficulty_bits: 18,
            challenge_ttl_seconds: 120,
            token_ttl_seconds: 60,
            retention_seconds: 300,
            requests_per_minute: 120,
            max_active_challenges: 1000,
        };
        private_write(&key, &core::random())?;
        private_write(config_path, &toml::to_string_pretty(&config)?)?;
        println!("Configuration created. Keep token.key private and persistent.");
        return Ok(());
    }
    if let Command::Solve {
        challenge,
        submit_url,
    } = cli.command
    {
        let c: core::Challenge = serde_json::from_str(&std::fs::read_to_string(challenge)?)?;
        let id = c.challenge_id.clone();
        let nonce = tokio::task::spawn_blocking(move || core::solve(&c)).await??;
        let solution = serde_json::json!({"challenge_id":id,"nonce":nonce});
        if let Some(url) = submit_url {
            let response = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()?
                .post(url)
                .json(&solution)
                .send()
                .await?;
            let status = response.status();
            let body = response.text().await?;
            if !status.is_success() {
                return Err(format!("submission failed: {status}: {body}").into());
            }
            println!("{body}");
        } else {
            println!("{solution}");
        }
        return Ok(());
    }
    let config: Config = toml::from_str(&std::fs::read_to_string(cli.config)?)?;
    let app = App::open(config)?;
    match cli.command {
        Command::Serve => service::serve(app).await?,
        Command::IntegrationCreate { name, actions } => {
            let (id, secret) = app.create_integration(&name, &actions)?;
            println!(
                "{}",
                serde_json::json!({"integration_id":id,"integration_secret":secret})
            );
        }
        Command::IntegrationRotate { id, grace_seconds } => {
            let secret = core::random();
            let count=app.db.lock().unwrap().execute("UPDATE integrations SET previous_hash=secret_hash,previous_until=?1,secret_hash=?2 WHERE id=?3 AND active=1",rusqlite::params![service::now()+grace_seconds as i64,core::hash(secret.as_bytes()),id])?;
            if count == 0 {
                return Err("integration not found or disabled".into());
            }
            println!("{}", serde_json::json!({"integration_secret":secret}));
        }
        Command::IntegrationDisable { id } => {
            let count = app
                .db
                .lock()
                .unwrap()
                .execute("UPDATE integrations SET active=0 WHERE id=?1", [id])?;
            if count == 0 {
                return Err("integration not found".into());
            }
            println!("Integration disabled");
        }
        Command::Cleanup => println!("Deleted {} expired rows", app.cleanup()?),
        _ => unreachable!(),
    }
    Ok(())
}
