---
name: koharu-batch-translate
description: >
  Batch-translates manga image archives using Koharu. When triggered, it:
  scans folder A for ZIP/CBZ archives, extracts them into per-archive folders,
  connects to the running Koharu app via MCP, loads the user's saved render/LLM/CBZ
  settings, processes each image folder (detect → OCR → translate → render → export),
  packages the results into CBZ files, and moves them to folder B.
  Use this skill when the user asks to batch-process, auto-translate, or convert manga archives.
requires:
  bins:
    - python3
user-invocable: true
---

# Koharu Batch Manga Translation Skill

> **Before using this skill**, review the sections marked `# 👤 USER SETUP` below.
> Sections marked `# ✅ LEAVE AS-IS` are auto-configured and do not need editing.

---

## Prerequisites

Install the Python dependencies:

```bash
pip install mcp httpx
```

---

## 👤 USER SETUP — Step 1: Set Your Inbox and Outbox Folders

Before running, configure two folders:

| Variable | What it is | Example |
|---|---|---|
| `KOHARU_INBOX` | **Folder A** — where your raw CBZ/ZIP archives are | `/Users/you/Manga/Inbox` |
| `KOHARU_OUTBOX` | **Folder B** — where translated CBZ files are saved | `/Users/you/Manga/Done` |

You can set them as environment variables, or the agent will ask you when triggered:

```bash
export KOHARU_INBOX="/Users/you/Manga/Inbox"
export KOHARU_OUTBOX="/Users/you/Manga/Done"
```

> ⚠️ Archives in `KOHARU_INBOX` **will be deleted** after extraction. Use a staging folder, not your main archive library.

---

## 👤 USER SETUP — Step 2: Save Settings in Koharu First

This skill reads your settings from Koharu's saved preferences. Before running:

1. Open Koharu
2. Go to the **Render** tab → configure your font, style settings → **Save**
3. Go to **Settings (File > Settings)** → configure CBZ quality/format → Save
4. Go to the **LLM** section → select your model and language → **Save**

The skill will auto-load these settings. If no settings are found, it uses defaults (WebP @ 78 quality, English output).

---

## ✅ LEAVE AS-IS — MCP Server URL

The skill connects to Koharu's built-in MCP server at:

```
http://127.0.0.1:9999/mcp
```

This is the default port set by Koharu's launch script (`--port=9999`). **Do not change this** unless you launched Koharu on a different port.

> Koharu **must be running** before you trigger this skill. The skill does not launch Koharu.

---

## ✅ LEAVE AS-IS — Preferences Auto-Detection

The script reads your Koharu config from the OS default location:

| OS | Path |
|---|---|
| macOS | `~/Library/Application Support/com.koharu/koharu-config.json` |
| Windows | `%APPDATA%/Koharu/koharu-config.json` |
| Linux | `~/.config/Koharu/koharu-config.json` |

No changes needed here.

---

## How to Trigger

Ask the agent:
> "Run Koharu batch translate"
> "Process my manga inbox with Koharu"
> "Batch translate everything in my inbox folder"

The agent will run:

```bash
python3 {baseDir}/scripts/batch_translate.py \
  --inbox "$KOHARU_INBOX" \
  --outbox "$KOHARU_OUTBOX" \
  --mcp-url "http://127.0.0.1:9999/mcp"
```

---

## What Happens Step by Step

1. **Unpack** — Scan Folder A for `.cbz`/`.zip` → extract each into a subfolder named after the archive → delete the archive
2. **Connect** — Connect to Koharu MCP at `http://127.0.0.1:9999/mcp`
3. **Load Settings** — Read saved LLM model, language, font, CBZ quality/format from Koharu preferences
4. **Process each folder** — For each extracted folder:
   - Load all images via `open_documents`
   - Load the LLM via `llm_load`
   - Run detect → OCR → inpaint → translate → render via `process`
   - Export each rendered page via `export_document`
   - Package exported pages into a `.cbz` archive
5. **Move output** — Move produced CBZ files to Folder B

