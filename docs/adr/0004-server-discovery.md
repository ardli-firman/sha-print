# Nearby-server discovery over multicast DNS

The discovery port selection (5353/5354), single-interface binding, and TTL=1 limit below are replaced by ADR 0014.

---
status: accepted
---

A client finds ShaPrint servers without being told an address. The server advertises itself while it
shares, using DNS-SD records carried in multicast DNS messages (RFC 1035 message format with the
multicast rules of RFC 6762 and the service conventions of RFC 6763), and the client browses for
them, so the same flow that approves a manual address approves a discovered one.

## Why discovery is not signed

Discovery carries no trust. A discovered server appears in the list as a *suggestion to review*: the
client opens TLS, shows the certificate's SHA-256 fingerprint, and pins it only when the user
approves it (ADR 0003). A forged advertisement can therefore achieve exactly one thing — it can put
an address in front of a user — and every defence that matters is already at the TLS layer. If the
fingerprint is not the one that was approved, the connection is blocked and the user is told.

This deliberately replaces the legacy `.NET` application's UDP discovery, whose responses were
signed with HMAC-SHA256. That design had no certificate pinning to anchor identity, so the signature
was the only thing standing between a client and a spoofed server. The Tauri application pins the
certificate instead, which is a stronger anchor: the signature verified a shared secret, while the
fingerprint identifies the exact server the user approved.

## The advertisement

One service instance per server, of type `_shaprint-ipps._tcp.local.`. A private service type is
deliberate: a ShaPrint browser must not offer every AirPrint printer on the network as a server to
review. The instance name is the computer name, and its `TXT` records carry `rp=ipp/print`, a
human-readable `name`, and one `queue` entry per shared queue. The advertisement is trimmed to fit
1400 bytes, so a server sharing more queues than fit in one datagram still appears, and the endpoint
returns the full, authoritative list once the user approves it.

Withdrawing is the same answer with a zero lifetime. It has to be *delivered*: a browsing client
never joins the multicast group (it asks from its own port and only hears the reply), so a goodbye
sent only to the group would be heard by nobody, and a stopped server would sit in every client's
list until its advertisement aged out. A responder therefore remembers the sockets that asked
recently, and sends the withdrawal back to each of them as well as to the group. Those browsers drop
the server at once; anyone who joins the group instead hears the standard goodbye. The list is
bounded, and the copy sent to the group keeps the advertisement conformant.

The advertisement exists exactly as long as sharing does — it is opened when sharing starts, follows
the shared-printer selection, and is withdrawn when sharing stops. A browser also bounds how long it
keeps any advertisement, so a server that vanishes without a goodbye (a crash, a pulled cable) leaves
the list within seconds rather than for the lifetime it once claimed. A machine whose advertisement
cannot be opened still shares printers, because being reachable by address is the product's job and
being discoverable is an addition; the failure is logged.

## Ports

A responder takes 5353 if it can, and 5354 otherwise, and a browser asks on both. 5353 is the
registered multicast DNS port, but on Windows the operating system's own multicast DNS responder
usually holds it, and on Linux `avahi-daemon` does. Competing for it would make discovery work on
some machines and not others. The trade is the same one ADR 0003 makes for the endpoint: a dedicated
port beats a port the operating system already owns. The cost is that a generic multicast DNS
browser will not find ShaPrint servers.

## Addresses

The client connects to the address the answer arrived *from*, with the port the advertisement names
in its `SRV` record. That address is reachable by construction, which matters more than resolving the
advertised `.local` host name: name resolution through the operating system's multicast DNS resolver
is optional on Windows and needs `nss-mdns` on Linux. The advertisement therefore carries no address
record at all, and the reader ignores record types it does not interpret rather than trusting them.
The address is also what identifies a nearby server: the list is keyed by it, so two servers that
advertise the same label are two candidates a user reviews separately, which matches how saved
approvals are keyed.

Discovery does not cross subnets. Manual address entry stays in the UI next to the discovered list,
and is the only path that works when the server is elsewhere.

## Who may talk to whom

A browsing client binds an ordinary socket and asks for the answer on it, so it needs no inbound
firewall rule: the answer comes back as the reply to a request this machine started. A server has to
accept a query on its discovery port, so the one setup action that already asks for administrator
permission (ADR 0003) also allows inbound UDP on the discovery ports. Nothing about discovery raises
a second prompt, and no discovery work runs elevated.

## Consequences

- Mature multicast DNS implementations probe for name conflicts, suppress known answers, and
  re-announce. This responder does none of those: it answers the queries it hears, which is enough
  for a browser that asks periodically. Nothing depends on the instance label being unique, because
  a client keys what it found by address.
- IPv4 only. IPv6, dual-stack, and a pinned interface are deferred with the rest of the Linux phase.
- A server that also runs a client sees its own advertisement and lists itself. That is accurate
  rather than harmful, and filtering it out would need a machine-identity comparison the MVP does
  not otherwise require.
- Discovery is not a trust decision, so it is not gated on the Network Channel either: a list of
  nearby server addresses is visible to anyone on the network, which is what the user asked for by
  sharing a printer.
- The advertisement is unsigned, which contradicts the blanket "discovery stays HMAC-SHA256" note
  that describes the legacy application's transport. The reasoning above is why the new path does
  not follow it.
