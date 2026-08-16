// The cVisor mark: a minimalist neon isometric box on near-black, glowing in
// synthwave magenta + cyan. Mirrors src-tauri/icons/icon.svg and the favicon.
export function Logo({ size = 22 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 64 64"
      fill="none"
      aria-hidden="true"
    >
      <defs>
        <filter id="cvNeon" x="-50%" y="-50%" width="200%" height="200%">
          <feGaussianBlur stdDeviation="1.6" result="blur" />
          <feMerge>
            <feMergeNode in="blur" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
      </defs>
      <rect x="2" y="2" width="60" height="60" rx="14" fill="#0B0B0D" />
      <g
        filter="url(#cvNeon)"
        strokeWidth="2.4"
        strokeLinejoin="round"
        strokeLinecap="round"
        fill="none"
      >
        {/* top face */}
        <path d="M32 12 50 22.5 32 33 14 22.5Z" stroke="#05D9E8" />
        {/* left + right + center vertical edges, bottom */}
        <path d="M14 22.5 14 41.5 32 52 50 41.5 50 22.5" stroke="#FF2A6D" />
        <path d="M32 33 32 52" stroke="#FF2A6D" />
      </g>
    </svg>
  );
}
