//! Drive the vault sync against a real repository.
//!
//! Inspecting is read-only; syncing writes and pushes, so it only happens when
//! asked for explicitly.
fn main() {
    let vault = std::env::args().nth(1).expect("a vault path");
    let path = std::path::PathBuf::from(&vault);
    let args: Vec<String> = std::env::args().collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);

    let status = int_knowledge_lib::git_sync::status(&path);
    println!(
        "status: repo={} remote={} branch={:?} dirty={} ahead={} behind={} blocked={:?}",
        status.is_repo, status.has_remote, status.branch, status.dirty,
        status.ahead, status.behind, status.blocked
    );

    if has("--recent") {
        println!("recent:");
        for c in int_knowledge_lib::git_sync::recent_changes(&path, 8) {
            println!(
                "  {} {:9} {:22} {}",
                c.at.get(..16).unwrap_or(&c.at),
                c.kind,
                c.author.unwrap_or_else(|| "—".into()),
                c.path
            );
        }
    }

    if has("--receive") {
        let outcome = int_knowledge_lib::git_sync::receive(&path);
        println!("recv  : changed={} — {}", outcome.changed, outcome.message);
    } else if has("--sync") {
        let outcome = int_knowledge_lib::git_sync::sync(&path);
        println!("sync  : changed={} — {}", outcome.changed, outcome.message);
    }
}
