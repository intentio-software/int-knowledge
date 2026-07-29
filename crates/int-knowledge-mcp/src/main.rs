//! `int-knowledge-mcp` — an MCP server over an Intentio Knowledge vault.
//!
//! Runs as a plain stdio process, so it works whether or not the desktop app is
//! open and needs no ports, tokens or background daemon:
//!
//! ```text
//! claude mcp add knowledge -- int-knowledge-mcp ~/Notes
//! ```

mod mcp;
mod tools;
mod workspace;

use std::path::PathBuf;
use std::process::ExitCode;

use tools::VaultTools;
use workspace::Workspace;

const USAGE: &str = "\
int-knowledge-mcp — MCP server for Intentio Knowledge vaults

USAGE:
    int-knowledge-mcp [OPTIONS] [VAULT_PATH]...

ARGS:
    <VAULT_PATH>...    One or more vault folders. Each must already exist.

OPTIONS:
    --vault <PATH>     Add a vault (repeatable; same as a positional path)
    -h, --help         Print this help
    -V, --version      Print version

ENVIRONMENT:
    INT_KNOWLEDGE_VAULT    Comma-separated vault paths, used when none are given

The server speaks MCP over stdio. Register it with an agent, for example:

    claude mcp add knowledge -- int-knowledge-mcp ~/Notes
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let roots = match parse_args(&args) {
        Ok(Some(roots)) => roots,
        // --help / --version already printed.
        Ok(None) => return ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    let workspace = match Workspace::open(&roots) {
        Ok(workspace) => workspace,
        Err(message) => {
            eprintln!("error: {message}");
            return ExitCode::FAILURE;
        }
    };

    // Startup diagnostics go to stderr; stdout carries protocol traffic only.
    eprintln!("[knowledge] serving {} vault(s): {}", roots.len(), workspace.names().join(", "));

    match mcp::serve(VaultTools::new(workspace)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Collect vault roots from the command line, falling back to the environment.
///
/// `Ok(None)` means the process printed help or version and should exit cleanly.
fn parse_args(args: &[String]) -> Result<Option<Vec<PathBuf>>, String> {
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut index = 0usize;

    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("int-knowledge-mcp {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "--vault" => {
                let value = args.get(index + 1).ok_or("`--vault` needs a path")?;
                roots.push(expand(value));
                index += 2;
            }
            other if other.starts_with('-') => return Err(format!("unknown option: {other}")),
            other => {
                roots.push(expand(other));
                index += 1;
            }
        }
    }

    if roots.is_empty() {
        if let Ok(from_env) = std::env::var("INT_KNOWLEDGE_VAULT") {
            roots.extend(from_env.split(',').map(str::trim).filter(|p| !p.is_empty()).map(expand));
        }
    }

    if roots.is_empty() {
        return Err("no vault given".into());
    }
    Ok(Some(roots))
}

/// Expand a leading `~`, which clients routinely pass through unexpanded.
fn expand(path: &str) -> PathBuf {
    let trimmed = path.trim();
    if trimmed == "~" || trimmed.starts_with("~/") {
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            return if trimmed == "~" { home } else { home.join(&trimmed[2..]) };
        }
    }
    PathBuf::from(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn collects_positional_and_flagged_vaults() {
        let roots = parse_args(&to_args(&["/a", "--vault", "/b"])).unwrap().unwrap();
        assert_eq!(roots, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
    }

    #[test]
    fn rejects_unknown_options_and_missing_values() {
        assert!(parse_args(&to_args(&["--nope"])).is_err());
        assert!(parse_args(&to_args(&["--vault"])).is_err());
        assert!(parse_args(&[]).is_err());
    }

    #[test]
    fn expands_home_relative_paths() {
        std::env::set_var("HOME", "/home/test");
        assert_eq!(expand("~/Notes"), PathBuf::from("/home/test/Notes"));
        assert_eq!(expand("~"), PathBuf::from("/home/test"));
        assert_eq!(expand("/absolute"), PathBuf::from("/absolute"));
    }
}
