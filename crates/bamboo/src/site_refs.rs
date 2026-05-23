//! Builds the path-to-url registry consumed by shortcodes that reference
//! other content (e.g. `[[link::doc.md]]`).

use crate::error::{BambooError, Result};
use crate::parsing::extract_frontmatter;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub(crate) fn build_ref_registry(
    input_dir: &Path,
    reserved_dirs: &[PathBuf],
) -> Result<HashMap<String, String>> {
    let content_dir = input_dir.join("content");
    let mut registry = HashMap::new();

    if !content_dir.exists() {
        return Ok(registry);
    }

    for entry in WalkDir::new(&content_dir).min_depth(1).into_iter() {
        let entry = entry.map_err(|error| BambooError::WalkDir {
            path: content_dir.clone(),
            message: error.to_string(),
        })?;

        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        if path
            .extension()
            .map(|extension| extension != "md")
            .unwrap_or(true)
        {
            continue;
        }

        let filename = path.file_name().unwrap().to_string_lossy();
        if filename.starts_with('_') && filename != "_index.md" {
            continue;
        }

        let relative = path
            .strip_prefix(&content_dir)
            .map_err(|_| BambooError::InvalidPath {
                path: path.to_path_buf(),
            })?;

        let relative_str = relative.to_string_lossy().replace('\\', "/");

        let parent_dir = path.parent().unwrap_or(path);
        let is_in_posts = parent_dir
            .strip_prefix(&content_dir)
            .map(|relative_parent| {
                relative_parent
                    .components()
                    .next()
                    .map(|component| component.as_os_str() == "posts")
                    .unwrap_or(false)
            })
            .unwrap_or(false);

        let is_in_collection = reserved_dirs
            .iter()
            .any(|reserved| parent_dir.starts_with(reserved) && !is_in_posts);

        let url = if filename == "_index.md"
            && relative
                .parent()
                .map(|parent| parent == Path::new(""))
                .unwrap_or(true)
        {
            "/".to_string()
        } else if is_in_posts {
            let (_, slug) =
                if let Some(parsed) = crate::parsing::parse_date_from_filename(&filename) {
                    parsed
                } else {
                    (
                        String::new(),
                        filename
                            .strip_suffix(".md")
                            .unwrap_or(&filename)
                            .to_string(),
                    )
                };
            format!("/posts/{}/", slug)
        } else if is_in_collection {
            let relative_to_content = path
                .strip_prefix(&content_dir)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let collection_name = relative_to_content.split('/').next().unwrap_or("");
            let within_collection = path
                .strip_prefix(content_dir.join(collection_name))
                .unwrap_or(path);
            let file_slug = filename
                .strip_suffix(".md")
                .unwrap_or(&filename)
                .to_string();
            let nested_dir = within_collection.parent().unwrap_or(Path::new(""));
            let slug = if nested_dir == Path::new("") {
                file_slug
            } else {
                let dir_part = nested_dir.to_string_lossy().replace('\\', "/");
                format!("{}/{}", dir_part, file_slug)
            };
            format!("/{}/{}/", collection_name, slug)
        } else {
            let relative_dir = relative.parent().unwrap_or(Path::new(""));
            let file_slug = if filename == "_index.md" {
                "index".to_string()
            } else {
                filename
                    .strip_suffix(".md")
                    .unwrap_or(&filename)
                    .to_string()
            };

            let slug = if relative_dir == Path::new("") {
                file_slug.clone()
            } else {
                let dir_part = relative_dir.to_string_lossy().replace('\\', "/");
                if file_slug == "index" {
                    dir_part.to_string()
                } else {
                    format!("{}/{}", dir_part, file_slug)
                }
            };

            if slug == "index" {
                "/".to_string()
            } else {
                format!("/{}/", slug)
            }
        };

        let url = if let Ok(file_content) = fs::read_to_string(path)
            && let Ok((frontmatter, _)) = extract_frontmatter(&file_content, path)
            && let Some(permalink) = frontmatter.get_string("permalink")
        {
            let clean = permalink.trim_matches('/');
            if clean.is_empty() {
                "/".to_string()
            } else {
                format!("/{}/", clean)
            }
        } else {
            url
        };

        registry.insert(relative_str.clone(), url.clone());
        registry.insert(filename.to_string(), url.clone());

        let without_extension = relative_str.strip_suffix(".md").unwrap_or(&relative_str);
        if without_extension != relative_str {
            registry.insert(without_extension.to_string(), url.clone());
        }
    }

    Ok(registry)
}
