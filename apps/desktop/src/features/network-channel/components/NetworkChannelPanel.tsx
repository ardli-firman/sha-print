import { KeyRound, RefreshCw } from "lucide-react";
import { useState, type FormEvent } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { PasswordInput } from "@/components/ui/input";
import type { NetworkChannelController } from "../hooks/useNetworkChannel";

export interface NetworkChannelPanelProps {
  settings: NetworkChannelController;
}

/** Lets the user set or replace the shared print authorization secret. */
export function NetworkChannelPanel({ settings }: NetworkChannelPanelProps) {
  const [value, setValue] = useState("");
  const [saved, setSaved] = useState(false);
  const { configured, saving, error, save, refresh } = settings;

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSaved(false);
    if (await save(value)) {
      setValue("");
      setSaved(true);
    }
  }

  return (
    <section className="panel" aria-labelledby="network-channel-heading">
      <div className="panel-header flex items-center justify-between">
        <div className="flex items-center gap-2.5">
          <div className="flex size-7 items-center justify-center rounded-lg bg-primary/10 text-primary">
            <KeyRound size={16} />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <h2 id="network-channel-heading" className="text-base font-semibold leading-none">
                Print authorization
              </h2>
              {configured !== null && (
                <Badge variant={configured ? "success" : "warning"} className="text-[10px]">
                  {configured ? "Active" : "Not Set"}
                </Badge>
              )}
            </div>
            <p className="hint text-xs mt-1 text-muted-foreground">
              Shared secret authorizing print jobs across computers.
            </p>
          </div>
        </div>

        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={() => void refresh()}
          disabled={saving}
          className="text-xs h-8"
        >
          <RefreshCw size={13} className={saving ? "animate-spin mr-1" : "mr-1"} />
          Refresh
        </Button>
      </div>

      <p className="hint text-xs text-muted-foreground leading-relaxed">
        {configured === null
          ? "Checking Network Channel status…"
          : configured
            ? "A Network Channel is configured. Enter a new value to replace it."
            : "Configure a Network Channel before clients can submit print jobs."}
      </p>

      {error ? <p className="banner" role="alert">{error.message}</p> : null}
      {saved ? <p className="hint text-xs text-emerald-600 dark:text-emerald-400 font-medium" role="status">Network Channel updated.</p> : null}

      <form onSubmit={(event) => void submit(event)} className="space-y-3 pt-1">
        <div className="space-y-1.5">
          <label htmlFor="network-channel" className="text-xs font-semibold text-foreground block">
            Network Channel
          </label>
          <div className="flex flex-col sm:flex-row gap-2">
            <PasswordInput
              id="network-channel"
              name="network-channel"
              autoComplete="new-password"
              placeholder="Enter authorization secret..."
              value={value}
              onChange={(event) => setValue(event.currentTarget.value)}
              required
              disabled={saving}
              className="flex-1 text-xs"
            />
            <Button
              type="submit"
              disabled={saving || value.trim().length === 0}
              className="text-xs h-9 px-4 shrink-0 font-medium"
            >
              {saving ? "Saving…" : "Set Network Channel"}
            </Button>
          </div>
        </div>

        <p className="hint text-[11px] text-muted-foreground">
          Both client and server must use the same channel to authorize print jobs.
        </p>
      </form>
    </section>
  );
}
