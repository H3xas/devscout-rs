//! Compare a rendered page with the committed one, or write it under `BLESS=1`.
//!
//! The write is deliberately narrow: an environment variable renames the
//! expectation, never the check. A page whose structure is wrong is refused
//! before this is reached, so a bless cannot commit a page the checks would
//! have rejected.

use std::path::Path;

/// What a sync did with one page.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Written,
    Unchanged,
}

/// Compare `content` with `path`, or write it when `bless` is set. Returns the
/// outcome, or the drift message that names the page and the command that
/// regenerates it.
pub fn sync_page(path: &Path, content: &str, bless: bool) -> Result<Outcome, String> {
    if bless {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        std::fs::write(path, content)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        return Ok(Outcome::Written);
    }
    let committed = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    if committed == content {
        return Ok(Outcome::Unchanged);
    }
    Err(format!(
        "{} is stale -- regenerate it with `{}`",
        path.display(),
        super::pages::REGENERATE
    ))
}
