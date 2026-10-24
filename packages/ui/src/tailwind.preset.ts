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
      },
      fontFamily: {
        sans: ["Manrope", "sans-serif"],
        mono: ['"JetBrains Mono"', "monospace"],
      },
      borderRadius: {
        r: "var(--r)",
        rs: "var(--rs)",
      },
    },
  },
} satisfies Config;
