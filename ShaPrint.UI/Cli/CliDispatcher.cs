namespace ShaPrint.UI.Cli;

/// <summary>
/// CLI dispatcher for command-line verbs (e.g. `shaprint send ...`).
///
/// PLACEHOLDER: always returns false so Program.Main proceeds to the GUI shell.
/// Task 8 implements the `send` verb: parse --printer/--file/--host, resolve
/// IPrintRelayClient from the App host, call SendAsync, then exit WITHOUT launching GUI.
/// </summary>
public static class CliDispatcher
{
    public static bool TryHandle(string[] args) => false;
}