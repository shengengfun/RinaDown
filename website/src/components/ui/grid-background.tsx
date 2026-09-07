import { cn } from "@/lib/utils";
import { useState, useEffect } from "react";

function useIsLight() {
  const [light, setLight] = useState(false);
  useEffect(() => {
    setLight(document.documentElement.classList.contains("light"));
    const handler = (e: CustomEvent<{ light: boolean }>) => setLight(e.detail.light);
    window.addEventListener("theme-change", handler as EventListener);
    return () => window.removeEventListener("theme-change", handler as EventListener);
  }, []);
  return light;
}

export function GridBackground({
  children,
  className,
  gridClassName,
}: {
  children?: React.ReactNode;
  className?: string;
  gridClassName?: string;
}) {
  const isLight = useIsLight();
  const stroke = isLight ? "rgb(0 0 0 / 0.06)" : "rgb(255 255 255 / 0.04)";
  return (
    <div className={cn("relative", className)}>
      <div
        className={cn(
          "absolute inset-0 [mask-image:radial-gradient(ellipse_80%_50%_at_50%_0%,#000_70%,transparent_110%)]",
          gridClassName,
        )}
        style={{
          backgroundImage: `url("data:image/svg+xml,%3csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32' width='32' height='32' fill='none' stroke='${encodeURIComponent(stroke)}'%3e%3cpath d='M0 .5H31.5V32'/%3e%3c/svg%3e")`,
        }}
      />
      {children}
    </div>
  );
}

export function DotBackground({
  children,
  className,
}: {
  children?: React.ReactNode;
  className?: string;
}) {
  const isLight = useIsLight();
  const fill = isLight ? "rgb(0 0 0 / 0.08)" : "rgb(255 255 255 / 0.06)";
  return (
    <div className={cn("relative", className)}>
      <div
        className="absolute inset-0 [mask-image:radial-gradient(ellipse_80%_50%_at_50%_50%,#000_40%,transparent_100%)]"
        style={{
          backgroundImage: `url("data:image/svg+xml,%3csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32' width='16' height='16' fill='none'%3e%3ccircle fill='${encodeURIComponent(fill)}' cx='10' cy='10' r='1'/%3e%3c/svg%3e")`,
        }}
      />
      {children}
    </div>
  );
}
