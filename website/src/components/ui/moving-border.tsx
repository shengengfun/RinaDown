import { useRef } from "react";
import { motion, useAnimationFrame } from "framer-motion";
import { cn } from "@/lib/utils";

export function MovingBorder({
  children,
  duration = 4000,
  className,
  containerClassName,
  borderClassName,
  as: Component = "div",
  ...props
}: {
  children: React.ReactNode;
  duration?: number;
  className?: string;
  containerClassName?: string;
  borderClassName?: string;
  as?: React.ElementType;
  [key: string]: unknown;
}) {
  return (
    <Component
      className={cn("relative overflow-hidden rounded-xl p-[1px]", containerClassName)}
      {...props}
    >
      <GradientBorder duration={duration} className={borderClassName} />
      <div className={cn("relative z-10 rounded-[11px] bg-dark-surface1", className)}>
        {children}
      </div>
    </Component>
  );
}

function GradientBorder({
  duration = 4000,
  className,
}: {
  duration?: number;
  className?: string;
}) {
  const pathRef = useRef<SVGRectElement>(null);
  const progress = useRef(0);

  useAnimationFrame((time) => {
    const length = pathRef.current?.getTotalLength();
    if (length) {
      progress.current = (time / duration) % 1;
      const point = pathRef.current?.getPointAtLength(progress.current * length);
      if (point) {
        const x = point.x;
        const y = point.y;
        document.documentElement.style.setProperty("--moving-border-x", `${x}px`);
        document.documentElement.style.setProperty("--moving-border-y", `${y}px`);
      }
    }
  });

  return (
    <div className="absolute inset-0 z-0">
      <svg
        className="absolute h-full w-full"
        viewBox="0 0 100 100"
        preserveAspectRatio="none"
        xmlns="http://www.w3.org/2000/svg"
      >
        <rect
          ref={pathRef}
          x="0"
          y="0"
          width="100"
          height="100"
          fill="none"
          className="invisible"
        />
      </svg>
      <motion.div
        className={cn(
          "absolute inset-0 rounded-xl",
          className,
        )}
        style={{
          background:
            "conic-gradient(from 0deg, transparent 0%, #38bdf8 10%, #06b6d4 20%, transparent 30%)",
        }}
        animate={{ rotate: 360 }}
        transition={{ duration: duration / 1000, repeat: Infinity, ease: "linear" }}
      />
    </div>
  );
}
