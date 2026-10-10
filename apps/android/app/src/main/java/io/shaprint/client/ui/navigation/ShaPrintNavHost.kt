package io.shaprint.client.ui.navigation

import androidx.compose.runtime.Composable
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import io.shaprint.client.features.discovery.DiscoveryScreen
import io.shaprint.client.features.print.PrintPreviewScreen
import io.shaprint.client.features.printers.PrintersScreen
import io.shaprint.client.features.servers.ServersScreen

@Composable
fun ShaPrintNavHost(
    navController: NavHostController = rememberNavController()
) {
    NavHost(
        navController = navController,
        startDestination = Screen.Discovery.route
    ) {
        composable(Screen.Discovery.route) {
            DiscoveryScreen(
                onNavigateToServers = { navController.navigate(Screen.Servers.route) },
                onNavigateToPrinters = { navController.navigate(Screen.Printers.route) },
                onServerClick = { navController.navigate(Screen.Printers.route) }
            )
        }
        composable(Screen.Servers.route) {
            ServersScreen(
                onNavigateBack = { navController.popBackStack() },
                onNavigateToPrinters = { navController.navigate(Screen.Printers.route) }
            )
        }
        composable(Screen.Printers.route) {
            PrintersScreen(
                onNavigateBack = { navController.popBackStack() },
                onSelectPrinter = { navController.navigate(Screen.PrintPreview.route) }
            )
        }
        composable(Screen.PrintPreview.route) {
            PrintPreviewScreen(
                onNavigateBack = { navController.popBackStack() }
            )
        }
    }
}
