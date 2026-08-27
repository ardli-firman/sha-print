using System.Diagnostics.CodeAnalysis;
using Microsoft.Extensions.DependencyInjection;

namespace ShaPrint.UI.Cli;

/// <summary>
/// CLI dispatcher for command-line verbs (e.g. `shaprint send ...`).
///
/// Contract (Task 8):
/// <code>
/// shaprint send --printer &lt;name&gt; --file &lt;path&gt; [--host &lt;ip&gt;]
/// </code>
///
/// Parse rules:
/// <list type="bullet">
///   <item>The verb is matched case-insensitively; any other first argument returns
///   <c>false</c> so <see cref="Program.Main"/> proceeds to the GUI shell (there is no
///   dedicated <c>--help</c> verb — unknown arguments after <c>send</c> are usage errors).</item>
///   <item><c>--printer</c> and <c>--file</c> are required; <c>--host</c> is optional.
///   Both space-separated (<c>--printer X</c>) and <c>=</c>-separated (<c>--printer=X</c>)
///   forms are accepted; option names themselves are case-sensitive.</item>
///   <item>A value that is missing or starts with <c>-</c> is a usage error (getopt-style).</item>
/// </list>
///
/// Exit codes:
/// <list type="bullet">
///   <item><c>0</c> — job relayed successfully.</item>
///   <item><c>1</c> — argument/usage error or file IO error (missing --printer/--file,
///   unreadable file, unknown option).</item>
///   <item><c>2</c> — send attempt failed (connection / discovery / authentication).</item>
/// </list>
///
/// Whenever this method returns <c>true</c> the process must exit with
/// <see cref="Environment.ExitCode"/> — the GUI is never started on the CLI path.
/// </summary>
public static class CliDispatcher
{
    /// <summary>Exit code: job relayed successfully.</summary>
    public const int ExitSuccess = 0;

    /// <summary>Exit code: argument/usage or file IO error (not a send attempt).</summary>
    public const int ExitParseOrIoError = 1;

    /// <summary>Exit code: send attempt failed (connection / discovery / authentication).</summary>
    public const int ExitSendFailed = 2;

    /// <summary>
    /// Handles a CLI verb. Returns <c>true</c> when the process should terminate right after
    /// (without starting Avalonia), <c>false</c> when the GUI shell should launch instead.
    /// </summary>
    public static bool TryHandle(string[] args)
    {
        if (args == null || args.Length == 0)
        {
            return false;
        }

        // Only the `send` verb is handled by the CLI; anything else falls through to the GUI.
        if (!string.Equals(args[0], "send", StringComparison.OrdinalIgnoreCase))
        {
            return false;
        }

        if (!TryParseSendArgs(args, out var options, out var error))
        {
            Console.Error.WriteLine($"shaprint send: {error}");
            Console.Error.WriteLine("Usage: shaprint send --printer <name> --file <path> [--host <ip>]");
            Environment.ExitCode = ExitParseOrIoError;
            return true;
        }

        // CLI host: a lightweight ServiceProvider with the same platform switch the GUI host
        // uses (Program.ConfigureServices). Unlike App.Host (built in
        // App.OnFrameworkInitializationCompleted), this container is never started, so no
        // hosted services run (the UpdateService auto-check stays dormant) and the Avalonia
        // lifetime is never touched. The dispatch blocks on the async send — the CLI runs on
        // the STA thread with no SynchronizationContext, so GetAwaiter().GetResult() cannot
        // deadlock.
        using var services = BuildCliServiceProvider();
        Environment.ExitCode = SendCommand.ExecuteAsync(services, options).GetAwaiter().GetResult();
        return true;
    }

    /// <summary>
    /// Builds the minimal CLI service provider: the platform runtime switch from
    /// <see cref="Program.ConfigureServices"/> plus nothing else. SendCommand falls back to a
    /// self-sufficient relay when the platform switch did not register one (e.g. the plain
    /// net8.0 build running on Windows, whose <c>AddPlatformWindows</c> body is compiled
    /// empty — see PlatformServiceCollectionExtensions).
    /// </summary>
    private static ServiceProvider BuildCliServiceProvider()
    {
        var services = new ServiceCollection();
        Program.ConfigureServices(services);
        return services.BuildServiceProvider();
    }

    /// <summary>
    /// Parses <c>send</c> arguments (args[0] is already known to be "send"). Both
    /// <c>--name value</c> and <c>--name=value</c> forms are accepted.
    /// </summary>
    private static bool TryParseSendArgs(string[] args, [NotNullWhen(true)] out SendOptions? options, [NotNullWhen(false)] out string? error)
    {
        options = null;
        error = null;

        string? printer = null;
        string? file = null;
        string? host = null;

        for (int i = 1; i < args.Length; i++)
        {
            string token = args[i];

            // Split "--name=value" into name + inline value; "--name value" has none.
            string name = token;
            string? inlineValue = null;
            int eq = token.IndexOf('=');
            if (eq > 0)
            {
                name = token[..eq];
                inlineValue = token[(eq + 1)..];
            }

            string? value = inlineValue;
            switch (name)
            {
                case "--printer":
                case "--file":
                case "--host":
                    if (value == null)
                    {
                        // getopt-style: an absent or option-like next token means no value.
                        if (i + 1 >= args.Length || args[i + 1].StartsWith('-'))
                        {
                            error = $"missing value for {name}.";
                            return false;
                        }
                        value = args[++i];
                    }

                    if (string.IsNullOrWhiteSpace(value))
                    {
                        error = $"empty value for {name}.";
                        return false;
                    }

                    switch (name)
                    {
                        case "--printer": printer = value; break;
                        case "--file": file = value; break;
                        case "--host": host = value; break;
                    }
                    break;

                default:
                    error = $"unknown argument '{token}'.";
                    return false;
            }
        }

        if (string.IsNullOrWhiteSpace(printer))
        {
            error = "missing required --printer <name>.";
            return false;
        }
        if (string.IsNullOrWhiteSpace(file))
        {
            error = "missing required --file <path>.";
            return false;
        }

        options = new SendOptions(printer, file, host);
        return true;
    }
}
