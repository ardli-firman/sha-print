package io.shaprint.client.core.security

import android.content.Context
import android.content.SharedPreferences
import io.shaprint.client.core.domain.model.TrustedServer
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject

class EncryptedTrustStore(
    private val preferences: SharedPreferences,
    private val crypto: KeystoreCrypto = KeystoreCrypto()
) : TrustStore {

    constructor(context: Context, crypto: KeystoreCrypto = KeystoreCrypto()) : this(
        context.getSharedPreferences(PREFERENCES_NAME, Context.MODE_PRIVATE),
        crypto
    )

    override suspend fun getTrustedServers(): List<TrustedServer> = withContext(Dispatchers.IO) {
        val index = preferences.getStringSet(KEY_SERVER_INDEX, emptySet()) ?: emptySet()
        index.mapNotNull { canonicalAddress ->
            loadServer(canonicalAddress)
        }
    }

    override suspend fun findByCanonicalAddress(canonicalAddress: String): TrustedServer? = withContext(Dispatchers.IO) {
        loadServer(canonicalAddress)
    }

    override suspend fun saveTrustedServer(server: TrustedServer): Unit = withContext(Dispatchers.IO) {
        val json = JSONObject().apply {
            put("host", server.host)
            put("port", server.port)
            put("computerName", server.computerName)
            put("sha256Fingerprint", server.sha256Fingerprint)
            put("isOnline", server.isOnline)
            server.networkChannel?.let { channel ->
                put("encryptedChannel", crypto.encrypt(channel))
            }
        }

        val index = (preferences.getStringSet(KEY_SERVER_INDEX, emptySet()) ?: emptySet()).toMutableSet()
        index.add(server.canonicalAddress)

        preferences.edit()
            .putString(serverKey(server.canonicalAddress), json.toString())
            .putStringSet(KEY_SERVER_INDEX, index)
            .apply()
    }

    override suspend fun removeTrustedServer(canonicalAddress: String): Unit = withContext(Dispatchers.IO) {
        val index = (preferences.getStringSet(KEY_SERVER_INDEX, emptySet()) ?: emptySet()).toMutableSet()
        index.remove(canonicalAddress)

        preferences.edit()
            .remove(serverKey(canonicalAddress))
            .putStringSet(KEY_SERVER_INDEX, index)
            .apply()
    }

    private fun loadServer(canonicalAddress: String): TrustedServer? {
        val jsonStr = preferences.getString(serverKey(canonicalAddress), null) ?: return null
        return try {
            val json = JSONObject(jsonStr)
            val host = json.getString("host")
            val port = json.getInt("port")
            val computerName = json.getString("computerName")
            val fingerprint = json.getString("sha256Fingerprint")
            val isOnline = json.optBoolean("isOnline", true)
            val encryptedChannel = if (json.has("encryptedChannel") && !json.isNull("encryptedChannel")) {
                json.getString("encryptedChannel")
            } else {
                null
            }
            val decryptedChannel = encryptedChannel?.let { crypto.decrypt(it) }

            TrustedServer(
                host = host,
                port = port,
                computerName = computerName,
                sha256Fingerprint = fingerprint,
                networkChannel = decryptedChannel,
                isOnline = isOnline
            )
        } catch (_: Exception) {
            null
        }
    }

    private fun serverKey(canonicalAddress: String): String = "server_${canonicalAddress}"

    companion object {
        private const val PREFERENCES_NAME = "shaprint_trusted_servers"
        private const val KEY_SERVER_INDEX = "trusted_servers_index"
    }
}
