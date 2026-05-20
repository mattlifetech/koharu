# Manga Offline Translate v0.37.1-preview1

## Unofficial Fork Notice

Manga Offline Translate is an unofficial fork of Koharu. It is not affiliated
with, sponsored by, or endorsed by the original Koharu maintainers.

Original project: https://github.com/mayocream/koharu

Fork source: https://github.com/mattlifetech/manga-offline-translate

Original authors: Mayo Takanashi, Wangchong Zhou, and Koharu contributors.

License: GNU General Public License v3.0 only. Source code for this build is
available in the fork repository above.

This fork is based on Koharu 0.37.0 source history. The closest upstream
merge-base in this fork is `23f7d9fb9b4e9fcbab16ce317660cb91576ce15f` from
`mayocream/koharu`.

## Changes In This Fork

- Bumped the preview build to 0.37.1.
- Removed the app logo from the main window header/menu bar.
- Switched the UI accent color from pink to blue to match the new logo.
- Renamed the distributed app to Manga Offline Translate.
- Replaced app icons and in-app branding with a distinct fork logo.
- Added macOS Apple Silicon preview DMG release workflow.
- Writes `inpainted/` and `rendered/` folders next to the opened source image
  folder for easier inspection and cleanup.
- Skips oversized or problematic inpainting regions instead of stopping an
  entire batch.
- Includes OpenClaw/MCP batch translation helper updates.

## Platform

This preview DMG is built for macOS Apple Silicon.
