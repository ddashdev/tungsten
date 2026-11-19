//! Codegen output: seeds web assets and writes the Luau/`.d.ts` modules.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::core::assets::asset::{WebAsset, stem_name};
use crate::core::postsync::codegen::{CodegenEntry, strip_dpi_suffix};
use relative_path::RelativePathBuf;

use super::dispatch::SyncContext;
use super::paths::relative_path;

/// The codegen name for a web asset key: the key with the input's glob base
/// stripped (`Assets/Anims/Walk.rbxm` under `Assets/Anims` -> `Walk.rbxm`).
pub fn web_asset_name(key: &str, base_path: &str) -> String {
    let name = key.replace('\\', "/");
    let name = name
        .strip_prefix(base_path.trim_end_matches('/'))
        .unwrap_or(&name);
    name.trim_start_matches('/').to_string()
}

/// Seeds web assets into codegen entries.
pub fn seed_web_assets(
    web_assets: &HashMap<RelativePathBuf, WebAsset>,
    base_path: &str,
    strip_extension: bool,
    strip_whitespace: bool,
    codegen_entries: &mut Vec<CodegenEntry>,
) {
    for (rel_path, web_asset) in web_assets {
        let name = web_asset_name(rel_path.as_str(), base_path);
        let key = if strip_extension {
            name.trim_end_matches('.').to_string()
        } else {
            name.to_string()
        };
        let key = if strip_whitespace {
            key.chars().filter(|c| !c.is_whitespace()).collect()
        } else {
            key
        };
        codegen_entries.push(CodegenEntry::asset_id(key, web_asset.id));
    }
}

/// Source extensions keyed by asset name (`icons/arrow.png` -> `icons/arrow`
/// = `png`). `@Nx` variants also register their base name, which is the name
/// a DPI group is emitted under; the 1x file's extension wins when it exists.
pub fn source_extensions(paths: &[PathBuf], base_path: &str) -> HashMap<String, String> {
    let mut exts = HashMap::new();
    for path in paths {
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let name = stem_name(&relative_path(path, base_path));
        let stem_start = name.rfind('/').map_or(0, |slash| slash + 1);
        let base = format!(
            "{}{}",
            &name[..stem_start],
            strip_dpi_suffix(&name[stem_start..])
        );
        if base != name {
            exts.entry(base).or_insert_with(|| ext.to_string());
        }
        exts.insert(name, ext.to_string());
    }
    exts
}

/// Generates (and writes) the Luau module for this input from its codegen
/// entries, honoring the input's resolved `[codegen]` settings.
pub fn write_codegen(ctx: &mut SyncContext<'_>, mut codegen_entries: Vec<CodegenEntry>) {
    // Local assets are named by their stem (also the lockfile key), so the
    // extension is put back here; web asset keys already carry theirs.
    if !ctx.codegen.strip_extension {
        for entry in &mut codegen_entries {
            if let Some(ext) = ctx.source_exts.get(&entry.name) {
                entry.name = format!("{}.{}", entry.name, ext);
            }
        }
    }

    let strip_whitespace = ctx.codegen.strip_whitespace;
    if strip_whitespace {
        for entry in &mut codegen_entries {
            entry.name = entry.name.chars().filter(|c| !c.is_whitespace()).collect();
        }
    }

    if let Err(e) = crate::core::postsync::codegen::generate(
        codegen_entries,
        ctx.input_name,
        &ctx.codegen.style,
        ctx.codegen.strip_extension,
        ctx.output_path,
        ctx.codegen.ts_declaration,
    ) {
        crate::log!(
            warn,
            "Failed to write codegen for \"{}\": {}",
            ctx.input_name,
            e
        );
        ctx.errors += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::sync::Target;
    use crate::commands::sync::dispatch::test_context;
    use crate::core::postsync::lockfile::Lockfile;
    use std::collections::HashSet;

    fn local_paths() -> Vec<PathBuf> {
        [
            "Assets/Animations/Units/Pomni/Attack1.rbxm",
            "Assets/Animations/Units/v1.2/Idle.rbxm",
            "Assets/Animations/icons/search.png",
            "Assets/Animations/icons/search@2x.png",
            "Assets/Animations/icons/retina@3x.svg",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect()
    }

    #[test]
    fn source_extensions_are_keyed_by_asset_name() {
        let exts = source_extensions(&local_paths(), "Assets/Animations");

        assert_eq!(exts["Units/Pomni/Attack1"], "rbxm");
        assert_eq!(exts["Units/v1.2/Idle"], "rbxm");
        assert_eq!(exts["icons/search"], "png");
        assert_eq!(exts["icons/search@2x"], "png");
        assert_eq!(
            exts["icons/retina"], "svg",
            "a DPI group without a 1x file takes a variant's extension"
        );
    }

    fn generate_local_and_web(strip_extension: bool) -> String {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("Animations.luau");
        let mut lockfile = Lockfile::default();
        let mut expected = HashSet::new();
        let mut ctx = test_context(
            "Animations",
            Target::Debug,
            false,
            &mut lockfile,
            &mut expected,
        );
        ctx.base_path = "Assets/Animations";
        ctx.output_path = out.to_str().unwrap();
        ctx.codegen.style = "nested".to_string();
        ctx.codegen.strip_extension = strip_extension;
        ctx.source_exts = source_extensions(&local_paths(), "Assets/Animations");

        let web: HashMap<RelativePathBuf, WebAsset> = [(
            RelativePathBuf::from("Assets/Animations/Units/Bubble/Idle.rbxm"),
            WebAsset { id: 2 },
        )]
        .into_iter()
        .collect();
        let mut entries = Vec::new();
        seed_web_assets(
            &web,
            "Assets/Animations",
            strip_extension,
            false,
            &mut entries,
        );
        entries.push(CodegenEntry::asset_id("Units/Pomni/Attack1".to_string(), 1));
        entries.push(CodegenEntry::dpi_group(
            "icons/search".to_string(),
            vec![(1, 3), (2, 4)],
        ));
        write_codegen(&mut ctx, entries);
        assert_eq!(ctx.errors, 0);

        std::fs::read_to_string(&out).unwrap()
    }

    #[test]
    fn local_assets_keep_their_extension_like_web_assets() {
        let luau = generate_local_and_web(false);

        assert!(
            luau.contains("[\"Attack1.rbxm\"] = \"rbxassetid://1\""),
            "{luau}"
        );
        assert!(
            luau.contains("[\"Idle.rbxm\"] = \"rbxassetid://2\""),
            "{luau}"
        );
        assert!(luau.contains("[\"search.png\"] = "), "{luau}");
    }

    #[test]
    fn strip_extension_strips_local_and_web_assets() {
        let luau = generate_local_and_web(true);

        assert!(luau.contains("Attack1 = \"rbxassetid://1\""), "{luau}");
        assert!(luau.contains("Idle = \"rbxassetid://2\""), "{luau}");
        assert!(luau.contains("search = "), "{luau}");
        assert!(!luau.contains(".rbxm"), "{luau}");
        assert!(!luau.contains(".png"), "{luau}");
    }

    #[test]
    fn web_only_input_seeds_keys_relative_to_base() {
        let web: HashMap<RelativePathBuf, WebAsset> = [
            ("Assets/Animations/Mannequin/Walk.rbxm", 91146021054199),
            ("Assets/Animations/Mannequin/Death.rbxm", 83889629498050),
            ("Assets/Animations/Idle.rbxm", 1),
        ]
        .into_iter()
        .map(|(path, id)| (RelativePathBuf::from(path), WebAsset { id }))
        .collect();

        let mut entries = Vec::new();
        seed_web_assets(&web, "Assets/Animations", false, false, &mut entries);

        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("Animations.luau");
        crate::core::postsync::codegen::generate(
            entries,
            "Animations",
            "nested",
            false,
            out.to_str().unwrap(),
            false,
        )
        .unwrap();

        let luau = std::fs::read_to_string(&out).unwrap();
        assert!(luau.contains("Mannequin = {"), "{luau}");
        assert!(
            luau.contains("[\"Walk.rbxm\"] = \"rbxassetid://91146021054199\""),
            "{luau}"
        );
        assert!(
            luau.contains("[\"Death.rbxm\"] = \"rbxassetid://83889629498050\""),
            "{luau}"
        );
        assert!(
            luau.contains("[\"Idle.rbxm\"] = \"rbxassetid://1\""),
            "{luau}"
        );
        assert!(
            !luau.contains("Assets"),
            "keys must be relative to the glob base:\n{luau}"
        );
    }
}
