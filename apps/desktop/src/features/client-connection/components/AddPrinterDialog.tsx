import { useState, useEffect } from "react";
import {
  Check,
  CheckCircle2,
  Copy,
  KeyRound,
  Laptop,
  Printer,
  Radio,
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
import type { NearbyServersController } from "../hooks/useNearbyServers";
import type { ServerConnectionsController } from "../hooks/useServerConnections";
import type { NetworkChannelController } from "@/features/network-channel";

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

const WIZARD_STEPS = ["Server", "Identity", "Printer"] as const;

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
  const currentStepIndex = step === "servers" ? 0 : step === "verify" ? 1 : 2;

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
      setChannelInput("");
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
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      ariaLabelledBy="add-printer-title"
      ariaDescribedBy="add-printer-description"
    >
      <DialogHeader className="printer-wizard-header">
        <DialogTitle id="add-printer-title">
          {step === "servers" && "Find a server"}
          {step === "verify" && "Check the server identity"}
          {step === "printers" && "Choose a printer"}
          {step === "channel" && "Set the Network Channel"}
          {step === "success" && "Printer installed"}
        </DialogTitle>
        <DialogDescription id="add-printer-description">
          {step === "servers" &&
            "Choose a nearby ShaPrint server or enter its address. Printers stay hidden until you approve its fingerprint."}
          {step === "verify" &&
            "Compare this fingerprint with the server owner before approving it. Approval lets ShaPrint list the shared printers."}
          {step === "printers" &&
            `Choose a printer shared by ${review?.address ?? "this server"}.`}
          {step === "channel" &&
            "This server requires a Network Channel to authorize print jobs. Use the same value configured on the server."}
          {step === "success" &&
            "The Windows printer queue is installed. Choose it from any Windows print dialog."}
        </DialogDescription>
      </DialogHeader>

      <ol className="wizard-progress" aria-label="Printer setup steps">
        {WIZARD_STEPS.map((label, index) => {
          const complete = step === "success" || index < currentStepIndex;
          const current = step !== "success" && index === currentStepIndex;
          return (
            <li
              key={label}
              className={`wizard-step${complete ? " is-complete" : ""}${current ? " is-current" : ""}`}
              aria-current={current ? "step" : undefined}
            >
              <span className="wizard-step-marker" aria-hidden="true">
                {complete ? <Check size={12} /> : index + 1}
              </span>
              <span>{label}</span>
            </li>
          );
        })}
      </ol>

      {error ? (
        <div className="wizard-error" role="alert">
          <span className="banner-code">{error.code}</span>
          <span>{error.message}</span>
        </div>
      ) : null}
      {step === "channel" && networkChannel.error ? (
        <div className="wizard-error" role="alert">
          <span className="banner-code">{networkChannel.error.code}</span>
          <span>{networkChannel.error.message}</span>
        </div>
      ) : null}

      {/* STEP 1: Select Server */}
      {step === "servers" && (
        <div className="space-y-4 my-2">
          <div className="space-y-2">
            <div className="wizard-section-heading">
              <span className="wizard-section-title">
                <Radio size={15} aria-hidden="true" />
                Nearby servers
              </span>
              <span className="wizard-server-count" role="status">
                {nearby.servers.length} {nearby.servers.length === 1 ? "server" : "servers"} found
              </span>
            </div>

            {nearby.servers.length === 0 ? (
              <div className="connection-empty wizard-empty" role="status">
                <strong>No servers found on this network.</strong>
                <p>Enter a server address below. Discovery does not cross subnets.</p>
              </div>
            ) : (
              <div className="max-h-56 overflow-y-auto space-y-2 pr-1">
                {nearby.servers.map((server: NearbyServer) => (
                  <div
                    key={server.address}
                    className="wizard-server-option"
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
                        ? "Reviewing…"
                        : "Review identity"}
                    </Button>
                  </div>
                ))}
              </div>
            )}
          </div>

          <div className="manual-server-entry">
            <label htmlFor="wizard-server-address" className="wizard-field-label">
              Or enter a server address
            </label>
            <form onSubmit={(e) => void handleManualSubmit(e)} className="flex gap-2">
              <Input
                id="wizard-server-address"
                placeholder="printer.local or 192.168.1.50:8631"
                value={manualAddress}
                onChange={(e) => setManualAddress(e.target.value)}
                autoComplete="off"
                disabled={busy !== null}
              />
              <Button type="submit" disabled={busy !== null || !manualAddress.trim()}>
                {busy === "inspect" ? "Reviewing…" : "Review identity"}
              </Button>
            </form>
          </div>
        </div>
      )}

      {/* STEP 2: Verify Identity */}
      {step === "verify" && review && (
        <div className="space-y-4 my-2">
          {review.previous_fingerprint ? (
            <div className="wizard-trust-warning" role="alert">
              <ShieldAlert size={18} className="shrink-0" aria-hidden="true" />
              <div>
                <strong>Server identity has changed.</strong>
                <p>
                  This fingerprint does not match the one you approved before. Verify the change
                  with the server owner over a trusted channel before reapproving.
                </p>
              </div>
            </div>
          ) : (
            <div className="wizard-trust-note" role="status">
              <ShieldCheck size={18} className="shrink-0 text-primary" aria-hidden="true" />
              <div>
                <strong>New server</strong>
                <p>
                  Compare this fingerprint with the code shown on the server computer. Printers
                  remain hidden until you approve the match.
                </p>
              </div>
            </div>
          )}

          <div className="wizard-fingerprint">
            <div className="flex items-center justify-between text-xs text-muted-foreground">
              <span>Server address</span>
              <span className="font-mono font-semibold text-foreground">{review.address}</span>
            </div>

            <div>
              <span className="wizard-field-label block mb-1">Server fingerprint (SHA-256)</span>
              <div className="wizard-fingerprint-value">
                <span>{chunkFingerprint(review.current_fingerprint)}</span>
                <Button
                  size="sm"
                  variant="ghost"
                  className="h-7 px-2 shrink-0"
                  onClick={() => void handleCopyFingerprint()}
                >
                  {copyStatus ? <Check size={13} className="text-emerald-500" /> : <Copy size={13} />}
                  <span className="text-[11px] ml-1" aria-live="polite">
                    {copyStatus ? "Copied" : "Copy"}
                  </span>
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
              {busy === "approve"
                ? "Saving approval…"
                : review.previous_fingerprint
                  ? "Reapprove fingerprint"
                  : "Approve fingerprint"}
            </Button>
          </div>
        </div>
      )}

      {/* STEP 3: Choose Printer */}
      {step === "printers" && (
        <div className="space-y-4 my-2">
          <div className="wizard-approved-server" role="status">
            <span>Approved server: {connections.address}</span>
            <Badge variant="success">Fingerprint approved</Badge>
          </div>

          <div className="space-y-2">
            <h3 className="wizard-section-title">Shared printers</h3>

            {busy === "printers" ? (
              <p className="wizard-loading" role="status">
                <RefreshCw className="size-4 text-primary" aria-hidden="true" />
                Loading shared printers…
              </p>
            ) : !result || result.printers.length === 0 ? (
              <div className="connection-empty" role="status">
                <strong>No printers shared yet.</strong>
                <p>Ask the server owner to select local printer queues on their Share page.</p>
              </div>
            ) : (
              <div className="space-y-2 max-h-56 overflow-y-auto pr-1">
                {result.printers.map((name: string) => (
                  <div
                    key={name}
                    className="wizard-printer-option"
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
                      disabled={busy !== null}
                      onClick={() => void handleInstallPrinter(name)}
                    >
                      {busy === "install" && selectedPrinter === name ? "Installing…" : "Install"}
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
          <div className="wizard-channel-note">
            <KeyRound size={18} className="shrink-0 text-warning" aria-hidden="true" />
            <div>
              <strong>Network Channel required</strong>
              <p>
                Enter the same value configured on the server. It authorizes print jobs between
                these computers.
              </p>
            </div>
          </div>

          <div className="space-y-1.5">
            <label htmlFor="wizard-network-channel" className="wizard-field-label">
              Network Channel
            </label>
            <PasswordInput
              id="wizard-network-channel"
              autoComplete="new-password"
              placeholder="Enter the shared Network Channel"
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
              {networkChannel.saving ? "Saving…" : "Save Network Channel and install"}
            </Button>
          </div>
        </form>
      )}

      {/* STEP 5: Success */}
      {step === "success" && installed && (
        <div className="wizard-success">
          <div className="wizard-success-heading" role="status">
            <CheckCircle2 size={22} aria-hidden="true" />
            <div>
              <h3>{installed.queue_name}</h3>
              <p>Installed. Choose this printer from any Windows print dialog.</p>
            </div>
          </div>

          <dl className="wizard-install-details" aria-label="Installed printer details">
            <div>
              <dt>Shared printer</dt>
              <dd>{installed.printer_name}</dd>
            </div>
            <div>
              <dt>Server</dt>
              <dd>{installed.server_address}</dd>
            </div>
          </dl>

          <div className="wizard-success-actions">
            <Button
              variant="outline"
              size="sm"
              onClick={() => {
                setStep("printers");
              }}
            >
              Install another
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
