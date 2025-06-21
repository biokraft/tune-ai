# 🎵 VS Code Guide for TidalCycles

## 1. Purpose

This guide provides step-by-step instructions for setting up Visual Studio Code to work with TidalCycles. It covers extension installation, configuration, and the workflow for running your compositions.

## 2. Extension Setup

To get started, you need to install two key extensions from the VS Code Marketplace.

1.  **Install the TidalCycles Extension**:
    -   Open the **Extensions** view (`Cmd+Shift+X` or `Ctrl+Shift+X`).
    -   Search for `TidalCycles`.
    -   Install the one published by the `tidalcycles` team.

2.  **Install Haskell Syntax Highlighting**:
    -   In the same **Extensions** view, search for `Haskell Syntax Highlighting`.
    -   Install the one published by `justusadam`. This will provide proper code coloring for your `.tidal` files.

## 3. VS Code Configuration

For the extensions to work correctly, you need to associate `.tidal` files with the Haskell language.

1.  Open your VS Code settings file by pressing `Cmd+Shift+P` (or `Ctrl+Shift+P`) and searching for `Preferences: Open Settings (JSON)`.
2.  Add the following configuration:

    ```json
    {
        "files.associations": {
            "*.tidal": "haskell"
        },
        "tidalcycles.ghciPath": "ghci",
        "tidalcycles.showOutputInConsoleChannel": true
    }
    ```

    -   `files.associations`: Tells VS Code to treat `.tidal` files as Haskell code for syntax highlighting.
    -   `tidalcycles.ghciPath`: Specifies the path to the Haskell interpreter. `ghci` should suffice if it's in your system's PATH.
    -   `tidalcycles.showOutputInConsoleChannel`: Ensures Tidal's output appears in the VS Code console.

## 4. How to Run Your Music: The Startup Sequence

The order of operations is crucial. Follow these steps every time you start a session.

### Step 1: Start SuperCollider and SuperDirt

SuperDirt is the audio engine and must be running before you send it any code from VS Code.

1.  Open the **SuperCollider** application.
2.  In the editor pane, type: `SuperDirt.start`
3.  Execute the line with `Cmd+Enter` (or `Ctrl+Enter`).
4.  Wait for the "pan" and "dirt" processes to appear in the post window. This confirms SuperDirt is ready.

### Step 2: Open and Run Your Script in VS Code

1.  Open your `tune-ai` project folder in VS Code.
2.  Open the file you want to play, for example: `compositions/techno/basic-techno-loop.tidal`.
3.  Place your cursor on a pattern line (e.g., `d1 $ sound "bd bd bd bd"`).
4.  Use the key commands below to evaluate the code and send it to SuperDirt.

## 5. Key Commands

These are the essential shortcuts for live coding in VS Code:

| Shortcut | Action | Description |
|---|---|---|
| `Shift + Enter` | **Evaluate Line** | Sends the line your cursor is on to Tidal. |
| `Ctrl + Enter` | **Evaluate Block** | Sends all currently selected lines to Tidal. |
| `Ctrl + Alt + H` | **Hush** | Immediately silences all running patterns. |

## 6. Troubleshooting

-   **Error: `Could not find module 'Sound.Tidal.Context'`**: This usually means there's an issue with your Haskell or TidalCycles installation.
    -   Ensure SuperDirt is running before you evaluate any code.
    -   Try reinstalling Tidal by running `cabal v1-install tidal` in your terminal.
    -   Confirm that `ghci` is accessible from your terminal. 