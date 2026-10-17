//! Opt-in live API check. Prints counts only, never credentials or diff contents.
use diffy::github::{Backend, OctocrabBackend, Repository};
fn main() -> Result<(), String> {
    let repo = std::env::args()
        .nth(1)
        .and_then(|s| Repository::parse(&format!("https://github.com/{s}")))
        .ok_or("Usage: cargo run --example github-check -- OWNER/REPO [PR_NUMBER]")?;
    let page = OctocrabBackend.list(&repo, 1)?;
    println!(
        "GitHub listing succeeded: {} open PRs on first page; more={}",
        page.items.len(),
        page.more
    );
    let number = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .or_else(|| page.items.first().map(|p| p.number));
    if let Some(number) = number {
        let diff = OctocrabBackend.diff(&repo, number)?;
        println!("GitHub diff succeeded: {} files", diff.snapshot.files.len());
    }
    Ok(())
}
