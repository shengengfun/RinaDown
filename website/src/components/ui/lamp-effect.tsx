import { motion } from "framer-motion";
import { cn } from "@/lib/utils";

export function LampEffect({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "relative flex flex-col items-center justify-center w-full",
        className,
      )}
    >
      {/* Lamp glow - positioned absolutely so it doesn't take up layout space */}
      <div className="absolute top-0 left-0 right-0 h-40 flex items-center justify-center isolate z-0">
        <motion.div
          initial={{ opacity: 0.5, width: "15rem" }}
          whileInView={{ opacity: 1, width: "30rem" }}
          transition={{ delay: 0.3, duration: 0.8, ease: "easeInOut" }}
          className="absolute right-1/2 h-32 w-[30rem] bg-gradient-to-r from-transparent via-brand-sky/20 to-transparent blur-[80px]"
          style={{ transform: "translateX(50%)" }}
        />
        <motion.div
          initial={{ opacity: 0.5, width: "15rem" }}
          whileInView={{ opacity: 1, width: "30rem" }}
          transition={{ delay: 0.3, duration: 0.8, ease: "easeInOut" }}
          className="absolute left-1/2 h-32 w-[30rem] bg-gradient-to-l from-transparent via-brand-cyan/20 to-transparent blur-[80px]"
          style={{ transform: "translateX(-50%)" }}
        />
        <motion.div
          initial={{ width: "8rem" }}
          whileInView={{ width: "16rem" }}
          transition={{ delay: 0.3, duration: 0.8, ease: "easeInOut" }}
          className="absolute z-30 h-20 w-64 rounded-full bg-brand-sky/10 blur-2xl"
        />

      </div>

      {/* Content */}
      <div className="relative z-10 pt-16 w-full min-w-0">{children}</div>
    </div>
  );
}
