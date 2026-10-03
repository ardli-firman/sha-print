import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

interface TooltipProps {
  content: string;
  children: ReactNode;
  className?: string;
  side?: "top" | "bottom" | "left" | "right";
}

export function Tooltip({
  content,
  children,
  className,
  side = "top",
}: TooltipProps) {
  const sideClasses = {
    top: "bottom-full left-1/2 -translate-x-1/2 mb-1.5",
    bottom: "top-full left-1/2 -translate-x-1/2 mt-1.5",
    left: "right-full top-1/2 -translate-y-1/2 mr-1.5",
    right: "left-full top-1/2 -translate-y-1/2 ml-1.5",
  };

  return (
    <div className={cn("relative group inline-flex items-center", className)}>
      {children}
      <div
        className={cn(
          "pointer-events-none absolute hidden group-hover:flex group-focus-within:flex flex-col items-center z-50 transition-opacity animate-in fade-in-0",
          sideClasses[side],
        )}
      >
        <div className="rounded-md bg-foreground text-background px-2.5 py-1 text-xs font-medium shadow-md whitespace-nowrap">
          {content}
        </div>
      </div>
    </div>
  );
}
