# ShaPrint Server Monitoring Service

This folder contains the implementation of the room-wide Server Monitoring dashboard.

## Components
1. **`MonitorService.cs`**: Periodic background status poller. Every 15 seconds, it queries active room servers using a staggered TCP request-response sequence. It uses `Constants.MonitorDiscoveryRequestMessage` for discovery to bypass visual notifications on room servers.
2. **`MonitorViewModel.cs`**: Encapsulates monitoring UI state, filtering, and sorting (putting offline/warning cards at the top).
3. **`MonitorPage.xaml` / `MonitorPage.xaml.cs`**: Premium dashboard UI styled with Wpf.Ui controls. Includes real-time indicators, cards list, and inline expanded tabs detailing printer queues, scanner busy states, active clients, and log history.
