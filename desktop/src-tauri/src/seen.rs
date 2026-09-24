//! Which version's release notes the person has already seen (ADR-0040).
//!
//! One line of text in a file of its own, in the application's data
//! directory. Not the credential vault, because it is not a secret, and not
//! `localStorage` (CLAUDE.md §2.8), because clearing the WebView's data would
//! bring the notes back for no reason.
//!
//! The file holds a version and nothing else, and writing is refused for
//! anything that does not look like one: whatever the WebView passes ends up on
//! disk.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

const FILE: &str = "release-notes-seen";

#[derive(Debug, thiserror::Error)]
pub enum SeenError {
    #[error("app data directory unavailable: {0}")]
    NoDirectory(String),
    #[error("could not read or write the record: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a version: {0:?}")]
    NotAVersion(String),
}

/// Vague on purpose, like the vault's: the real message can name paths.
impl serde::Serialize for SeenError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("release notes record unavailable")
    }
}

fn record(app: &AppHandle) -> Result<PathBuf, SeenError> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join(FILE))
        .map_err(|e| SeenError::NoDirectory(e.to_string()))
}

/// Three dot-separated numbers, like `2.0.1`.
fn is_version(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty() && part.len() <= 9 && part.bytes().all(|b| b.is_ascii_digit())
        })
}

/// `None` when nothing was ever recorded, which is the ordinary first run.
fn read(path: &Path) -> Result<Option<String>, SeenError> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let text = text.trim();
            // Um arquivo estragado vale como "nada visto": o pior que acontece é
            // mostrar as novidades uma vez a mais.
            Ok(is_version(text).then(|| text.to_owned()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn write(path: &Path, version: &str) -> Result<(), SeenError> {
    if !is_version(version) {
        return Err(SeenError::NotAVersion(version.to_owned()));
    }
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)?;
    }
    std::fs::write(path, version)?;
    Ok(())
}

#[tauri::command]
pub fn release_notes_seen(app: AppHandle) -> Result<Option<String>, SeenError> {
    read(&record(&app)?)
}

#[tauri::command]
pub fn release_notes_mark_seen(app: AppHandle, version: String) -> Result<(), SeenError> {
    write(&record(&app)?, &version)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("ldktela-seen-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        directory.join(FILE)
    }

    #[test]
    fn nothing_recorded_is_not_an_error() {
        let path = scratch("empty");
        assert_eq!(read(&path).expect("leitura"), None);
    }

    #[test]
    fn a_recorded_version_comes_back() {
        let path = scratch("roundtrip");
        write(&path, "2.0.1").expect("gravacao");
        assert_eq!(read(&path).expect("leitura").as_deref(), Some("2.0.1"));
    }

    /// O WebView escolhe o que vai para o disco. Só uma versão passa.
    #[test]
    fn anything_that_is_not_a_version_is_refused() {
        let path = scratch("refused");
        for bad in ["", "2.0", "2.0.1.4", "../2.0.1", "2.0.x", "2.0.1\nrm"] {
            assert!(write(&path, bad).is_err(), "{bad:?} nao devia ser gravado");
        }
        assert_eq!(read(&path).expect("leitura"), None);
    }

    #[test]
    fn a_damaged_record_reads_as_nothing_seen() {
        let path = scratch("damaged");
        if let Some(directory) = path.parent() {
            std::fs::create_dir_all(directory).expect("diretorio");
        }
        std::fs::write(&path, "lixo").expect("gravacao");
        assert_eq!(read(&path).expect("leitura"), None);
    }
}
