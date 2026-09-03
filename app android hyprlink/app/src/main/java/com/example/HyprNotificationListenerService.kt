package com.example

import android.app.Notification
import android.service.notification.NotificationListenerService
import android.service.notification.StatusBarNotification
import android.content.Context
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import java.util.concurrent.ConcurrentHashMap

class HyprNotificationListenerService : NotificationListenerService() {

    companion object {
        private const val TAG = "HyprNotificationListener"
        
        // Cache for active notifications so we can trigger actions or dismiss them
        val cachedNotifications = ConcurrentHashMap<String, StatusBarNotification>()
        
        // Reference to active listener service instance
        @Volatile
        var instance: HyprNotificationListenerService? = null
            private set
    }

    private val serviceScope = CoroutineScope(Dispatchers.Default + SupervisorJob())

    override fun onCreate() {
        super.onCreate()
        instance = this
        Log.d(TAG, "HyprNotificationListenerService created")
    }

    override fun onDestroy() {
        super.onDestroy()
        if (instance == this) {
            instance = null
        }
        Log.d(TAG, "HyprNotificationListenerService destroyed")
    }

    override fun onListenerConnected() {
        super.onListenerConnected()
        instance = this
        Log.d(TAG, "HyprNotificationListenerService connected")
        // Cache all currently active notifications
        try {
            val sbns = getActiveNotifications()
            cachedNotifications.clear()
            if (sbns != null) {
                for (sbn in sbns) {
                    cachedNotifications[sbn.key] = sbn
                }
            }
        } catch (e: Exception) {
            Log.e(TAG, "Error caching active notifications: ${e.message}")
        }
    }

    override fun onListenerDisconnected() {
        super.onListenerDisconnected()
        if (instance == this) {
            instance = null
        }
        cachedNotifications.clear()
        Log.d(TAG, "HyprNotificationListenerService disconnected")
    }

    override fun onNotificationPosted(sbn: StatusBarNotification) {
        super.onNotificationPosted(sbn)
        
        // Cache it
        cachedNotifications[sbn.key] = sbn

        // Check if mirroring is enabled
        val sharedPrefs = getSharedPreferences("hyprlink_prefs", Context.MODE_PRIVATE)
        val isMirrorEnabled = sharedPrefs.getBoolean("mirror_notifications", true)
        if (!isMirrorEnabled) {
            return
        }

        // Check active connection
        val activeConn = ConnectionRepository.activeConnection
        if (activeConn == null) {
            return
        }

        // Filtering
        val pkg = sbn.packageName
        // Ignore our own package
        if (pkg == packageName) {
            return
        }

        val notification = sbn.notification ?: return

        // Ignore ongoing low-value event notifications (FLAG_ONGOING_EVENT)
        val isOngoing = (notification.flags and Notification.FLAG_ONGOING_EVENT) != 0
        if (isOngoing) {
            return
        }

        // Ignore group summaries
        val isGroupSummary = (notification.flags and Notification.FLAG_GROUP_SUMMARY) != 0
        if (isGroupSummary) {
            return
        }

        // Extract metadata
        val title = notification.extras?.getCharSequence(Notification.EXTRA_TITLE)?.toString() ?: ""
        var rawText = notification.extras?.getCharSequence(Notification.EXTRA_TEXT)?.toString() ?: ""
        if (rawText.isBlank()) {
            val bigText = notification.extras?.getCharSequence(Notification.EXTRA_BIG_TEXT)?.toString()
            if (!bigText.isNullOrBlank()) {
                rawText = bigText
            }
        }
        val text = if (rawText.length > 2000) rawText.substring(0, 2000) else rawText

        // Resolve readable app name
        var appName = pkg
        try {
            val pm = packageManager
            val ai = pm.getApplicationInfo(pkg, 0)
            appName = pm.getApplicationLabel(ai).toString()
        } catch (e: Exception) {
            // fallback to package name
        }

        // Extract actions, including those with RemoteInput
        val rawActions = notification.actions
        val actionList = mutableListOf<Map<String, Any>>()
        if (rawActions != null) {
            for (i in rawActions.indices) {
                val action = rawActions[i] ?: continue
                val hasRemoteInput = action.remoteInputs != null && action.remoteInputs.isNotEmpty()
                actionList.add(mapOf(
                    "idx" to i,
                    "label" to (action.title?.toString() ?: "Ação"),
                    "is_reply" to hasRemoteInput
                ))
            }
        }

        // Post to PC
        val body = mapOf(
            "key" to sbn.key,
            "app" to appName,
            "title" to title,
            "text" to text,
            "actions" to actionList
        )

        serviceScope.launch {
            try {
                ConnectionRepository.sendOneWayPacket("notification.post", body)
                ConnectionRepository.incrementMirroredCount()
                ConnectionRepository.appendLog("[NOTIF] Mirrored notification from $appName: $title")
            } catch (e: Exception) {
                Log.e(TAG, "Error sending notification post: ${e.message}")
            }
        }
    }

    override fun onNotificationRemoved(sbn: StatusBarNotification) {
        super.onNotificationRemoved(sbn)
        
        cachedNotifications.remove(sbn.key)

        // Check if mirroring is enabled
        val sharedPrefs = getSharedPreferences("hyprlink_prefs", Context.MODE_PRIVATE)
        val isMirrorEnabled = sharedPrefs.getBoolean("mirror_notifications", true)
        if (!isMirrorEnabled) {
            return
        }

        // Check active connection
        val activeConn = ConnectionRepository.activeConnection ?: return

        val pkg = sbn.packageName
        if (pkg == packageName) {
            return
        }

        // Send dismiss notification to PC
        val body = mapOf("key" to sbn.key)
        serviceScope.launch {
            try {
                ConnectionRepository.sendOneWayPacket("notification.dismissed", body)
                ConnectionRepository.appendLog("[NOTIF] Notification dismissed: ${sbn.key}")
            } catch (e: Exception) {
                Log.e(TAG, "Error sending notification dismiss: ${e.message}")
            }
        }
    }
}
