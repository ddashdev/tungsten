//! Input path collection: globs an input, resolves its base dir, loads its `.tmeta` and web assets.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::core::assets::asset::{self, AssetMeta, WebAsset};
use crate::log;
use crate::utils::config::InputConfig;
use anyhow::{Context, Result};
use glob::glob;
use relative_path::RelativePathBuf;

pub fn collect_paths(pattern: &str) -> Result<Vec<PathBuf>> {
    let paths = glob(pattern)
        .with_context(|| {
            format!(
                "Invalid glob pattern \"{}\"\n  Hint: Example: path = \"assets/**/*.png\"",
                pattern
            )
        })?
        .filter_map(|entry| match entry {
            Ok(p) => {
                let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                if asset::is_supported_ext(ext) {
                    Some(p)
                } else {
                    None
                }
            }
            Err(e) => {
                log!(warn, "skipping unreadable path: {}", e);
                None
            }
        })
        .collect();
    Ok(paths)
}

pub fn glob_base(pattern: &str) -> String {
    pattern
        .split('*')
        .next()
        .unwrap_or("")
        .trim_end_matches('/')
        .to_string()
}

pub fn relative_path(path: &Path, base: &str) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn load_input_meta(base_path: &str) -> AssetMeta {
    if base_path.is_empty() {
        return AssetMeta::default();
    }
    AssetMeta::load_for(Path::new(base_path)).unwrap_or_default()
}

/// The deepest directory that holds everything `pattern` can match: the text
/// before the last `/` that precedes the first wildcard (`Assets/Anim*/**`
/// -> `Assets`). Empty means the current directory. A pattern without
/// wildcards is its own root.
pub fn walk_root(pattern: &str) -> &str {
    match pattern.find(['*', '?', '[']) {
        None => pattern.trim_end_matches('/'),
        Some(i) => match pattern[..i].rfind('/') {
            Some(0) => "/",
            Some(j) => &pattern[..j],
            None => "",
        },
    }
}

/// An input's web assets and the number of sidecar problems warned about.
pub struct WebAssets {
    pub assets: HashMap<RelativePathBuf, WebAsset>,
    pub warnings: u32,
}

/// The input's web assets: `[inputs.<name>.web]` merged with the `[web]`
/// tables of directory sidecars at and below the input's walk root. Sidecar
/// keys are relative to the directory the sidecar describes, which doesn't
/// have to exist, and only keys the input's glob matches are kept. Entries
/// naming the same asset are deduplicated by codegen name: `tungsten.toml`
/// wins over sidecars and a deeper sidecar over a shallower one.
pub fn collect_web_assets(input: &InputConfig) -> WebAssets {
    let codegen_base = glob_base(&input.path);
    let pattern = glob::Pattern::new(&input.path).ok();
    let match_options = glob::MatchOptions {
        require_literal_separator: true,
        ..Default::default()
    };

    let mut warnings = 0;
    let mut by_name: HashMap<String, (RelativePathBuf, WebAsset)> = HashMap::new();
    let mut insert = |key: String, asset: WebAsset, source: &str, warnings: &mut u32| {
        let name = super::codegen_write::web_asset_name(&key, &codegen_base);
        if let Some((_, previous)) =
            by_name.insert(name, (RelativePathBuf::from(key.clone()), asset.clone()))
            && previous.id != asset.id
        {
            log!(
                warn,
                "Web asset \"{}\" is declared more than once; using id {} from {}",
                key,
                asset.id,
                source
            );
            *warnings += 1;
        }
    };

    let root = walk_root(&input.path);
    let mut targets = sidecar_targets(root);
    targets.sort_by_key(|target| (target.matches('/').count(), target.clone()));

    for target in targets {
        let target_path = Path::new(&target);
        let Some(meta_path) = asset::find_meta_file(target_path) else {
            continue;
        };
        let meta = match AssetMeta::load_file(&meta_path) {
            Ok(meta) => meta,
            Err(e) => {
                log!(warn, "{:#}", e);
                warnings += 1;
                continue;
            }
        };
        if meta.web.is_empty() {
            continue;
        }
        if target_path.is_file() {
            log!(
                warn,
                "Ignoring [web] in \"{}\": only directory sidecars can declare web assets",
                meta_path.display()
            );
            warnings += 1;
            continue;
        }

        let source = format!("\"{}\"", meta_path.display());
        let mut entries: Vec<_> = meta.web.into_iter().collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (rel, asset) in entries {
            let Some(key) = sidecar_key(&target, rel.as_str()) else {
                log!(
                    warn,
                    "Ignoring web asset \"{}\" in {}: keys must be relative paths inside the directory",
                    rel,
                    source
                );
                warnings += 1;
                continue;
            };
            if pattern
                .as_ref()
                .is_some_and(|p| !p.matches_with(&key, match_options))
            {
                log!(
                    debug,
                    "Web asset \"{}\" doesn't match \"{}\"",
                    key,
                    input.path
                );
                continue;
            }
            insert(key, asset, &source, &mut warnings);
        }
    }

    for (key, asset) in &input.web {
        insert(
            key.to_string(),
            asset.clone(),
            "tungsten.toml",
            &mut warnings,
        );
    }

    WebAssets {
        assets: by_name.into_values().collect(),
        warnings,
    }
}

/// `/`-joins two path texts, treating an empty side as absent.
fn join_text(dir: &str, name: &str) -> String {
    match (dir, name) {
        ("", name) => name.to_string(),
        (dir, "") => dir.to_string(),
        (dir, name) => format!("{}/{}", dir.trim_end_matches('/'), name),
    }
}

/// A sidecar key resolved against the directory text it belongs to, or `None`
/// if it is empty, absolute or escapes the directory.
fn sidecar_key(dir: &str, key: &str) -> Option<String> {
    let key = key.replace('\\', "/");
    if key.starts_with('/') {
        return None;
    }
    let normalized = relative_path::RelativePath::new(&key).normalize();
    if normalized.as_str().is_empty()
        || normalized.components().next() == Some(relative_path::Component::ParentDir)
    {
        return None;
    }
    Some(join_text(dir, normalized.as_str()))
}

/// The paths described by every sidecar at and below `root`, as `/`-joined
/// text that keeps `root` spelled as given, plus `root` itself.
fn sidecar_targets(root: &str) -> Vec<String> {
    let mut targets = Vec::new();
    if !root.is_empty() {
        targets.push(root.to_string());
    }
    let walk_from = if root.is_empty() { "." } else { root };
    let mut ancestors = Vec::new();
    walk_sidecars(Path::new(walk_from), root, &mut ancestors, &mut targets);
    targets.sort();
    targets.dedup();
    targets
}

/// Follows directory symlinks like `glob` does, skipping any that loop back
/// to a directory already on the current path.
fn walk_sidecars(
    dir: &Path,
    dir_text: &str,
    ancestors: &mut Vec<PathBuf>,
    targets: &mut Vec<String>,
) {
    let Ok(canonical) = dir.canonicalize() else {
        return;
    };
    if ancestors.contains(&canonical) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    ancestors.push(canonical);
    for entry in entries.filter_map(|entry| entry.ok()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if path.is_dir() {
            walk_sidecars(&path, &join_text(dir_text, &name), ancestors, targets);
        } else if let Some(target) = asset::meta_target(&name) {
            targets.push(join_text(dir_text, target));
        }
    }
    ancestors.pop();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Temp dir text with `/` separators, plus an input over `<root>/Anims`.
    fn setup(glob_tail: &str, web_toml: &str) -> (tempfile::TempDir, String, InputConfig) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_string_lossy().replace('\\', "/");
        let input = toml::from_str(&format!(
            "path = \"{root}/{glob_tail}\"\noutput_path = \"out.luau\"\n{web_toml}"
        ))
        .unwrap();
        (tmp, root, input)
    }

    fn write(root: &str, rel: &str, contents: &str) {
        let path = Path::new(root).join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    /// Codegen names and ids, sorted.
    fn names(input: &InputConfig) -> Vec<(String, u64)> {
        let base = glob_base(&input.path);
        let mut names: Vec<(String, u64)> = collect_web_assets(input)
            .assets
            .iter()
            .map(|(k, v)| {
                (
                    super::super::codegen_write::web_asset_name(k.as_str(), &base),
                    v.id,
                )
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn walk_root_stops_at_the_last_complete_segment() {
        assert_eq!(walk_root("Assets/Anims/**/*"), "Assets/Anims");
        assert_eq!(walk_root("Assets/Anim*/**/*"), "Assets");
        assert_eq!(walk_root("./Assets/**/*.png"), "./Assets");
        assert_eq!(walk_root("**/*"), "");
        assert_eq!(walk_root("/**/*"), "/");
        assert_eq!(walk_root("Assets/logo.png"), "Assets/logo.png");
    }

    #[test]
    fn sidecar_keys_are_validated() {
        assert_eq!(
            sidecar_key("A", "Walk.rbxm").as_deref(),
            Some("A/Walk.rbxm")
        );
        assert_eq!(
            sidecar_key("./A", "./M/Walk.rbxm").as_deref(),
            Some("./A/M/Walk.rbxm")
        );
        assert_eq!(
            sidecar_key("/abs/A", "M\\Run.rbxm").as_deref(),
            Some("/abs/A/M/Run.rbxm")
        );
        assert_eq!(sidecar_key("", "Walk.rbxm").as_deref(), Some("Walk.rbxm"));
        assert_eq!(
            sidecar_key("A", "M/../Walk.rbxm").as_deref(),
            Some("A/Walk.rbxm")
        );
        assert_eq!(sidecar_key("A", "../Escape.rbxm"), None);
        assert_eq!(sidecar_key("A", "/Abs.rbxm"), None);
        assert_eq!(sidecar_key("A", ""), None);
        assert_eq!(sidecar_key("A", "."), None);
    }

    #[test]
    fn sidecar_web_assets_merge_with_config() {
        let (_tmp, root, input) = setup("Anims/**/*", "");
        std::fs::create_dir_all(format!("{root}/Anims")).unwrap();
        write(
            &root,
            "Anims.tmeta.toml",
            "type = \"animation\"\n[web]\n\"Mannequin/Walk.rbxm\" = { id = 1 }\n\"Idle.rbxm\" = { id = 2 }\n",
        );
        // Mannequin/ itself doesn't exist, only its sidecar.
        write(
            &root,
            "Anims/Mannequin.tmeta",
            "[web]\n\"Death.rbxm\" = { id = 3 }\n\"Walk.rbxm\" = { id = 4 }\n",
        );
        let config_web = format!(
            "[web]\n\"{root}/Anims/Idle.rbxm\" = {{ id = 5 }}\n\"Mannequin/Death.rbxm\" = {{ id = 6 }}\n"
        );
        let input = InputConfig {
            web: toml::from_str::<InputConfig>(&format!(
                "path = \"x\"\noutput_path = \"x\"\n{config_web}"
            ))
            .unwrap()
            .web,
            ..input
        };

        assert_eq!(
            names(&input),
            vec![
                ("Idle.rbxm".to_string(), 5),
                ("Mannequin/Death.rbxm".to_string(), 6),
                ("Mannequin/Walk.rbxm".to_string(), 4),
            ],
            "tungsten.toml (full or base-relative keys) beats sidecars, deeper sidecars beat shallower ones"
        );
    }

    #[test]
    fn base_sidecar_works_without_the_directory() {
        let (_tmp, root, input) = setup("Anims/**/*", "");
        write(
            &root,
            "Anims.tmeta.toml",
            "[web]\n\"Walk.rbxm\" = { id = 7 }\n",
        );
        assert_eq!(names(&input), vec![("Walk.rbxm".to_string(), 7)]);
    }

    #[test]
    fn dot_segments_in_the_base_are_kept() {
        let (_tmp, root, input) = setup("./Anims/**/*", "");
        write(
            &root,
            "Anims.tmeta.toml",
            "[web]\n\"./Walk.rbxm\" = { id = 1 }\n",
        );
        let keys: Vec<String> = collect_web_assets(&input)
            .assets
            .keys()
            .map(|k| k.to_string())
            .collect();
        assert_eq!(keys, vec![format!("{root}/./Anims/Walk.rbxm")]);
        assert_eq!(names(&input), vec![("Walk.rbxm".to_string(), 1)]);
    }

    #[test]
    fn entries_outside_the_glob_are_dropped() {
        let (_tmp, root, input) = setup("Assets/**/*.png", "");
        write(&root, "Assets/icon.png", "");
        write(
            &root,
            "Assets/Anims.tmeta",
            "[web]\n\"Walk.rbxm\" = { id = 1 }\n\"Badge.png\" = { id = 2 }\n",
        );
        assert_eq!(names(&input), vec![("Anims/Badge.png".to_string(), 2)]);
    }

    #[test]
    fn mid_segment_wildcards_walk_from_the_parent() {
        let (_tmp, root, input) = setup("Assets/Anim*/**/*", "");
        std::fs::create_dir_all(format!("{root}/Assets/Animations")).unwrap();
        write(
            &root,
            "Assets/Animations.tmeta",
            "[web]\n\"Walk.rbxm\" = { id = 1 }\n",
        );
        write(
            &root,
            "Assets/Sounds.tmeta",
            "[web]\n\"Boom.ogg\" = { id = 2 }\n",
        );
        let keys: Vec<String> = collect_web_assets(&input)
            .assets
            .keys()
            .map(|k| k.to_string())
            .collect();
        assert_eq!(keys, vec![format!("{root}/Assets/Animations/Walk.rbxm")]);
    }

    #[test]
    fn invalid_keys_and_file_sidecars_warn() {
        let (_tmp, root, input) = setup("Anims/**/*", "");
        write(&root, "Anims/logo.png", "");
        write(
            &root,
            "Anims/logo.png.tmeta",
            "[web]\n\"x.png\" = { id = 1 }\n",
        );
        write(
            &root,
            "Anims.tmeta",
            "[web]\n\"../Escape.rbxm\" = { id = 2 }\n\"Walk.rbxm\" = { id = 3 }\n",
        );
        write(&root, "Anims/Broken.tmeta", "[web\n");
        let web = collect_web_assets(&input);
        assert_eq!(web.warnings, 3);
        assert_eq!(web.assets.len(), 1);
    }
}
