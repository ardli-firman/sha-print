import { useState, useEffect } from "react";
import {
  Check,
  CheckCircle2,
  Copy,
  KeyRound,
  Laptop,
  Plus,
  Printer,
  RefreshCw,
  ShieldAlert,
  ShieldCheck,
} from "lucide-react";

import type { ClientQueue, NearbyServer } from "@/api/types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input, PasswordInput } from "@/components/ui/input";
import type { NearbyServersController } from "@/hooks/useNearbyServers";
import type { NetworkChannelController } from "@/hooks/useNetworkChannel";
import type { ServerConnectionsController } from "@/hooks/useServerConnections";

export function chunkFingerprint(fingerprint: string): string {
  if (!fingerprint) return "";
  const parts = fingerprint.split(":");
  if (parts.length <= 1) return fingerprint;
  const chunks: string[] = [];
  for (let i = 0; i < parts.length; i += 4) {
    chunks.push(parts.slice(i, i + 4).join(""));
  }
  return chunks.join(" • ");
}

interface AddPrinterDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  nearby: NearbyServersController;
  connections: ServerConnectionsController;
  networkChannel: NetworkChannelController;
  onInstalled?: (queue: ClientQueue) => void;
  onEnsureProxyRunning?: () => void;
}

type WizardStep = "servers" | "verify" | "printers" | "channel" | "success";

export function AddPrinterDialog({
  open,
  onOpenChange,
  nearby,
  connections,
  networkChannel,
  onInstalled,
  onEnsureProxyRunning,
}: AddPrinterDialogProps) {
  const [step, setStep] = useState<WizardStep>("servers");
  const [manualAddress, setManualAddress] = useState("");
  const [selectedPrinter, setSelectedPrinter] = useState<string | null>(null);
  const [channelInput, setChannelInput] = useState("");
  const [copyStatus, setCopyStatus] = useState(false);

  const { review, result, installed, busy, error } = connections;

  // Reset dialog state when opened
  useEffect(() => {
    if (open) {
      setStep("servers");
      setSelectedPrinter(null);
      setCopyStatus(false);
      setChannelInput("");
    }
  }, [open]);

  // When review updates
  useEffect(() => {
    if (!review) return;

    if (review.trusted) {
      // If already trusted, query printers automatically and jump to printers step
      void connections.query();
      setStep("printers");
    } else {
      // If untrusted/new/changed, show verification step
      setStep("verify");
    }
  }, [review]);

  // When result arrives
  useEffect(() => {
    if (result && step !== "success") {
      setStep("printers");
    }
  }, [result]);

  // When queue installed successfully
  useEffect(() => {
    if (installed) {
      onInstalled?.(installed);
      onEnsureProxyRunning?.();
      setStep("success");
    }
  }, [installed]);

  async function handleSelectServer(address: string) {
    await connections.reviewAddress(address);
  }

  async function handleManualSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!manualAddress.trim()) return;
    await connections.reviewAddress(manualAddress.trim());
  }

  async function handleApproveAndContinue() {
    await connections.approve();
    await connections.query();
    setStep("printers");
  }

  async function handleInstallPrinter(printerName: string) {
    setSelectedPrinter(printerName);

    // If local network channel is not configured, prompt for it before installing
    if (networkChannel.configured === false) {
      setStep("channel");
      return;
    }

    await connections.install(printerName);
  }

  async function handleSaveChannelAndInstall(e: React.FormEvent) {
    e.preventDefault();
    if (!channelInput.trim() || !selectedPrinter) return;

    const saved = await networkChannel.save(channelInput);
    if (saved) {
      await connections.install(selectedPrinter);
    }
  }

  async function handleCopyFingerprint() {
    if (!review?.current_fingerprint) return;
    try {
      await navigator.clipboard.writeText(review.current_fingerprint);
      setCopyStatus(true);
      setTimeout(() => setCopyStatus(false), 2000);
    } catch {
      // Clipboard write failed
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogHeader>
        <div className="flex items-center gap-2 text-primary font-semibold text-xs tracking-wider uppercase">
          <Plus size={14} />
          <span>Add Remote Printer</span>
        </div>
        <DialogTitle>
          {step === "servers" && "Find or Connect to a Server"}
          {step === "verify" && "Verify Server Security Identity"}
          {step === "printers" && "Choose a Shared Printer"}
          {step === "channel" && "Set Network Channel"}
          {step === "success" && "Printer Installed Successfully"}
        </DialogTitle>
        <DialogDescription>
          {step === "servers" &&
            "Select a ShaPrint server detected on your local network, or enter an IP address directly."}
          {step === "verify" &&
            "Confirm the server certificate fingerprint to establish a trusted, encrypted link."}
          {step === "printers" &&
            `Select which printer from ${review?.address ?? "the server"} you want to install on your computer.`}
          {step === "channel" &&
            "A shared Network Channel is required so the server recognizes your print requests."}
          {step === "success" &&
            "The Windows print queue has been created and is ready to accept print jobs."}
        </DialogDescription>
      </DialogHeader>

      {error ? (
        <div className="mb-4 rounded-lg border border-destructive/30 bg-destructive/10 p-3 text-xs flex items-center justify-between text-destructive">
          <div>
            <span className="font-mono font-bold mr-2">{error.code}:</span>
            <span>{error.message}</span>
          </div>
        </div>
      ) : null}

      {/* STEP 1: Select Server */}
      {step === "servers" && (
        <div className="space-y-4 my-2">
          <div className="space-y-2">
            <div className="flex items-center justify-between text-xs font-semibold text-muted-foreground">
              <span className="flex items-center gap-1.5">
                <span className="relative flex h-2 w-2">
                  <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75" />
                  <span className="relative inline-flex rounded-full h-2 w-2 bg-emerald-500" />
                </span>
                <span>Nearby Servers on Local Network</span>
              </span>
              <span>{nearby.servers.length} found</span>
            </div>

            {nearby.servers.length === 0 ? (
              <div className="rounded-lg border border-dashed border-border p-6 text-center text-xs text-muted-foreground">
                <Laptop className="mx-auto size-8 text-muted-foreground/50 mb-2" />
                <p className="font-medium text-foreground">No servers detected yet</p>
                <p className="mt-1">
                  Ensure the server PC has ShaPrint open and printer sharing turned on.
                </p>
              </div>
            ) : (
              <div className="max-h-56 overflow-y-auto space-y-2 pr-1">
                {nearby.servers.map((server: NearbyServer) => (
                  <div
                    key={server.address}
                    className="flex items-center justify-between p-3 rounded-lg border border-border bg-card hover:border-primary/50 hover:bg-accent/40 transition-colors"
                  >
                    <div className="min-w-0 pr-2">
                      <div className="flex items-center gap-2">
                        <Laptop size={16} className="text-primary shrink-0" />
                        <span className="font-semibold text-sm truncate">{server.name}</span>
                      </div>
                      <div className="text-xs text-muted-foreground mt-0.5 font-mono">
                        {server.address}
                      </div>
                      <div className="text-[11px] text-muted-foreground mt-1">
                        {server.printers.length > 0
                          ? `Shares: ${server.printers.join(", ")}`
                          : "Shares 0 printers"}
                      </div>
                    </div>
                    <Button
                      size="sm"
                      onClick={() => void handleSelectServer(server.address)}
                      disabled={busy !== null}
                    >
                      {busy === "inspect" && connections.address === server.address
                        ? "Connecting..."
                        : "Connect"}
                    </Button>
                  </div>
                ))}
              </div>
            )}
          </div>

          <div className="relative border-t border-border pt-4">
            <span className="text-xs font-semibold text-muted-foreground block mb-2">
              Or connect by IP address / Hostname:
            </span>
            <form onSubmit={(e) => void handleManualSubmit(e)} className="flex gap-2">
              <Input
                placeholder="e.g. 192.168.1.50:8631"
                value={manualAddress}
                onChange={(e) => setManualAddress(e.target.value)}
                disabled={busy !== null}
              />
              <Button type="submit" disabled={busy !== null || !manualAddress.trim()}>
                {busy === "inspect" ? "Checking..." : "Connect"}
              </Button>
            </form>
          </div>
        </div>
      )}

      {/* STEP 2: Verify Identity */}
      {step === "verify" && review && (
        <div className="space-y-4 my-2">
          {review.previous_fingerprint ? (
            <div className="rounded-lg border border-destructive/40 bg-destructive/10 p-3 text-xs text-destructive flex gap-2">
              <ShieldAlert size={18} className="shrink-0 mt-0.5" />
              <div>
                <strong>Warning: Server Identity Changed!</strong>
                <p className="mt-0.5 text-muted-foreground">
                  The security certificate presented by this server does not match the previously
                  approved fingerprint. Verify this change with the server owner.
                </p>
              </div>
            </div>
          ) : (
            <div className="rounded-lg border border-border bg-muted/30 p-3 text-xs text-muted-foreground flex gap-2">
              <ShieldCheck size={18} className="shrink-0 text-primary mt-0.5" />
              <div>
                <strong className="text-foreground">First-Time Connection</strong>
                <p className="mt-0.5">
                  ShaPrint encrypts all communications with TLS. Compare this fingerprint with the
                  code shown on the server computer.
                </p>
              </div>
            </div>
          )}

          <div className="rounded-lg border border-border bg-card p-3 space-y-2">
            <div className="flex items-center justify-between text-xs text-muted-foreground">
              <span>Server Address:</span>
              <span className="font-mono font-semibold text-foreground">{review.address}</span>
            </div>

            <div>
              <span className="text-xs text-muted-foreground block mb-1">SHA-256 Fingerprint:</span>
              <div className="rounded bg-muted/70 p-2.5 font-mono text-xs text-foreground tracking-wider break-all flex items-center justify-between gap-2">
                <span>{chunkFingerprint(review.current_fingerprint)}</span>
                <Button
                  size="sm"
                  variant="ghost"
                  className="h-7 px-2 shrink-0"
                  onClick={() => void handleCopyFingerprint()}
                >
                  {copyStatus ? <Check size={13} className="text-emerald-500" /> : <Copy size={13} />}
                  <span className="text-[11px] ml-1">{copyStatus ? "Copied" : "Copy"}</span>
                </Button>
              </div>
            </div>
          </div>

          <div className="flex justify-end gap-2 pt-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => setStep("servers")}
              disabled={busy !== null}
            >
              Back
            </Button>
            <Button
              type="button"
              size="sm"
              onClick={() => void handleApproveAndContinue()}
              disabled={busy !== null}
            >
              {busy === "approve" ? "Trusting..." : "Trust Server & Continue"}
            </Button>
          </div>
        </div>
      )}

      {/* STEP 3: Choose Printer */}
      {step === "printers" && (
        <div className="space-y-4 my-2">
          <div className="rounded-lg border border-emerald-500/20 bg-emerald-500/5 p-2.5 text-xs text-emerald-700 dark:text-emerald-400 flex items-center justify-between">
            <span className="flex items-center gap-1.5 font-medium">
              <ShieldCheck size={15} />
              <span>Connected to {connections.address}</span>
            </span>
            <Badge variant="success">Trusted</Badge>
          </div>

          <div className="space-y-2">
            <span className="text-xs font-semibold text-muted-foreground block">
              Printers Available for Installation:
            </span>

            {busy === "printers" ? (
              <div className="py-8 text-center text-xs text-muted-foreground flex flex-col items-center gap-2">
                <RefreshCw className="animate-spin size-4 text-primary" />
                <span>Loading available queues...</span>
              </div>
            ) : !result || result.printers.length === 0 ? (
              <div className="rounded-lg border border-dashed border-border p-6 text-center text-xs text-muted-foreground">
                <Printer className="mx-auto size-7 text-muted-foreground/40 mb-1" />
                <p className="font-semibold text-foreground">No printers shared</p>
                <p className="mt-0.5">The server owner has not shared any local printer queues yet.</p>
              </div>
            ) : (
              <div className="space-y-2 max-h-56 overflow-y-auto pr-1">
                {result.printers.map((name: string) => (
                  <div
                    key={name}
                    className="flex items-center justify-between p-3 rounded-lg border border-border bg-card hover:border-primary/40 transition-colors"
                  >
                    <div className="flex items-center gap-2.5">
                      <Printer size={17} className="text-primary shrink-0" />
                      <div>
                        <span className="font-semibold text-sm">{name}</span>
                        <p className="text-[11px] text-muted-foreground">
                          Installs as Windows queue via local proxy
                        </p>
                      </div>
                    </div>
                    <Button
                      size="sm"
                      disabled={busy === "install"}
                      onClick={() => void handleInstallPrinter(name)}
                    >
                      {busy === "install" && selectedPrinter === name ? "Installing..." : "Install"}
                    </Button>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      )}

      {/* STEP 4: Network Channel Prompt */}
      {step === "channel" && (
        <form onSubmit={(e) => void handleSaveChannelAndInstall(e)} className="space-y-4 my-2">
          <div className="rounded-lg border border-warning/30 bg-warning/10 p-3 text-xs text-warning-foreground flex gap-2">
            <KeyRound size={18} className="shrink-0 text-warning mt-0.5" />
            <div>
              <strong>Network Channel Required</strong>
              <p className="mt-0.5 text-muted-foreground">
                The Network Channel authorizes print jobs between ShaPrint computers. Enter the same
                shared secret that is configured on the server.
              </p>
            </div>
          </div>

          <div className="space-y-1.5">
            <label className="text-xs font-semibold text-foreground">Network Channel Secret:</label>
            <PasswordInput
              placeholder="Enter shared secret..."
              value={channelInput}
              onChange={(e) => setChannelInput(e.target.value)}
              required
              disabled={networkChannel.saving || busy !== null}
            />
          </div>

          <div className="flex justify-end gap-2 pt-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => setStep("printers")}
              disabled={networkChannel.saving || busy !== null}
            >
              Back
            </Button>
            <Button
              type="submit"
              size="sm"
              disabled={networkChannel.saving || !channelInput.trim() || busy !== null}
            >
              {networkChannel.saving ? "Saving..." : "Save & Install Printer"}
            </Button>
          </div>
        </form>
      )}

      {/* STEP 5: Success */}
      {step === "success" && installed && (
        <div className="space-y-4 my-4 text-center">
          <div className="mx-auto flex size-12 items-center justify-center rounded-full bg-emerald-500/10 text-emerald-600 dark:text-emerald-400">
            <CheckCircle2 size={28} />
          </div>

          <div className="space-y-1">
            <h3 className="font-semibold text-base text-foreground">
              {installed.queue_name} is Ready!
            </h3>
            <p className="text-xs text-muted-foreground max-w-sm mx-auto">
              Your Windows print queue has been created and routes through ShaPrint. You can now select
              this printer in any document or application.
            </p>
          </div>

          <div className="rounded-lg border border-border bg-muted/40 p-3 text-left text-xs space-y-1.5 max-w-sm mx-auto">
            <div className="flex justify-between">
              <span className="text-muted-foreground">Queue Name:</span>
              <span className="font-semibold text-foreground">{installed.queue_name}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-muted-foreground">Server Printer:</span>
              <span className="font-medium text-foreground">{installed.printer_name}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-muted-foreground">Server Address:</span>
              <span className="font-mono text-foreground">{installed.server_address}</span>
            </div>
          </div>

          <div className="pt-2 flex justify-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => {
                setStep("printers");
              }}
            >
              Install Another Printer
            </Button>
            <Button size="sm" onClick={() => onOpenChange(false)}>
              Done
            </Button>
          </div>
        </div>
      )}
    </Dialog>
  );
}
