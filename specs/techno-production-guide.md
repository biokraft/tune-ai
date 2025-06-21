# Techno Production Guide

## 1. Purpose

This document provides a practical guide to producing Techno music using TidalCycles. It covers the core rhythmic, melodic, and structural elements of the genre, with specific code examples.

## 2. Core Concepts of Techno

- **Tempo:** Typically ranges from 120 to 150 BPM. The default TidalCycles tempo is around 138 BPM (`cps` of 0.575), which is a good starting point. You can set it explicitly with `setcps(130/60/4)`.
- **Rhythm:** The foundation is a strong, driving four-on-the-floor beat.
- **Key Elements:**
    - **Kick Drum:** The anchor of the track, hitting on every quarter note.
    - **Hi-Hats:** Provide rhythmic energy, often with open and closed hat patterns.
    - **Snares/Claps:** Typically on the 2nd and 4th beats, adding backbeat emphasis.
    - **Basslines:** Repetitive, sub-bass-heavy melodic or rhythmic lines.
    - **Atmospherics:** Drones, pads, and noisy textures that create space and tension.

## 3. Building a Techno Track in TidalCycles

### Step 1: The Four-on-the-Floor Kick

The kick drum is the heartbeat of a techno track. Start with a simple, driving pattern. `d1` to `d16` represent 16 different channels you can use.

```tidal
-- A kick drum on every beat. This is the foundation.
d1 $ sound "bd bd bd bd"
```

### Step 2: Adding Off-beat Hi-Hats

Hi-hats create the rhythmic pulse. A common pattern is to place a closed hi-hat on the off-beats.

```tidal
-- Use a second channel (d2) for hi-hats.
-- The "~" represents a rest, or silence.
d2 $ sound "~ ch ~ ch"
```

To create a more driving feel, use 16th-note hi-hats. The `*` syntax is a shorthand for repetition. `ch*4` is the same as `ch ch ch ch`.

```tidal
-- 16th note hi-hats. 
-- The brackets [] create a group, so this pattern fits into one cycle.
d2 $ sound "[ch*4, ch*4, ch*4, ch*4]"
```

### Step 3: Layering Snares and Claps

The snare or clap provides the backbeat. It typically lands on beats 2 and 4.

```tidal
-- Layer a snare sound on channel 3.
d3 $ sound "~ sn ~ sn"
```

### Step 4: Writing a Bassline

Use the `note` function for melodic elements. Numbers represent notes in a scale. Use `s` to specify the sound source.

```tidal
-- A simple, repetitive bassline using the "arpy" synth.
-- This plays a C, then a G, then an E-flat.
d4 $ note "0 7 3" # s "arpy"
```

### Step 5: Bringing It All Together

Live coding is about building up and modifying layers. You can run all these patterns at once.

```tidal
-- A full basic techno groove.
-- Evaluate each line separately to hear the layers build up.

-- Kick
d1 $ sound "bd bd bd bd"

-- 16th-note hi-hats
d2 $ sound "ch*16"

-- Snare on 2 and 4
d3 $ sound "~ sn ~ sn"

-- Simple bassline
d4 $ note "0 0 3 0 7 0 3 0" # s "arpy" # gain 0.8
```

## 4. Next Steps

- **Variation:** Use functions like `every` to introduce changes over time.
- **Effects:** Add depth with effects like `delay`, `reverb`, and `pan`.
- **Experimentation:** Swap out samples (`bd`, `ch`, `sn`) with others from the SuperDirt library to find your sound. 