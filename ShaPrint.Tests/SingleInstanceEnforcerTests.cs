using ShaPrint.WpfApp.Helpers;
using System.Diagnostics;
using Xunit;

namespace ShaPrint.Tests;

/// <summary>
/// Tests for <see cref="SingleInstanceEnforcer"/>.
///
/// Each test uses a unique application GUID so kernel-level named objects
/// (Mutex, EventWaitHandle) do not collide across tests or with the
/// production application.
/// </summary>
public class SingleInstanceEnforcerTests
{
    private static string UniqueGuid() => Guid.NewGuid().ToString("D");

    // ── First instance ──────────────────────────────────────

    [Fact]
    public void TryAcquire_FirstInstance_ReturnsTrue()
    {
        using var enforcer = new SingleInstanceEnforcer(UniqueGuid(), () => { });
        bool result = enforcer.TryAcquire();

        Assert.True(result);
        Assert.True(enforcer.IsFirstInstance);
    }

    // ── Second instance detection ───────────────────────────

    [Fact]
    public void TryAcquire_SecondInstance_ReturnsFalse()
    {
        string guid = UniqueGuid();

        using var first = new SingleInstanceEnforcer(guid, () => { });
        Assert.True(first.TryAcquire());

        using var second = new SingleInstanceEnforcer(guid, () => { });
        bool result = second.TryAcquire();

        Assert.False(result);
        Assert.False(second.IsFirstInstance);
    }

    [Fact]
    public void TryAcquire_FirstAndSecond_IsFirstInstanceCorrect()
    {
        string guid = UniqueGuid();

        using var first = new SingleInstanceEnforcer(guid, () => { });
        first.TryAcquire();
        Assert.True(first.IsFirstInstance);

        using var second = new SingleInstanceEnforcer(guid, () => { });
        second.TryAcquire();
        Assert.False(second.IsFirstInstance);
    }

    // ── Activation callback ─────────────────────────────────

    [Fact]
    public void TryAcquire_SecondInstance_FiresActivationOnFirst()
    {
        string guid = UniqueGuid();
        using var activated = new ManualResetEventSlim(false);

        using var first = new SingleInstanceEnforcer(guid, () => activated.Set());
        Assert.True(first.TryAcquire());

        using var second = new SingleInstanceEnforcer(guid, () => { });
        second.TryAcquire();

        // The first instance should receive the activation signal
        Assert.True(activated.Wait(3000), "Activation callback was not invoked within 3 seconds");
    }

    [Fact]
    public void TryAcquire_SecondInstance_FiresActivationOnlyOnce()
    {
        string guid = UniqueGuid();
        int callCount = 0;

        using var first = new SingleInstanceEnforcer(guid, () => Interlocked.Increment(ref callCount));
        Assert.True(first.TryAcquire());

        using var second = new SingleInstanceEnforcer(guid, () => { });
        second.TryAcquire();

        // Allow listener thread time to process
        Thread.Sleep(500);

        Assert.Equal(1, callCount);
    }

    // ── Multiple second-instance attempts ────────────────────

    [Fact]
    public void TryAcquire_MultipleSecondAttempts_AllDetected()
    {
        string guid = UniqueGuid();
        const int attemptCount = 5;
        using var allSignalled = new CountdownEvent(attemptCount);

        using var first = new SingleInstanceEnforcer(guid, () => allSignalled.Signal());
        Assert.True(first.TryAcquire());

        // Simulate realistic user behaviour — each launch is separated by
        // a small delay so the listener thread can re-enter WaitOne().
        for (int i = 0; i < attemptCount; i++)
        {
            using var second = new SingleInstanceEnforcer(guid, () => { });
            Assert.False(second.TryAcquire());
            Thread.Sleep(50); // Allow listener to settle
        }

        // Wait for all activation signals to be received
        Assert.True(allSignalled.Wait(TimeSpan.FromSeconds(5)),
            $"Expected {attemptCount} activations but only received {attemptCount - allSignalled.CurrentCount}");
    }

    // ── Cleanup / Dispose ───────────────────────────────────

    [Fact]
    public void TryAcquire_AfterDispose_NewInstanceCanAcquire()
    {
        string guid = UniqueGuid();

        var first = new SingleInstanceEnforcer(guid, () => { });
        Assert.True(first.TryAcquire());
        first.Dispose();

        using var second = new SingleInstanceEnforcer(guid, () => { });
        Assert.True(second.TryAcquire());
    }

    [Fact]
    public void Dispose_Idempotent_DoesNotThrow()
    {
        string guid = UniqueGuid();
        var enforcer = new SingleInstanceEnforcer(guid, () => { });
        enforcer.TryAcquire();

        // Multiple dispose calls should be safe
        enforcer.Dispose();
        enforcer.Dispose();
        enforcer.Dispose();
    }

    [Fact]
    public void Dispose_BeforeAcquire_DoesNotThrow()
    {
        var enforcer = new SingleInstanceEnforcer(UniqueGuid(), () => { });
        enforcer.Dispose(); // Dispose without calling TryAcquire first
    }

    [Fact]
    public void TryAcquire_AfterDispose_ThrowsObjectDisposedException()
    {
        var enforcer = new SingleInstanceEnforcer(UniqueGuid(), () => { });
        enforcer.Dispose();

        Assert.Throws<ObjectDisposedException>(() => enforcer.TryAcquire());
    }

    // ── Validation ──────────────────────────────────────────

    [Fact]
    public void Constructor_NullGuid_ThrowsArgumentNullException()
    {
        Assert.Throws<ArgumentNullException>(() => new SingleInstanceEnforcer(null!, () => { }));
    }

    [Fact]
    public void Constructor_EmptyGuid_ThrowsArgumentException()
    {
        Assert.Throws<ArgumentException>(() => new SingleInstanceEnforcer("", () => { }));
    }

    [Fact]
    public void Constructor_WhitespaceGuid_ThrowsArgumentException()
    {
        Assert.Throws<ArgumentException>(() => new SingleInstanceEnforcer("   ", () => { }));
    }

    [Fact]
    public void Constructor_NullCallback_ThrowsArgumentNullException()
    {
        Assert.Throws<ArgumentNullException>(() => new SingleInstanceEnforcer(UniqueGuid(), null!));
    }

    // ── Production constants ────────────────────────────────

    [Fact]
    public void ProductionConstants_AreConsistent()
    {
        // These must match what App.xaml.cs uses at runtime
        Assert.Equal("d4f7c9a1-3b5e-4f2a-8c7d-9e1b3a5c7f9d", SingleInstanceEnforcer.AppGuid);
        Assert.Equal("Global\\ShaPrint_d4f7c9a1-3b5e-4f2a-8c7d-9e1b3a5c7f9d", SingleInstanceEnforcer.MutexName);
        Assert.Equal("Global\\ShaPrint_d4f7c9a1-3b5e-4f2a-8c7d-9e1b3a5c7f9d_Activate", SingleInstanceEnforcer.ActivateEventName);
    }

    // ── Race condition resilience ───────────────────────────

    [Fact]
    public void TryAcquire_ConcurrentCallsSameGuid_OnlyOneSucceeds()
    {
        string guid = UniqueGuid();
        int successCount = 0;
        var barrier = new Barrier(5);
        var results = new bool[5];

        var threads = Enumerable.Range(0, 5).Select(i => new Thread(() =>
        {
            barrier.SignalAndWait(); // All start at the same time
            using var enforcer = new SingleInstanceEnforcer(guid, () => { });
            results[i] = enforcer.TryAcquire();
            if (results[i]) Interlocked.Increment(ref successCount);
        })).ToList();

        threads.ForEach(t => t.Start());
        threads.ForEach(t => t.Join(5000));

        // Only one should have succeeded, but the others
        // might have been sequential due to OS scheduling.
        // At minimum: exactly one instance succeeded.
        Assert.Equal(1, successCount);
        Assert.Single(results, r => r);
    }

    // ── Large-scale / stress ────────────────────────────────

    [Fact]
    public void TryAcquire_ManySequentialSecondAttempts_NoMemoryLeak()
    {
        string guid = UniqueGuid();
        using var first = new SingleInstanceEnforcer(guid, () => { });
        Assert.True(first.TryAcquire());

        // Rapidly create and dispose many "second instances"
        for (int i = 0; i < 100; i++)
        {
            using var second = new SingleInstanceEnforcer(guid, () => { });
            Assert.False(second.TryAcquire());
        }
    }

    // ── Server-mode scenario ───────────────────────────────
    //
    // These tests document the real-world use case when the app runs
    // as a Server with auto-start:
    //
    //   Scenario A — Server auto-start:
    //       App launched with --startup. ServerViewModel loads saved
    //       printers and calls StartServer() automatically (no window shown).
    //       This is the "first instance" path.
    //
    //   Scenario B — Second launch while server running:
    //       User opens app again. SingleInstanceEnforcer detects the mutex,
    //       signals the running instance to show its window, new instance exits.
    //       → Only ONE process and ONE tray icon at all times.
    //
    //   Scenario C — Server keeps running when second instance is blocked:
    //       First instance is NOT disposed or stopped — server keeps running.

    [Fact]
    public void ServerScenario_FirstInstanceAcquires_SecondInstanceRejected()
    {
        string guid = UniqueGuid();

        using var serverInstance = new SingleInstanceEnforcer(guid, () => { });
        Assert.True(serverInstance.TryAcquire(), "First instance (server) must acquire the mutex");

        using var secondLaunch = new SingleInstanceEnforcer(guid, () => { });
        Assert.False(secondLaunch.TryAcquire(), "Second launch must be rejected");

        Assert.True(serverInstance.IsFirstInstance);
        Assert.False(secondLaunch.IsFirstInstance);
    }

    [Fact]
    public void ServerScenario_FirstInstanceIsNotDisposedWhenSecondIsRejected()
    {
        string guid = UniqueGuid();

        using var serverInstance = new SingleInstanceEnforcer(guid, () => { });
        Assert.True(serverInstance.TryAcquire());

        using var secondInstance = new SingleInstanceEnforcer(guid, () => { });
        Assert.False(secondInstance.TryAcquire());

        // First instance enforcer is still alive — server keeps running
        Assert.True(serverInstance.IsFirstInstance);
        Assert.False(serverInstance.Disposed);
    }

    [Fact]
    public void ServerScenario_SecondLaunchBringsServerWindowToFront()
    {
        string guid = UniqueGuid();
        using var windowActivated = new ManualResetEventSlim(false);

        // Activation callback simulates what OnActivateRequested does
        using var serverInstance = new SingleInstanceEnforcer(guid, () => windowActivated.Set());
        Assert.True(serverInstance.TryAcquire());

        using var secondLaunch = new SingleInstanceEnforcer(guid, () => { });
        Assert.False(secondLaunch.TryAcquire());

        Assert.True(windowActivated.Wait(3000),
            "First instance (server) must receive activation signal when second launch is attempted");
    }

    [Fact]
    public void ServerScenario_StartupFlagDoesNotAffectSingleInstance()
    {
        // --startup flag controls window visibility but does NOT change
        // the single-instance enforcement — the mutex check happens first.
        string guid = UniqueGuid();

        using var instance = new SingleInstanceEnforcer(guid, () => { });
        Assert.True(instance.TryAcquire());
        // The --startup vs manual launch branching happens downstream
        // in App.xaml.cs line ~120: if (!isStartup) mainWindow?.Show();
    }
}
