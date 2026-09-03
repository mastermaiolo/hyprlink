package com.example

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import tech.kwik.core.QuicClientConnection

class HyprLinkConnectionService : Service() {

    private val CHANNEL_ID = "hyprlink_service_channel"
    private val NOTIFICATION_ID = 101

    private val serviceScope = CoroutineScope(Dispatchers.IO + SupervisorJob())
    private var connectionJob: Job? = null
    private var activeQuicConn: QuicClientConnection? = null
    private var lastWorkstationId: String? = null
    private val connectionGeneration = java.util.concurrent.atomic.AtomicInteger(0)

    private var connectivityManager: ConnectivityManager? = null
    private var networkCallback: ConnectivityManager.NetworkCallback? = null

    private var batteryReceiver: android.content.BroadcastReceiver? = null
    private var lastSentPct: Int = -1
    private var lastSentCharging: Boolean = false

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        registerNetworkCallback()
        registerBatteryReceiver()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val notification = createNotification("A iniciar ligação...")
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(NOTIFICATION_ID, notification, android.content.pm.ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }

        startConnectionLoop()
        return START_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? {
        return null
    }

    override fun onDestroy() {
        super.onDestroy()
        serviceScope.cancel()
        unregisterNetworkCallback()
        unregisterBatteryReceiver()
        closeConnection()
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                CHANNEL_ID,
                "HyprLink Connection Service",
                NotificationManager.IMPORTANCE_LOW
            ).apply {
                description = "Notification for active HyprLink connection in background"
            }
            val manager = getSystemService(NotificationManager::class.java)
            manager?.createNotificationChannel(channel)
        }
    }

    private fun createNotification(text: String): Notification {
        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("HyprLink Daemon Connection")
            .setContentText(text)
            .setSmallIcon(android.R.drawable.ic_menu_share)
            .setOngoing(true)
            .build()
    }

    private fun updateNotification(text: String) {
        val notification = createNotification(text)
        val manager = getSystemService(NotificationManager::class.java)
        manager?.notify(NOTIFICATION_ID, notification)
    }

    private fun startConnectionLoop() {
        val sharedPrefs = getSharedPreferences("hyprlink_prefs", Context.MODE_PRIVATE)
        val activeId = sharedPrefs.getString("active_workstation_id", "") ?: ""

        // If workstation ID hasn't changed, connection loop is active, and connection is established, do not interrupt
        if (activeId == lastWorkstationId && connectionJob?.isActive == true && activeQuicConn?.isConnected == true) {
            return
        }
        lastWorkstationId = activeId

        connectionJob?.cancel()
        connectionJob = serviceScope.launch {
            var backoffMs = 1000L
            val maxBackoffMs = 30000L

            while (isActive) {
                val db = AppDatabase.getDatabase(applicationContext)
                val dao = db.workstationDao()

                val currentActiveId = sharedPrefs.getString("active_workstation_id", "") ?: ""
                val ws = if (currentActiveId.isNotBlank()) {
                    dao.getById(currentActiveId)
                } else {
                    try {
                        dao.getAll().first().firstOrNull()
                    } catch (e: Exception) {
                        null
                    }
                }

                if (ws == null) {
                    updateNotification("A aguardar configuração de workstation...")
                    delay(5000)
                    continue
                }

                updateNotification("A tentar ligar a ${ws.deviceName}...")
                ConnectionRepository.setStatus(ConnectionStatus.CONNECTING)

                val success = try {
                    connectAndKeepAlive(ws)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    false
                }

                if (success) {
                    backoffMs = 1000L
                } else {
                    delay(backoffMs)
                    backoffMs = (backoffMs * 2).coerceAtMost(maxBackoffMs)
                }
            }
        }
    }

    private suspend fun connectAndKeepAlive(ws: PairedWorkstation): Boolean = withContext(Dispatchers.IO) {
        ConnectionRepository.setStatus(ConnectionStatus.CONNECTING)
        val myGeneration = connectionGeneration.incrementAndGet()
        var quicConn: QuicClientConnection? = null
        try {
            val portInt = ws.port.toIntOrNull() ?: ConnectionUtils.HYPRLINK_SERVICE_PORT

            // Load persisted local mTLS device identity
            val identity = DeviceIdentity.getInstance(applicationContext)

            quicConn = ConnectionUtils.connectPinned(
                host = ws.host,
                port = portInt,
                applicationProtocol = "hyprlink/1",
                expectedFingerprint = ws.fingerprint,
                identity = identity
            )

            if (myGeneration != connectionGeneration.get()) {
                quicConn?.close()
                return@withContext false // fui superado, não assumo o estado partilhado
            }

            if (!quicConn.isConnected) {
                return@withContext false
            }

            activeQuicConn = quicConn
            updateNotification("Ligado a ${ws.deviceName}")

            // Open stream and send hello
            val stream = quicConn.createStream(true)
            val sharedPrefs = getSharedPreferences("hyprlink_prefs", Context.MODE_PRIVATE)
            val localDeviceName = sharedPrefs.getString("local_device_name", "Android-Phone-Client") ?: "Android-Phone-Client"

            // Send core.hello with pairingToken = null since we are already paired
            val cborHello = buildHelloCbor(localDeviceName, null)
            val size = cborHello.size

            val frameHeader = byteArrayOf(
                ((size shr 24) and 0xFF).toByte(),
                ((size shr 16) and 0xFF).toByte(),
                ((size shr 8) and 0xFF).toByte(),
                (size and 0xFF).toByte()
            )

            val out = stream.outputStream
            out.write(frameHeader)
            out.write(cborHello)
            out.flush()

            // Read response
            val inp = stream.inputStream
            val header = ByteArray(4)
            var headerReadBytes = 0
            while (headerReadBytes < 4) {
                val r = ConnectionUtils.readWithDeadline(stream, 10_000) {
                    inp.read(header, headerReadBytes, 4 - headerReadBytes)
                }
                if (r == -1) {
                    quicConn.close()
                    return@withContext false
                }
                headerReadBytes += r
            }

            val responseSize = ((header[0].toInt() and 0xFF) shl 24) or
                               ((header[1].toInt() and 0xFF) shl 16) or
                               ((header[2].toInt() and 0xFF) shl 8) or
                               (header[3].toInt() and 0xFF)

            if (responseSize <= 0 || responseSize > 5 * 1024 * 1024) {
                quicConn.close()
                return@withContext false
            }

            val responseBody = ByteArray(responseSize)
            var bodyReadBytes = 0
            while (bodyReadBytes < responseSize) {
                val r = ConnectionUtils.readWithDeadline(stream, 10_000) {
                    inp.read(responseBody, bodyReadBytes, responseSize - bodyReadBytes)
                }
                if (r == -1) {
                    quicConn.close()
                    return@withContext false
                }
                bodyReadBytes += r
            }

            val workstationName = parseCoreHelloDeviceName(responseBody)
            if (workstationName == null) {
                quicConn.close()
                return@withContext false
            }

            // Connection handshake succeeded! Register with ConnectionRepository
            if (myGeneration != connectionGeneration.get()) {
                quicConn.close()
                return@withContext false
            }
            ConnectionRepository.registerActiveConnection(quicConn, applicationContext)
            updateNotification("Ligado a ${ws.deviceName}")
            ConnectionRepository.setStatus(ConnectionStatus.CONNECTED)
            ConnectionRepository.appendLog("[SERVICE] Successfully connected to workstation ${ws.deviceName} ($workstationName)")

            // Send initial battery status immediately after core.hello
            try {
                val batteryStatus: Intent? = applicationContext.registerReceiver(null, android.content.IntentFilter(Intent.ACTION_BATTERY_CHANGED))
                if (batteryStatus != null) {
                    val level = batteryStatus.getIntExtra(android.os.BatteryManager.EXTRA_LEVEL, -1)
                    val scale = batteryStatus.getIntExtra(android.os.BatteryManager.EXTRA_SCALE, 100)
                    val pct = if (scale > 0) (level * 100 / scale) else -1
                    val status = batteryStatus.getIntExtra(android.os.BatteryManager.EXTRA_STATUS, -1)
                    val charging = status == android.os.BatteryManager.BATTERY_STATUS_CHARGING ||
                                   status == android.os.BatteryManager.BATTERY_STATUS_FULL
                    
                    lastSentPct = pct
                    lastSentCharging = charging
                    
                    ConnectionRepository.sendPhoneBatteryState(pct, charging)
                }
            } catch (e: Exception) {
                ConnectionRepository.appendLog("[WARN] Initial phone battery send failed: ${e.message}")
            }

            // Connection handshake succeeded! Monitor connection with application-level keepalive
            var consecutiveFailures = 0
            var pingTimer = 0L
            while (isActive && quicConn.isConnected) {
                delay(1000)
                pingTimer += 1000
                if (pingTimer >= 10_000) {
                    pingTimer = 0
                    if (ConnectionRepository.activeConnection != null) {
                        val pingSuccess = ConnectionRepository.sendPing()
                        if (pingSuccess) {
                            consecutiveFailures = 0
                        } else {
                            consecutiveFailures++
                            ConnectionRepository.appendLog("[PING] Ping failed. Consecutive failures: $consecutiveFailures")
                            if (consecutiveFailures >= 2) {
                                ConnectionRepository.appendLog("[PING] 2 consecutive keepalive failures. Declaring connection dead.")
                                quicConn.close()
                                break
                            }
                        }
                    }
                }
            }

            quicConn.close()
            return@withContext true
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            e.printStackTrace()
            return@withContext false
        } finally {
            if (myGeneration == connectionGeneration.get()) {
                closeConnection(quicConn) // só limpo se ainda for a tentativa atual
            } else {
                try { quicConn?.close() } catch (e: Exception) {} // fecho a MINHA conexão sem tocar no estado partilhado
            }
        }
    }

    private fun closeConnection(connectionToClose: QuicClientConnection? = null) {
        try {
            if (connectionToClose != null) {
                connectionToClose.close()
                if (activeQuicConn === connectionToClose) {
                    activeQuicConn = null
                    ConnectionRepository.clearActiveConnection()
                }
            } else {
                activeQuicConn?.close()
                activeQuicConn = null
                ConnectionRepository.clearActiveConnection()
            }
        } catch (e: Exception) {
            // ignore
        }
    }

    private fun registerNetworkCallback() {
        connectivityManager = getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager
        networkCallback = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                super.onAvailable(network)
                forceReconnect()
            }

            override fun onLost(network: Network) {
                super.onLost(network)
                forceReconnect()
            }
        }

        val request = NetworkRequest.Builder()
            .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
            .build()

        try {
            networkCallback?.let {
                connectivityManager?.registerNetworkCallback(request, it)
            }
        } catch (e: Exception) {
            e.printStackTrace()
        }
    }

    private fun unregisterNetworkCallback() {
        try {
            networkCallback?.let {
                connectivityManager?.unregisterNetworkCallback(it)
            }
        } catch (e: Exception) {
            e.printStackTrace()
        }
        networkCallback = null
    }

    private fun forceReconnect() {
        startConnectionLoop()
    }

    private fun registerBatteryReceiver() {
        batteryReceiver = object : android.content.BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent) {
                if (intent.action == Intent.ACTION_BATTERY_CHANGED) {
                    val level = intent.getIntExtra(android.os.BatteryManager.EXTRA_LEVEL, -1)
                    val scale = intent.getIntExtra(android.os.BatteryManager.EXTRA_SCALE, 100)
                    val pct = if (scale > 0) (level * 100 / scale) else -1
                    val status = intent.getIntExtra(android.os.BatteryManager.EXTRA_STATUS, -1)
                    val charging = status == android.os.BatteryManager.BATTERY_STATUS_CHARGING ||
                                   status == android.os.BatteryManager.BATTERY_STATUS_FULL

                    if (pct != lastSentPct || charging != lastSentCharging) {
                        lastSentPct = pct
                        lastSentCharging = charging
                        serviceScope.launch {
                            try {
                                if (ConnectionRepository.activeConnection != null) {
                                    ConnectionRepository.sendPhoneBatteryState(pct, charging)
                                }
                            } catch (e: Exception) {
                                ConnectionRepository.appendLog("[WARN] Failed to send battery state change: ${e.message}")
                            }
                        }
                    }
                }
            }
        }
        val filter = android.content.IntentFilter(Intent.ACTION_BATTERY_CHANGED)
        registerReceiver(batteryReceiver, filter)
    }

    private fun unregisterBatteryReceiver() {
        try {
            batteryReceiver?.let {
                unregisterReceiver(it)
            }
        } catch (e: Exception) {
            e.printStackTrace()
        }
        batteryReceiver = null
    }
}
