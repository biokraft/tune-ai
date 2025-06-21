# 🎵 tune-ai 🎵

This repository documents my journey into the world of algorithmic music creation using [TidalCycles](https://tidalcycles.org/). I'll be using Cursor, an AI-powered code editor, to assist me in this learning process.

## 🎯 Goal

The main goal of this project is to learn how to create music with code, specifically with TidalCycles. This repository will serve as a collection of my experiments, notes, and compositions as I explore its capabilities.

## 🤖 The Process

I'll be using this space to:
- ✍️ Write and experiment with TidalCycles patterns.
- 📚 Keep notes and document my learning.
- 🎶 Compose and save musical ideas.
- 🤖 Collaborate with an AI assistant to accelerate my learning.

## ⚙️ Environment Setup

Follow these steps to set up your environment for creating music with TidalCycles on macOS.

### 1. Install SuperCollider

[SuperCollider](https://supercollider.github.io/) is the audio engine that TidalCycles uses. Install it using [Homebrew](https://brew.sh/):

```bash
brew install --cask supercollider
```

### 2. Install SuperDirt

[SuperDirt](https://github.com/musikinformatik/SuperDirt) is the "sound engine" part of TidalCycles. It runs inside SuperCollider.

1.  Open the newly installed **SuperCollider** application.
2.  Paste the following line into the editor window:
    ```haskell
    Quarks.install("SuperDirt")
    ```
3.  Press `Cmd + Enter` to run the code and wait for the installation to complete.
4.  Recompile the class library by navigating to `Language > Recompile Class Library` in the menu bar.

### 3. Install TidalCycles

[TidalCycles](https://tidalcycles.org/) is the live coding language itself. It requires the [Haskell](https://www.haskell.org/) programming language environment.

1.  **Install the Haskell toolchain manager (`ghcup`):**
    ```bash
    brew install ghcup
    ```

2.  **Install the GHC compiler and Cabal build tool:**
    `ghcup` will manage your Haskell installations. Use it to install the recommended versions.
    ```bash
    ghcup install ghc
    ghcup install cabal
    ```

3.  **Set the active tool versions:**
    After installation, `ghcup` might require you to set the installed versions as the default. Run `ghcup tui` to see and manage versions, or use the `set` command. For example:
    ```bash
    # This version might differ, use 'ghcup list' to see what's recommended
    ghcup set ghc 9.6.7
    ```
    > **Note:** You may need to restart your terminal session for the `cabal` command to become available in your `PATH`.

4.  **Install TidalCycles:**
    Finally, use the Cabal package manager to install Tidal.
    ```bash
    cabal update
    cabal install tidal
    ```

After these steps, your environment should be ready!

## 🚀 How to Play Your Compositions

With the environment installed, you need a way to send your code to the sound engine. This project is set up to use **Visual Studio Code (VS Code)**.

### Quick Start Workflow

1.  **Start the Sound Engine**: Open the **SuperCollider** application and run `SuperDirt.start`.
2.  **Open Your Code**: Open this project folder in **VS Code**.
3.  **Play a Pattern**: Open a `.tidal` file (like `compositions/techno/basic-techno-loop.tidal`) and use the key commands to start the music.

For a complete, step-by-step guide on configuring VS Code, including the necessary extensions and key commands, please read our dedicated guide:

-   **➡️ [Full VS Code Guide for TidalCycles](./specs/vscode-guide.md)**

## What is TidalCycles?

TidalCycles (or Tidal for short) is a live coding environment for making patterns with code. It is a powerful tool for creating complex musical patterns from simple components.

---

Let the live coding begin!
