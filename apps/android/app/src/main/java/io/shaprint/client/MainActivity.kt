package io.shaprint.client

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import io.shaprint.client.ui.navigation.ShaPrintNavHost
import io.shaprint.client.ui.theme.ShaPrintTheme

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            ShaPrintTheme {
                ShaPrintNavHost()
            }
        }
    }
}
