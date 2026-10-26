import { useId } from "react";

export type TuneMarkProps = {
  /** Width and height in px. */
  size?: number;
  /** Set when the mark stands alone: it then reads as an image named "TuneAI". */
  standalone?: boolean;
  className?: string;
};

const LIT: [number, number][] = [
  [19, 19],
  [41, 19],
  [63, 19],
  [41, 41],
  [63, 41],
  [41, 63],
  [63, 63],
  [41, 85],
  [63, 85],
];
const UNLIT: [number, number][] = [
  [19, 41],
  [85, 41],
  [19, 63],
  [85, 63],
  [19, 85],
  [85, 85],
];

const step = ([x, y]: [number, number]) => (
  <rect key={`${x}-${y}`} x={x} y={y} width="16" height="16" rx="4" />
);

/** The TuneAI mark: a 4x4 step grid whose lit steps spell a T, the top-right one cyan. */
export function TuneMark({ size = 24, standalone = false, className }: TuneMarkProps) {
  const uid = useId().replace(/:/g, "");
  const bg = `${uid}-bg`;
  const glow = `${uid}-glow`;
  return (
    <svg
      viewBox="0 0 120 120"
      width={size}
      height={size}
      className={className}
      {...(standalone ? { role: "img", "aria-label": "TuneAI" } : { "aria-hidden": true as const })}
    >
      <defs>
        <linearGradient id={bg} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#2B1A4F" />
          <stop offset="1" stopColor="#0E0819" />
        </linearGradient>
        <radialGradient id={glow} cx=".25" cy=".15" r=".9">
          <stop offset="0" stopColor="#7B3FD0" stopOpacity=".45" />
          <stop offset="1" stopColor="#7B3FD0" stopOpacity="0" />
        </radialGradient>
      </defs>
      <rect width="120" height="120" rx="28" fill={`url(#${bg})`} />
      <rect width="120" height="120" rx="28" fill={`url(#${glow})`} />
      <rect
        x=".5"
        y=".5"
        width="119"
        height="119"
        rx="27.5"
        fill="none"
        stroke="#FFFFFF"
        strokeOpacity=".12"
      />
      <g fill="#FFFFFF" fillOpacity=".08">
        {UNLIT.map(step)}
      </g>
      <g fill="#A8F04C">{LIT.map(step)}</g>
      <rect x="85" y="19" width="16" height="16" rx="4" fill="#3FDCEB" />
    </svg>
  );
}
