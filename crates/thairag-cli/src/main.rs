use clap::{Parser, Subcommand};
use colored::Colorize;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Parser)]
#[command(
    name = "thairag",
    about = "ThaiRAG CLI — manage your RAG platform",
    version
)]
struct Cli {
    /// API server URL
    #[arg(
        short,
        long,
        default_value = "http://localhost:8080",
        env = "THAIRAG_URL"
    )]
    url: String,

    /// API key for authentication
    #[arg(short = 'k', long, env = "THAIRAG_API_KEY")]
    api_key: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Check system health
    Health {
        /// Perform deep health check (probes all providers)
        #[arg(long)]
        deep: bool,
    },
    /// Show system status overview
    Status,
    /// Configuration management
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Backup and restore
    Backup {
        #[command(subcommand)]
        action: BackupAction,
    },
    /// Generate deployment files
    Deploy {
        /// Output directory
        #[arg(short, long, default_value = ".")]
        output: String,
        /// Deployment profile (free, standard, premium)
        #[arg(short, long, default_value = "standard")]
        profile: String,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Show current provider configuration
    Show,
    /// Get a specific setting value
    Get {
        /// Setting key (dot-separated, e.g. "llm.model")
        key: String,
    },
}

#[derive(Subcommand)]
enum BackupAction {
    /// Create a backup and save to file
    Create {
        /// Output file path (default: thairag-backup-<timestamp>.zip)
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Preview what would be included in a backup
    Preview,
}

// ── Response types ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct HealthResponse {
    status: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    providers: Option<HashMap<String, Value>>,
}

// ── HTTP helpers ───────────────────────────────────────────────────

fn build_client(api_key: &Option<String>) -> reqwest::blocking::Client {
    let mut builder =
        reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(30));

    if let Some(key) = api_key {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "X-API-Key",
            reqwest::header::HeaderValue::from_str(key).expect("invalid api key"),
        );
        builder = builder.default_headers(headers);
    }

    builder.build().expect("failed to build HTTP client")
}

fn get_json(client: &reqwest::blocking::Client, url: &str) -> Result<Value, String> {
    client
        .get(url)
        .send()
        .map_err(|e| format!("Request failed: {e}"))?
        .json::<Value>()
        .map_err(|e| format!("Failed to parse JSON: {e}"))
}

fn post_json(
    client: &reqwest::blocking::Client,
    url: &str,
    body: &Value,
) -> Result<(u16, Value), String> {
    let resp = client
        .post(url)
        .json(body)
        .send()
        .map_err(|e| format!("Request failed: {e}"))?;
    let status = resp.status().as_u16();
    let json = resp
        .json::<Value>()
        .map_err(|e| format!("Failed to parse JSON: {e}"))?;
    Ok((status, json))
}

// ── Command implementations ────────────────────────────────────────

fn cmd_health(base_url: &str, api_key: &Option<String>, deep: bool) {
    let client = build_client(api_key);
    let url = if deep {
        format!("{base_url}/health?deep=true")
    } else {
        format!("{base_url}/health")
    };

    match client.get(&url).send() {
        Err(e) => {
            eprintln!("{} {e}", "Error:".red().bold());
            std::process::exit(1);
        }
        Ok(resp) => {
            let status_code = resp.status();
            match resp.json::<HealthResponse>() {
                Ok(health) => {
                    let status_str = if health.status == "ok" || health.status == "healthy" {
                        health.status.green().bold()
                    } else {
                        health.status.red().bold()
                    };
                    println!("{} {}", "Status:".bold(), status_str);
                    if let Some(v) = &health.version {
                        println!("{} {}", "Version:".bold(), v);
                    }
                    if let Some(providers) = &health.providers {
                        println!("{}", "Providers:".bold());
                        for (name, val) in providers {
                            let indicator = if val.as_str().map(|s| s == "ok").unwrap_or(false) {
                                "OK".green()
                            } else {
                                val.to_string().yellow()
                            };
                            println!("  {:<20} {}", name, indicator);
                        }
                    }
                }
                Err(_) => {
                    // Server responded but not JSON — just show HTTP status
                    if status_code.is_success() {
                        println!("{} {}", "Status:".bold(), "ok".green().bold());
                    } else {
                        println!(
                            "{} {} {}",
                            "Status:".bold(),
                            "error".red().bold(),
                            status_code
                        );
                        std::process::exit(1);
                    }
                }
            }
        }
    }
}

fn cmd_status(base_url: &str, api_key: &Option<String>) {
    let client = build_client(api_key);

    // Fetch health
    let health_url = format!("{base_url}/health");
    let health_ok = match client.get(&health_url).send() {
        Ok(r) if r.status().is_success() => r
            .json::<HealthResponse>()
            .map(|h| h.status == "ok" || h.status == "healthy")
            .unwrap_or(false),
        _ => false,
    };

    println!("{}", "=== ThaiRAG Status ===".bold().cyan());
    println!(
        "{:<20} {}",
        "API Server:",
        if health_ok {
            "online".green().bold()
        } else {
            "offline".red().bold()
        }
    );
    println!("{:<20} {}", "Server URL:", base_url);

    // Fetch usage stats
    let usage_url = format!("{base_url}/api/km/settings/usage");
    match get_json(&client, &usage_url) {
        Ok(json) => {
            println!("{}", "\n--- Usage Statistics ---".bold());
            if let Some(docs) = json.get("total_documents").and_then(|v| v.as_u64()) {
                println!("{:<20} {}", "Documents:", docs);
            }
            if let Some(orgs) = json.get("total_organizations").and_then(|v| v.as_u64()) {
                println!("{:<20} {}", "Organizations:", orgs);
            }
            if let Some(users) = json.get("total_users").and_then(|v| v.as_u64()) {
                println!("{:<20} {}", "Users:", users);
            }
            if let Some(tokens) = json.get("total_tokens_used").and_then(|v| v.as_u64()) {
                println!("{:<20} {}", "Tokens Used:", tokens);
            }
        }
        Err(e) => {
            println!("{} Could not fetch usage stats: {}", "Warning:".yellow(), e);
        }
    }
}

fn cmd_config_show(base_url: &str, api_key: &Option<String>) {
    let client = build_client(api_key);
    let url = format!("{base_url}/api/km/settings/providers");
    match get_json(&client, &url) {
        Ok(json) => {
            println!("{}", "=== Provider Configuration ===".bold().cyan());
            print_json_pretty(&json, 0);
        }
        Err(e) => {
            eprintln!("{} {e}", "Error:".red().bold());
            std::process::exit(1);
        }
    }
}

fn cmd_config_get(base_url: &str, api_key: &Option<String>, key: &str) {
    let client = build_client(api_key);
    let url = format!("{base_url}/api/km/settings/providers");
    match get_json(&client, &url) {
        Ok(json) => {
            // Navigate dot-separated key path
            let parts: Vec<&str> = key.split('.').collect();
            let mut current = &json;
            for part in &parts {
                match current.get(part) {
                    Some(v) => current = v,
                    None => {
                        eprintln!("{} Key '{}' not found", "Error:".red().bold(), key);
                        std::process::exit(1);
                    }
                }
            }
            println!("{}: {}", key.bold(), format_value(current));
        }
        Err(e) => {
            eprintln!("{} {e}", "Error:".red().bold());
            std::process::exit(1);
        }
    }
}

fn cmd_backup_create(base_url: &str, api_key: &Option<String>, output: &Option<String>) {
    let client = build_client(api_key);
    let url = format!("{base_url}/api/km/admin/backup");
    let body = serde_json::json!({
        "include_settings": true,
        "include_users": true,
        "include_documents": true,
        "include_org_structure": true
    });

    println!("{}", "Creating backup...".bold());

    match client.post(&url).json(&body).send() {
        Err(e) => {
            eprintln!("{} {e}", "Error:".red().bold());
            std::process::exit(1);
        }
        Ok(resp) => {
            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().unwrap_or_default();
                eprintln!("{} HTTP {} — {}", "Error:".red().bold(), status, text);
                std::process::exit(1);
            }

            let bytes = match resp.bytes() {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("{} Failed to read response: {e}", "Error:".red().bold());
                    std::process::exit(1);
                }
            };

            let filename = output.clone().unwrap_or_else(|| {
                let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
                format!("thairag-backup-{ts}.zip")
            });

            match std::fs::write(&filename, &bytes) {
                Ok(_) => {
                    println!(
                        "{} Backup saved to {} ({} bytes)",
                        "Success:".green().bold(),
                        filename.bold(),
                        bytes.len()
                    );
                }
                Err(e) => {
                    eprintln!("{} Failed to write file: {e}", "Error:".red().bold());
                    std::process::exit(1);
                }
            }
        }
    }
}

fn cmd_backup_preview(base_url: &str, api_key: &Option<String>) {
    let client = build_client(api_key);
    let url = format!("{base_url}/api/km/admin/backup/preview");
    let body = serde_json::json!({});

    println!("{}", "Backup Preview".bold().cyan());

    match post_json(&client, &url, &body) {
        Ok((_status, json)) => {
            print_json_pretty(&json, 0);
        }
        Err(e) => {
            eprintln!("{} {e}", "Error:".red().bold());
            std::process::exit(1);
        }
    }
}

fn cmd_deploy(output_dir: &str, profile: &str) {
    let compose_content = generate_docker_compose(profile);
    let output_path = format!("{output_dir}/docker-compose.{profile}.yml");

    match std::fs::write(&output_path, &compose_content) {
        Ok(_) => {
            println!(
                "{} Generated deployment file: {}",
                "Success:".green().bold(),
                output_path.bold()
            );
            println!();
            println!("Profile: {}", profile.bold());
            match profile {
                "free" => {
                    println!("  - ThaiRAG API");
                    println!("  - PostgreSQL");
                    println!("  - Redis");
                    println!("  - Ollama (local LLM)");
                    println!("  - FastEmbed (local embeddings)");
                    println!("  - In-memory vector store");
                }
                "standard" => {
                    println!("  - ThaiRAG API");
                    println!("  - PostgreSQL");
                    println!("  - Redis");
                    println!("  - Qdrant (vector database)");
                    println!("  - Prometheus + Grafana");
                    println!("  - Admin UI");
                }
                "premium" => {
                    println!("  - ThaiRAG API (scaled)");
                    println!("  - PostgreSQL (HA)");
                    println!("  - Redis Cluster");
                    println!("  - Qdrant (vector database)");
                    println!("  - Prometheus + Grafana");
                    println!("  - Admin UI");
                    println!("  - Nginx load balancer");
                }
                _ => {}
            }
            println!();
            println!("Next steps:");
            println!("  1. Copy .env.example to .env and configure your settings");
            println!("  2. Run: docker compose -f {output_path} up -d");
        }
        Err(e) => {
            eprintln!("{} Failed to write file: {e}", "Error:".red().bold());
            std::process::exit(1);
        }
    }
}

// ── Docker Compose generators ──────────────────────────────────────

fn generate_docker_compose(profile: &str) -> String {
    match profile {
        "free" => FREE_COMPOSE.to_string(),
        "premium" => PREMIUM_COMPOSE.to_string(),
        _ => STANDARD_COMPOSE.to_string(),
    }
}

const FREE_COMPOSE: &str = r#"# ThaiRAG — Free Tier Deployment
# Uses Ollama for local LLM, FastEmbed for embeddings, in-memory vector store.
# No external API keys required.

services:
  postgres:
    image: postgres:16-alpine
    ports:
      - "5432:5432"
    environment:
      POSTGRES_DB: ${POSTGRES_DB:-thairag}
      POSTGRES_USER: ${POSTGRES_USER:-thairag}
      POSTGRES_PASSWORD: ${POSTGRES_PASSWORD:-changeme}
    volumes:
      - postgres-data:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U ${POSTGRES_USER:-thairag}"]
      interval: 5s
      timeout: 5s
      retries: 5

  redis:
    image: redis:7-alpine
    ports:
      - "6379:6379"
    volumes:
      - redis-data:/data
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 5s
      timeout: 5s
      retries: 5

  ollama:
    image: ollama/ollama:latest
    ports:
      - "11434:11434"
    volumes:
      - ollama-models:/root/.ollama

  thairag:
    image: thairag:latest
    ports:
      - "8080:8080"
    volumes:
      - thairag-data:/data
    env_file: .env
    environment:
      THAIRAG__SERVER__HOST: "0.0.0.0"
      THAIRAG_TIER: free
    depends_on:
      postgres:
        condition: service_healthy
      redis:
        condition: service_healthy

  admin-ui:
    image: thairag-admin-ui:latest
    ports:
      - "8081:80"
    depends_on:
      - thairag

volumes:
  postgres-data:
  redis-data:
  thairag-data:
  ollama-models:
"#;

const STANDARD_COMPOSE: &str = r#"# ThaiRAG — Standard Tier Deployment
# Uses cloud LLM providers, Qdrant for vector storage, with monitoring.

services:
  postgres:
    image: postgres:16-alpine
    ports:
      - "5432:5432"
    environment:
      POSTGRES_DB: ${POSTGRES_DB:-thairag}
      POSTGRES_USER: ${POSTGRES_USER:-thairag}
      POSTGRES_PASSWORD: ${POSTGRES_PASSWORD}
    volumes:
      - postgres-data:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U ${POSTGRES_USER:-thairag}"]
      interval: 5s
      timeout: 5s
      retries: 5

  redis:
    image: redis:7-alpine
    ports:
      - "6379:6379"
    volumes:
      - redis-data:/data
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 5s
      timeout: 5s
      retries: 5

  qdrant:
    image: qdrant/qdrant:latest
    ports:
      - "6333:6333"
      - "6334:6334"
    volumes:
      - qdrant-data:/qdrant/storage

  thairag:
    image: thairag:latest
    ports:
      - "8080:8080"
    volumes:
      - thairag-data:/data
    env_file: .env
    environment:
      THAIRAG__SERVER__HOST: "0.0.0.0"
      THAIRAG_TIER: standard
    depends_on:
      postgres:
        condition: service_healthy
      redis:
        condition: service_healthy

  admin-ui:
    image: thairag-admin-ui:latest
    ports:
      - "8081:80"
    depends_on:
      - thairag

  prometheus:
    image: prom/prometheus:latest
    ports:
      - "9090:9090"
    volumes:
      - ./prometheus.yml:/etc/prometheus/prometheus.yml:ro
      - prometheus-data:/prometheus
    depends_on:
      - thairag

  grafana:
    image: grafana/grafana:latest
    ports:
      - "3001:3000"
    environment:
      GF_SECURITY_ADMIN_USER: admin
      GF_SECURITY_ADMIN_PASSWORD: ${GRAFANA_PASSWORD:-admin}
      GF_USERS_ALLOW_SIGN_UP: "false"
    volumes:
      - grafana-data:/var/lib/grafana
    depends_on:
      - prometheus

volumes:
  postgres-data:
  redis-data:
  qdrant-data:
  thairag-data:
  prometheus-data:
  grafana-data:
"#;

const PREMIUM_COMPOSE: &str = r#"# ThaiRAG — Premium Tier Deployment
# High-availability setup with Nginx load balancing and full monitoring.

services:
  postgres:
    image: postgres:16-alpine
    ports:
      - "5432:5432"
    environment:
      POSTGRES_DB: ${POSTGRES_DB:-thairag}
      POSTGRES_USER: ${POSTGRES_USER:-thairag}
      POSTGRES_PASSWORD: ${POSTGRES_PASSWORD}
    volumes:
      - postgres-data:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U ${POSTGRES_USER:-thairag}"]
      interval: 5s
      timeout: 5s
      retries: 5

  redis:
    image: redis:7-alpine
    ports:
      - "6379:6379"
    volumes:
      - redis-data:/data
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 5s
      timeout: 5s
      retries: 5

  qdrant:
    image: qdrant/qdrant:latest
    ports:
      - "6333:6333"
      - "6334:6334"
    volumes:
      - qdrant-data:/qdrant/storage

  thairag-1:
    image: thairag:latest
    volumes:
      - thairag-data:/data
    env_file: .env
    environment:
      THAIRAG__SERVER__HOST: "0.0.0.0"
      THAIRAG_TIER: premium
    depends_on:
      postgres:
        condition: service_healthy
      redis:
        condition: service_healthy

  thairag-2:
    image: thairag:latest
    volumes:
      - thairag-data:/data
    env_file: .env
    environment:
      THAIRAG__SERVER__HOST: "0.0.0.0"
      THAIRAG_TIER: premium
    depends_on:
      postgres:
        condition: service_healthy
      redis:
        condition: service_healthy

  nginx:
    image: nginx:alpine
    ports:
      - "8080:80"
    volumes:
      - ./nginx/nginx.conf:/etc/nginx/nginx.conf:ro
    depends_on:
      - thairag-1
      - thairag-2

  admin-ui:
    image: thairag-admin-ui:latest
    ports:
      - "8081:80"
    depends_on:
      - nginx

  prometheus:
    image: prom/prometheus:latest
    ports:
      - "9090:9090"
    volumes:
      - ./prometheus.yml:/etc/prometheus/prometheus.yml:ro
      - prometheus-data:/prometheus
    depends_on:
      - thairag-1
      - thairag-2

  grafana:
    image: grafana/grafana:latest
    ports:
      - "3001:3000"
    environment:
      GF_SECURITY_ADMIN_USER: admin
      GF_SECURITY_ADMIN_PASSWORD: ${GRAFANA_PASSWORD:-admin}
      GF_USERS_ALLOW_SIGN_UP: "false"
    volumes:
      - grafana-data:/var/lib/grafana
    depends_on:
      - prometheus

volumes:
  postgres-data:
  redis-data:
  qdrant-data:
  thairag-data:
  prometheus-data:
  grafana-data:
"#;

// ── Pretty-print helpers ───────────────────────────────────────────

fn print_json_pretty(value: &Value, indent: usize) {
    let pad = "  ".repeat(indent);
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                match v {
                    Value::Object(_) | Value::Array(_) => {
                        println!("{}{}:", pad, k.bold());
                        print_json_pretty(v, indent + 1);
                    }
                    _ => {
                        println!("{}{}: {}", pad, k.bold(), format_value(v));
                    }
                }
            }
        }
        Value::Array(arr) => {
            for (i, item) in arr.iter().enumerate() {
                println!("{}[{}]", pad, i);
                print_json_pretty(item, indent + 1);
            }
        }
        _ => {
            println!("{}{}", pad, format_value(value));
        }
    }
}

fn format_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Bool(b) => {
            if *b {
                "true".green().to_string()
            } else {
                "false".red().to_string()
            }
        }
        Value::Null => "null".dimmed().to_string(),
        other => other.to_string(),
    }
}

// ── Entry point ────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let base_url = cli.url.trim_end_matches('/').to_string();

    match cli.command {
        Commands::Health { deep } => {
            cmd_health(&base_url, &cli.api_key, deep);
        }
        Commands::Status => {
            cmd_status(&base_url, &cli.api_key);
        }
        Commands::Config { action } => match action {
            ConfigAction::Show => {
                cmd_config_show(&base_url, &cli.api_key);
            }
            ConfigAction::Get { key } => {
                cmd_config_get(&base_url, &cli.api_key, &key);
            }
        },
        Commands::Backup { action } => match action {
            BackupAction::Create { output } => {
                cmd_backup_create(&base_url, &cli.api_key, &output);
            }
            BackupAction::Preview => {
                cmd_backup_preview(&base_url, &cli.api_key);
            }
        },
        Commands::Deploy { output, profile } => {
            cmd_deploy(&output, &profile);
        }
    }
}
