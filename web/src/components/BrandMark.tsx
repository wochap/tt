/** Block geometry on the 64-unit grid: [x, y, width, height, color variable]. */
type Block = [number, number, number, number, string];

const SMALL: Block[] = [
  [8, 12, 30, 12, "--ctp-mauve"],
  [22, 26, 34, 12, "--ctp-peach"],
  [12, 40, 26, 12, "--ctp-green"],
];
const MID: Block[] = [
  [10, 14, 26, 10, "--ctp-mauve"],
  [22, 27, 32, 10, "--ctp-peach"],
  [14, 40, 24, 10, "--ctp-green"],
];
const FULL: Block[] = [
  [12, 16, 22, 8, "--ctp-mauve"],
  [22, 28, 30, 8, "--ctp-peach"],
  [16, 40, 18, 8, "--ctp-green"],
  [38, 40, 14, 8, "--ctp-blue"],
];

/** The Timeline blocks mark in the current theme's colors: small ≤24 px, mid ≤40 px, full above. */
export function BrandMark({ size, className }: { size: number; className?: string }) {
  const full = size > 40;
  const blocks = size <= 24 ? SMALL : full ? FULL : MID;
  return (
    <svg
      viewBox="0 0 64 64"
      width={size}
      height={size}
      role="img"
      aria-label="tt"
      focusable="false"
      className={className}
    >
      <rect
        width="64"
        height="64"
        rx="14"
        fill="var(--c-base)"
        {...(full ? { stroke: "var(--c-s1)" } : {})}
        data-tile=""
      />
      {blocks.map(([x, y, width, height, color]) => (
        <rect key={color} x={x} y={y} width={width} height={height} rx="3" fill={`var(${color})`} data-block="" />
      ))}
    </svg>
  );
}
