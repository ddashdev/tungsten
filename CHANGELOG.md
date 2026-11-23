# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- Animations (`.rbxm`/`.rbxmx` files detected or pinned as animations) were uploaded with the `Model` asset type, so Roblox refused to load them as animations. They now upload as `Animation`. Animations uploaded by an earlier version have to be removed from `tungsten.lock` to be uploaded again
- `type` in a directory sidecar was ignored for the files below it, despite the docs. It now reaches every file in the directory and its nested directories, the nearest sidecar winning
- On the `cloud` target, a file whose contents changed kept its cached asset id instead of being uploaded again
- `sync -v` now logs the asset type each upload is sent as

## [4.0.0-web.3]

### Fixed

- `strip_extension = false` only kept the extension on web asset keys; local files always lost theirs, so one module could mix `["Walk.rbxm"]` and `Idle`. Local assets now keep their source extension too (`["Idle.rbxm"]`, `["icon.png"]`). Lockfile keys are unchanged, so nothing is re-uploaded

## [4.0.0-web.2]

### Fixed

- Inputs with no matching files and no web assets were skipped, leaving their previous codegen module (and any entries removed since) in place; they now write an empty module

## [4.0.0-web.1]

Fork release (ddashdev/tungsten) built on top of the unreleased 4.0.0-rc.1 changes below it.

### Added

- Metadata sidecars can use a `.tmeta.toml` extension (e.g. `logo.png.tmeta.toml`, `icons.tmeta.toml`) so editors treat them as TOML. `.tmeta` wins if both exist for the same target
- Directory sidecars can declare web assets in a `[web]` table, keyed by paths relative to the directory, instead of listing them all under `[inputs.<name>.web]`. Every sidecar at and below the input's base is read (the directory itself doesn't have to exist), only entries the input's glob matches are kept, and `tungsten.toml` wins over sidecars, deeper sidecars over shallower ones
- `tungsten test` counts sidecar problems (unreadable sidecars, invalid web keys) as warnings

### Fixed

- Directory sidecars for directories with a dot in their name (e.g. `v1.2/`) were looked up as `v1.tmeta`; they now use the full name (`v1.2.tmeta`)
- `watch` now re-syncs when the input base directory's own sidecar (e.g. `assets/icons.tmeta`, which sits outside the watched directory) changes, and watches from the last complete path segment for globs like `assets/icon*/**/*`
- Inputs with `[inputs.<name>.web]` entries but no matching local files now still generate their codegen module instead of being skipped, and `tungsten test` counts them as passing
- `sync` no longer requires an API key for the `cloud` target when every input is web-only, since nothing is uploaded
- Flat codegen output is now sorted by name, so unchanged inputs no longer rewrite their module on every sync

## [4.0.0-rc.1]

### Breaking

- The lockfile was renamed from `tungsten.lock.toml` to `tungsten.lock` and upgraded to v3:
  - Old v2 lockfiles are migrated automatically on the next `sync`. A `.v2.bak` backup is always written first, and an existing backup is never overwritten
  - To migrate explicitly (with a summary of migrated and unmapped entries), run `tungsten migrate-lock`. It supports `--dry-run`, `--config`, `--force`, and `--no-backup`
  - Cache entries are now keyed by asset name instead of content hash, so encoder or compression tweaks no longer force re-uploads of unchanged files

### Added

- `migrate-lock` subcommand to migrate a v2 lockfile to v3
- Optional `type` field in `.tmeta` files to override the asset type for a specific asset, taking precedence over the input-level `type`

### Changed

- `sync` now targets `cloud` by default when `--target` is omitted
- `sync` delivers the first file much sooner (~3× on SVG-heavy inputs) by uploading/copying each image as it finishes instead of waiting for the whole batch

## [v3.1.1] - 2026-08-24

### Added

- Uploads now automatically retry rate limits and transient network or server errors with backoff capped at 30s
- When an upload fails fatally, remaining in-flight uploads are cancelled immediately instead of running to completion
- Images are compressed entirely in memory — no temp files, and the original bytes are kept when compression wouldn't save space
- High-DPI variants are now uploaded/copied per variant instead of being waitlisted with placeholder codegen entries

### Changed

- Individual, packed, and multi-DPI assets now go through one shared sync pipeline, so dry-run, caching, and the cloud/studio/debug targets behave identically for every input type
- Docs rewritten across getting-started, guides, and reference pages

### Fixed

- Packed sprites could disappear from the generated code; their entries are now always preserved
- Multi-DPI icon groups could emit duplicate or nondeterministic codegen entries — now exactly one per icon
- Compression failures no longer upload a corrupt (empty) asset; the original bytes are used instead
- SVGs are rasterized with straight alpha, fixing incorrect transparency in converted output

## [v3.1.0] - 2026-08-03

### Added

- Web asset support: `rbxm`/`rbxmx`/`fbx` files sync as web assets with automatic animation detection, in every sync mode
- Optional `asset_type` in input configuration, with per-input overrides
- `seed_web_assets` helper in generated code
- Rewritten `init` command with interactive prompts for creator, folder selection, and codegen output, including a folder selector with type cycling and multi-select
- Global `--verbose` flag that enables debug-level log output for troubleshooting

### Changed

- Log formatting updated to colored badges and section headers
- Capitalized section headers in the `test` and `watch` commands and in `--help`

### Fixed

- Punctuation in CLI error messages (stray trailing period)

## [v3.0.0] - 2026-07-14

### Added

- Configurable upload concurrency via `max_concurrent_uploads` in `tungsten.toml` (default: 10)
- Studio sync cleanup: stale files are removed from `.tungsten-debug/`
- `StudioConfig` section with `studio_path` and `auto_route_version` for advanced Studio path handling, including automatic version routing when enabled
- Alpha bleeding to fill transparent spritesheet edges
- Automatic spritesheet packing similar to Adobe Animate: largest-first sorting, 1024×1024 atlases, additional atlases generated as needed, and trimming of unused space
- DPI variant pre-processing and spritesheet encoding run in parallel for faster syncs

### Changed

- Studio sync is now incremental and preserves assets between Studio version updates, instead of wiping previous contents
- SVG scaling is now per-file based on the viewBox rather than a global input scale
- Log output uses bracketed labels ([INFO], [SUCCESS], [WARNING], [ERROR]) instead of symbols, and progress bars use aligned, zero-padded counters
- API keys now load from standard `.env` files with `TUNGSTEN_API_KEY` instead of `tungsten_api_key.env`
- Added a `bleed` option to inputs to control alpha bleeding (defaults to `true`)

### Fixed

- Studio sync no longer changes file extensions for audio and model assets (e.g., `.mp3` → `.audio`, `.rbxm` → `.model`)

### Removed

- Adding `tungsten_api_key.env` to `.gitignore`, as it's no longer used
- DPI variants are no longer packed into spritesheets; they're waitlisted for manual upload but still generate DPI group code entries

## [v3.0.0-rc.2] - 2026-07-13

### Added

- Studio sync cleanup logic to remove stale files from `.tungsten-debug/` folder
- File tracking mechanism for Studio sync to track expected files during sync operations
- `StudioConfig` section with `studio_path` and `auto_route_version` for advanced Studio path handling
- Automatic version routing via `https://setup.roblox.com/versionQTStudio` when `auto_route_version` is enabled

### Changed

- Studio sync is now incremental and preserves assets between Studio version updates, instead of wiping previous contents
- SVG scaling is now per-file based on the viewBox rather than a global input scale
- Log output uses bracketed labels ([INFO], [SUCCESS], [WARNING], [ERROR]) instead of symbols, and progress bars use aligned, zero-padded counters
- API keys now load from standard `.env` files with `TUNGSTEN_API_KEY` instead of `tungsten_api_key.env`

### Fixed

- Studio sync no longer changes file extensions for audio and model assets (e.g., `.mp3` → `.audio`, `.rbxm` → `.model`)

### Removed

- Adding `tungsten_api_key.env` to `.gitignore`, as it's no longer used

## [v3.0.0-rc.1] - 2026-06-27

### Added

- CHANGELOG.md file to track changes between versions
- **Breaking:** Required `type` field on inputs, to specify the asset type (e.g., decal, image) independent of file kind

### Changed

- Meta files now follow a dual naming convention: for files, `name.format.tmeta` (e.g., `logo.png.tmeta`) takes priority over `name.tmeta`; directories use `name.tmeta`
- Added `bleed` option to inputs to control alpha bleeding (defaults to `true`)
- Automatic spritesheet packing similar to Adobe Animate: largest-first sorting, 1024×1024 atlases generated on demand, unused padding trimmed
- Documentation fixes across the getting-started and reference pages

### Removed

- DPI variants are no longer packed into spritesheets; they're waitlisted for manual upload but still generate DPI group code entries

## [v2.1.1] - 2026-04-27

### Changed

- Removed unused parameters from `compress_options`

## [v2.1.0] - 2026-04-27

### Added

- Image compression support

### Changed

- Log output redesigned so the terminal doesn't get clogged

### Removed

- Format conversion (SVGs are still rasterized, but the target format is no longer configurable)

## [v2.0.1] - 2026-04-19

### Fixed

- `init` had inverted `packable` logic

## [v2.0.0] - 2026-04-19

### Breaking

- The `--target` flag was deprecated in favor of new, more explicit command patterns, and the `none` target was replaced by the `--dry-run` flag. CI/CD scripts and local aliases need updating

### Added

- Watch mode: detects file edits and syncs changes to your target in real-time
- Expanded targets: sync directly to Roblox Studio or your local workspace, alongside cloud
- `ts_declaration` support: auto-generates `.d.ts` files for roblox-ts users
- Alpha bleeding, enabled globally, prevents halos/fringing on sprite edges
- High DPI detection: DPI textures are detected and callable functions retrieve the correct asset

### Changed

- `init` was overhauled: it scans your existing directory structure to automatically generate a configuration file

## [v1.0.0] - 2026-04-06

### Added

- First stable release
- More asset types: audio, animations, and models
- Meta files for custom names and descriptions for assets
- Conversion support for images and audio
- SVG support

### Changed

- `test` now uploads an image

### Removed

- Examples (to come back in the documentation once updated)
- Foreman support, as it couldn't detect the executable when multiple were present in the release ZIP

## [v0.1.3] - 2026-03-31

### Added

- API key detection: use the `--api-key` flag, a `.env` file with an `API_KEY` field, or a system/user `TUNGSTEN_GLOBAL_APIKEY` environment variable

## [v0.1.2] - 2026-03-29

### Changed

- Uploads now reuse TCP connections through a shared pooled HTTP client
- Rate limiting now prefers the server's `x-ratelimit-reset` header and falls back to exponential backoff
- Improved error messages for missing API keys and upload timeouts

## [v0.1.1] - 2026-03-25

### Fixed

- Spritesheet packing overflow issue

### Changed

- Tungsten-specific `.gitignore` entries and release workflow updates

## [v0.1.0] - 2026-03-25

### Added

- Initial release
- Sync images to the Roblox Open Cloud Assets API
- Spritesheet packing for `packable` inputs
- Luau code generation referencing uploaded assets
- `init` command to scaffold a `tungsten.toml` configuration
- `test` command to verify upload connectivity with a sample asset
- Lockfile (`tungsten.lock.toml`) caching uploaded asset IDs
- Example spritesheets project under `examples/`
