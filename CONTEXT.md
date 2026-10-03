# Print sharing

ShaPrint lets one computer share its printers so users on other computers can print to them. A computer can share its own printers and use printers shared by others.

## Language

**Server**:
A ShaPrint installation that shares a printer with clients. The same installation can also act as a client of another server.

**Client**:
A ShaPrint installation that sends print jobs to a server's shared printers.

**Shared printer**:
A printer made available by a server to ShaPrint clients.

**Print job**:
A document and its requested print settings submitted to a shared printer.

**Network Channel**:
A shared secret configured by ShaPrint installations to authorize print jobs between clients and servers.

**Nearby server**:
A server a client has seen advertise itself on the local network, as opposed to an address entered by
hand. A nearby server is a hint to review, never a trusted server.

**Client queue**:
A native Windows print queue installed on a client computer that routes print jobs through ShaPrint's
local client proxy to a server's shared printer.

**Loaded media**:
The physical paper size loaded in a shared printer's tray and configured on the server's printer queue.
_Avoid_: physical paper, server default form, tray paper

**Requested media**:
The page size a client specifies for a print job, which may be smaller than the shared printer's loaded
media.
_Avoid_: client paper size, document paper size


