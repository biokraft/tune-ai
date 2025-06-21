# House Production Guide

## 1. Purpose

This document provides a practical guide to producing House music using TidalCycles. It highlights the key elements of House, focusing on groove, chords, and basslines, with specific code examples.

## 2. Core Concepts of House Music

- **Tempo:** Typically ranges from 118 to 135 BPM. You may want to set this explicitly, for example: `setcps(125/60/4)`.
- **Rhythm:** While often four-on-the-floor, the defining feature is **swing** or **shuffle**, which creates the characteristic House groove.
- **Key Elements:**
    - **Kick Drum:** A solid four-on-the-floor kick is common.
    - **Hi-Hats:** Open and closed hi-hats with a shuffle feel are crucial for the groove.
    - **Snares/Claps:** Sharp claps or snares on the 2nd and 4th beats.
    - **Chords:** Melodic, often jazzy or soulful chord progressions are central to House.
    - **Basslines:** Funky, melodic, and prominent basslines that work with the chords.

## 3. Building a House Track in TidalCycles

### Step 1: The Kick and Clap

Start with the kick and a classic House clap on beats 2 and 4.

```tidal
-- A standard 4/4 kick.
d1 $ sound "bd bd bd bd"

-- A clap on the backbeat. The "cp" sample is a good choice.
d2 $ sound "~ cp ~ cp"
```

### Step 2: Creating a Swing Hi-Hat Pattern

Swing is about subtle timing variations. We can create a simple swing feel by shifting the timing of every second hi-hat slightly.

```tidal
-- A classic open/closed hat pattern with shuffle.
-- An outer group of two events, where the second is delayed.
-- The < > brackets group events together.
d3 $ sound "[oh, <[~ oh] [~ oh]>]" # pan sine
```

For a more direct approach, you can experiment with the `swing` function, though manual placement often gives more control.

### Step 3: Writing a Chord Progression

House music is built on chords. The `n` function can take a sequence of chords. Chords are created by grouping notes inside `< >`.

```tidal
-- A simple minor key chord progression (Am - G - C).
-- Numbers represent semitones from the root note. "am" is a built-in chord pattern.
d4 $ n (chord "am g c") # s "superpiano" # gain 0.8
```

You can write the chords manually for more control:
```tidal
-- A minor 7th, to a G major, to a C major.
d4 $ n "<0 3 7 10> <7 11 2> <3 7 10>" # s "superpiano" # gain 0.8
```

### Step 4: Adding a Funky Bassline

The bassline should complement the chords. It's often more melodic and syncopated than in Techno.

```tidal
-- A bassline that follows the Am - G - C progression.
-- The notes are A, A, G, G, C, C.
d5 $ note "0 0 7 7 3 3" # s "bass" # gain 0.9
```

### Step 5: Bringing It All Together

Layer the elements to create the full groove.

```tidal
-- A full basic house groove.
-- Evaluate each line separately to build the track.

-- Kick
d1 $ sound "bd*4"

-- Clap
d2 $ sound "~ cp ~ cp"

-- Swing Hats
d3 $ sound "[oh, <[~ oh] [~ oh]>]"

-- Chords
d4 $ n (chord "am g c") # s "superpiano" # gain 0.7

-- Bassline
d5 $ note "0 0 7 7 3 3" # s "bass" # gain 0.9
```

## 4. Next Steps

- **Groove:** Experiment with different rhythmic placements (`<`, `>`,`[]`) to refine the groove.
- **Melody:** Layer a simple lead melody on top of the chords.
- **Samples:** House often uses samples. Explore the default library or add your own.
- **Arrangement:** Use functions like `stack` and `cat` to structure your track into sections. 