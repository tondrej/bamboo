//! Loader for the `data/` directory and the nested-insert helpers used to
//! merge sibling files into the same parent object.

use crate::error::{BambooError, IoContext, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const MAX_DATA_DEPTH: usize = 10;

pub(crate) fn load_data(input_dir: &Path) -> Result<HashMap<String, Value>> {
    let data_dir = input_dir.join("data");
    let mut data = HashMap::new();

    if !data_dir.exists() {
        return Ok(data);
    }

    for entry in WalkDir::new(&data_dir)
        .min_depth(1)
        .max_depth(MAX_DATA_DEPTH)
        .into_iter()
    {
        let entry = entry.map_err(|error| BambooError::WalkDir {
            path: data_dir.clone(),
            message: error.to_string(),
        })?;

        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");

        if !["toml", "yaml", "yml", "json"].contains(&extension) {
            continue;
        }

        let relative = path
            .strip_prefix(&data_dir)
            .map_err(|_| BambooError::InvalidPath {
                path: path.to_path_buf(),
            })?;

        let content = fs::read_to_string(path).io_context("reading data file", path)?;

        let value: Value = match extension {
            "toml" => toml::from_str(&content).map_err(|error| BambooError::TomlParse {
                path: path.to_path_buf(),
                message: error.to_string(),
            })?,
            "yaml" | "yml" => {
                serde_yml::from_str(&content).map_err(|error| BambooError::YamlParse {
                    path: path.to_path_buf(),
                    message: error.to_string(),
                })?
            }
            "json" => serde_json::from_str(&content).map_err(|error| BambooError::JsonParse {
                path: path.to_path_buf(),
                message: error.to_string(),
            })?,
            _ => continue,
        };

        let key = build_data_key(relative);
        insert_nested_value(&mut data, &key, value);
    }

    Ok(data)
}

pub(crate) fn collect_assets(input_dir: &Path) -> Result<Vec<crate::types::Asset>> {
    let static_dir = input_dir.join("static");
    let mut assets = Vec::new();

    if !static_dir.exists() {
        return Ok(assets);
    }

    for entry in WalkDir::new(&static_dir).min_depth(1).into_iter() {
        let entry = entry.map_err(|error| BambooError::WalkDir {
            path: static_dir.clone(),
            message: error.to_string(),
        })?;

        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let relative = path.strip_prefix(&static_dir).unwrap();

        assets.push(crate::types::Asset {
            source: path.to_path_buf(),
            dest: PathBuf::from(relative),
        });
    }

    Ok(assets)
}

fn build_data_key(path: &Path) -> Vec<String> {
    let mut parts: Vec<String> = path
        .parent()
        .map(|parent| {
            parent
                .iter()
                .map(|segment| segment.to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();

    if let Some(stem) = path.file_stem() {
        parts.push(stem.to_string_lossy().to_string());
    }

    parts
}

trait NestedInsert {
    fn get_value(&self, key: &str) -> Option<&Value>;
    fn get_value_mut(&mut self, key: &str) -> Option<&mut Value>;
    fn insert_value(&mut self, key: String, value: Value);
    fn entry_or_insert(&mut self, key: String) -> &mut Value;
}

impl NestedInsert for HashMap<String, Value> {
    fn get_value(&self, key: &str) -> Option<&Value> {
        self.get(key)
    }
    fn get_value_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.get_mut(key)
    }
    fn insert_value(&mut self, key: String, value: Value) {
        self.insert(key, value);
    }
    fn entry_or_insert(&mut self, key: String) -> &mut Value {
        self.entry(key)
            .or_insert_with(|| Value::Object(serde_json::Map::new()))
    }
}

impl NestedInsert for serde_json::Map<String, Value> {
    fn get_value(&self, key: &str) -> Option<&Value> {
        self.get(key)
    }
    fn get_value_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.get_mut(key)
    }
    fn insert_value(&mut self, key: String, value: Value) {
        self.insert(key, value);
    }
    fn entry_or_insert(&mut self, key: String) -> &mut Value {
        self.entry(key)
            .or_insert_with(|| Value::Object(serde_json::Map::new()))
    }
}

fn insert_nested_value<M: NestedInsert>(container: &mut M, keys: &[String], value: Value) {
    if keys.is_empty() {
        return;
    }

    if keys.len() == 1 {
        if let Some(existing) = container.get_value(&keys[0])
            && existing.is_object()
        {
            if let Value::Object(new_map) = &value
                && let Some(existing_map) = container
                    .get_value_mut(&keys[0])
                    .and_then(|v| v.as_object_mut())
            {
                for (key, val) in new_map {
                    existing_map.insert(key.clone(), val.clone());
                }
            }
            return;
        }
        container.insert_value(keys[0].clone(), value);
        return;
    }

    let first = &keys[0];
    let rest = &keys[1..];

    let nested = container.entry_or_insert(first.clone());

    if !nested.is_object() {
        return;
    }

    if let Value::Object(map) = nested {
        insert_nested_value(map, rest, value);
    }
}
