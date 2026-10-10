package io.shaprint.client.core.network.mdns

import android.content.Context
import android.net.wifi.WifiManager
import io.shaprint.client.core.domain.model.NearbyServer
import io.shaprint.client.core.network.Ports
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.flowOn
import kotlinx.coroutines.isActive
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.net.SocketTimeoutException
import kotlin.coroutines.coroutineContext

interface MdnsScanner {
    fun scan(timeoutMs: Long = 10000L): Flow<NearbyServer>
}

class DefaultMdnsScanner(private val context: Context) : MdnsScanner {

    override fun scan(timeoutMs: Long): Flow<NearbyServer> = flow {
        val wifiManager = context.applicationContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager
        val multicastLock = wifiManager?.createMulticastLock("ShaPrintMdnsLock")?.apply {
            setReferenceCounted(true)
        }

        var socket: DatagramSocket? = null
        try {
            multicastLock?.acquire()

            socket = DatagramSocket()
            socket.soTimeout = 1000
            socket.broadcast = true

            val queryBytes = DnsPacket.buildQuery(Ports.MDNS_SERVICE_TYPE)
            val mcastAddress = InetAddress.getByName("224.0.0.251")
            val queryPacket = DatagramPacket(queryBytes, queryBytes.size, mcastAddress, Ports.DISCOVERY_DEFAULT_PORT)

            // Send query
            socket.send(queryPacket)

            val startTime = System.currentTimeMillis()
            val buffer = ByteArray(2048)
            val seenInstances = mutableSetOf<String>()

            while (coroutineContext.isActive && (System.currentTimeMillis() - startTime) < timeoutMs) {
                val responsePacket = DatagramPacket(buffer, buffer.size)
                try {
                    socket.receive(responsePacket)
                    val discovered = DnsPacket.parseResponse(
                        responsePacket.data,
                        responsePacket.length,
                        responsePacket.address
                    )

                    if (discovered != null && !discovered.isGoodbye) {
                        val key = "${discovered.host}:${discovered.port}"
                        if (seenInstances.add(key)) {
                            emit(
                                NearbyServer(
                                    host = discovered.host,
                                    port = discovered.port,
                                    computerName = discovered.computerName,
                                    advertisedQueues = discovered.advertisedQueues,
                                    isOnline = true
                                )
                            )
                        }
                    }
                } catch (e: SocketTimeoutException) {
                    // Loop until overall timeoutMs expires
                } catch (e: Exception) {
                    break
                }
            }
        } finally {
            try {
                socket?.close()
            } catch (_: Exception) {}

            try {
                if (multicastLock?.isHeld == true) {
                    multicastLock.release()
                }
            } catch (_: Exception) {}
        }
    }.flowOn(Dispatchers.IO)
}
