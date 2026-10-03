import { useState, type ComponentProps } from "react";
import { Eye, EyeOff } from "lucide-react";

import { cn } from "@/lib/utils";

export type InputProps = ComponentProps<"input">;

export function Input({ className, type, ...props }: InputProps) {
  return (
    <input
      type={type}
      data-slot="input"
      className={cn(
        "flex h-9 w-full rounded-md border border-input bg-background px-3 py-1 text-sm shadow-xs transition-colors placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      {...props}
    />
  );
}

export type PasswordInputProps = Omit<InputProps, "type"> & {
  visibilityLabel: string;
};

export function PasswordInput({
  className,
  disabled,
  id,
  visibilityLabel,
  ...props
}: PasswordInputProps) {
  const [show, setShow] = useState(false);
  const actionLabel = `${show ? "Hide" : "Show"} ${visibilityLabel}`;

  return (
    <div className="password-field relative flex w-full items-center">
      <Input
        id={id}
        type={show ? "text" : "password"}
        disabled={disabled}
        className={cn("pr-10", className)}
        {...props}
      />
      <button
        type="button"
        data-slot="password-visibility-toggle"
        className="password-visibility-toggle absolute right-1 flex size-7 items-center justify-center"
        onClick={() => setShow((prev) => !prev)}
        disabled={disabled}
        aria-label={actionLabel}
        aria-controls={id}
        title={actionLabel}
      >
        {show ? (
          <EyeOff size={15} aria-hidden="true" />
        ) : (
          <Eye size={15} aria-hidden="true" />
        )}
      </button>
    </div>
  );
}
