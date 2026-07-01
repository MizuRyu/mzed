//! Standalone PoC: watch Zed's DB and log the active project on every switch.
//! Run this, then switch projects in Zed and confirm log lines appear.

#[path = "../zed.rs"]
mod zed;

fn main() -> anyhow::Result<()> {
    let db = zed::default_zed_db_path()
        .ok_or_else(|| anyhow::anyhow!("Zed stable DB not found. Is Zed installed?"))?;
    eprintln!("watching: {}", db.display());

    zed::watch(&db, |active| match active {
        Some(p) => eprintln!("[active] {}  ({})", p.paths, p.timestamp),
        None => eprintln!("[active] <none>"),
    })?;

    Ok(())
}
