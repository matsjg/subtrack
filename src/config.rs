use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name = "subtrack")]
#[command(about = "Self-hosted subscription tracker", long_about = None)]
pub struct Config {
    /// HTTP server port
    #[arg(short, long, env = "SUBTRACK_PORT", default_value = "8080")]
    pub port: u16,

    /// Bind address
    #[arg(short = 'H', long, env = "SUBTRACK_HOST", default_value = "127.0.0.1")]
    pub host: String,

    /// SQLite database path
    #[arg(short, long, env = "SUBTRACK_DB", default_value = "./subtrack.db")]
    pub database: String,

    /// Optional authentication password
    #[arg(long, env = "SUBTRACK_PASSWORD")]
    pub password: Option<String>,

    /// Log level
    #[arg(long, env = "RUST_LOG", default_value = "info")]
    pub log_level: String,
}

impl Config {
    pub fn load() -> Self {
        Config::parse()
    }

    pub fn bind_address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}
