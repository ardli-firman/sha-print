package io.shaprint.client.features.print

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder

/**
 * Android 15 (API 35) compliant Foreground Service (`shortService` / `dataSync`)
 * keeping the process alive while rasterizing and streaming multi-page PWG-raster jobs.
 */
class PrintJobForegroundService : Service() {

    override fun onCreate() {
        super.onCreate()
        ensureNotificationChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val jobName = intent?.getStringExtra(EXTRA_JOB_NAME) ?: "Document"
        val printerName = intent?.getStringExtra(EXTRA_PRINTER_NAME) ?: "Shared Printer"

        val notification = buildNotification(jobName, printerName)

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(
                NOTIFICATION_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC
            )
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }

        return START_NOT_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun ensureNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                CHANNEL_ID,
                "ShaPrint Print Jobs",
                NotificationManager.IMPORTANCE_LOW
            ).apply {
                description = "Active PWG-raster print job transmission to ShaPrint server"
            }
            val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            manager.createNotificationChannel(channel)
        }
    }

    private fun buildNotification(jobName: String, printerName: String): Notification {
        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(this, CHANNEL_ID)
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(this)
        }

        return builder
            .setContentTitle("Printing: $jobName")
            .setContentText("Streaming PWG-raster to $printerName...")
            .setSmallIcon(android.R.drawable.ic_menu_send)
            .setOngoing(true)
            .build()
    }

    companion object {
        const val CHANNEL_ID = "shaprint_print_jobs"
        const val NOTIFICATION_ID = 48631
        const val EXTRA_JOB_NAME = "extra_job_name"
        const val EXTRA_PRINTER_NAME = "extra_printer_name"
    }
}
