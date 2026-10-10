package io.shaprint.client.ui.navigation

sealed class Screen(val route: String, val title: String) {
    data object Discovery : Screen("discovery", "Nearby Servers")
    data object Servers : Screen("servers", "Trusted Servers")
    data object Printers : Screen("printers", "Shared Printers")
    data object PrintPreview : Screen("preview", "Print Document")
}
