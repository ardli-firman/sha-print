# Temporary authorization for Linux setup

---
status: accepted
extends: ADR 0018, ADR 0019
---

## Context

Fresh Linux setup changes the Network Channel, Trusted Servers, Client queues, and service configuration. Independent polkit challenges for each administrative area interrupt that flow. Passive diagnostics also invoke firewall tools whose information queries can ask polkit for authorization; refreshing status must not initiate authentication.

## Decision

Before a desktop mutation, authorize `org.shaprint.control.setup` for the exact GUI process using its PID, kernel start time, and effective UID. This polkit meta action uses `auth_admin_keep` for active sessions and implies only the six existing bounded ShaPrint control actions. Each helper execution still uses its original action and validated command. Purge, generic root execution, and general firewall administration are excluded. Inactive or remote subjects cannot acquire the setup meta authorization.

Recheck the meta authorization before every mutation, with one shared gate across runner clones to prevent simultaneous prompts. Polkit owns temporary authorization and its expiration; ShaPrint does not retain passwords, tokens, or an authorization boolean. Denial prevents helper execution. The authorization is temporary, typically about five minutes under the distribution's polkit policy, rather than a guarantee for an arbitrarily long setup session. Expiration or revocation can require authentication again. Existing per-action policy remains available to standalone privileged CLI invocations.

This deliberately replaces per-change Network Channel reauthentication during an authorized desktop setup interval. Outside that interval, its narrow action still requires authentication. The desktop prompt describes all included capabilities before authorization.

Passive diagnostics do not launch firewall tools from an unprivileged process, including the system daemon. They return a warning that firewall rules were not inspected, with an explicit administrative inspection command. CLI and desktop distinguish that warning from a confirmed firewall failure. A root host diagnostic may inspect firewall rules normally. No firewall allowance is inferred from unavailable inspection.

## Consequences

Linux setup can reuse one temporary administrator authorization across different setup operations. Opening the application and refreshing diagnostics never acquire that authorization. Failed mutations still report their actual outcome; authorizing setup alone does not mark a configuration change successful.

No breaking change is introduced. Stable Clients can still print to Nightly Servers and Nightly Clients to Stable Servers under the same supported-version assumptions. Ports, IPPS wire protocol, Network Channel authentication headers, queue URI paths, and TLS identity/fingerprint storage are unchanged. Persisted settings, Trusted Servers, shared printer selections, and Network Channel storage remain backward-compatible on downgrade. No settings migration or `SETTINGS_RISK=true` is required.

Reference: [polkit meta actions and temporary authorization](https://polkit.pages.freedesktop.org/polkit/polkit.8.html).
