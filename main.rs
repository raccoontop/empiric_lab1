use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File};
use std::io::{self, Read};
use std::env;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use anyhow::{Result, Context};
use rusqlite::{Connection, params};

use clap::Parser;
use tracing::{info, warn, error};
use tracing_subscriber::EnvFilter;

/// CLI arguments.
/// Allows creating, reading and deleting text snippets.
/// Snippets can be stored in JSON or SQLite storage depending on configuration.
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {

    /// Creates a new snippet with a given name.
    /// Use together with `--download` or provide content via STDIN.
    #[arg(long)]
    name: Option<String>,

    /// Reads a snippet by name and prints it to STDOUT.
    #[arg(long)]
    read: Option<String>,

    /// Deletes a snippet by name.
    #[arg(long)]
    delete: Option<String>,

    /// Downloads snippet content from a given URL when creating a snippet.
    #[arg(long)]
    download: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Snippet {
    pub content: String,
    pub created_at: OffsetDateTime,
}

pub trait SnippetStorage {
    fn load(&mut self) -> Result<HashMap<String, Snippet>>;
    fn save(&mut self, data: &HashMap<String, Snippet>) -> Result<()>;
}

pub struct JsonStorage {
    path: String,
}

impl JsonStorage {
    pub fn new(path: String) -> Self {
        Self { path }
    }
}

impl SnippetStorage for JsonStorage {
    fn load(&mut self) -> Result<HashMap<String, Snippet>> {
        if !std::path::Path::new(&self.path).exists() {
            return Ok(HashMap::new());
        }

        let file = File::open(&self.path)
            .with_context(|| format!("Cannot open JSON file '{}'", self.path))?;

        let data = serde_json::from_reader(file)
            .with_context(|| format!("Cannot parse JSON file '{}'", self.path))?;

        Ok(data)
    }

    fn save(&mut self, data: &HashMap<String, Snippet>) -> Result<()> {
        let file = File::create(&self.path)
            .with_context(|| format!("Cannot create JSON file '{}'", self.path))?;

        serde_json::to_writer_pretty(file, data)
            .with_context(|| "Failed to write JSON".to_string())?;

        Ok(())
    }
}

pub struct SqliteStorage {
    conn: Connection,
}

impl SqliteStorage {
    pub fn new(path: String) -> Result<Self> {
        let conn = Connection::open(&path)
            .with_context(|| format!("Failed to open SQLite '{}'", path))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS snippets (
                name TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL
            )",
            [],
        )?;

        Ok(Self { conn })
    }
}

impl SnippetStorage for SqliteStorage {
    fn load(&mut self) -> Result<HashMap<String, Snippet>> {
        let mut stmt = self.conn.prepare("SELECT name, content, created_at FROM snippets")?;
        let rows = stmt.query_map([], |row| {
            let name: String = row.get(0)?;
            let content: String = row.get(1)?;
            let created_at_str: String = row.get(2)?;
            let created_at = OffsetDateTime::parse(&created_at_str, &Rfc3339)
                .unwrap();

            Ok((name, Snippet { content, created_at }))
        })?;

        let mut map = HashMap::new();
        for r in rows {
            let (name, sn) = r?;
            map.insert(name, sn);
        }

        Ok(map)
    }

    fn save(&mut self, data: &HashMap<String, Snippet>) -> Result<()> {
        self.conn.execute("DELETE FROM snippets", [])?;

        for (name, snippet) in data {
            self.conn.execute(
                "INSERT INTO snippets (name, content, created_at) VALUES (?, ?, ?)",
                params![
                    name,
                    snippet.content,
                    snippet.created_at.format(&Rfc3339)?
                ],
            )?;
        }

        Ok(())
    }
}

fn init_logging() {
    let level = env::var("SNIPPETS_APP_LOG_LEVEL").unwrap_or("info".into());
    let log_path = env::var("SNIPPETS_APP_LOG_PATH").unwrap_or("snippets.log".into());

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(level))
        .with_writer(std::fs::File::create(log_path).unwrap())
        .init();
}

fn init_storage() -> Result<Box<dyn SnippetStorage>> {
    let config = env::var("SNIPPETS_APP_STORAGE")
        .context("Environment variable SNIPPETS_APP_STORAGE is not set")?;

    let (kind, path) = config
        .split_once(':')
        .context("SNIPPETS_APP_STORAGE must be JSON:path or SQLITE:path")?;

    match kind {
        "JSON" => Ok(Box::new(JsonStorage::new(path.into()))),
        "SQLITE" => Ok(Box::new(SqliteStorage::new(path.into())?)),
        _ => anyhow::bail!("Unknown storage provider"),
    }
}

fn main() -> Result<()> {
    init_logging();
    let args = Args::parse();

    let mut storage = init_storage()?;
    let mut map = storage.load()?;

    if let Some(name) = args.name {
        let content = if let Some(url) = args.download {
            info!("Downloading snippet from {url}");
            reqwest::blocking::get(url)?.text()?
        } else {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            buf
        };

        let sn = Snippet {
            content,
            created_at: OffsetDateTime::now_utc(),
        };

        map.insert(name, sn);
        storage.save(&map)?;
        info!("Snippet saved");
        println!("Snippet saved.");
        return Ok(());
    }

    if let Some(name) = args.read {
        if let Some(sn) = map.get(&name) {
            println!("Created at: {}", sn.created_at.format(&Rfc3339)?);
            println!("{}", sn.content);
        } else {
            println!("Snippet not found.");
        }
        return Ok(());
    }

    if let Some(name) = args.delete {
        if map.remove(&name).is_some() {
            storage.save(&map)?;
            println!("Snippet deleted.");
        } else {
            println!("Snippet not found.");
        }
        return Ok(());
    }

    println!("Usage:");
    println!("  --name <name> [--download URL]");
    println!("  --read <name>");
    println!("  --delete <name>");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_snippet_creation() {
        let s = Snippet {
            content: "hello".into(),
            created_at: OffsetDateTime::now_utc(),
        };
        assert_eq!(s.content, "hello");
    }

    #[test]
    fn test_json_storage_save_load() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("snippets.json");
        let path_str = file_path.to_str().unwrap().to_string();

        let mut storage = JsonStorage::new(path_str.clone());
        let mut map = HashMap::new();

        map.insert("test".into(), Snippet {
            content: "content".into(),
            created_at: OffsetDateTime::now_utc(),
        });

        storage.save(&map).unwrap();

        let mut storage = JsonStorage::new(path_str);
        let loaded = storage.load().unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded["test"].content, "content");
    }

    #[test]
    fn test_sqlite_storage_save_load() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("db.sqlite");
        let path_str = file_path.to_str().unwrap().to_string();

        let mut storage = SqliteStorage::new(path_str.clone()).unwrap();
        let mut map = HashMap::new();

        map.insert("hello".into(), Snippet {
            content: "world".into(),
            created_at: OffsetDateTime::now_utc(),
        });

        storage.save(&map).unwrap();

        let mut storage = SqliteStorage::new(path_str).unwrap();
        let loaded = storage.load().unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded["hello"].content, "world");
    }

    use assert_cmd::Command;

    #[test]
    fn test_cli_shows_usage_without_args() {
        let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();
        cmd.assert()
            .success()
            .stdout(predicates::str::contains("Usage:"));
    }

    #[test]
fn test_cli_create_snippet_json() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("file.json");
    let storage = format!("JSON:{}", file_path.to_str().unwrap());

    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();
    cmd.env("SNIPPETS_APP_STORAGE", &storage)
        .write_stdin("hello world")
        .args(["--name", "test"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Snippet saved"));

    let mut storage = JsonStorage::new(file_path.to_str().unwrap().into());
    let map = storage.load().unwrap();
    assert_eq!(map["test"].content, "hello world");
}
}