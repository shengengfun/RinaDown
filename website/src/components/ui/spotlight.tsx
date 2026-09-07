import { useRef, useState, useCallback } from "react";
import { motion, useMotionTemplate, useMotionValue } from "framer-motion";
import { cn } from "@/lib/utils";

export function Spotlight({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const mouseX = useMotionValue(-100);
  const mouseY = useMotionValue(-100);

  const handleMouseMove = useCallback(
    (e: React.MouseEvent) => {
      const rect = containerRef.current?.getBoundingClientRect();
      if (rect) {
        mouseX.set(e.clientX - rect.left);
        mouseY.set(e.clientY - rect.top);
      }
    },
    [mouseX, mouseY],
  );

  return (
    <div
      ref={containerRef}
      onMouseMove={handleMouseMove}
      className={cn("relative overflow-hidden", className)}
    >
      <motion.div
        className="pointer-events-none absolute -inset-px z-0 opacity-0 transition-opacity duration-300 group-hover:opacity-100"
        style={{
          background: useMotionTemplate`radial-gradient(650px circle at ${mouseX}px ${mouseY}px, rgba(56, 189, 248, 0.06), transparent 80%)`,
        }}
      />
      {children}
    </div>
  );
}
