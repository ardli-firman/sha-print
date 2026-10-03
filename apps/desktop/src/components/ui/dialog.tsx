import { useEffect, type ReactNode } from "react";
import { X } from "lucide-react";

import { cn } from "@/lib/utils";

interface DialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  children: ReactNode;
  ariaLabelledBy?: string;
  ariaDescribedBy?: string;
}

export function Dialog({
  open,
  onOpenChange,
  children,
  ariaLabelledBy,
  ariaDescribedBy,
}: DialogProps) {
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape" && open) {
        onOpenChange(false);
      }
    }
    if (open) {
      document.body.style.overflow = "hidden";
      window.addEventListener("keydown", handleKeyDown);
    }
    return () => {
      document.body.style.overflow = "";
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [open, onOpenChange]);

  if (!open) return null;

  return (
    <div
      className="dialog-backdrop fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/55 animate-in fade-in-0"
      onClick={() => onOpenChange(false)}
      role="dialog"
      aria-modal="true"
      aria-labelledby={ariaLabelledBy}
      aria-describedby={ariaDescribedBy}
    >
      <div
        className="dialog-surface relative w-full max-w-xl rounded-md border border-border bg-card p-6 text-card-foreground shadow-xl duration-200 animate-in zoom-in-95 max-h-[90vh] overflow-y-auto"
        onClick={(e) => e.stopPropagation()}
      >
        {children}
        <button
          type="button"
          onClick={() => onOpenChange(false)}
          className="dialog-close absolute right-4 top-4 rounded-sm p-1 text-muted-foreground opacity-70 transition-opacity hover:opacity-100 hover:text-foreground focus:outline-none focus:ring-2 focus:ring-ring"
          aria-label="Close"
        >
          <X size={16} />
        </button>
      </div>
    </div>
  );
}

export function DialogHeader({
  className,
  children,
}: {
  className?: string;
  children: ReactNode;
}) {
  return (
    <div className={cn("flex flex-col space-y-1.5 text-left mb-4", className)}>
      {children}
    </div>
  );
}

export function DialogTitle({
  id,
  className,
  children,
}: {
  id?: string;
  className?: string;
  children: ReactNode;
}) {
  return (
    <h2 id={id} className={cn("text-lg font-semibold leading-none tracking-tight", className)}>
      {children}
    </h2>
  );
}

export function DialogDescription({
  id,
  className,
  children,
}: {
  id?: string;
  className?: string;
  children: ReactNode;
}) {
  return (
    <p id={id} className={cn("text-sm text-muted-foreground mt-1.5 leading-relaxed", className)}>
      {children}
    </p>
  );
}

export function DialogFooter({
  className,
  children,
}: {
  className?: string;
  children: ReactNode;
}) {
  return (
    <div
      className={cn(
        "flex flex-col-reverse sm:flex-row sm:justify-end sm:space-x-2 gap-2 mt-6 pt-4 border-t border-border",
        className,
      )}
    >
      {children}
    </div>
  );
}
