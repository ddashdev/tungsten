//! Codegen output: seeds web assets and writes the Luau/`.d.ts` modules.

use crate::core::assets::asset::WebAsset;
use crate::core::postsync::codegen::CodegenEntry;
use relative_path::RelativePathBuf;

use super::dispatch::SyncContext;

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
    web_assets: &std::collections::HashMap<RelativePathBuf, WebAsset>,
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

/// Generates (and writes) the Luau module for this input from its codegen
/// entries, honoring the input's resolved `[codegen]` settings.
pub fn write_codegen(ctx: &mut SyncContext<'_>, mut codegen_entries: Vec<CodegenEntry>) {
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
    use std::collections::HashMap;

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
