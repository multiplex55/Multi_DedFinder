use std::path::Path;

use anyhow::{Context, Result};

pub mod json;
pub mod text;

pub fn ensure_parent_dir(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };

    if parent.as_os_str().is_empty() {
        return Ok(());
    }

    std::fs::create_dir_all(parent).with_context(|| {
        format!(
            "failed to create parent directory {} for output file {}",
            parent.display(),
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn unique_temp_path(test_name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "eve-ded-output-{test_name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn bare_filename_parent_is_no_op() {
        ensure_parent_dir(Path::new("route.json"))
            .expect("bare filename should not require parent creation");
    }

    #[test]
    fn parent_creation_error_mentions_parent_and_target_paths() {
        let parent = unique_temp_path("blocked-parent");
        fs::write(&parent, "not a directory").expect("fixture file should write");
        let target = parent.join("route.json");

        let error = ensure_parent_dir(&target).expect_err("file parent should fail");
        let message = format!("{error:#}");

        assert!(message.contains("failed to create parent directory"));
        assert!(message.contains(&parent.display().to_string()));
        assert!(message.contains(&target.display().to_string()));

        let _ = fs::remove_file(parent);
    }
}
