import ContentLoader from "react-content-loader";

// Animated placeholders tuned to the synthwave dark surfaces. Preferred over
// spinners for list/content loading states.
const BASE = "#190D2E";
const HIGHLIGHT = "#2C1B45";

export function SandboxListSkeleton({ rows = 5 }: { rows?: number }) {
  return (
    <div className="flex flex-col gap-1 px-2">
      {Array.from({ length: rows }).map((_, i) => (
        <ContentLoader
          key={i}
          speed={2}
          width="100%"
          height={40}
          backgroundColor={BASE}
          foregroundColor={HIGHLIGHT}
          className="w-full"
        >
          <circle cx="10" cy="20" r="4" />
          <rect x="24" y="9" rx="4" ry="4" width="60%" height="9" />
          <rect x="24" y="23" rx="3" ry="3" width="35%" height="7" />
        </ContentLoader>
      ))}
    </div>
  );
}

export function InlineSkeleton({
  width = 120,
  height = 12,
}: {
  width?: number;
  height?: number;
}) {
  return (
    <ContentLoader
      speed={2}
      width={width}
      height={height}
      backgroundColor={BASE}
      foregroundColor={HIGHLIGHT}
    >
      <rect x="0" y="0" rx="4" ry="4" width={width} height={height} />
    </ContentLoader>
  );
}

export function LoadingMoreRow() {
  return (
    <div className="flex items-center justify-center gap-2 py-3 text-[11px] text-default-400">
      <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-secondary shadow-[0_0_6px_#05D9E8]" />
      loading more…
    </div>
  );
}

export function OutputSkeleton() {
  return (
    <ContentLoader
      speed={2}
      width="100%"
      height={120}
      backgroundColor={BASE}
      foregroundColor={HIGHLIGHT}
      className="w-full"
    >
      <rect x="0" y="0" rx="4" ry="4" width="90%" height="10" />
      <rect x="0" y="20" rx="4" ry="4" width="80%" height="10" />
      <rect x="0" y="40" rx="4" ry="4" width="95%" height="10" />
      <rect x="0" y="60" rx="4" ry="4" width="60%" height="10" />
      <rect x="0" y="80" rx="4" ry="4" width="75%" height="10" />
    </ContentLoader>
  );
}
