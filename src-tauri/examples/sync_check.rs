//! Drive the vault sync against a real repository, to check the behaviour that
//! matters: that it syncs, and that it refuses to guess when it cannot.
fn main() {
    let vault = std::env::args().nth(1).expect("a vault path");
    let path = std::path::PathBuf::from(&vault);
    let status = int_knowledge_lib::git_sync::status(&path);
    println!(
        "status: repo={} remote={} branch={:?} dirty={} ahead={} behind={} blocked={:?}",
        status.is_repo, status.has_remote, status.branch, status.dirty, status.ahead, status.behind, status.blocked
    );
    let outcome = int_knowledge_lib::git_sync::sync(&path);
    println!("sync  : changed={} blocked={:?}\n        {}", outcome.changed, outcome.blocked.is_some(), outcome.message);
}
