# Reporting print failures

---
status: accepted
---

When a print job does not come out, the window explains why in one place: a stable code, a sentence
derived from that code and the queue, and the single action that resolves it. Both print paths report
into the same surface — the client proxy when it cannot get a job to the server, and the server when
it cannot submit an accepted job to a Windows queue.

## A record has no room for a secret

`PrintFailure` holds a path, a code, an optional queue name that already passed the same validation
the spooler and the IPP client apply, and a timestamp. The message and the recovery action are
derived from exactly those fields when the record is created — there is no constructor that takes
free text from the print path — so there is no field a Network Channel value, a document, or a
driver's raw bytes could travel in. That is stronger than redacting a free-form message: the property
is structural, and it keeps holding for every future caller.

Both the problem sentence and its recovery come from one table keyed by path and code, so a
condition cannot describe itself without also telling the user what to do about it.

## The server reports only what its user can act on

The IPPS endpoint is reachable by anyone who can reach the port. A rejected Network Channel is not
something the server user can fix, and it is exactly the event an anonymous caller could use to fill
that user's screen with notices. The server therefore reports only failures observed *after*
authorization succeeded: a queue submission the spooler refused, and a spooler that is not available
at all.

The client applies the mirror-image rule: a rejected `Print-Job` is reported, while the attribute
queries a driver makes while probing are not.

## Availability is shown before an attempt fails

A stopped client proxy never reaches this app at all: the installed queue's request is refused on the
loopback socket, so there is nothing for the app to observe. The window therefore also derives the
recovery action from live service state and shows it beside the Start button that fixes it.

## Consequences

- New stable error codes (`server-unavailable`, `server-untrusted`, `server-identity-changed`,
  `not-authorized`, `printer-not-shared`, `queue-unavailable`) name the print conditions a user can
  act on. The earlier generic codes remain for everything else.
- The failure a user sees is the latest one; dismissing it hides it, and it returns when the
  condition occurs again.
