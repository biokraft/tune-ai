import type { Config } from "tailwindcss";

const token = (name: string) => `hsl(var(--${name}) / <alpha-value>)`;

/** Maps the instrument tokens in theme/tokens.css onto Tailwind colours and fonts. */
export default {
  content: [],
  theme: {
    extend: {
      colors: {
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
        label: ['"Barlow Condensed"', "sans-serif"],
        value: ["Barlow", "sans-serif"],
      },
      borderRadius: {
        lg: "var(--radius)",
        md: "calc(var(--radius) - 2px)",
        sm: "calc(var(--radius) - 4px)",
      },
    },
  },
} satisfies Config;
