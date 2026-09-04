package com.example

import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.os.Build
import androidx.core.app.NotificationCompat
import co.nstant.`in`.cbor.CborBuilder
import co.nstant.`in`.cbor.CborDecoder
import co.nstant.`in`.cbor.CborEncoder
import co.nstant.`in`.cbor.model.*
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import tech.kwik.core.QuicClientConnection
import tech.kwik.core.QuicStream
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.security.MessageDigest
import java.util.concurrent.atomic.AtomicLong

data class Packet(
    val id: Long,
    val type: String,
    val body: Map<String, Any?>?,
    val hasPayload: Boolean = false
)

data class MediaState(
    val player: String,
    val status: String,
    val title: String?,
    val artist: String?,
    val album: String?
)

enum class ConnectionStatus {
    DISCONNECTED,
    CONNECTING,
    CONNECTED
}

enum class TransferDirection { UPLOAD, DOWNLOAD }

enum class TransferStatus {
    EM_CURSO,
    VERIFICADO,
    NAO_VERIFICADO,
    ERRO
}

data class TransferItem(
    val id: Long,
    val name: String,
    val size: Long,
    val direction: TransferDirection,
    val status: TransferStatus,
    val progress: Float, // 0f to 1f
    val bytesTransferred: Long,
    val error: String? = null,
    val sha256Local: String? = null,
    val sha256Remote: String? = null,
    val timestamp: Long = System.currentTimeMillis(),
    val uri: android.net.Uri? = null
)

data class ShareDonePacket(
    val id: Long,
    val ok: Boolean,
    val sha256: String,
    val bytes: Long,
    val error: String?
)

data class DownloadSession(
    val uri: android.net.Uri,
    var localSha256: String? = null,
    var isStreamFinished: Boolean = false,
    val doneDeferred: CompletableDeferred<ShareDonePacket> = CompletableDeferred()
)

data class AudioSink(
    val id: Long,
    val name: String,
    val description: String,
    val volume: Int,
    val muted: Boolean,
    val is_default: Boolean,
    val is_phone: Boolean
)

data class AudioApp(
    val id: Long,
    val name: String,
    val media: String?,
    val volume: Int,
    val muted: Boolean,
    val sink_id: Long
)

data class AudioState(
    val default_sink: String,
    val sinks: List<AudioSink>,
    val apps: List<AudioApp>
)

object ConnectionRepository {
    private val repositoryScope = CoroutineScope(Dispatchers.Default + SupervisorJob())
    
    private val packetIdCounter = AtomicLong(1)
    
    var lastReceivedClipboardText: String = ""

    @Volatile
    private var appContext: Context? = null

    private val logLock = Any()

    fun initialize(context: Context) {
        appContext = context.applicationContext
        synchronized(logLock) {
            try {
                val logFile = java.io.File(context.filesDir, "diagnostics.log")
                if (logFile.exists()) {
                    val lines = logFile.readLines()
                    val lastLines = if (lines.size > 1000) lines.takeLast(1000) else lines
                    _logs.value = lastLines
                }
            } catch (e: Exception) {
                android.util.Log.e("HyprLink", "Failed to load initial logs from file", e)
            }
        }
    }

    data class FileAnnouncement(val name: String, val size: Long)
    private val fileAnnouncements = java.util.concurrent.ConcurrentHashMap<Long, FileAnnouncement>()

    // Active downloads map
    private val activeDownloads = java.util.concurrent.ConcurrentHashMap<Long, DownloadSession>()

    // Observable transfers list
    private val _transfers = MutableStateFlow<List<TransferItem>>(emptyList())
    val transfers = _transfers.asStateFlow()

    fun updateTransfer(item: TransferItem) {
        val current = _transfers.value.toMutableList()
        val index = current.indexOfFirst { it.id == item.id }
        if (index != -1) {
            current[index] = item
        } else {
            current.add(0, item) // Insert at top (newest first)
        }
        _transfers.value = current

        // Persist completed / terminal transfers to Room
        if (item.status == TransferStatus.VERIFICADO ||
            item.status == TransferStatus.NAO_VERIFICADO ||
            item.status == TransferStatus.ERRO
        ) {
            val context = appContext
            if (context != null) {
                repositoryScope.launch {
                    try {
                        val db = AppDatabase.getDatabase(context)
                        val record = TransferRecord(
                            name = item.name,
                            size = item.size,
                            direction = item.direction.name,
                            status = item.status.name,
                            sha256Local = item.sha256Local,
                            sha256Remote = item.sha256Remote,
                            error = item.error,
                            timestamp = item.timestamp
                        )
                        db.transferRecordDao().insert(record)
                        appendLog("[ROOM] Persisted transfer record for ${item.name} (${item.status})")
                    } catch (e: Exception) {
                        appendLog("[ROOM] Failed to persist transfer: ${e.message}")
                    }
                }
            }
        }
    }

    fun getPersistentHistory(context: Context): Flow<List<TransferRecord>> {
        val db = AppDatabase.getDatabase(context)
        return db.transferRecordDao().getRecent()
    }

    fun clearPersistentHistory(context: Context) {
        val contextToUse = appContext ?: context.applicationContext
        repositoryScope.launch {
            try {
                val db = AppDatabase.getDatabase(contextToUse)
                db.transferRecordDao().clearAll()
                appendLog("[ROOM] Cleared persistent transfer history")
            } catch (e: Exception) {
                appendLog("[ROOM] Failed to clear persistent history: ${e.message}")
            }
        }
    }

    fun clearTransfers() {
        _transfers.value = emptyList()
    }

    // Observable states
    private val _connectionStatus = MutableStateFlow(ConnectionStatus.DISCONNECTED)
    val connectionStatus = _connectionStatus.asStateFlow()

    private val _batteryLevel = MutableStateFlow<Int?>(null)
    val batteryLevel = _batteryLevel.asStateFlow()

    private val _batteryCharging = MutableStateFlow<Boolean?>(null)
    val batteryCharging = _batteryCharging.asStateFlow()

    private val _mediaState = MutableStateFlow<MediaState?>(null)
    val mediaState = _mediaState.asStateFlow()

    private val _fileSendingProgress = MutableStateFlow<Float?>(null)
    val fileSendingProgress = _fileSendingProgress.asStateFlow()

    private val _fileSendingStatus = MutableStateFlow<String?>(null)
    val fileSendingStatus = _fileSendingStatus.asStateFlow()

    private val _logs = MutableStateFlow<List<String>>(emptyList())
    val logs = _logs.asStateFlow()

    private val _hyprEvents = MutableSharedFlow<String>(extraBufferCapacity = 64)
    val hyprEvents = _hyprEvents.asSharedFlow()

    private val _mirroredNotificationsCount = MutableStateFlow(0)
    val mirroredNotificationsCount = _mirroredNotificationsCount.asStateFlow()

    fun incrementMirroredCount() {
        _mirroredNotificationsCount.value++
    }

    var activeConnection: QuicClientConnection? = null
        private set

    fun appendLog(message: String) {
        val timePrefix = java.text.SimpleDateFormat("HH:mm:ss.SSS", java.util.Locale.getDefault()).format(java.util.Date())
        val formattedMsg = "[$timePrefix] $message"
        
        android.util.Log.d("HyprLink", formattedMsg)

        synchronized(logLock) {
            val current = _logs.value.toMutableList()
            current.add(formattedMsg)
            if (current.size > 1000) {
                current.removeAt(0)
            }
            _logs.value = current

            appContext?.let { ctx ->
                try {
                    val logFile = java.io.File(ctx.filesDir, "diagnostics.log")
                    
                    // Limit file size to 5MB to avoid using too much storage
                    if (logFile.exists() && logFile.length() > 5 * 1024 * 1024) {
                        val lines = logFile.readLines()
                        if (lines.size > 2000) {
                            val newLines = lines.drop(lines.size / 2)
                            logFile.writeText(newLines.joinToString("\n") + "\n")
                        }
                    }
                    
                    logFile.appendText(formattedMsg + "\n")
                } catch (e: Exception) {
                    android.util.Log.e("HyprLink", "Failed to write log to file", e)
                }
            }
        }
    }

    fun getFullLogText(): String {
        return try {
            val file = appContext?.let { java.io.File(it.filesDir, "diagnostics.log") }
            if (file != null && file.exists()) {
                file.readText()
            } else {
                _logs.value.joinToString("\n")
            }
        } catch (e: Exception) {
            _logs.value.joinToString("\n")
        }
    }

    fun clearLogs() {
        synchronized(logLock) {
            _logs.value = emptyList()
            appContext?.let { ctx ->
                try {
                    val logFile = java.io.File(ctx.filesDir, "diagnostics.log")
                    if (logFile.exists()) {
                        logFile.delete()
                    }
                } catch (e: Exception) {
                    android.util.Log.e("HyprLink", "Failed to delete log file", e)
                }
            }
        }
    }

    fun setStatus(status: ConnectionStatus) {
        _connectionStatus.value = status
        if (status == ConnectionStatus.DISCONNECTED) {
            _batteryLevel.value = null
            _batteryCharging.value = null
            _mediaState.value = null
            _mirroredNotificationsCount.value = 0
        }
    }

    fun registerActiveConnection(connection: QuicClientConnection, context: Context) {
        appContext = context.applicationContext
        activeConnection = connection
        setStatus(ConnectionStatus.CONNECTED)
        appendLog("[CONN] Registered active connection!")

        // Listen for peer-initiated streams (from daemon)
        connection.setPeerInitiatedStreamCallback(java.util.function.Consumer { stream ->
            repositoryScope.launch {
                if (stream.isUnidirectional()) {
                    handlePeerUniStream(stream, context)
                } else {
                    handlePeerStream(stream, context)
                }
            }
        })

        // Request initial battery status immediately on connection
        repositoryScope.launch {
            try {
                sendBatteryRequest()
            } catch (e: Exception) {
                appendLog("[WARN] Initial battery request failed: ${e.message}")
            }
        }
    }

    fun clearActiveConnection() {
        activeConnection = null
        AudioStreamPlayer.stop()
        setStatus(ConnectionStatus.DISCONNECTED)
    }

    // --- PACKET SENDING METHODS ---

    suspend fun sendClipboardText(text: String) = withContext(Dispatchers.IO) {
        appendLog("[CLIPBOARD] Sending clipboard text to workstation...")
        val body = mapOf("text" to text)
        sendOneWayPacket("clipboard.set", body)
    }

    suspend fun sendPhoneBatteryState(level: Int, charging: Boolean) = withContext(Dispatchers.IO) {
        appendLog("[BATTERY] Sending phone battery state: level=$level, charging=$charging")
        val body = mapOf("level" to level, "charging" to charging)
        sendOneWayPacket("battery.state", body)
    }

    suspend fun sendMediaCommand(command: String) = withContext(Dispatchers.IO) {
        appendLog("[MEDIA] Sending media command: $command")
        val body = mapOf("command" to command)
        sendOneWayPacket("media.command", body)
    }

    suspend fun sendUrl(url: String) = withContext(Dispatchers.IO) {
        appendLog("[SHARE] Sending URL: $url")
        val body = mapOf("url" to url)
        sendOneWayPacket("share.url", body)
    }

    suspend fun sendInputMove(dx: Int, dy: Int) = withContext(Dispatchers.IO) {
        val body = mapOf("dx" to dx, "dy" to dy)
        sendOneWayPacket("input.move", body)
    }

    suspend fun sendInputScroll(dx: Int, dy: Int) = withContext(Dispatchers.IO) {
        val body = mapOf("dx" to dx, "dy" to dy)
        sendOneWayPacket("input.scroll", body)
    }

    suspend fun sendInputClick(button: String) = withContext(Dispatchers.IO) {
        val body = mapOf("button" to button)
        sendOneWayPacket("input.click", body)
    }

    suspend fun sendInputType(text: String) = withContext(Dispatchers.IO) {
        val body = mapOf("text" to text)
        sendOneWayPacket("input.type", body)
    }

    suspend fun sendInputKey(key: String) = withContext(Dispatchers.IO) {
        val body = mapOf("key" to key)
        sendOneWayPacket("input.key", body)
    }

    suspend fun fetchHyprWorkspaces(): String = withContext(Dispatchers.IO) {
        val conn = activeConnection ?: throw IllegalStateException("Not connected")
        val packetId = packetIdCounter.incrementAndGet()
        val packet = Packet(id = packetId, type = "hypr.workspaces", body = null, hasPayload = false)
        val stream = conn.createStream(true)
        val out = stream.outputStream
        val inp = stream.inputStream

        try {
            writeFramedBytes(out, encodePacket(packet))
            out.close() // Half-close output

            val respBytes = ConnectionUtils.readWithDeadline(stream, 10_000) { readFramedBytes(inp) }
            val respPacket = decodePacket(respBytes)
            if (respPacket != null && respPacket.type == "hypr.workspaces_state") {
                val ok = respPacket.body?.get("ok") as? Boolean ?: false
                val data = respPacket.body?.get("data") as? String ?: ""
                if (ok) {
                    return@withContext data
                } else {
                    throw Exception(data.ifEmpty { "Workspaces retrieval returned error status" })
                }
            } else {
                throw Exception("Received unexpected packet type in response: ${respPacket?.type}")
            }
        } finally {
            try {
                stream.closeInput(0)
            } catch (e: Exception) {}
        }
    }

    suspend fun fetchHyprClients(): String = withContext(Dispatchers.IO) {
        val conn = activeConnection ?: throw IllegalStateException("Not connected")
        val packetId = packetIdCounter.incrementAndGet()
        val packet = Packet(id = packetId, type = "hypr.clients", body = null, hasPayload = false)
        val stream = conn.createStream(true)
        val out = stream.outputStream
        val inp = stream.inputStream

        try {
            writeFramedBytes(out, encodePacket(packet))
            out.close() // Half-close output

            val respBytes = ConnectionUtils.readWithDeadline(stream, 10_000) { readFramedBytes(inp) }
            val respPacket = decodePacket(respBytes)
            if (respPacket != null && respPacket.type == "hypr.clients_state") {
                val ok = respPacket.body?.get("ok") as? Boolean ?: false
                val data = respPacket.body?.get("data") as? String ?: ""
                if (ok) {
                    return@withContext data
                } else {
                    throw Exception(data.ifEmpty { "Clients retrieval returned error status" })
                }
            } else {
                throw Exception("Received unexpected packet type in response: ${respPacket?.type}")
            }
        } finally {
            try {
                stream.closeInput(0)
            } catch (e: Exception) {}
        }
    }

    suspend fun dispatchHyprCommand(cmd: String): Boolean = withContext(Dispatchers.IO) {
        val conn = activeConnection ?: throw IllegalStateException("Not connected")
        val packetId = packetIdCounter.incrementAndGet()
        val packet = Packet(id = packetId, type = "hypr.dispatch", body = mapOf("cmd" to cmd), hasPayload = false)
        val stream = conn.createStream(true)
        val out = stream.outputStream
        val inp = stream.inputStream

        try {
            writeFramedBytes(out, encodePacket(packet))
            out.close() // Half-close output

            val respBytes = ConnectionUtils.readWithDeadline(stream, 10_000) { readFramedBytes(inp) }
            val respPacket = decodePacket(respBytes)
            if (respPacket != null && respPacket.type == "hypr.dispatch_result") {
                val ok = respPacket.body?.get("ok") as? Boolean ?: false
                val data = respPacket.body?.get("data") as? String ?: ""
                if (ok) {
                    return@withContext true
                } else {
                    throw Exception(data.ifEmpty { "Dispatch command rejected" })
                }
            } else {
                throw Exception("Received unexpected packet type in response: ${respPacket?.type}")
            }
        } finally {
            try {
                stream.closeInput(0)
            } catch (e: Exception) {}
        }
    }

    suspend fun sendBatteryRequest() = withContext(Dispatchers.IO) {
        val conn = activeConnection ?: throw IllegalStateException("Not connected")
        val packetId = packetIdCounter.incrementAndGet()
        appendLog("[BATTERY] Requesting workstation battery status (packetId=$packetId)...")

        val packet = Packet(id = packetId, type = "battery.request", body = null, hasPayload = false)
        val stream = conn.createStream(true)
        val out = stream.outputStream
        val inp = stream.inputStream

        try {
            // Write request
            writeFramedBytes(out, encodePacket(packet))
            out.close() // Half-close output

            // Read response on same stream
            val respBytes = ConnectionUtils.readWithDeadline(stream, 10_000) { readFramedBytes(inp) }
            val respPacket = decodePacket(respBytes)
            if (respPacket != null && respPacket.type == "battery.state") {
                handleBatteryState(respPacket.body)
            } else {
                appendLog("[BATTERY] Received unexpected packet type in response: ${respPacket?.type}")
            }
        } catch (e: Exception) {
            appendLog("[BATTERY] Error requesting battery: ${e.message}")
            throw e
        } finally {
            try {
                stream.closeInput(0)
            } catch (e: Exception) {}
        }
    }

    suspend fun sendPing(): Boolean = withContext(Dispatchers.IO) {
        val conn = activeConnection ?: return@withContext false
        val packetId = packetIdCounter.incrementAndGet()
        val packet = Packet(id = packetId, type = "core.ping", body = null, hasPayload = false)
        val stream = conn.createStream(true)
        val out = stream.outputStream
        val inp = stream.inputStream
        try {
            writeFramedBytes(out, encodePacket(packet))
            out.close() // Half-close output
            
            val respBytes = ConnectionUtils.readWithDeadline(stream, 10_000) { readFramedBytes(inp) }
            val respPacket = decodePacket(respBytes)
            if (respPacket != null && respPacket.type == "core.pong") {
                return@withContext true
            } else {
                appendLog("[PING] Received unexpected packet type in response: ${respPacket?.type}")
                return@withContext false
            }
        } catch (e: Exception) {
            appendLog("[PING] Ping failed: ${e.message}")
            return@withContext false
        } finally {
            try {
                stream.closeInput(0)
            } catch (e: Exception) {}
        }
    }

    suspend fun sendOneWayPacket(type: String, body: Map<String, Any?>?) {
        sendAnnouncedPacket(type, body, hasPayload = false)
    }

    suspend fun sendAnnouncedPacket(type: String, body: Map<String, Any?>?, hasPayload: Boolean = true): Long? {
        val conn = activeConnection ?: return null
        val packetId = packetIdCounter.incrementAndGet()
        val packet = Packet(id = packetId, type = type, body = body, hasPayload = hasPayload)
        val stream = conn.createStream(true)
        val out = stream.outputStream

        try {
            writeFramedBytes(out, encodePacket(packet))
            out.close() // Half-close
            
            if (hasPayload) {
                // Read any response/acknowledgement (EOF) to ensure flush
                val inp = stream.inputStream
                ConnectionUtils.readWithDeadline(stream, 10_000) {
                    inp.readBytes()
                }
            }
            return packetId
        } catch (e: Exception) {
            appendLog("[WARN] Failed to send packet $type: ${e.message}")
            return null
        } finally {
            // Fechar SEMPRE ambos os lados — um send falhado deixava o output
            // aberto e esgotava os stream credits QUIC (matava sends seguintes)
            try { out.close() } catch (e: Exception) {}
            try { stream.closeInput(0) } catch (e: Exception) {}
        }
    }

    suspend fun fetchAudioState(): AudioState = withContext(Dispatchers.IO) {
        val conn = activeConnection ?: throw IllegalStateException("Not connected")
        val packetId = packetIdCounter.incrementAndGet()
        val packet = Packet(id = packetId, type = "audio.state", body = null, hasPayload = false)
        val stream = conn.createStream(true)
        val out = stream.outputStream
        val inp = stream.inputStream

        try {
            writeFramedBytes(out, encodePacket(packet))
            out.close() // Half-close output

            val respBytes = ConnectionUtils.readWithDeadline(stream, 10_000) { readFramedBytes(inp) }
            val respPacket = decodePacket(respBytes)
            if (respPacket != null && respPacket.type == "audio.state_reply") {
                val body = respPacket.body ?: throw Exception("Empty body in audio.state_reply")
                val defaultSink = body["default_sink"] as? String ?: ""
                
                val sinksList = (body["sinks"] as? List<*>)?.mapNotNull { item ->
                    val map = item as? Map<*, *> ?: return@mapNotNull null
                    val id = (map["id"] as? Number)?.toLong() ?: 0L
                    val name = map["name"] as? String ?: ""
                    val description = map["description"] as? String ?: ""
                    val volume = (map["volume"] as? Number)?.toInt() ?: 0
                    val muted = map["muted"] as? Boolean ?: false
                    val is_default = map["is_default"] as? Boolean ?: false
                    val is_phone = map["is_phone"] as? Boolean ?: false
                    AudioSink(id, name, description, volume, muted, is_default, is_phone)
                } ?: emptyList()

                val appsList = (body["apps"] as? List<*>)?.mapNotNull { item ->
                    val map = item as? Map<*, *> ?: return@mapNotNull null
                    val id = (map["id"] as? Number)?.toLong() ?: 0L
                    val name = map["name"] as? String ?: ""
                    val media = map["media"] as? String
                    val volume = (map["volume"] as? Number)?.toInt() ?: 0
                    val muted = map["muted"] as? Boolean ?: false
                    val sink_id = (map["sink_id"] as? Number)?.toLong() ?: 0L
                    AudioApp(id, name, media, volume, muted, sink_id)
                } ?: emptyList()

                return@withContext AudioState(defaultSink, sinksList, appsList)
            } else {
                throw Exception("Received unexpected packet type in response: ${respPacket?.type}")
            }
        } finally {
            try {
                stream.closeInput(0)
            } catch (e: Exception) {}
        }
    }

    suspend fun setAudioVolume(kind: String, id: Long, volume: Int): Boolean = withContext(Dispatchers.IO) {
        val conn = activeConnection ?: return@withContext false
        val packetId = packetIdCounter.incrementAndGet()
        val packet = Packet(
            id = packetId,
            type = "audio.set_volume",
            body = mapOf("kind" to kind, "id" to id, "volume" to volume),
            hasPayload = false
        )
        val stream = conn.createStream(true)
        val out = stream.outputStream
        val inp = stream.inputStream
        try {
            writeFramedBytes(out, encodePacket(packet))
            out.close()
            val respBytes = ConnectionUtils.readWithDeadline(stream, 5000) { readFramedBytes(inp) }
            val respPacket = decodePacket(respBytes)
            return@withContext (respPacket != null && respPacket.type == "audio.ack")
        } catch (e: Exception) {
            appendLog("[WARN] Failed to set audio volume: ${e.message}")
            return@withContext false
        } finally {
            try { stream.closeInput(0) } catch (e: Exception) {}
        }
    }

    suspend fun setAudioMute(kind: String, id: Long, muted: Boolean): Boolean = withContext(Dispatchers.IO) {
        val conn = activeConnection ?: return@withContext false
        val packetId = packetIdCounter.incrementAndGet()
        val packet = Packet(
            id = packetId,
            type = "audio.set_mute",
            body = mapOf("kind" to kind, "id" to id, "muted" to muted),
            hasPayload = false
        )
        val stream = conn.createStream(true)
        val out = stream.outputStream
        val inp = stream.inputStream
        try {
            writeFramedBytes(out, encodePacket(packet))
            out.close()
            val respBytes = ConnectionUtils.readWithDeadline(stream, 5000) { readFramedBytes(inp) }
            val respPacket = decodePacket(respBytes)
            return@withContext (respPacket != null && respPacket.type == "audio.ack")
        } catch (e: Exception) {
            appendLog("[WARN] Failed to set audio mute: ${e.message}")
            return@withContext false
        } finally {
            try { stream.closeInput(0) } catch (e: Exception) {}
        }
    }

    suspend fun setAudioDefaultSink(name: String): Boolean = withContext(Dispatchers.IO) {
        val conn = activeConnection ?: return@withContext false
        val packetId = packetIdCounter.incrementAndGet()
        val packet = Packet(
            id = packetId,
            type = "audio.set_default_sink",
            body = mapOf("name" to name),
            hasPayload = false
        )
        val stream = conn.createStream(true)
        val out = stream.outputStream
        val inp = stream.inputStream
        try {
            writeFramedBytes(out, encodePacket(packet))
            out.close()
            val respBytes = ConnectionUtils.readWithDeadline(stream, 5000) { readFramedBytes(inp) }
            val respPacket = decodePacket(respBytes)
            return@withContext (respPacket != null && respPacket.type == "audio.ack")
        } catch (e: Exception) {
            appendLog("[WARN] Failed to set default audio sink: ${e.message}")
            return@withContext false
        } finally {
            try { stream.closeInput(0) } catch (e: Exception) {}
        }
    }

    fun registerAudioTapAnnouncement(id: Long) {
        fileAnnouncements[id] = FileAnnouncement("__audio_tap__", 0L)
        appendLog("[AUDIO] Registered audio tap announcement with packetId=$id")
    }

    suspend fun startAudioTap(): Long? = withContext(Dispatchers.IO) {
        val conn = activeConnection ?: return@withContext null
        val packetId = packetIdCounter.incrementAndGet()
        val packet = Packet(id = packetId, type = "audio.tap_start", body = null, hasPayload = false)
        val stream = conn.createStream(true)
        val out = stream.outputStream
        val inp = stream.inputStream
        try {
            writeFramedBytes(out, encodePacket(packet))
            out.close()
            val respBytes = ConnectionUtils.readWithDeadline(stream, 10_000) { readFramedBytes(inp) }
            val respPacket = decodePacket(respBytes)
            if (respPacket != null && respPacket.type == "audio.tap_ready") {
                val id = (respPacket.body?.get("id") as? Number)?.toLong() ?: return@withContext null
                registerAudioTapAnnouncement(id)
                return@withContext id
            }
            return@withContext null
        } catch (e: Exception) {
            appendLog("[WARN] Failed to start audio tap: ${e.message}")
            return@withContext null
        } finally {
            try { stream.closeInput(0) } catch (e: Exception) {}
        }
    }

    suspend fun stopAudioTap(): Boolean = withContext(Dispatchers.IO) {
        val conn = activeConnection ?: return@withContext false
        val packetId = packetIdCounter.incrementAndGet()
        val packet = Packet(id = packetId, type = "audio.tap_stop", body = null, hasPayload = false)
        val stream = conn.createStream(true)
        val out = stream.outputStream
        val inp = stream.inputStream
        try {
            writeFramedBytes(out, encodePacket(packet))
            out.close()
            val respBytes = ConnectionUtils.readWithDeadline(stream, 5000) { readFramedBytes(inp) }
            val respPacket = decodePacket(respBytes)
            return@withContext (respPacket != null && respPacket.type == "audio.ack")
        } catch (e: Exception) {
            appendLog("[WARN] Failed to stop audio tap: ${e.message}")
            return@withContext false
        } finally {
            try { stream.closeInput(0) } catch (e: Exception) {}
        }
    }

    suspend fun sendFile(context: Context, uri: android.net.Uri, name: String, size: Long) = withContext(Dispatchers.IO) {
        val conn = activeConnection
        if (conn == null) {
            appendLog("[ERROR] Cannot send file: Not connected to workstation")
            return@withContext
        }
        _fileSendingProgress.value = 0f
        _fileSendingStatus.value = "A preparar envio de $name..."
        val packetId = packetIdCounter.incrementAndGet()
        try {
            appendLog("[SHARE] Initiating file send: $name ($size bytes), packetId=$packetId")
            
            val item = TransferItem(
                id = packetId,
                name = name,
                size = size,
                direction = TransferDirection.UPLOAD,
                status = TransferStatus.EM_CURSO,
                progress = 0f,
                bytesTransferred = 0L
            )
            updateTransfer(item)

            // 1. Send share.file Packet on a control stream
            val packet = Packet(
                id = packetId,
                type = "share.file",
                body = mapOf("name" to name, "size" to size),
                hasPayload = true
            )
            val controlStream = conn.createStream(true)
            val cborBytes = encodePacket(packet)
            val outCtrl = controlStream.outputStream
            writeFramedBytes(outCtrl, cborBytes)
            outCtrl.close() // half-close
            
            // Wait for EOF on control stream
            val inCtrl = controlStream.inputStream
            try { 
                ConnectionUtils.readWithDeadline(controlStream, 10_000) {
                    inCtrl.readBytes()
                }
            } catch (e: Exception) {}
            controlStream.closeInput(0)
            
            // 2. Open unidirectional stream for file payload
            val uniStream = conn.createStream(false)
            val outUni = uniStream.outputStream
            
            // Write 8-byte big-endian packet ID
            val idBytes = ByteArray(8)
            for (i in 0..7) {
                idBytes[i] = ((packetId shr (56 - i * 8)) and 0xFF).toByte()
            }
            outUni.write(idBytes)
            
            val digest = MessageDigest.getInstance("SHA-256")
            
            // Stream chunks from file
            context.contentResolver.openInputStream(uri).use { inputStream ->
                if (inputStream == null) throw IOException("Cannot open input stream for Uri: $uri")
                val buffer = ByteArray(32 * 1024)
                var bytesSent = 0L
                while (true) {
                    val read = inputStream.read(buffer)
                    if (read == -1) break
                    outUni.write(buffer, 0, read)
                    digest.update(buffer, 0, read)
                    bytesSent += read
                    
                    _fileSendingProgress.value = if (size > 0) bytesSent.toFloat() / size else 1f
                    _fileSendingStatus.value = "A enviar: ${bytesSent * 100 / size}% (${bytesSent / (1024 * 1024)} MB / ${size / (1024 * 1024)} MB)"
                    
                    val currentItem = _transfers.value.find { it.id == packetId }
                    if (currentItem != null && currentItem.status == TransferStatus.EM_CURSO) {
                        val progress = if (size > 0) bytesSent.toFloat() / size else 0f
                        if (bytesSent > currentItem.bytesTransferred) {
                            updateTransfer(currentItem.copy(
                                bytesTransferred = bytesSent,
                                progress = progress
                            ))
                        }
                    }
                }
            }
            outUni.flush()
            outUni.close()
            
            val hashBytes = digest.digest()
            val localSha256 = hashBytes.joinToString("") { "%02x".format(it) }
            
            val completedLocalItem = _transfers.value.find { it.id == packetId }
            if (completedLocalItem != null) {
                updateTransfer(completedLocalItem.copy(sha256Local = localSha256))
            }
            
            appendLog("[SHARE] Successfully sent file: $name. Calculated local SHA-256: $localSha256")
            _fileSendingStatus.value = "Ficheiro $name enviado com sucesso!"
        } catch (e: Exception) {
            appendLog("[SHARE] Error sending file: ${e.message}")
            _fileSendingStatus.value = "Erro ao enviar: ${e.message}"
            
            val currentItem = _transfers.value.find { it.id == packetId }
            if (currentItem != null) {
                updateTransfer(currentItem.copy(
                    status = TransferStatus.ERRO,
                    error = e.message ?: "Erro desconhecido no envio"
                ))
            }
        } finally {
            delay(2500)
            _fileSendingProgress.value = null
            _fileSendingStatus.value = null
        }
    }

    // --- INCOMING PEER-INITIATED STREAM HANDLER ---

    private suspend fun handlePeerStream(stream: QuicStream, context: Context) = withContext(Dispatchers.IO) {
        val inp = stream.inputStream
        try {
            val cborBytes = ConnectionUtils.readWithDeadline(stream, 10_000) { readFramedBytes(inp) }
            val packet = decodePacket(cborBytes) ?: return@withContext
            
            val routerPrefix = packet.type.substringBefore(".")
            appendLog("[RECV] Incoming stream packet type: ${packet.type} (routerPrefix: $routerPrefix)")

            when (routerPrefix) {
                "clipboard" -> {
                    if (packet.type == "clipboard.set") {
                        val text = packet.body?.get("text") as? String
                        if (text != null) {
                            setClipboardText(context, text)
                        }
                    }
                }
                "notification" -> {
                    if (packet.type == "notification.send") {
                        val title = packet.body?.get("title") as? String ?: "Workstation Notification"
                        val body = packet.body?.get("body") as? String ?: ""
                        val appName = packet.body?.get("app_name") as? String ?: "System"
                        postNotification(context, title, body, appName)
                    } else if (packet.type == "notification.action") {
                        val key = packet.body?.get("key") as? String
                        val actionValue = packet.body?.get("action") ?: packet.body?.get("idx")
                        if (key != null && actionValue != null) {
                            val sbn = HyprNotificationListenerService.cachedNotifications[key]
                            if (sbn != null) {
                                val actions = sbn.notification.actions
                                if (actions != null) {
                                    val idx = when (actionValue) {
                                        is Number -> actionValue.toInt()
                                        is String -> actionValue.toIntOrNull() ?: -1
                                        else -> -1
                                    }
                                    
                                    var triggeredAction: android.app.Notification.Action? = null
                                    var triggeredIdx = -1
                                    
                                    if (idx >= 0 && idx < actions.size) {
                                        triggeredAction = actions[idx]
                                        triggeredIdx = idx
                                    } else if (actionValue is String) {
                                        val match = actions.withIndex().find { 
                                            it.value.title?.toString()?.equals(actionValue, ignoreCase = true) == true 
                                        }
                                        if (match != null) {
                                            triggeredAction = match.value
                                            triggeredIdx = match.index
                                        }
                                    }
                                    
                                    if (triggeredAction != null) {
                                        try {
                                            triggeredAction.actionIntent.send()
                                            appendLog("[NOTIF] Triggered action $triggeredIdx ('${triggeredAction.title}') for key $key")
                                        } catch (e: Exception) {
                                            appendLog("[WARN] Failed to trigger action: ${e.message}")
                                        }
                                    } else {
                                        appendLog("[WARN] Action '$actionValue' not resolved for key $key")
                                    }
                                }
                            } else {
                                appendLog("[WARN] Notification key $key not found in active cache for action")
                            }
                        }
                    } else if (packet.type == "notification.reply") {
                        val key = packet.body?.get("key") as? String
                        val idxVal = packet.body?.get("idx")
                        val text = packet.body?.get("text") as? String
                        if (key != null && idxVal != null && text != null) {
                            val idx = when (idxVal) {
                                is Number -> idxVal.toInt()
                                is String -> idxVal.toIntOrNull() ?: -1
                                else -> -1
                            }
                            val sbn = HyprNotificationListenerService.cachedNotifications[key]
                            if (sbn == null) {
                                appendLog("[WARN] Notification key $key not found in active cache for reply")
                            } else {
                                val actions = sbn.notification.actions
                                val action = actions?.getOrNull(idx)
                                if (action == null) {
                                    appendLog("[WARN] Action at index $idx not found for key $key")
                                } else {
                                    val remoteInputs = action.remoteInputs
                                    if (remoteInputs.isNullOrEmpty()) {
                                        appendLog("[WARN] No remote inputs found on action $idx for key $key")
                                    } else {
                                        val fillIntent = Intent()
                                        val results = Bundle()
                                        for (ri in remoteInputs) {
                                            results.putCharSequence(ri.resultKey, text)
                                        }
                                        android.app.RemoteInput.addResultsToIntent(remoteInputs, fillIntent, results)
                                        try {
                                            val listenerContext = HyprNotificationListenerService.instance ?: context
                                            action.actionIntent.send(listenerContext, 0, fillIntent)
                                            appendLog("[NOTIF] Reply sent for key $key")
                                        } catch (e: Exception) {
                                            appendLog("[WARN] Reply failed: ${e.message}")
                                        }
                                    }
                                }
                            }
                        }
                    } else if (packet.type == "notification.dismiss") {
                        val key = packet.body?.get("key") as? String
                        if (key != null) {
                            val listener = HyprNotificationListenerService.instance
                            if (listener != null) {
                                try {
                                    listener.cancelNotification(key)
                                    appendLog("[NOTIF] Dismissed notification locally for key $key")
                                } catch (e: Exception) {
                                    appendLog("[WARN] Failed to dismiss notification: ${e.message}")
                                }
                            } else {
                                appendLog("[WARN] HyprNotificationListenerService instance not running")
                            }
                        }
                    }
                }
                "media" -> {
                    if (packet.type == "media.state") {
                        handleMediaState(packet.body)
                    }
                }
                "battery" -> {
                    if (packet.type == "battery.state") {
                        handleBatteryState(packet.body)
                    }
                }
                "hypr" -> {
                    if (packet.type == "hypr.event") {
                        val eventName = packet.body?.get("event") as? String ?: ""
                        appendLog("[HYPR] Received workstation event: $eventName")
                        _hyprEvents.tryEmit(eventName)
                    }
                }
                "share" -> {
                    if (packet.type == "share.file") {
                        val name = packet.body?.get("name") as? String
                        val sizeVal = packet.body?.get("size")
                        val size = when (sizeVal) {
                            is Number -> sizeVal.toLong()
                            else -> 0L
                        }
                        if (name != null) {
                            fileAnnouncements[packet.id] = FileAnnouncement(name, size)
                            appendLog("[SHARE] Received file announcement: $name ($size bytes), packetId=${packet.id}")
                        }
                    } else if (packet.type == "share.progress") {
                        val idVal = packet.body?.get("id")
                        val id = when (idVal) {
                            is Number -> idVal.toLong()
                            else -> 0L
                        }
                        val bytesVal = packet.body?.get("bytes")
                        val bytes = when (bytesVal) {
                            is Number -> bytesVal.toLong()
                            else -> 0L
                        }
                        val totalVal = packet.body?.get("total")
                        val total = when (totalVal) {
                            is Number -> totalVal.toLong()
                            else -> 0L
                        }
                        handleShareProgress(id, bytes, total)
                    } else if (packet.type == "share.done") {
                        val idVal = packet.body?.get("id")
                        val id = when (idVal) {
                            is Number -> idVal.toLong()
                            else -> 0L
                        }
                        val ok = packet.body?.get("ok") as? Boolean ?: false
                        val sha256 = packet.body?.get("sha256") as? String ?: ""
                        val bytesVal = packet.body?.get("bytes")
                        val bytes = when (bytesVal) {
                            is Number -> bytesVal.toLong()
                            else -> 0L
                        }
                        val error = packet.body?.get("error") as? String
                        handleShareDone(id, ok, sha256, bytes, error, context)
                    }
                }
                "webcam" -> {
                    if (packet.type == "webcam.start") {
                        val width = (packet.body?.get("width") as? Number)?.toInt() ?: 1280
                        val height = (packet.body?.get("height") as? Number)?.toInt() ?: 720
                        val fps = (packet.body?.get("fps") as? Number)?.toInt() ?: 24
                        val codec = (packet.body?.get("codec") as? String) ?: "h264"
                        val conn = activeConnection
                        if (conn != null) {
                            WebcamStreamer.start(context, conn, packet.id, width, height, fps, codec)
                            // Trazer app para primeiro plano
                            val intent = Intent(context, MainActivity::class.java).apply {
                                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
                            }
                            context.startActivity(intent)
                        }
                    } else if (packet.type == "webcam.stop") {
                        WebcamStreamer.stop()
                    } else if (packet.type == "webcam.mic_start_request") {
                        WebcamStreamer.startMic(context)
                    } else if (packet.type == "webcam.mic_stop_request") {
                        WebcamStreamer.stopMic()
                    }
                }
                "phone_audio" -> {
                    val am = context.getSystemService(Context.AUDIO_SERVICE) as android.media.AudioManager
                    val nm = context.getSystemService(Context.NOTIFICATION_SERVICE) as android.app.NotificationManager

                    if (packet.type == "phone_audio.state") {
                        fun pct(streamType: Int): Int {
                            val cur = am.getStreamVolume(streamType)
                            val max = am.getStreamMaxVolume(streamType).coerceAtLeast(1)
                            return (cur * 100 / max)
                        }
                        val ringerMode = when (am.ringerMode) {
                            android.media.AudioManager.RINGER_MODE_SILENT -> "silent"
                            android.media.AudioManager.RINGER_MODE_VIBRATE -> "vibrate"
                            else -> "normal"
                        }
                        val body = mapOf(
                            "ring_percent" to pct(android.media.AudioManager.STREAM_RING),
                            "media_percent" to pct(android.media.AudioManager.STREAM_MUSIC),
                            "alarm_percent" to pct(android.media.AudioManager.STREAM_ALARM),
                            "ringer_mode" to ringerMode,
                            "dnd_access" to nm.isNotificationPolicyAccessGranted,
                            "dnd_enabled" to (nm.currentInterruptionFilter != android.app.NotificationManager.INTERRUPTION_FILTER_ALL)
                        )
                        val replyPacket = Packet(
                            id = packet.id,
                            type = "phone_audio.state_reply",
                            body = body,
                            hasPayload = false
                        )
                        val out = stream.outputStream
                        writeFramedBytes(out, encodePacket(replyPacket))
                        out.close()
                    } else if (packet.type == "phone_audio.set_volume") {
                        val streamName = packet.body?.get("stream") as? String
                        val percent = (packet.body?.get("percent") as? Number)?.toInt()?.coerceIn(0, 100)
                        val streamType = when (streamName) {
                            "ring" -> android.media.AudioManager.STREAM_RING
                            "media" -> android.media.AudioManager.STREAM_MUSIC
                            "alarm" -> android.media.AudioManager.STREAM_ALARM
                            else -> null
                        }
                        if (streamType != null && percent != null) {
                            val max = am.getStreamMaxVolume(streamType)
                            am.setStreamVolume(streamType, (percent * max / 100), 0)
                        }
                    } else if (packet.type == "phone_audio.set_ringer_mode") {
                        val mode = packet.body?.get("mode") as? String
                        if (nm.isNotificationPolicyAccessGranted) {
                            try {
                                am.ringerMode = when (mode) {
                                    "silent" -> android.media.AudioManager.RINGER_MODE_SILENT
                                    "vibrate" -> android.media.AudioManager.RINGER_MODE_VIBRATE
                                    else -> android.media.AudioManager.RINGER_MODE_NORMAL
                                }
                            } catch (e: SecurityException) { /* sem permissão de verdade, ignora */ }
                        } else {
                            // Sem acesso a "Não Perturbe" — abre a tela de permissão pro usuário conceder ali mesmo.
                            val intent = Intent(android.provider.Settings.ACTION_NOTIFICATION_POLICY_ACCESS_SETTINGS).apply {
                                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                            }
                            context.startActivity(intent)
                        }
                    } else if (packet.type == "phone_audio.set_dnd") {
                        val enabled = packet.body?.get("enabled") as? Boolean ?: false
                        if (nm.isNotificationPolicyAccessGranted) {
                            try {
                                nm.setInterruptionFilter(
                                    if (enabled) android.app.NotificationManager.INTERRUPTION_FILTER_PRIORITY
                                    else android.app.NotificationManager.INTERRUPTION_FILTER_ALL
                                )
                            } catch (e: SecurityException) { /* sem permissão de verdade, ignora */ }
                        } else {
                            val intent = Intent(android.provider.Settings.ACTION_NOTIFICATION_POLICY_ACCESS_SETTINGS).apply {
                                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                            }
                            context.startActivity(intent)
                        }
                    }
                }
                else -> {
                    appendLog("[WARN] Unknown packet prefix router: $routerPrefix (type: ${packet.type})")
                }
            }
        } catch (e: Exception) {
            appendLog("[WARN] Error handling peer-initiated stream: ${e.message}")
        } finally {
            try {
                stream.closeInput(0)
            } catch (e: Exception) {}
        }
    }

    private suspend fun handlePeerUniStream(stream: QuicStream, context: Context) = withContext(Dispatchers.IO) {
        val inp = stream.inputStream
        var handedOffToAudioTap = false
        try {
            // Read 8 bytes for packet ID
            val idBytes = ConnectionUtils.readWithDeadline(stream, 10_000) { readExact(inp, 8) }
            var packetId = 0L
            for (i in 0..7) {
                packetId = (packetId shl 8) or (idBytes[i].toLong() and 0xFF)
            }
            appendLog("[SHARE] Received unidirectional stream with packetId=$packetId")

            // Wait for announcement if not present yet (out-of-order delivery)
            var announcement: FileAnnouncement? = null
            for (retry in 1..40) { // 40 * 200ms = 8 seconds
                announcement = fileAnnouncements[packetId]
                if (announcement != null) break
                delay(200)
            }

            if (announcement == null) {
                appendLog("[ERROR] Timeout waiting for file announcement with packetId=$packetId")
                return@withContext
            }

            // Remove announcement to avoid leaks
            fileAnnouncements.remove(packetId)

            if (announcement.name == "__audio_tap__") {
                appendLog("[AUDIO] Redirecting unidirectional stream for audio tap to AudioStreamPlayer...")
                handedOffToAudioTap = true
                AudioStreamPlayer.playStream(stream)
                return@withContext
            }

            val rawName = announcement.name
            val size = announcement.size

            // 4. Sanitiza name (usa só o último componente do path; rejeita vazio, "." e "..")
            val sanitizedName = sanitizeFileName(rawName)
            if (sanitizedName == null) {
                appendLog("[ERROR] Sanitization failed for filename: $rawName")
                return@withContext
            }

            appendLog("[SHARE] Starting file receive for $sanitizedName ($size bytes)...")
            saveFileFromStream(stream, sanitizedName, size, context, packetId)

        } catch (e: Exception) {
            appendLog("[ERROR] Error receiving file: ${e.message}")
        } finally {
            if (!handedOffToAudioTap) {
                try {
                    stream.closeInput(0)
                } catch (e: Exception) {}
            }
        }
    }

    private fun sanitizeFileName(name: String): String? {
        val normalized = name.replace('\\', '/')
        val fileName = normalized.substringAfterLast('/')
        val trimmed = fileName.trim()
        if (trimmed.isEmpty() || trimmed == "." || trimmed == "..") {
            return null
        }
        if (trimmed.contains("/") || trimmed.contains("..")) {
            return null
        }
        return trimmed
    }

    fun handleShareProgress(id: Long, bytes: Long, total: Long) {
        val currentItem = _transfers.value.find { it.id == id } ?: return
        if (currentItem.status == TransferStatus.EM_CURSO) {
            val progress = if (total > 0) bytes.toFloat() / total else 0f
            updateTransfer(currentItem.copy(
                bytesTransferred = bytes,
                progress = progress
            ))
        }
    }

    fun handleShareDone(id: Long, ok: Boolean, sha256: String, bytes: Long, error: String?, context: Context) {
        val currentItem = _transfers.value.find { it.id == id }
        if (currentItem == null) {
            appendLog("[WARN] Received share.done for unknown transfer id=$id")
            return
        }
        
        if (currentItem.direction == TransferDirection.UPLOAD) {
            if (!ok) {
                val errMsg = error ?: "Transferência falhou no PC"
                updateTransfer(currentItem.copy(
                    status = TransferStatus.ERRO,
                    error = errMsg
                ))
                appendLog("[SHARE] Transfer id=$id failed: $errMsg")
                return
            }
            val localHash = currentItem.sha256Local ?: ""
            if (localHash.equals(sha256, ignoreCase = true)) {
                updateTransfer(currentItem.copy(
                    status = TransferStatus.VERIFICADO,
                    progress = 1f,
                    bytesTransferred = bytes,
                    sha256Remote = sha256
                ))
                appendLog("[SHARE] Transfer id=$id VERIFIED ✓ (SHA256: $sha256)")
            } else {
                updateTransfer(currentItem.copy(
                    status = TransferStatus.ERRO,
                    progress = 1f,
                    bytesTransferred = bytes,
                    sha256Remote = sha256,
                    error = "ERRO: hash não corresponde"
                ))
                appendLog("[SHARE] Transfer id=$id ERROR: SHA256 mismatch. Local: $localHash, Remote: $sha256")
            }
        } else {
            val session = activeDownloads[id]
            if (session != null) {
                session.doneDeferred.complete(ShareDonePacket(id, ok, sha256, bytes, error))
            } else {
                appendLog("[WARN] Received share.done for download id=$id but session not found or already completed")
            }
        }
    }

    private suspend fun saveFileFromStream(
        stream: QuicStream,
        name: String,
        size: Long,
        context: Context,
        packetId: Long
    ) = withContext(Dispatchers.IO) {
        val resolver = context.contentResolver
        val contentValues = android.content.ContentValues().apply {
            put(android.provider.MediaStore.MediaColumns.DISPLAY_NAME, name)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                put(android.provider.MediaStore.MediaColumns.RELATIVE_PATH, "${android.os.Environment.DIRECTORY_DOWNLOADS}/HyprLink")
            }
        }
        
        val externalUri = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            android.provider.MediaStore.Downloads.EXTERNAL_CONTENT_URI
        } else {
            android.net.Uri.parse("content://media/external/downloads")
        }
        
        val uri: android.net.Uri? = resolver.insert(externalUri, contentValues)
        if (uri == null) {
            throw IOException("Failed to create MediaStore entry for $name")
        }

        val item = TransferItem(
            id = packetId,
            name = name,
            size = size,
            direction = TransferDirection.DOWNLOAD,
            status = TransferStatus.EM_CURSO,
            progress = 0f,
            bytesTransferred = 0L,
            uri = uri
        )
        updateTransfer(item)

        val doneDeferred = CompletableDeferred<ShareDonePacket>()
        val session = DownloadSession(uri = uri, doneDeferred = doneDeferred)
        activeDownloads[packetId] = session

        val inputStream = stream.inputStream
        val digest = MessageDigest.getInstance("SHA-256")
        try {
            _fileSendingProgress.value = 0f
            _fileSendingStatus.value = "A receber $name..."

            resolver.openOutputStream(uri).use { outputStream ->
                if (outputStream == null) throw IOException("Failed to open output stream for Uri: $uri")
                val buffer = ByteArray(32 * 1024)
                var bytesReceived = 0L
                while (true) {
                    val read = ConnectionUtils.readWithDeadline(stream, 10_000) {
                        inputStream.read(buffer)
                    }
                    if (read == -1) break
                    outputStream.write(buffer, 0, read)
                    digest.update(buffer, 0, read)
                    bytesReceived += read
                    
                    _fileSendingProgress.value = if (size > 0) bytesReceived.toFloat() / size else 1f
                    _fileSendingStatus.value = "A receber: ${if (size > 0) (bytesReceived * 100 / size).toInt() else 100}% (${bytesReceived / (1024 * 1024)} MB / ${size / (1024 * 1024)} MB)"
                    
                    val currentItem = _transfers.value.find { it.id == packetId }
                    if (currentItem != null) {
                        val progress = if (size > 0) bytesReceived.toFloat() / size else 0f
                        updateTransfer(currentItem.copy(
                            bytesTransferred = bytesReceived,
                            progress = progress
                        ))
                    }
                }
            }

            val localSha256 = digest.digest().joinToString("") { "%02x".format(it) }
            session.localSha256 = localSha256
            session.isStreamFinished = true

            appendLog("[SHARE] Stream EOF reached for download id=$packetId. Calculated local SHA-256: $localSha256. Waiting up to 15s for share.done...")

            try {
                withTimeout(15000) {
                    val donePacket = doneDeferred.await()
                    if (donePacket.ok) {
                        if (localSha256.equals(donePacket.sha256, ignoreCase = true)) {
                            updateTransfer(TransferItem(
                                id = packetId,
                                name = name,
                                size = size,
                                direction = TransferDirection.DOWNLOAD,
                                status = TransferStatus.VERIFICADO,
                                progress = 1f,
                                bytesTransferred = donePacket.bytes,
                                sha256Local = localSha256,
                                sha256Remote = donePacket.sha256,
                                uri = uri
                            ))
                            appendLog("[SHARE] Download id=$packetId VERIFIED ✓ (SHA256: $localSha256)")
                            postNotification(context, "Ficheiro recebido", "$name foi verificado e guardado com sucesso nos Downloads.", "HyprLink")
                        } else {
                            try {
                                resolver.delete(uri, null, null)
                            } catch (e: Exception) {
                                appendLog("[SHARE] Failed to delete mismatched file: ${e.message}")
                            }
                            updateTransfer(TransferItem(
                                id = packetId,
                                name = name,
                                size = size,
                                direction = TransferDirection.DOWNLOAD,
                                status = TransferStatus.ERRO,
                                progress = 1f,
                                bytesTransferred = donePacket.bytes,
                                sha256Local = localSha256,
                                sha256Remote = donePacket.sha256,
                                error = "ERRO: hash não corresponde",
                                uri = uri
                            ))
                            appendLog("[SHARE] Download id=$packetId ERROR: SHA256 mismatch. Local: $localSha256, Remote: ${donePacket.sha256}. Deleted file.")
                            postNotification(context, "Erro de verificação", "Ficheiro $name foi corrompido ou modificado durante a transferência.", "HyprLink")
                        }
                    } else {
                        try {
                            resolver.delete(uri, null, null)
                        } catch (e: Exception) {}
                        val errMsg = donePacket.error ?: "Transferência falhou no PC"
                        updateTransfer(TransferItem(
                            id = packetId,
                            name = name,
                            size = size,
                            direction = TransferDirection.DOWNLOAD,
                            status = TransferStatus.ERRO,
                            progress = 1f,
                            bytesTransferred = donePacket.bytes,
                            error = errMsg,
                            uri = uri
                        ))
                        appendLog("[SHARE] Download id=$packetId failed on PC side: $errMsg")
                        postNotification(context, "Transferência falhou", "O PC reportou erro ao processar o ficheiro.", "HyprLink")
                    }
                }
            } catch (e: TimeoutCancellationException) {
                updateTransfer(TransferItem(
                    id = packetId,
                    name = name,
                    size = size,
                    direction = TransferDirection.DOWNLOAD,
                    status = TransferStatus.NAO_VERIFICADO,
                    progress = 1f,
                    bytesTransferred = size,
                    sha256Local = localSha256,
                    error = "Não verificado (timeout de confirmação)",
                    uri = uri
                ))
                appendLog("[SHARE] Download id=$packetId timeout waiting for share.done. Kept file, marked NAO_VERIFICADO.")
                postNotification(context, "Ficheiro guardado (Não verificado)", "$name foi guardado mas não pôde ser verificado.", "HyprLink")
            }

        } catch (e: Exception) {
            try {
                resolver.delete(uri, null, null)
            } catch (ex: Exception) {}
            
            updateTransfer(TransferItem(
                id = packetId,
                name = name,
                size = size,
                direction = TransferDirection.DOWNLOAD,
                status = TransferStatus.ERRO,
                progress = 1f,
                bytesTransferred = 0L,
                error = e.message ?: "Erro desconhecido na receção",
                uri = uri
            ))
            throw e
        } finally {
            activeDownloads.remove(packetId)
            _fileSendingProgress.value = null
            _fileSendingStatus.value = null
        }
    }

    private fun handleBatteryState(body: Map<String, Any?>?) {
        if (body == null) return
        val levelVal = body["level"]
        val chargingVal = body["charging"]
        
        val level = when (levelVal) {
            is Number -> levelVal.toInt()
            else -> null
        }
        val charging = when (chargingVal) {
            is Boolean -> chargingVal
            else -> null
        }

        if (level != null) {
            _batteryLevel.value = level
            appendLog("[BATTERY] Workstation Battery: $level% (Charging: $charging)")
        }
        if (charging != null) {
            _batteryCharging.value = charging
        }
    }

    private fun handleMediaState(body: Map<String, Any?>?) {
        if (body == null) return
        val player = body["player"] as? String ?: "Unknown Player"
        val status = body["status"] as? String ?: "stopped"
        val title = body["title"] as? String
        val artist = body["artist"] as? String
        val album = body["album"] as? String

        _mediaState.value = MediaState(
            player = player,
            status = status,
            title = title,
            artist = artist,
            album = album
        )
        appendLog("[MEDIA] Now Playing updated: $title - $artist [$status]")
    }

    private fun setClipboardText(context: Context, text: String) {
        val handler = android.os.Handler(android.os.Looper.getMainLooper())
        handler.post {
            try {
                val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                val clip = ClipData.newPlainText("HyprLink Workstation Clipboard", text)
                lastReceivedClipboardText = text
                clipboard.setPrimaryClip(clip)
                appendLog("[CLIPBOARD] Copied from workstation!")
            } catch (e: Exception) {
                appendLog("[CLIPBOARD] Error setting clipboard: ${e.message}")
            }
        }
    }

    private fun postNotification(context: Context, title: String, body: String, appName: String) {
        val channelId = "workstation_notifications_channel"
        val notificationManager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                channelId,
                "Notificações da Workstation",
                NotificationManager.IMPORTANCE_DEFAULT
            )
            notificationManager.createNotificationChannel(channel)
        }
        val notification = NotificationCompat.Builder(context, channelId)
            .setContentTitle(title)
            .setContentText(body)
            .setSubText(appName)
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setAutoCancel(true)
            .build()
        
        notificationManager.notify(System.currentTimeMillis().toInt(), notification)
        appendLog("[NOTIF] Posted notification from workstation: $title")
    }

    // --- HELPERS FOR STREAM READING/WRITING WITH FRAMING ---

    private fun writeFramedBytes(outputStream: java.io.OutputStream, bytes: ByteArray) {
        val size = bytes.size
        val frameHeader = byteArrayOf(
            ((size shr 24) and 0xFF).toByte(),
            ((size shr 16) and 0xFF).toByte(),
            ((size shr 8) and 0xFF).toByte(),
            (size and 0xFF).toByte()
        )
        outputStream.write(frameHeader)
        outputStream.write(bytes)
        outputStream.flush()
    }

    private fun readFramedBytes(inputStream: java.io.InputStream): ByteArray {
        val header = readExact(inputStream, 4)
        val responseSize = ((header[0].toInt() and 0xFF) shl 24) or
                           ((header[1].toInt() and 0xFF) shl 16) or
                           ((header[2].toInt() and 0xFF) shl 8) or
                           (header[3].toInt() and 0xFF)
        
        if (responseSize <= 0 || responseSize > 10 * 1024 * 1024) {
            throw IOException("Invalid frame size: $responseSize")
        }
        return readExact(inputStream, responseSize)
    }

    private fun readExact(inputStream: java.io.InputStream, size: Int): ByteArray {
        val buffer = ByteArray(size)
        var readBytes = 0
        while (readBytes < size) {
            val r = inputStream.read(buffer, readBytes, size - readBytes)
            if (r == -1) {
                throw IOException("End of stream reached")
            }
            readBytes += r
        }
        return buffer
    }

    // --- CBOR PACKET SERIALIZER / DESERIALIZER ---

    private fun encodeCborValue(value: Any?): DataItem {
        return when (value) {
            is String -> UnicodeString(value)
            is Number -> {
                val l = value.toLong()
                if (l < 0) NegativeInteger(l) else UnsignedInteger(l)
            }
            is Boolean -> if (value) SimpleValue.TRUE else SimpleValue.FALSE
            is ByteArray -> ByteString(value)
            is List<*> -> {
                val array = co.nstant.`in`.cbor.model.Array()
                array.setChunked(true)
                for (item in value) {
                    array.add(encodeCborValue(item))
                }
                array.add(Special.BREAK)
                array
            }
            is Map<*, *> -> {
                val map = co.nstant.`in`.cbor.model.Map()
                if (value.isNotEmpty()) {
                    map.setChunked(true)
                    for ((k, v) in value) {
                        val keyStr = k?.toString() ?: "null"
                        map.put(UnicodeString(keyStr), encodeCborValue(v))
                    }
                }
                map
            }
            null -> SimpleValue.NULL
            else -> UnicodeString(value.toString())
        }
    }

    private fun decodeCborValue(item: DataItem?): Any? {
        return when (item) {
            is UnicodeString -> item.string
            is UnsignedInteger -> item.value.toLong()
            is NegativeInteger -> item.value.toLong()
            is SimpleValue -> {
                when (item) {
                    SimpleValue.TRUE -> true
                    SimpleValue.FALSE -> false
                    SimpleValue.NULL -> null
                    else -> item.toString()
                }
            }
            is ByteString -> item.bytes
            is co.nstant.`in`.cbor.model.Array -> {
                item.getDataItems()
                    .filter { it != Special.BREAK }
                    .map { decodeCborValue(it) }
            }
            is co.nstant.`in`.cbor.model.Map -> {
                val map = mutableMapOf<String, Any?>()
                for (keyItem in item.keys) {
                    if (keyItem is UnicodeString) {
                        map[keyItem.string] = decodeCborValue(item.get(keyItem))
                    }
                }
                map
            }
            else -> item?.toString()
        }
    }

    fun encodePacket(packet: Packet): ByteArray {
        val baos = ByteArrayOutputStream()
        val builder = CborBuilder()
        val mapBuilder = builder.addMap()
        
        mapBuilder.put(UnicodeString("id"), UnsignedInteger(packet.id))
        mapBuilder.put(UnicodeString("type"), UnicodeString(packet.type))
        
        if (packet.body != null) {
            val bodyMap = co.nstant.`in`.cbor.model.Map()
            if (packet.body.isNotEmpty()) {
                bodyMap.setChunked(true)
                for ((key, value) in packet.body) {
                    bodyMap.put(UnicodeString(key), encodeCborValue(value))
                }
            }
            mapBuilder.put(UnicodeString("body"), bodyMap)
        } else {
            mapBuilder.put(UnicodeString("body"), SimpleValue.NULL)
        }
        
        mapBuilder.put(UnicodeString("has_payload"), if (packet.hasPayload) SimpleValue.TRUE else SimpleValue.FALSE)
        mapBuilder.end()
        
        CborEncoder(baos).encode(builder.build())
        return baos.toByteArray()
    }

    fun decodePacket(bytes: ByteArray): Packet? {
        try {
            val bais = ByteArrayInputStream(bytes)
            val decoded = CborDecoder(bais).decode()
            if (decoded.isEmpty()) return null
            val mainItem = decoded[0] as? co.nstant.`in`.cbor.model.Map ?: return null
            
            val idItem = mainItem.get(UnicodeString("id"))
            val id = when (idItem) {
                is UnsignedInteger -> idItem.value.toLong()
                is NegativeInteger -> idItem.value.toLong()
                else -> 0L
            }
            
            val typeItem = mainItem.get(UnicodeString("type")) as? UnicodeString ?: return null
            val type = typeItem.string
            
            val bodyItem = mainItem.get(UnicodeString("body"))
            val body = if (bodyItem is co.nstant.`in`.cbor.model.Map) {
                decodeCborValue(bodyItem) as? Map<String, Any?>
            } else null
            
            val hasPayloadItem = mainItem.get(UnicodeString("has_payload"))
            val hasPayload = hasPayloadItem == SimpleValue.TRUE
            
            return Packet(id, type, body, hasPayload)
        } catch (e: Exception) {
            e.printStackTrace()
        }
        return null
    }
}
