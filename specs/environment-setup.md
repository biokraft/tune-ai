# Environment Setup Specification

## 1. Purpose

This document outlines the necessary software, tools, and procedures required to set up a local development environment for creating music with TidalCycles.

## 2. Core Components

The environment consists of two primary components that communicate with each other:

- **TidalCycles:** The live coding language for generating musical patterns.
- **SuperDirt:** The sound engine (a SuperCollider quark) that receives messages from TidalCycles to produce audio.

## 3. Required Software

| Software | Purpose | Installation Guide |
|---|---|---|
| **SuperCollider** | An audio programming language and environment that hosts SuperDirt. | [Official Downloads](https://supercollider.github.io/downloads) |
| **SuperDirt** | The default sampler/synthesizer for TidalCycles. It is installed as a "quark" within SuperCollider. | [Installation Docs](https://github.com/musikinformatik/SuperDirt) |
| **TidalCycles** | The pattern language itself. | [Installation Docs](https://tidalcycles.org/docs/getting-started/macos_install) |
| **Text Editor** | An editor with a TidalCycles plugin is required to send code to the running Tidal process. Cursor is used for this project. | N/A |

## 4. Startup Procedure

To start a new music creation session, follow these steps in order:

1.  **Launch SuperCollider:** Open the SuperCollider IDE.
2.  **Start SuperDirt:** In the SuperCollider editor, type `SuperDirt.start` and execute the line (usually with `Cmd+Enter` or `Ctrl+Enter`). Wait for the process to boot up and display its output in the post window.
3.  **Launch TidalCycles in Editor:**
    - Open your project in Cursor.
    - Create or open a file with a `.tidal` extension (e.g., `compositions/techno/session1.tidal`).
    - The TidalCycles plugin in your editor should automatically connect to the running SuperDirt instance. You can test the connection by running a simple command like `d1 $ sound "bd"`.

## 5. Notes

- SuperDirt must be running before TidalCycles can successfully send messages to it.
- The first time you run SuperDirt, it will need to download a large library of default samples. This requires an internet connection. 