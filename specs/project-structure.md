# Project Structure Specification

## 1. Purpose

This document defines the standard directory structure for the `tune-ai` repository. A consistent structure ensures that compositions, specifications, and other assets are easy to locate and manage.

## 2. Top-Level Directory Layout

The root of the project will be organized as follows:

```
tune-ai/
├── 🎵 compositions/
│   ├── 🏠 house/
│   └── 🤖 techno/
├── 📝 notes/
│   └── ...
├── 🔊 samples/
│   └── custom/
├── 📜 specs/
│   ├── environment-setup.md
│   └── project-structure.md
├── .gitignore
├── LICENSE
├── README.md
└── SPECS.md
```

## 3. Directory Descriptions

| Path | Description |
|---|---|
| `compositions/` | Contains all musical work. Each subdirectory represents a specific genre or project. All files within should be `.tidal` files. |
| `notes/` | A space for unstructured notes, thoughts, and learnings about TidalCycles, music theory, or the creative process. Can contain Markdown files or other formats. |
| `samples/` | For storing custom audio samples to be used with SuperDirt. The `custom/` subdirectory is a recommended starting point. |
| `specs/` | Holds all formal specification documents that define the project's goals, architecture, and workflow. |
| `SPECS.md` | The root index file for the entire specification library. Provides a high-level overview and links to all documents in the `specs/` directory. 