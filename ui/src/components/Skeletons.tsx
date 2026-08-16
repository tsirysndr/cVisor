import ContentLoader from "react-content-loader";
import { useAtomValue } from "jotai";
import { themeAtom } from "../state/atoms";

// Animated placeholders (preferred over spinners for list/content loading
// states) with shimmer colors matched to the active theme's surfaces.
const COLORS = {
  dark: { base: "#27264D", highlight: "#32305A" },
  light: { base: "#ECECF1", highlight: "#F6F6F9" },
};

function useSkeletonColors() {
  const theme = useAtomValue(themeAtom);
  return COLORS[theme === "light" ? "light" : "dark"];
}

export function SandboxListSkeleton({ rows = 5 }: { rows?: number }) {
  const { base, highlight } = useSkeletonColors();
  return (
    <div className="flex flex-col gap-1 px-2">
      {Array.from({ length: rows }).map((_, i) => (
        <ContentLoader
          key={i}
          speed={2}
          width="100%"
          height={40}
          backgroundColor={base}
          foregroundColor={highlight}
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
  const { base, highlight } = useSkeletonColors();
  return (
    <ContentLoader
      speed={2}
      width={width}
      height={height}
      backgroundColor={base}
      foregroundColor={highlight}
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
  const { base, highlight } = useSkeletonColors();
  return (
    <ContentLoader
      speed={2}
      width="100%"
      height={120}
      backgroundColor={base}
      foregroundColor={highlight}
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
