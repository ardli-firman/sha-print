import { useEffect, useState } from "react";

import { getControlMode } from "@/api/ipc";
import type { ControlMode } from "@/api/types";

/**
 * The control plane the shell administers, read once per window.
 *
 * Defaults to `local` so the UI keeps its in-process wording when the command is missing or the
 * first read fails: only a shell that explicitly reports `machine` changes what the user is told.
 */
export function useControlMode(): ControlMode {
  const [mode, setMode] = useState<ControlMode>("local");

  useEffect(() => {
    let active = true;

    getControlMode()
      .then((next) => {
        if (active) {
          setMode(next);
        }
      })
      .catch(() => {
        // A shell without the command is a local runtime; nothing to switch to.
      });

    return () => {
      active = false;
    };
  }, []);

  return mode;
}
