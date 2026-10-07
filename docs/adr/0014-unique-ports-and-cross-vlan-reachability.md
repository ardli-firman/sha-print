# Dedicated ports, stale-process port reclamation, and cross-VLAN server reachability

---
status: accepted
---

ShaPrint binds fixed, dedicated high ports so its runtime services do not collide with Windows system responders or common local developer servers: TCP `48631` for the server IPPS sharing endpoint, TCP `48632` for the loopback client proxy (`127.0.0.1:48632`), and UDP `48633` for multicast DNS discovery (`_shaprint-ipps._tcp.local.`). Keeping the ports fixed preserves firewall rules, manual cross-VLAN server addresses, and installed Windows client queues across restarts, while re-installing an existing ShaPrint client queue updates its loopback port in place when it points to an older ShaPrint proxy port.

Only one desktop instance may own these ports at a time. On shutdown or service stop, ShaPrint closes its listeners before exiting. If a port is still in use when a service starts, ShaPrint inspects the owning process ID from the operating system's socket table: when the holder is a stale `shaprint` process, ShaPrint terminates that process and waits for the operating system to release the port before binding; when the holder is any other program, ShaPrint never terminates it and instead reports the conflicting process name and PID.

To keep servers reachable across multi-adapter Windows machines and across VLANs, multicast DNS binds and queries every active non-loopback IPv4 interface with a multicast hop limit above link-local (`TTL = 32`) so networks with multicast routing or mDNS reflection can discover nearby servers across subnets. Because routers may block multicast entirely between VLANs, the client retains every trusted server in its persistent trust store, surfaces trusted servers alongside nearby servers with live unicast status checks and an explicit forget action, and accepts Windows IPP spooler requests over both `ipp://` and `http://` loopback URIs (including multi-step job operations and large PWG-raster transfers) when forwarding print jobs to the server.
