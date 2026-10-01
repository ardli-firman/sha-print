//! Client-side discovery: the servers visible on the local network, and the seams that feed it.
//!
//! Discovery is split the way the rest of the shell is: the use case here owns the cache the shell
//! publishes, and the adapters own the wire. A server advertises through [`ServerAdvertiser`] while
//! it shares, and a client browses through [`DiscoveryBrowser`], so the cache can be exercised
//! without a network and the protocol can be exercised without the cache.
//!
//! Nothing here trusts an advertisement. It only records what the network said so a user can decide
//! whether to review that server (ADR 0004).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::watch;

use crate::application::{RuntimeService, ServiceContext, Shutdown};
use crate::domain::{AppError, NearbyServer, PrinterName, ServiceId};

/// How often the cache drops advertisements whose lifetime ran out.
const DEFAULT_SWEEP_INTERVAL: Duration = Duration::from_secs(1);

/// The server-side seam: tells the local network which queues this server shares.
#[async_trait]
pub trait ServerAdvertiser: Send + Sync + 'static {
    /// Advertises `queues`, reachable on the endpoint's `port`.
    ///
    /// Fails when this machine cannot advertise at all — for example because no discovery socket
    /// can be opened — so the caller can decide whether that is fatal to sharing.
    async fn advertise(
        &self,
        port: u16,
        queues: &[PrinterName],
    ) -> Result<Arc<dyn Advertisement>, AppError>;
}

/// One live advertisement this server keeps up to date.
#[async_trait]
pub trait Advertisement: Send + Sync {
    /// Replaces the advertised queues; an empty list withdraws the advertisement.
    async fn replace(&self, queues: &[PrinterName]) -> Result<(), AppError>;

    /// Announces that this server stopped sharing and stops answering queries.
    async fn withdraw(&self) -> Result<(), AppError>;
}

/// The client-side seam: hears what the local network advertises.
#[async_trait]
pub trait DiscoveryBrowser: Send + Sync + 'static {
    /// Opens the browse session.
    ///
    /// Fails when this machine cannot browse at all, which is what makes discovery report `failed`
    /// instead of silently finding nothing.
    async fn open(&self) -> Result<Box<dyn Browse>, AppError>;
}

/// One open browse session.
#[async_trait]
pub trait Browse: Send + Sync {
    /// Reports every advertisement it hears to `sink` until the shell cancels it.
    async fn run(
        &self,
        sink: Arc<dyn AdvertisementSink>,
        shutdown: Shutdown,
    ) -> Result<(), AppError>;
}

/// Where a browse session reports what it hears.
pub trait AdvertisementSink: Send + Sync + 'static {
    /// One advertisement, valid for `lifetime`. A zero lifetime withdraws it.
    fn advertised(&self, server: NearbyServer, lifetime: Duration);

    /// One advertisement that announced it is going away.
    fn withdrawn(&self, instance: &str);
}

/// One advertisement and the moment it stops being valid.
#[derive(Debug)]
struct Entry {
    server: NearbyServer,
    expires: Instant,
}

/// The servers currently visible on the local network.
///
/// The cache is what the UI reads and what the shell publishes, so it is the one place that decides
/// when an advertisement is gone: at its own goodbye, or when its lifetime runs out.
pub struct Discovery {
    entries: Mutex<BTreeMap<String, Entry>>,
    changes: watch::Sender<Vec<NearbyServer>>,
    sweep_interval: Duration,
}

impl Default for Discovery {
    fn default() -> Self {
        Self::new()
    }
}

impl Discovery {
    pub fn new() -> Self {
        let (changes, _) = watch::channel(Vec::new());
        Self {
            entries: Mutex::new(BTreeMap::new()),
            changes,
            sweep_interval: DEFAULT_SWEEP_INTERVAL,
        }
    }

    /// Overrides how often expired advertisements are checked for. Used by tests.
    #[must_use]
    pub fn with_sweep_interval(mut self, interval: Duration) -> Self {
        self.sweep_interval = interval;
        self
    }

    /// The visible servers, ordered by label so the list does not jump between reads.
    pub fn nearby_servers(&self) -> Vec<NearbyServer> {
        match self.lock() {
            Ok(mut entries) => {
                expire(&mut entries, Instant::now());
                snapshot(&entries)
            }
            Err(()) => Vec::new(),
        }
    }

    /// Follows the visible servers; the receiver also holds the current list.
    pub fn subscribe(&self) -> watch::Receiver<Vec<NearbyServer>> {
        self.changes.subscribe()
    }

    /// Publishes expiries until the shell cancels the sweep.
    ///
    /// Without this the UI would keep showing a server whose advertisement ran out until something
    /// else happened to read the cache.
    pub async fn sweep(&self, mut shutdown: Shutdown) -> Result<(), AppError> {
        let mut ticker = tokio::time::interval(self.sweep_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                () = shutdown.cancelled() => return Ok(()),
                _ = ticker.tick() => self.prune(),
            }
        }
    }

    /// Drops what expired and tells the UI when that changed anything.
    fn prune(&self) {
        let servers = match self.lock() {
            Ok(mut entries) => {
                expire(&mut entries, Instant::now());
                snapshot(&entries)
            }
            Err(()) => return,
        };
        self.publish(servers);
    }

    fn lock(&self) -> Result<MutexGuard<'_, BTreeMap<String, Entry>>, ()> {
        match self.entries.lock() {
            Ok(entries) => Ok(entries),
            Err(_) => {
                log::warn!("discovery cache lock is poisoned");
                Err(())
            }
        }
    }

    /// Publishes `servers` unless the UI already shows exactly this list.
    fn publish(&self, servers: Vec<NearbyServer>) {
        if *self.changes.borrow() == servers {
            return;
        }
        self.changes.send_replace(servers);
    }
}

impl AdvertisementSink for Discovery {
    fn advertised(&self, server: NearbyServer, lifetime: Duration) {
        // A goodbye is an advertisement with no lifetime left (RFC 6762 §10.1).
        if lifetime.is_zero() {
            self.withdrawn(server.instance());
            return;
        }
        let servers = match self.lock() {
            Ok(mut entries) => {
                entries.insert(
                    server.instance().to_owned(),
                    Entry {
                        server,
                        expires: Instant::now() + lifetime,
                    },
                );
                snapshot(&entries)
            }
            Err(()) => return,
        };
        self.publish(servers);
    }

    fn withdrawn(&self, instance: &str) {
        let servers = match self.lock() {
            Ok(mut entries) => {
                entries.remove(instance);
                snapshot(&entries)
            }
            Err(()) => return,
        };
        self.publish(servers);
    }
}

/// Drops every entry whose lifetime ran out.
fn expire(entries: &mut BTreeMap<String, Entry>, now: Instant) {
    entries.retain(|_, entry| entry.expires > now);
}

/// The visible servers, ordered by label and then by address.
fn snapshot(entries: &BTreeMap<String, Entry>) -> Vec<NearbyServer> {
    let mut servers: Vec<NearbyServer> =
        entries.values().map(|entry| entry.server.clone()).collect();
    servers.sort_by(|left, right| {
        left.name()
            .cmp(right.name())
            .then_with(|| left.address().cmp(right.address()))
    });
    servers
}

/// Supervises browsing for nearby servers.
///
/// The client always browses: a user has to see nearby servers without starting anything, and an
/// installation that never acts as a client still pays nothing for it.
pub struct DiscoveryService {
    discovery: Arc<Discovery>,
    browser: Arc<dyn DiscoveryBrowser>,
}

impl DiscoveryService {
    pub fn new(discovery: Arc<Discovery>, browser: Arc<dyn DiscoveryBrowser>) -> Self {
        Self { discovery, browser }
    }
}

#[async_trait]
impl RuntimeService for DiscoveryService {
    fn id(&self) -> ServiceId {
        ServiceId::ServerDiscovery
    }

    fn autostart(&self) -> bool {
        true
    }

    async fn run(&self, context: ServiceContext) -> Result<(), AppError> {
        // Opening the socket first means a machine that cannot browse is reported as `failed`
        // rather than quietly showing an empty list forever.
        let browse = self.browser.open().await?;
        context.reporter().ready()?;

        let sink: Arc<dyn AdvertisementSink> = self.discovery.clone();
        let listening = browse.run(sink, context.shutdown());
        let sweeping = self.discovery.sweep(context.shutdown());
        tokio::pin!(listening, sweeping);

        tokio::select! {
            result = &mut listening => result,
            result = &mut sweeping => result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::PrinterName;

    fn server(label: &str, address: &str, printers: &[&str]) -> NearbyServer {
        NearbyServer::new(
            &format!("{label}._shaprint-ipps._tcp.local."),
            label,
            address,
            printers
                .iter()
                .map(|name| PrinterName::parse(name).expect("valid printer name"))
                .collect(),
        )
        .expect("a valid nearby server")
    }

    fn labels(servers: &[NearbyServer]) -> Vec<&str> {
        servers.iter().map(NearbyServer::name).collect()
    }

    fn minute() -> Duration {
        Duration::from_secs(60)
    }

    #[test]
    fn an_advertisement_appears_for_the_user_to_review() {
        let discovery = Discovery::new();
        let expected = server("DESKTOP-ABC", "192.0.2.10:8631", &["Zebra"]);

        discovery.advertised(expected.clone(), minute());

        assert_eq!(discovery.nearby_servers(), vec![expected]);
    }

    #[test]
    fn a_withdrawn_advertisement_disappears_immediately() {
        let discovery = Discovery::new();
        let advertised = server("DESKTOP-ABC", "192.0.2.10:8631", &["Zebra"]);
        discovery.advertised(advertised.clone(), minute());
        assert_eq!(discovery.nearby_servers().len(), 1);

        discovery.withdrawn(advertised.instance());

        assert!(discovery.nearby_servers().is_empty());
    }

    #[test]
    fn a_goodbye_is_a_withdrawal() {
        let discovery = Discovery::new();
        let advertised = server("DESKTOP-ABC", "192.0.2.10:8631", &["Zebra"]);
        discovery.advertised(advertised.clone(), minute());

        discovery.advertised(advertised.clone(), Duration::ZERO);

        assert!(discovery.nearby_servers().is_empty());
    }

    #[test]
    fn an_advertisement_that_outlives_its_lifetime_stops_being_offered() {
        let discovery = Discovery::new();
        discovery.advertised(
            server("DESKTOP-ABC", "192.0.2.10:8631", &["Zebra"]),
            Duration::from_millis(30),
        );
        assert_eq!(discovery.nearby_servers().len(), 1);

        std::thread::sleep(Duration::from_millis(120));

        assert!(discovery.nearby_servers().is_empty());
    }

    #[test]
    fn a_repeated_advertisement_replaces_what_the_server_told_us_before() {
        let discovery = Discovery::new();
        discovery.advertised(
            server("DESKTOP-ABC", "192.0.2.10:8631", &["Zebra"]),
            minute(),
        );

        discovery.advertised(
            server("DESKTOP-ABC", "192.0.2.11:8631", &["Zebra", "Canon"]),
            minute(),
        );

        let servers = discovery.nearby_servers();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].address(), "192.0.2.11:8631");
        assert_eq!(
            servers[0]
                .printers()
                .iter()
                .map(PrinterName::as_str)
                .collect::<Vec<_>>(),
            vec!["Zebra", "Canon"]
        );
    }

    #[test]
    fn servers_are_ordered_by_label_so_the_list_does_not_jump() {
        let discovery = Discovery::new();
        discovery.advertised(server("ZULU", "192.0.2.20:8631", &["Zebra"]), minute());
        discovery.advertised(server("ALPHA", "192.0.2.10:8631", &["Zebra"]), minute());

        assert_eq!(labels(&discovery.nearby_servers()), vec!["ALPHA", "ZULU"]);
    }

    #[tokio::test]
    async fn the_ui_is_told_when_the_visible_servers_change() {
        let discovery = Discovery::new();
        let mut changes = discovery.subscribe();

        discovery.advertised(
            server("DESKTOP-ABC", "192.0.2.10:8631", &["Zebra"]),
            minute(),
        );
        changes.changed().await.expect("publishes the new server");
        assert_eq!(labels(&changes.borrow_and_update()), vec!["DESKTOP-ABC"]);

        // Re-advertising what the UI already shows is not a change worth a repaint.
        discovery.advertised(
            server("DESKTOP-ABC", "192.0.2.10:8631", &["Zebra"]),
            minute(),
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(50), changes.changed())
                .await
                .is_err()
        );

        discovery.withdrawn("DESKTOP-ABC._shaprint-ipps._tcp.local.");
        changes.changed().await.expect("publishes the removal");
        assert!(changes.borrow_and_update().is_empty());
    }

    #[tokio::test]
    async fn the_sweeper_publishes_an_expiry_without_anyone_asking() {
        let discovery = Arc::new(Discovery::new());
        let mut changes = discovery.subscribe();
        discovery.advertised(
            server("DESKTOP-ABC", "192.0.2.10:8631", &["Zebra"]),
            Duration::from_millis(20),
        );

        let (cancel, shutdown) = Shutdown::channel();
        let sweeping = tokio::spawn({
            let discovery = Arc::clone(&discovery);
            async move { discovery.sweep(shutdown).await }
        });

        let expired = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                changes.changed().await.expect("publishes every change");
                if changes.borrow_and_update().is_empty() {
                    return;
                }
            }
        })
        .await;

        let _ = cancel.send_replace(true);
        sweeping.await.expect("the sweeper joins").expect("sweeps");

        expired.expect("the expired server is published as gone");
    }
}
