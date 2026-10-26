# TuneAI brand

TuneAI is the name of the studio app. The command-line tool, the repo and the
crates keep the name `tune`.

## The mark

A 4x4 grid of steps, like one bar of a 16-step sequencer, on a dusk-purple
rounded tile. Lit acid-green steps spell a T. The top-right step of the T is
cyan: in the studio, cyan means "Claude did this". The unlit steps are faint
white.

## Colours

| Role | Hex |
| --- | --- |
| Lit step | `#A8F04C` |
| Claude step, "AI" in the wordmark | `#3FDCEB` |
| Wordmark ink ("Tune") | `#EEE9F8` |
| Tile gradient | `#2B1A4F` to `#0E0819`, with a `#7B3FD0` glow |
| Unlit step | white at 8% opacity |

## Wordmark

"Tune" in ink (`#EEE9F8`) followed by "AI" in cyan (`#3FDCEB`), set in Manrope
800 with letter-spacing -0.02em. No space between the two parts.

## Files

- `tuneai-mark.svg`: the full tile. Favicon, in-app mark, README header.
- `tuneai-glyph.svg`: the grid alone, no tile. Use on dark backgrounds that
  already provide the tile feel.
- `tuneai-app-icon.svg`: 1024x1024 macOS app icon, tile at 824 px with a drop
  shadow. Source for `apps/desktop/src-tauri/icons/`.

## Minimum size

Use `tuneai-mark.svg` down to 16 px. Below 16 px the steps blur; do not use
the mark there.

## Don'ts

- Do not recolour the cyan step. It always marks the Claude step.
- Do not stretch or skew the mark; scale it uniformly.
- Do not change the number or position of the steps.
- Do not put the wordmark in another typeface or colour.
