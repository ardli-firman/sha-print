import { useState, type FormEvent } from "react";

import type { NetworkChannelController } from "../hooks/useNetworkChannel";

interface NetworkChannelPanelProps {
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
      <div className="panel-header">
        <h2 id="network-channel-heading">Print authorization</h2>
        <button type="button" onClick={() => void refresh()} disabled={saving}>Refresh</button>
      </div>
      <p className="hint">
        {configured === null
          ? "Checking Network Channel status…"
          : configured
            ? "A Network Channel is configured. Enter a new value to replace it."
            : "Configure a Network Channel before clients can submit print jobs."}
      </p>
      {error ? <p className="banner" role="alert">{error.message}</p> : null}
      {saved ? <p className="hint" role="status">Network Channel updated.</p> : null}
      <form onSubmit={(event) => void submit(event)}>
        <label htmlFor="network-channel">Network Channel</label>
        <input
          id="network-channel"
          name="network-channel"
          type="password"
          autoComplete="new-password"
          value={value}
          onChange={(event) => setValue(event.currentTarget.value)}
          required
          disabled={saving}
        />
        <button type="submit" disabled={saving || value.length === 0}>
          {saving ? "Saving…" : configured ? "Update Network Channel" : "Set Network Channel"}
        </button>
      </form>
    </section>
  );
}
