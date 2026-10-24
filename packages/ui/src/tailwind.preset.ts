import type { Config } from "tailwindcss";

const token = (name: string) => `rgb(var(--${name}) / <alpha-value>)`;
const raw = (name: string) => `var(--${name})`;

/** Maps the Dusk Glass tokens in theme/tokens.css onto Tailwind colours and fonts. */
export default {
  content: [],
  theme: {
    extend: {
      colors: {
        bg: token("bg"),
        ink: token("ink"),
        ink2: token("ink2"),
        ink3: token("ink3"),
        agent: token("agent"),
        play: token("play"),
        kick: token("kick"),
        clap: token("clap"),
        hat: token("hat"),
        ohat: token("ohat"),
        ride: token("ride"),
        acid: token("acid"),
        line: raw("line"),
        line2: raw("line2"),
        glass: raw("glass"),
        glass2: raw("glass2"),
        chassis: token("chassis"),
        faceplate: { DEFAULT: token("faceplate"), hi: token("faceplate-hi") },
        silkscreen: token("silkscreen"),
        led: token("led"),
        slide: token("slide"),
        claude: token("claude"),
        border: token("border"),
        input: token("input"),
        ring: token("ring"),
        background: token("background"),
        foreground: token("foreground"),
        primary: { DEFAULT: token("primary"), foreground: token("primary-foreground") },
        secondary: { DEFAULT: token("secondary"), foreground: token("secondary-foreground") },
        muted: { DEFAULT: token("muted"), foreground: token("muted-foreground") },
        accent: { DEFAULT: token("accent"), foreground: token("accent-foreground") },
        destructive: { DEFAULT: token("destructive"), foreground: token("destructive-foreground") },
        card: { DEFAULT: token("card"), foreground: token("card-foreground") },
        popover: { DEFAULT: token("popover"), foreground: token("popover-foreground") },
      },
      fontFamily: {
        sans: ["Manrope", "sans-serif"],
        mono: ['"JetBrains Mono"', "monospace"],
        // Old names, kept until the screens are restyled.
        label: ["Manrope", "sans-serif"],
        value: ["Manrope", "sans-serif"],
      },
      borderRadius: {
        r: "var(--r)",
        rs: "var(--rs)",
        lg: "var(--radius)",
        md: "calc(var(--radius) - 2px)",
        sm: "calc(var(--radius) - 4px)",
      },
    },
  },
} satisfies Config;
