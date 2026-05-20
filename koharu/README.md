# Manga Offline Translate app crate

Desktop app that stitches together detection, OCR, inpainting, translation, and text rendering for manga translation. Built with Rust and Tauri and powered by the `koharu-*` workspace crates.

## Run from source
```bash
cargo run -p manga-offline-translate --release                     # CPU-only
cargo run -p manga-offline-translate --release --features cuda     # enable CUDA + ORT GPU provider
```

`bundle` enables Velopack auto-updates for packaged builds. The UI expects `ui/out` to exist; run `bun run build` in the repo root before packaging.

## License

Licensed under GPL-3.0-only.
