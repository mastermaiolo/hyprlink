package com.example

import tech.kwik.core.QuicClientConnection
import tech.kwik.core.QuicStream
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runInterruptible
import java.security.MessageDigest

object ConnectionUtils {
    const val HYPRLINK_SERVICE_PORT = 7443

    suspend fun <T> readWithDeadline(stream: QuicStream, timeoutMs: Long = 10_000, block: () -> T): T {
        return try {
            withTimeout(timeoutMs) {
                runInterruptible(Dispatchers.IO) { block() }
            }
        } catch (e: kotlinx.coroutines.TimeoutCancellationException) {
            try { stream.abortReading(0) } catch (ignored: Exception) {}
            throw java.io.IOException("Stream read timed out after ${timeoutMs}ms", e)
        }
    }

    fun connectPinned(
        host: String,
        port: Int,
        applicationProtocol: String,
        expectedFingerprint: String,
        identity: DeviceIdentity
    ): QuicClientConnection {
        val quicConn = QuicClientConnection.newBuilder()
            .host(host)
            .port(port)
            .applicationProtocol(applicationProtocol)
            .noServerCertificateCheck()
            .maxOpenPeerInitiatedBidirectionalStreams(100)
            .maxOpenPeerInitiatedUnidirectionalStreams(100)
            .clientCertificate(identity.certificate)
            .clientCertificateKey(identity.privateKey)
            .build()

        quicConn.connect()

        if (quicConn.isConnected) {
            val chain = quicConn.serverCertificateChain
            if (chain.isNullOrEmpty()) {
                quicConn.close()
                throw SecurityException("Server certificate chain is empty")
            }
            val serverCert = chain[0]
            val md = MessageDigest.getInstance("SHA-256")
            val fingerprintBytes = md.digest(serverCert.encoded)
            val actual = fingerprintBytes.joinToString("") { "%02X".format(it) }

            val actualClean = actual.replace(":", "").uppercase()
            val expectedClean = expectedFingerprint.replace(":", "").uppercase()

            if (actualClean != expectedClean) {
                quicConn.close()
                throw SecurityException("Server fingerprint mismatch — connection closed")
            }
        }
        return quicConn
    }
}
