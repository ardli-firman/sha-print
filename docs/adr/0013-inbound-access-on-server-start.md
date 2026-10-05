# Ensure inbound Client access when Server Sharing starts

---
status: accepted
---

Server Sharing must not report itself ready while Clients cannot reach its IPPS endpoint or discovery responder through Windows Firewall. The Start action checks effective inbound firewall policy for TCP 8631 and UDP 5353/5354. Complete, unrestricted allowances on each enabled firewall profile require no UAC prompt even if an administrator named the rules differently. Restricted address, program, service, or interface scopes and conflicting block rules do not count as available Client access. Otherwise ShaPrint requests administrator approval using the existing narrowly scoped elevated helper, installs the rules, verifies them again, and only then opens Server Sharing. The window no longer offers a separate firewall button.

The check runs outside the service startup deadline because the user may need time to decide at the Windows prompt. The server remains stopped if permission is denied, a check fails, or the rules remain unavailable after setup; Start can be used to retry. Concurrent starts cannot trigger multiple prompts. The check is tied to the server runtime, including restored sharing after login, rather than only to one UI route. Discovery remains unsigned because Client identity is pinned at TLS (ADR 0004).

The rules are read without elevation from the effective Windows policy and the helper changes machine-wide state only on demand. A managed firewall policy that prevents adding the rules is reported as unavailable; ShaPrint cannot promise reachability through third-party firewalls or network isolation. Print jobs still require the configured Network Channel (ADR 0003); firewall access alone never authorizes them.
