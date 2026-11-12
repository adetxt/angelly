# Packaging assets

`Assets.xcassets` supplies icons for the Rust macOS and Linux packaging scripts.
Keep the paths used by `rust/scripts/bundle-macos.sh` and
`rust/scripts/package-linux.sh` in sync when changing assets.

The application code, Swift platform shim, and scripting definition live under
`rust/`. Build the macOS bundle from the repository root with
`rust/scripts/bundle-macos.sh --debug`.
