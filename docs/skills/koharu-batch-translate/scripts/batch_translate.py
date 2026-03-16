#!/usr/bin/env python3
"""
Koharu Batch Translate Script
Connects to Koharu's MCP server, processes folders of manga images,
and exports translated CBZ archives.

Usage:
    python3 batch_translate.py --inbox /path/to/inbox --outbox /path/to/outbox
    python3 batch_translate.py --inbox /path/to/inbox --outbox /path/to/outbox --mcp-url http://127.0.0.1:9999/mcp
"""

import argparse
import json
import os
import platform
import shutil
import sys
import tempfile
import zipfile
from pathlib import Path

import httpx

# ---------------------------------------------------------------------------
# Config / constants
# ---------------------------------------------------------------------------

MCP_DEFAULT_URL = "http://127.0.0.1:9999/mcp"
SUPPORTED_ARCHIVES = {".cbz", ".zip"}
SUPPORTED_IMAGES = {".jpg", ".jpeg", ".png", ".webp", ".bmp"}


# ---------------------------------------------------------------------------
# MCP client (minimal streamable HTTP client)
# ---------------------------------------------------------------------------

class KoharuMcpClient:
    """Minimal MCP client using the streamable HTTP transport."""

    def __init__(self, base_url: str):
        self.base_url = base_url.rstrip("/")
        self.session_id: str | None = None
        self._client = httpx.Client(timeout=300.0)  # long timeout for ML ops

    def _call(self, method: str, params: dict | None = None) -> dict:
        payload = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params or {},
        }
        headers = {"Content-Type": "application/json"}
        if self.session_id:
            headers["mcp-session-id"] = self.session_id

        resp = self._client.post(self.base_url, json=payload, headers=headers)
        resp.raise_for_status()

        # Capture session id from response headers if provided
        if sid := resp.headers.get("mcp-session-id"):
            self.session_id = sid

        data = resp.json()
        if "error" in data:
            raise RuntimeError(f"MCP error: {data['error']}")
        return data.get("result", {})

    def initialize(self):
        result = self._call("initialize", {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "koharu-batch-translate", "version": "1.0.0"},
        })
        self._call("notifications/initialized", {})
        return result

    def call_tool(self, name: str, arguments: dict | None = None) -> dict:
        return self._call("tools/call", {"name": name, "arguments": arguments or {}})

    def close(self):
        self._client.close()


# ---------------------------------------------------------------------------
# Preferences loader
# ---------------------------------------------------------------------------

def get_prefs_path() -> Path | None:
    """Return path to Koharu's saved preferences JSON."""
    system = platform.system()
    if system == "Darwin":
        base = Path.home() / "Library" / "Application Support" / "com.koharu" / "koharu-config.json"
        # Try alternate location
        if not base.exists():
            base = Path.home() / "Library" / "Application Support" / "Koharu" / "koharu-config.json"
    elif system == "Windows":
        base = Path(os.environ.get("APPDATA", "")) / "Koharu" / "koharu-config.json"
    else:
        base = Path.home() / ".config" / "Koharu" / "koharu-config.json"
    return base if base.exists() else None


def load_preferences() -> dict:
    """Load and return Koharu preferences, or return sensible defaults."""
    path = get_prefs_path()
    if path:
        print(f"[config] Loading preferences from {path}")
        with open(path) as f:
            prefs = json.load(f)
    else:
        print("[config] No saved preferences found — using defaults")
        prefs = {}

    # Extract relevant settings with defaults
    return {
        "llm_model": prefs.get("llmModel", None),
        "llm_language": prefs.get("llmLanguage", "English"),
        "font_family": prefs.get("fontFamily", None),
        "cbz_max_size": prefs.get("cbzSettings", {}).get("maxSize", 1080),
        "cbz_quality": prefs.get("cbzSettings", {}).get("quality", 78),
        "cbz_format": prefs.get("cbzSettings", {}).get("imageFormat", "webp"),
        "cbz_archive_format": prefs.get("cbzSettings", {}).get("archiveFormat", "cbz"),
    }


# ---------------------------------------------------------------------------
# Archive extraction
# ---------------------------------------------------------------------------

def extract_archives(inbox: Path) -> list[Path]:
    """
    Extract all .cbz/.zip archives in inbox into subfolders.
    Deletes the archive after extraction.
    Returns list of extracted folder paths.
    """
    extracted = []

    archives = [p for p in inbox.iterdir() if p.suffix.lower() in SUPPORTED_ARCHIVES]
    if not archives:
        print(f"[inbox] No archives found in {inbox}")
        return []

    for archive in archives:
        dest = inbox / archive.stem
        dest.mkdir(exist_ok=True)
        print(f"[extract] {archive.name} → {dest.name}/")

        with zipfile.ZipFile(archive, "r") as zf:
            # Only extract image files
            image_members = [
                m for m in zf.namelist()
                if Path(m).suffix.lower() in SUPPORTED_IMAGES and not m.startswith("__MACOSX")
            ]
            for member in image_members:
                # Flatten into dest (strip subdirs from archive)
                out_path = dest / Path(member).name
                with zf.open(member) as src, open(out_path, "wb") as dst:
                    shutil.copyfileobj(src, dst)

        archive.unlink()
        print(f"[extract] Deleted {archive.name}")
        extracted.append(dest)

    return extracted


# ---------------------------------------------------------------------------
# Koharu processing
# ---------------------------------------------------------------------------

def get_image_paths(folder: Path) -> list[str]:
    """Return sorted list of image paths in a folder."""
    images = sorted(
        p for p in folder.iterdir()
        if p.suffix.lower() in SUPPORTED_IMAGES
    )
    return [str(p) for p in images]


def process_folder(mcp: KoharuMcpClient, folder: Path, prefs: dict, out_dir: Path) -> Path | None:
    """
    Process a single folder of images through Koharu's full pipeline.
    Returns path to the created CBZ, or None if failed.
    """
    images = get_image_paths(folder)
    if not images:
        print(f"  [skip] No images found in {folder.name}")
        return None

    print(f"\n[process] {folder.name} ({len(images)} images)")

    # 1. Load LLM model if specified
    if prefs["llm_model"]:
        print(f"  [llm] Loading model: {prefs['llm_model']}")
        try:
            mcp.call_tool("llm_load", {"id": prefs["llm_model"]})
        except Exception as e:
            print(f"  [llm] Warning: failed to load model: {e}")

    # 2. Export dir for this folder
    export_dir = out_dir / folder.name
    export_dir.mkdir(parents=True, exist_ok=True)

    # 3. Process images in batches (Koharu loads all docs at once)
    print(f"  [load] Opening {len(images)} images...")
    mcp.call_tool("open_documents", {"paths": images})

    # 4. Run full pipeline on each document by index
    for i, img_path in enumerate(images):
        img_name = Path(img_path).name
        print(f"  [run] [{i+1}/{len(images)}] {img_name}")
        try:
            mcp.call_tool("process", {
                "index": i,
                "llm_model_id": prefs["llm_model"],
                "language": prefs["llm_language"],
                "font_family": prefs["font_family"],
            })
        except Exception as e:
            print(f"  [run] Warning: pipeline failed for {img_name}: {e}")
            continue

        # 5. Export rendered image
        ext = ".webp" if prefs["cbz_format"] == "webp" else ".jpg"
        out_img = export_dir / f"{str(i+1).zfill(6)}{ext}"
        try:
            mcp.call_tool("export_document", {
                "index": i,
                "output_path": str(out_img),
            })
            print(f"  [export] → {out_img.name}")
        except Exception as e:
            print(f"  [export] Warning: export failed for {img_name}: {e}")

    # 6. Package into CBZ
    archive_ext = prefs["cbz_archive_format"]  # cbz or zip
    cbz_path = out_dir / f"{folder.name}.{archive_ext}"
    exported_images = sorted(export_dir.iterdir())
    if not exported_images:
        print(f"  [cbz] No exported images to package.")
        return None

    print(f"  [cbz] Packaging {len(exported_images)} images → {cbz_path.name}")
    with zipfile.ZipFile(cbz_path, "w", zipfile.ZIP_DEFLATED) as zf:
        for img in exported_images:
            zf.write(img, img.name)

    return cbz_path


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(description="Koharu Batch Manga Translator")
    parser.add_argument("--inbox", required=True, help="Folder A: input directory with CBZ/ZIP archives")
    parser.add_argument("--outbox", required=True, help="Folder B: output directory for translated CBZ files")
    parser.add_argument("--mcp-url", default=MCP_DEFAULT_URL, help=f"Koharu MCP server URL (default: {MCP_DEFAULT_URL})")
    args = parser.parse_args()

    inbox = Path(args.inbox).expanduser().resolve()
    outbox = Path(args.outbox).expanduser().resolve()
    outbox.mkdir(parents=True, exist_ok=True)

    if not inbox.exists():
        print(f"[error] Inbox folder does not exist: {inbox}")
        sys.exit(1)

    # Load preferences
    prefs = load_preferences()
    print(f"[config] LLM model: {prefs['llm_model'] or '(none)'}")
    print(f"[config] Language: {prefs['llm_language']}")
    print(f"[config] CBZ format: {prefs['cbz_format']} @ quality {prefs['cbz_quality']}")

    # Connect to Koharu MCP
    print(f"\n[mcp] Connecting to {args.mcp_url}...")
    mcp = KoharuMcpClient(args.mcp_url)
    try:
        mcp.initialize()
        print("[mcp] Connected.")
    except Exception as e:
        print(f"[error] Cannot connect to Koharu MCP server: {e}")
        print("[error] Make sure Koharu is running and accessible at the MCP URL.")
        sys.exit(1)

    # Extract archives
    print(f"\n[inbox] Scanning {inbox}...")
    extracted_folders = extract_archives(inbox)
    if not extracted_folders:
        print("[done] Nothing to process.")
        return

    # Process each folder
    with tempfile.TemporaryDirectory(prefix="koharu_batch_") as tmp:
        tmp_path = Path(tmp)
        produced_cbz = []

        for folder in extracted_folders:
            try:
                cbz = process_folder(mcp, folder, prefs, tmp_path)
                if cbz:
                    produced_cbz.append(cbz)
            except Exception as e:
                print(f"[error] Failed to process {folder.name}: {e}")

        # Move produced CBZ files to outbox
        if produced_cbz:
            print(f"\n[outbox] Moving {len(produced_cbz)} CBZ file(s) to {outbox}...")
            for cbz in produced_cbz:
                dest = outbox / cbz.name
                shutil.move(str(cbz), str(dest))
                print(f"  ✓ {cbz.name}")
        else:
            print("\n[done] No CBZ files were produced.")

    mcp.close()
    print("\n[done] Batch translation complete.")


if __name__ == "__main__":
    main()
