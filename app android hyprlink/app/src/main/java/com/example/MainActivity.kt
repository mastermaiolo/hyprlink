package com.example

import android.Manifest
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.*
import androidx.compose.animation.core.*
import androidx.compose.foundation.*
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.LocalIndication
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.lifecycleScope
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.pointer.pointerInput
import com.example.ui.theme.MyApplicationTheme
import com.example.ui.theme.*
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.isActive
import java.security.MessageDigest

class MainActivity : ComponentActivity() {

    private var clipboardListener: ClipboardManager.OnPrimaryClipChangedListener? = null

    override fun onDestroy() {
        super.onDestroy()
        AudioStreamPlayer.stop()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        ConnectionRepository.initialize(applicationContext)
        enableEdgeToEdge()
        handleShareIntent(intent)
        
        setContent {
            MyApplicationTheme {
                Scaffold(
                    modifier = Modifier.fillMaxSize(),
                    containerColor = MaterialTheme.colorScheme.background
                ) { innerPadding ->
                    HyprLinkDashboard(
                        modifier = Modifier
                            .fillMaxSize()
                            .padding(innerPadding)
                    )
                }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleShareIntent(intent)
    }

    override fun onResume() {
        super.onResume()
        // Synchronize clipboard (foreground only)
        try {
            val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
            val listener = ClipboardManager.OnPrimaryClipChangedListener {
                if (clipboard.hasPrimaryClip()) {
                    val clipData = clipboard.primaryClip
                    if (clipData != null && clipData.itemCount > 0) {
                        val text = clipData.getItemAt(0).text?.toString() ?: ""
                        if (text.isNotEmpty() && text != ConnectionRepository.lastReceivedClipboardText) {
                            lifecycleScope.launch(Dispatchers.IO) {
                                try {
                                    ConnectionRepository.sendClipboardText(text)
                                } catch (e: Exception) {
                                    ConnectionRepository.appendLog("[CLIPBOARD] Error pushing: ${e.message}")
                                }
                            }
                        }
                    }
                }
            }
            clipboard.addPrimaryClipChangedListener(listener)
            clipboardListener = listener
        } catch (e: Exception) {
            ConnectionRepository.appendLog("[CLIPBOARD] Register failed: ${e.message}")
        }
    }

    override fun onPause() {
        super.onPause()
        clipboardListener?.let {
            val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
            clipboard.removePrimaryClipChangedListener(it)
        }
        clipboardListener = null
    }

    private fun handleShareIntent(intent: Intent?) {
        if (intent == null) return
        val action = intent.action
        val type = intent.type
        if (Intent.ACTION_SEND == action && type != null) {
            if ("text/plain" == type || type.startsWith("text/")) {
                val sharedText = intent.getStringExtra(Intent.EXTRA_TEXT)
                if (sharedText != null) {
                    lifecycleScope.launch {
                        delay(600)
                        if (ConnectionRepository.activeConnection != null) {
                            val isUrl = android.util.Patterns.WEB_URL.matcher(sharedText).matches() || android.webkit.URLUtil.isNetworkUrl(sharedText)
                            if (isUrl) {
                                ConnectionRepository.sendUrl(sharedText)
                                Toast.makeText(this@MainActivity, "URL partilhada!", Toast.LENGTH_SHORT).show()
                            } else {
                                ConnectionRepository.sendClipboardText(sharedText)
                                Toast.makeText(this@MainActivity, "Enviado para o clipboard da workstation!", Toast.LENGTH_SHORT).show()
                            }
                        } else {
                            Toast.makeText(this@MainActivity, "Nenhuma Workstation conectada!", Toast.LENGTH_LONG).show()
                            ConnectionRepository.appendLog("[SHARE] Share failed (offline): $sharedText")
                        }
                    }
                }
            } else {
                val uri = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
                } else {
                    @Suppress("DEPRECATION")
                    intent.getParcelableExtra(Intent.EXTRA_STREAM) as? Uri
                }
                if (uri != null) {
                    lifecycleScope.launch {
                        delay(600)
                        if (ConnectionRepository.activeConnection != null) {
                            val name = getFileName(uri) ?: "shared_file.bin"
                            val size = getFileSize(uri) ?: 0L
                            ConnectionRepository.sendFile(applicationContext, uri, name, size)
                        } else {
                            Toast.makeText(this@MainActivity, "Erro: Workstation offline!", Toast.LENGTH_LONG).show()
                        }
                    }
                }
            }
        }
    }

    private fun getFileName(uri: Uri): String? {
        var result: String? = null
        if (uri.scheme == "content") {
            val cursor = contentResolver.query(uri, null, null, null, null)
            try {
                if (cursor != null && cursor.moveToFirst()) {
                    val index = cursor.getColumnIndex(android.provider.OpenableColumns.DISPLAY_NAME)
                    if (index != -1) {
                        result = cursor.getString(index)
                    }
                }
            } finally {
                cursor?.close()
            }
        }
        if (result == null) {
            result = uri.path
            val cut = result?.lastIndexOf('/')
            if (cut != null && cut != -1) {
                result = result?.substring(cut + 1)
            }
        }
        return result
    }

    private fun getFileSize(uri: Uri): Long? {
        var result: Long? = null
        if (uri.scheme == "content") {
            val cursor = contentResolver.query(uri, null, null, null, null)
            try {
                if (cursor != null && cursor.moveToFirst()) {
                    val index = cursor.getColumnIndex(android.provider.OpenableColumns.SIZE)
                    if (index != -1) {
                        result = cursor.getLong(index)
                    }
                }
            } finally {
                cursor?.close()
            }
        }
        return result
    }
}

@Composable
fun HyprLinkDashboard(modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val sharedPrefs = remember { context.getSharedPreferences("hyprlink_prefs", Context.MODE_PRIVATE) }
    val coroutineScope = rememberCoroutineScope()

    val lifecycleOwner = androidx.lifecycle.compose.LocalLifecycleOwner.current
    var isBatteryOptIgnored by remember {
        val pm = context.getSystemService(Context.POWER_SERVICE) as? android.os.PowerManager
        mutableStateOf(pm?.isIgnoringBatteryOptimizations(context.packageName) ?: true)
    }
    var isDndGranted by remember {
        val nm = context.getSystemService(Context.NOTIFICATION_SERVICE) as? android.app.NotificationManager
        mutableStateOf(nm?.isNotificationPolicyAccessGranted ?: false)
    }
    var isNotifListenerGranted by remember {
        mutableStateOf(isNotificationListenerEnabled(context))
    }

    DisposableEffect(lifecycleOwner) {
        val observer = androidx.lifecycle.LifecycleEventObserver { _, event ->
            if (event == androidx.lifecycle.Lifecycle.Event.ON_RESUME) {
                val pm = context.getSystemService(Context.POWER_SERVICE) as? android.os.PowerManager
                isBatteryOptIgnored = pm?.isIgnoringBatteryOptimizations(context.packageName) ?: true
                val nm = context.getSystemService(Context.NOTIFICATION_SERVICE) as? android.app.NotificationManager
                isDndGranted = nm?.isNotificationPolicyAccessGranted ?: false
                isNotifListenerGranted = isNotificationListenerEnabled(context)
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
        }
    }

    var showInitialPermissionsDialog by remember { mutableStateOf(false) }
    var hasRequestedBatteryExemptionThisSession by rememberSaveable { mutableStateOf(false) }
    var showBatteryExemptionDialog by remember { mutableStateOf(false) }
    var hasRequestedDndThisSession by rememberSaveable { mutableStateOf(false) }
    var showDndDialog by remember { mutableStateOf(false) }

    fun checkAndTriggerInitialPermissions() {
        val hasSeen = sharedPrefs.getBoolean("has_seen_initial_permissions_dialog", false)
        if (!hasSeen && (!isBatteryOptIgnored || !isDndGranted || !isNotifListenerGranted)) {
            showInitialPermissionsDialog = true
        }
    }

    // Request all runtime permissions on startup (Android 13+ Notifs, Camera, Microphone)
    val permissionLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.RequestMultiplePermissions(),
        onResult = { _ ->
            checkAndTriggerInitialPermissions()
        }
    )

    LaunchedEffect(Unit) {
        val permissionsToRequest = mutableListOf<String>()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            if (ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
                permissionsToRequest.add(Manifest.permission.POST_NOTIFICATIONS)
            }
        }
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED) {
            permissionsToRequest.add(Manifest.permission.CAMERA)
        }
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            permissionsToRequest.add(Manifest.permission.RECORD_AUDIO)
        }
        if (permissionsToRequest.isNotEmpty()) {
            permissionLauncher.launch(permissionsToRequest.toTypedArray())
        } else {
            checkAndTriggerInitialPermissions()
        }
    }

    // Connect with ConnectionRepository observable flows
    val connStatus by ConnectionRepository.connectionStatus.collectAsStateWithLifecycle()
    val batteryLevel by ConnectionRepository.batteryLevel.collectAsStateWithLifecycle()
    val batteryCharging by ConnectionRepository.batteryCharging.collectAsStateWithLifecycle()
    val mediaState by ConnectionRepository.mediaState.collectAsStateWithLifecycle()
    val fileSendingProgress by ConnectionRepository.fileSendingProgress.collectAsStateWithLifecycle()
    val fileSendingStatus by ConnectionRepository.fileSendingStatus.collectAsStateWithLifecycle()
    val repositoryLogs by ConnectionRepository.logs.collectAsStateWithLifecycle()
    val isWebcamStreaming by WebcamStreamer.isStreaming.collectAsStateWithLifecycle()
    val lensLabel by WebcamStreamer.lensLabel.collectAsStateWithLifecycle()
    val rotationDeg by WebcamStreamer.rotationDeg.collectAsStateWithLifecycle()
    val mirrored by WebcamStreamer.mirrored.collectAsStateWithLifecycle()
    val codecPreference by WebcamStreamer.codecPreference.collectAsStateWithLifecycle()
    val activeCodec by WebcamStreamer.activeCodec.collectAsStateWithLifecycle()
    val isMicOn by WebcamStreamer.isMicOn.collectAsStateWithLifecycle()
    val isTorchOn by WebcamStreamer.isTorchOn.collectAsStateWithLifecycle()
    val zoomRatio by WebcamStreamer.zoomRatio.collectAsStateWithLifecycle()

    val micPermissionLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.RequestPermission(),
        onResult = { isGranted ->
            if (isGranted) {
                WebcamStreamer.startMic(context)
            } else {
                ConnectionRepository.appendLog("[WEBCAM] Permissão de microfone rejeitada pelo utilizador")
            }
        }
    )

    var isServiceActive by rememberSaveable {
        mutableStateOf(sharedPrefs.getBoolean("is_service_active", true))
    }

    // Room DB instances
    val db = remember { AppDatabase.getDatabase(context) }
    val repository = remember { WorkstationRepository(db.workstationDao()) }
    val workstations by repository.allWorkstations.collectAsStateWithLifecycle(initialValue = emptyList())

    var activeWorkstationId by rememberSaveable {
        mutableStateOf(sharedPrefs.getString("active_workstation_id", "") ?: "")
    }

    LaunchedEffect(workstations) {
        if (activeWorkstationId.isEmpty() && workstations.isNotEmpty()) {
            activeWorkstationId = workstations.first().id
        }
    }

    val activeWorkstation = workstations.find { it.id == activeWorkstationId } ?: workstations.firstOrNull()

    LaunchedEffect(isServiceActive, activeWorkstation) {
        val intent = Intent(context, HyprLinkConnectionService::class.java)
        val editor = sharedPrefs.edit()
        editor.putBoolean("is_service_active", isServiceActive)
        if (activeWorkstation != null) {
            editor.putString("active_workstation_id", activeWorkstation.id)
            editor.apply()
            if (isServiceActive) {
                ContextCompat.startForegroundService(context, intent)
                if (!hasRequestedBatteryExemptionThisSession && !showInitialPermissionsDialog) {
                    val pm = context.getSystemService(Context.POWER_SERVICE) as? android.os.PowerManager
                    val isIgnoring = pm?.isIgnoringBatteryOptimizations(context.packageName) ?: true
                    if (!isIgnoring && sharedPrefs.getBoolean("has_seen_initial_permissions_dialog", false)) {
                        showBatteryExemptionDialog = true
                    }
                }
            } else {
                context.stopService(intent)
            }
        } else {
            editor.putString("active_workstation_id", "")
            editor.apply()
            context.stopService(intent)
        }
    }

    val deviceName = activeWorkstation?.deviceName ?: "Sem Workstation"
    val deviceHost = activeWorkstation?.host ?: "0.0.0.0"
    val devicePort = activeWorkstation?.port ?: "----"
    val deviceFingerprint = activeWorkstation?.fingerprint ?: "Nenhum fingerprint configurado"

    // Dialog state
    var showNewPairDialog by remember { mutableStateOf(false) }
    var showIdentityDialog by remember { mutableStateOf(false) }
    var showUrlShareDialog by remember { mutableStateOf(false) }
    var showRegenerateConfirmDialog by remember { mutableStateOf(false) }
    var showShareChoiceDialog by remember { mutableStateOf(false) }
    var showMediaControlsDialog by remember { mutableStateOf(false) }
    var showWorkstationSelectorDialog by remember { mutableStateOf(false) }

    val filePickerLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.GetContent()
    ) { uri ->
        if (uri != null) {
            coroutineScope.launch {
                val name = getFileName(context, uri) ?: "file.bin"
                val size = getFileSize(context, uri) ?: 0L
                ConnectionRepository.sendFile(context, uri, name, size)
            }
        }
    }

    // Glow and pulse transition animations
    val infiniteTransition = rememberInfiniteTransition(label = "HyprGlowPulse")
    
    val pulseAlpha by infiniteTransition.animateFloat(
        initialValue = 0.4f,
        targetValue = 1.0f,
        animationSpec = infiniteRepeatable(
            animation = tween(1400, easing = FastOutSlowInEasing),
            repeatMode = RepeatMode.Reverse
        ),
        label = "GlowAlpha"
    )

    val pulseScale by infiniteTransition.animateFloat(
        initialValue = 0.95f,
        targetValue = 1.05f,
        animationSpec = infiniteRepeatable(
            animation = tween(2000, easing = LinearOutSlowInEasing),
            repeatMode = RepeatMode.Reverse
        ),
        label = "GlowScale"
    )

    val rotationDegrees by infiniteTransition.animateFloat(
        initialValue = 0f,
        targetValue = 360f,
        animationSpec = infiniteRepeatable(
            animation = tween(5000, easing = LinearEasing),
            repeatMode = RepeatMode.Restart
        ),
        label = "RotationDegrees"
    )

    var currentTab by rememberSaveable { mutableStateOf("dashboard") }
    var showFullScreenConsole by remember { mutableStateOf(false) }

    val linkColor by animateColorAsState(
        targetValue = when (connStatus) {
            ConnectionStatus.CONNECTED -> CyanActive
            ConnectionStatus.CONNECTING -> AmberWarning
            ConnectionStatus.DISCONNECTED -> RedError
        },
        animationSpec = tween(durationMillis = 300, easing = LinearOutSlowInEasing),
        label = "LinkColor"
    )

    BoxWithConstraints(modifier = modifier) {
        val isDesktopWidth = maxWidth > 800.dp
        
        if (isDesktopWidth) {
            DesktopMainLayout(
                connStatus = connStatus,
                deviceName = deviceName,
                isServiceActive = isServiceActive,
                onServiceActiveChange = { isServiceActive = it },
                mediaState = mediaState,
                fileSendingProgress = fileSendingProgress,
                fileSendingStatus = fileSendingStatus,
                repositoryLogs = repositoryLogs,
                pulseAlpha = pulseAlpha,
                pulseScale = pulseScale,
                rotationDegrees = rotationDegrees,
                onPickFile = { filePickerLauncher.launch("*/*") },
                onShowMediaControls = { showMediaControlsDialog = true },
                onShowWorkstationSelector = { showWorkstationSelectorDialog = true },
                onNavigateToTab = { currentTab = it },
                onSendClipboardText = { text ->
                    val clipboardManager = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                    if (clipboardManager.hasPrimaryClip()) {
                        val clipData = clipboardManager.primaryClip
                        if (clipData != null && clipData.itemCount > 0) {
                            val clipText = clipData.getItemAt(0).text?.toString() ?: ""
                            if (clipText.isNotEmpty()) {
                                coroutineScope.launch {
                                    try {
                                        ConnectionRepository.sendClipboardText(clipText)
                                        Toast.makeText(context, "Clip enviado!", Toast.LENGTH_SHORT).show()
                                    } catch (e: Exception) {
                                        Toast.makeText(context, "Erro: ${e.message}", Toast.LENGTH_SHORT).show()
                                    }
                                }
                            } else {
                                Toast.makeText(context, "Clip está vazio!", Toast.LENGTH_SHORT).show()
                            }
                        }
                    } else {
                        Toast.makeText(context, "Sem conteúdo para copiar.", Toast.LENGTH_SHORT).show()
                    }
                },
                onExecuteCommand = { cmd ->
                    if (connStatus != ConnectionStatus.CONNECTED) {
                        Toast.makeText(context, "Comando '$cmd' simulado (Sem Workstation conectada)", Toast.LENGTH_SHORT).show()
                    } else {
                        coroutineScope.launch {
                            try {
                                ConnectionRepository.appendLog("[HYPRLAND] Executar comando rápido: $cmd")
                                val success = ConnectionRepository.dispatchHyprCommand(cmd)
                                if (success) {
                                    Toast.makeText(context, "Comando '$cmd' enviado com sucesso!", Toast.LENGTH_SHORT).show()
                                } else {
                                    Toast.makeText(context, "Falha ao executar comando rápido", Toast.LENGTH_SHORT).show()
                                }
                            } catch (e: Exception) {
                                Toast.makeText(context, "Erro: ${e.message}", Toast.LENGTH_SHORT).show()
                            }
                        }
                    }
                },
                currentTab = currentTab,
                context = context
            )
        } else {
            // Main layout container with Scaffold for Navigation (Mobile only)
            val isAudioStreamingPhone by AudioStreamPlayer.isPlaying.collectAsStateWithLifecycle()
            val transfersList by ConnectionRepository.transfers.collectAsStateWithLifecycle()

            var audioSinks by remember {
                mutableStateOf(
                    listOf(
                        com.example.ui.AudioSinkItem(1L, "Speakers", "Realtek ALC897 (analógico)", 74, false, true, false),
                        com.example.ui.AudioSinkItem(2L, "Headphones", "PipeWire Bluetooth Sink", 50, false, false, false),
                        com.example.ui.AudioSinkItem(3L, "Telefone", "HyprLink Audio Stream (este dispositivo)", 80, false, false, true)
                    )
                )
            }

            var audioApps by remember {
                mutableStateOf(
                    listOf(
                        com.example.ui.AudioAppItem(101L, "Spotify", mediaState?.title ?: "Nenhuma faixa", 82, false, mediaState?.status.equals("playing", ignoreCase = true), 0),
                        com.example.ui.AudioAppItem(102L, "Firefox", "YouTube - lofi hip hop radio", 65, false, true, 0),
                        com.example.ui.AudioAppItem(103L, "Discord", "Canal de Voz", 90, false, false, 4)
                    )
                )
            }

            LaunchedEffect(currentTab, connStatus) {
                if (connStatus == ConnectionStatus.CONNECTED && currentTab == "audio") {
                    try {
                        val fetched = ConnectionRepository.fetchAudioState()
                        if (fetched.sinks.isNotEmpty()) {
                            audioSinks = fetched.sinks.map { s ->
                                com.example.ui.AudioSinkItem(
                                    id = s.id,
                                    name = s.name,
                                    description = s.description,
                                    volume = s.volume,
                                    isMuted = s.muted,
                                    isDefault = s.is_default,
                                    isPhone = s.is_phone
                                )
                            }
                        }
                        if (fetched.apps.isNotEmpty()) {
                            audioApps = fetched.apps.map { a ->
                                com.example.ui.AudioAppItem(
                                    id = a.id,
                                    name = a.name,
                                    media = a.media ?: "",
                                    volume = a.volume,
                                    isMuted = a.muted,
                                    isPlaying = true,
                                    idleMinutes = 0
                                )
                            }
                        }
                    } catch (e: Exception) {}
                }
            }

            var mcWorkspaces by remember {
                mutableStateOf(
                    listOf(
                        com.example.ui.WorkspaceItem(1, "1", 2, true),
                        com.example.ui.WorkspaceItem(2, "2", 1, false),
                        com.example.ui.WorkspaceItem(3, "3", 1, false),
                        com.example.ui.WorkspaceItem(4, "4", 0, false),
                        com.example.ui.WorkspaceItem(5, "5", 0, false)
                    )
                )
            }

            var mcWindows by remember {
                mutableStateOf(
                    listOf(
                        com.example.ui.WindowClientItem("0x555555abcd10", "Firefox — Hyprland Wiki", "firefox", 1, true),
                        com.example.ui.WindowClientItem("0x555555abcd20", "kitty ~ terminal", "kitty", 1, false),
                        com.example.ui.WindowClientItem("0x555555abcd30", "Visual Studio Code — main.rs", "code", 2, false),
                        com.example.ui.WindowClientItem("0x555555abcd40", "Spotify Premium", "spotify", 3, false)
                    )
                )
            }

            val currentLinkState = when (connStatus) {
                ConnectionStatus.CONNECTED -> com.example.ui.theme.LinkState.CONNECTED
                ConnectionStatus.CONNECTING -> com.example.ui.theme.LinkState.NEGOTIATING
                ConnectionStatus.DISCONNECTED -> com.example.ui.theme.LinkState.DISCONNECTED
            }

            val isMediaPlaying = mediaState?.status.equals("playing", ignoreCase = true)
            val mediaProgress = if (isMediaPlaying) 0.42f else 0.0f
            val mediaElapsedTime = if (isMediaPlaying) "1:42" else "0:00"
            val mediaTotalTime = "3:58"
            val clipboardText = ConnectionRepository.lastReceivedClipboardText

            val activeTransfers = remember(transfersList) {
                transfersList.filter { it.status == TransferStatus.EM_CURSO }.map {
                    com.example.ui.ActiveTransferItem(
                        id = it.id.toString(),
                        name = it.name,
                        isIncoming = it.direction == TransferDirection.DOWNLOAD,
                        currentBytes = (it.size * it.progress).toLong(),
                        totalBytes = it.size,
                        progress = it.progress
                    )
                }
            }
            val completedTransfers = remember(transfersList) {
                transfersList.filter { it.status == TransferStatus.VERIFICADO || it.status == TransferStatus.NAO_VERIFICADO }.map {
                    com.example.ui.CompletedTransferItem(
                        id = it.id.toString(),
                        name = it.name,
                        isIncoming = it.direction == TransferDirection.DOWNLOAD,
                        totalBytes = it.size,
                        dateLabel = "Hoje",
                        sha256Hash = it.sha256Local ?: "SHA-256 verificado",
                        isVerified = it.status == TransferStatus.VERIFICADO
                    )
                }
            }
            val errorTransfers = remember(transfersList) {
                transfersList.filter { it.status == TransferStatus.ERRO }.map {
                    com.example.ui.ErrorTransferItem(
                        id = it.id.toString(),
                        name = it.name,
                        isIncoming = it.direction == TransferDirection.DOWNLOAD,
                        totalBytes = it.size,
                        failedPercentage = (it.progress * 100).toInt(),
                        technicalMessage = it.error ?: "Stream reset",
                        friendlyExplanation = "Falha de rede durante a transferência"
                    )
                }
            }

            Scaffold(
                modifier = Modifier.fillMaxSize(),
                containerColor = HyprColors.Background,
                bottomBar = {
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .height(56.dp)
                            .background(HyprColors.Background)
                            .drawBehind {
                                drawLine(
                                    color = HyprColors.BorderNormal,
                                    start = Offset(0f, 0f),
                                    end = Offset(size.width, 0f),
                                    strokeWidth = 1.dp.toPx()
                                )
                            }
                            .testTag("bottom_nav_bar"),
                        horizontalArrangement = Arrangement.SpaceEvenly,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        val navItems = listOf(
                            Triple("dashboard", Icons.Default.Terminal, "INÍCIO"),
                            Triple("audio", Icons.Default.PlayArrow, "MÉDIA"),
                            Triple("touchpad", Icons.Default.Keyboard, "RATO"),
                            Triple("desktop", Icons.Default.Monitor, "AÇÕES"),
                            Triple("transfers", Icons.Default.SwapVert, "FICH."),
                            Triple("notifications", Icons.Default.Settings, "CONFIG")
                        )

                        navItems.forEach { (route, icon, label) ->
                            val isSelected = currentTab == route
                            val itemColor = if (isSelected) HyprColors.NeonGreen else HyprColors.InactiveGray

                            Column(
                                modifier = Modifier
                                    .weight(1f)
                                    .fillMaxHeight()
                                    .clickable { currentTab = route },
                                horizontalAlignment = Alignment.CenterHorizontally,
                                verticalArrangement = Arrangement.Center
                            ) {
                                Icon(
                                    imageVector = icon,
                                    contentDescription = label,
                                    tint = itemColor,
                                    modifier = Modifier.size(20.dp)
                                )
                                Spacer(modifier = Modifier.height(3.dp))
                                Text(
                                    text = label,
                                    style = MaterialTheme.typography.labelSmall.copy(
                                        fontSize = 9.sp,
                                        fontWeight = FontWeight.Bold,
                                        fontFamily = JetBrainsMono
                                    ),
                                    color = itemColor
                                )
                                Spacer(modifier = Modifier.height(3.dp))
                                Box(
                                    modifier = Modifier
                                        .width(16.dp)
                                        .height(2.dp)
                                        .background(if (isSelected) HyprColors.NeonGreen else Color.Transparent, CircleShape)
                                )
                            }
                        }
                    }
                }
            ) { innerPadding ->
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .padding(innerPadding)
                        .background(HyprColors.Background)
                ) {
                    when (currentTab) {
                        "dashboard" -> {
                            com.example.ui.DashboardScreen(
                                linkState = currentLinkState,
                                deviceName = deviceName,
                                onDeviceClick = { showWorkstationSelectorDialog = true },
                                onStateButtonClick = { isServiceActive = !isServiceActive },
                                mediaTitle = mediaState?.title ?: "Nenhuma faixa a reproduzir",
                                mediaArtist = mediaState?.artist ?: "Hyprland Audio Sink",
                                isPlaying = isMediaPlaying,
                                onPrevClick = { coroutineScope.launch { ConnectionRepository.sendMediaCommand("previous") } },
                                onPlayPauseClick = { coroutineScope.launch { ConnectionRepository.sendMediaCommand("playpause") } },
                                onNextClick = { coroutineScope.launch { ConnectionRepository.sendMediaCommand("next") } },
                                progress = mediaProgress,
                                elapsedTime = mediaElapsedTime,
                                totalTime = mediaTotalTime,
                                clipboardContent = clipboardText,
                                clipboardStateText = if (clipboardText.isNotEmpty()) "sincronizado há pouco" else "vazio",
                                clipboardStateColor = if (clipboardText.isNotEmpty()) HyprColors.NeonGreen else HyprColors.InactiveGray,
                                onSendClipboardClick = {
                                    val clipboardManager = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                                    if (clipboardManager.hasPrimaryClip()) {
                                        val clipData = clipboardManager.primaryClip
                                        if (clipData != null && clipData.itemCount > 0) {
                                            val clipText = clipData.getItemAt(0).text?.toString() ?: ""
                                            if (clipText.isNotEmpty()) {
                                                coroutineScope.launch {
                                                    try {
                                                        ConnectionRepository.sendClipboardText(clipText)
                                                        Toast.makeText(context, "Clip enviado!", Toast.LENGTH_SHORT).show()
                                                    } catch (e: Exception) {
                                                        Toast.makeText(context, "Erro: ${e.message}", Toast.LENGTH_SHORT).show()
                                                    }
                                                }
                                            }
                                        }
                                    }
                                },
                                onReceiveClipboardClick = {
                                    val text = ConnectionRepository.lastReceivedClipboardText
                                    if (text.isNotEmpty()) {
                                        val clipboardManager = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                                        clipboardManager.setPrimaryClip(ClipData.newPlainText("HyprLink", text))
                                        Toast.makeText(context, "Copiado para o Android!", Toast.LENGTH_SHORT).show()
                                    }
                                },
                                onSendFilesClick = { filePickerLauncher.launch("*/*") },
                                onWebcamClick = {
                                    if (isWebcamStreaming) {
                                        WebcamStreamer.stop()
                                    } else {
                                        Toast.makeText(context, "Aguarde solicitação da câmara pelo PC", Toast.LENGTH_SHORT).show()
                                    }
                                },
                                webcamActive = isWebcamStreaming,
                                controlService = if (currentLinkState == com.example.ui.theme.LinkState.CONNECTED) com.example.ui.theme.ServiceState.ACTIVE else com.example.ui.theme.ServiceState.INACTIVE,
                                webcamService = if (isWebcamStreaming) com.example.ui.theme.ServiceState.ACTIVE else com.example.ui.theme.ServiceState.INACTIVE,
                                micBridgeService = if (isMicOn) com.example.ui.theme.ServiceState.ACTIVE else com.example.ui.theme.ServiceState.INACTIVE,
                                notifsService = com.example.ui.theme.ServiceState.ACTIVE,
                                cpuUsage = 14,
                                ramUsageGB = 6,
                                tempCelsius = 43,
                                batteryPercent = batteryLevel ?: 100,
                                onDetailClick = { currentTab = "desktop" },
                                terminalLogs = repositoryLogs,
                                modifier = Modifier.fillMaxSize()
                            )
                        }
                        "audio" -> {
                            com.example.ui.AudioScreen(
                                onBack = { currentTab = "dashboard" },
                                mediaAppSource = "Spotify",
                                mediaTitle = mediaState?.title ?: "Nenhuma faixa a reproduzir",
                                mediaArtist = mediaState?.artist ?: "Hyprland Audio Sink",
                                elapsedTime = mediaElapsedTime,
                                totalTime = mediaTotalTime,
                                isPlaying = isMediaPlaying,
                                onPrevClick = { coroutineScope.launch { ConnectionRepository.sendMediaCommand("previous") } },
                                onPlayPauseClick = { coroutineScope.launch { ConnectionRepository.sendMediaCommand("playpause") } },
                                onNextClick = { coroutineScope.launch { ConnectionRepository.sendMediaCommand("next") } },
                                isListenOnPhone = isAudioStreamingPhone,
                                onToggleListenOnPhone = { enable ->
                                    if (enable) {
                                        coroutineScope.launch {
                                            val tapId = ConnectionRepository.startAudioTap()
                                            if (tapId == null) {
                                                Toast.makeText(context, "Erro ao iniciar áudio no telemóvel", Toast.LENGTH_SHORT).show()
                                            }
                                        }
                                    } else {
                                        coroutineScope.launch {
                                            ConnectionRepository.stopAudioTap()
                                            AudioStreamPlayer.stop()
                                        }
                                    }
                                },
                                sinks = audioSinks,
                                onSelectDefaultSink = { sinkName ->
                                    coroutineScope.launch { ConnectionRepository.setAudioDefaultSink(sinkName) }
                                },
                                onSinkVolumeChange = { sinkId, vol ->
                                    audioSinks = audioSinks.map { if (it.id == sinkId) it.copy(volume = vol) else it }
                                    coroutineScope.launch { ConnectionRepository.setAudioVolume("sink", sinkId, vol) }
                                },
                                onSinkMuteToggle = { sinkId, mute ->
                                    audioSinks = audioSinks.map { if (it.id == sinkId) it.copy(isMuted = mute) else it }
                                    coroutineScope.launch { ConnectionRepository.setAudioMute("sink", sinkId, mute) }
                                },
                                apps = audioApps,
                                onAppVolumeChange = { appId, vol ->
                                    audioApps = audioApps.map { if (it.id == appId) it.copy(volume = vol) else it }
                                    coroutineScope.launch { ConnectionRepository.setAudioVolume("app", appId, vol) }
                                },
                                onAppMuteToggle = { appId, mute ->
                                    audioApps = audioApps.map { if (it.id == appId) it.copy(isMuted = mute) else it }
                                    coroutineScope.launch { ConnectionRepository.setAudioMute("app", appId, mute) }
                                },
                                modifier = Modifier.fillMaxSize()
                            )
                        }
                        "touchpad" -> {
                            com.example.ui.TouchpadScreen(
                                deviceName = deviceName,
                                onMouseMove = { dx, dy ->
                                    coroutineScope.launch { ConnectionRepository.sendInputMove((dx * 1.5f).toInt(), (dy * 1.5f).toInt()) }
                                },
                                onMouseClick = { btn ->
                                    val btnStr = when (btn) {
                                        2 -> "middle"
                                        3 -> "right"
                                        else -> "left"
                                    }
                                    coroutineScope.launch { ConnectionRepository.sendInputClick(btnStr) }
                                },
                                onScroll = { dx, dy ->
                                    coroutineScope.launch { ConnectionRepository.sendInputScroll(dx.toInt(), dy.toInt()) }
                                },
                                onSendKey = { key ->
                                    coroutineScope.launch { ConnectionRepository.sendInputKey(key) }
                                },
                                onSendText = { text ->
                                    coroutineScope.launch { ConnectionRepository.sendInputType(text) }
                                },
                                onToggleKeyboardDialog = {
                                    // toggle software keyboard
                                },
                                modifier = Modifier.fillMaxSize()
                            )
                        }
                        "desktop" -> {
                            com.example.ui.MissionControlScreen(
                                workspaces = mcWorkspaces,
                                windows = mcWindows,
                                onSelectWorkspace = { wsId ->
                                    mcWorkspaces = mcWorkspaces.map { it.copy(isActive = it.id == wsId) }
                                    coroutineScope.launch { ConnectionRepository.dispatchHyprCommand("workspace $wsId") }
                                },
                                onLaunchApp = { appName ->
                                    coroutineScope.launch { ConnectionRepository.dispatchHyprCommand("exec $appName") }
                                },
                                onCloseWindow = { addr ->
                                    mcWindows = mcWindows.filter { it.address != addr }
                                    coroutineScope.launch { ConnectionRepository.dispatchHyprCommand("closewindow address:$addr") }
                                },
                                onFocusWindow = { addr ->
                                    mcWindows = mcWindows.map { it.copy(isFocused = it.address == addr) }
                                    coroutineScope.launch { ConnectionRepository.dispatchHyprCommand("focuswindow address:$addr") }
                                },
                                onRefresh = {
                                    coroutineScope.launch {
                                        if (connStatus == ConnectionStatus.CONNECTED) {
                                            try {
                                                ConnectionRepository.dispatchHyprCommand("j/workspaces")
                                            } catch (e: Exception) {}
                                        }
                                    }
                                },
                                modifier = Modifier.fillMaxSize()
                            )
                        }
                        "transfers" -> {
                            com.example.ui.TransfersScreen(
                                onBack = { currentTab = "dashboard" },
                                activeTransfers = activeTransfers,
                                completedTransfers = completedTransfers,
                                errorTransfers = errorTransfers,
                                onSendFileClick = { filePickerLauncher.launch("*/*") },
                                onRetryTransfer = { id ->
                                    filePickerLauncher.launch("*/*")
                                },
                                onDeleteErrorTransfer = { id ->
                                    ConnectionRepository.clearTransfers()
                                },
                                onClearHistory = {
                                    ConnectionRepository.clearPersistentHistory(context)
                                    ConnectionRepository.clearTransfers()
                                },
                                modifier = Modifier.fillMaxSize()
                            )
                        }
                        "notifications" -> {
                            NotificationsSettingsScreen(
                                onBack = { currentTab = "dashboard" },
                                modifier = Modifier.fillMaxSize()
                            )
                        }
                    }
                }
            }
        }
    }

        // --- DIALOGS REGISTRATION ---

    if (isWebcamStreaming) {
        val currentView = androidx.compose.ui.platform.LocalView.current
        DisposableEffect(Unit) {
            currentView.keepScreenOn = true
            onDispose {
                currentView.keepScreenOn = false
            }
        }

        androidx.compose.ui.window.Dialog(
            onDismissRequest = { /* Cannot dismiss via tap, must use Stop button */ },
            properties = androidx.compose.ui.window.DialogProperties(usePlatformDefaultWidth = false)
        ) {
            var previewView by remember { mutableStateOf<androidx.camera.view.PreviewView?>(null) }
            var isScreenOff by remember { mutableStateOf(false) }
            var currentZoom by remember { mutableStateOf(1.0f) }

            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .background(Color.Black)
            ) {
                // Full-screen camera preview with pinch-to-zoom and tap-to-focus
                androidx.compose.ui.viewinterop.AndroidView(
                    factory = { ctx ->
                        androidx.camera.view.PreviewView(ctx).also { previewView = it }.apply {
                            implementationMode = androidx.camera.view.PreviewView.ImplementationMode.COMPATIBLE
                            WebcamStreamer.attachPreview(this.surfaceProvider)
                        }
                    },
                    onRelease = {
                        previewView = null
                        WebcamStreamer.detachPreview()
                    },
                    modifier = Modifier
                        .fillMaxSize()
                        .pointerInput(Unit) {
                            detectTransformGestures { _, _, zoom, _ ->
                                currentZoom = (currentZoom * zoom).coerceIn(1.0f, 10.0f)
                                WebcamStreamer.setZoomRatio(currentZoom)
                            }
                        }
                        .pointerInput(Unit) {
                            detectTapGestures(
                                onTap = { offset ->
                                    val point = previewView?.meteringPointFactory?.createPoint(offset.x, offset.y)
                                    if (point != null) {
                                        WebcamStreamer.focusAt(point)
                                    }
                                }
                            )
                        }
                )

                // Top floating status pill (avoids notch and camera hole)
                Row(
                    modifier = Modifier
                        .statusBarsPadding()
                        .padding(top = 20.dp, start = 20.dp, end = 20.dp)
                        .fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Row(
                        modifier = Modifier
                            .clip(RoundedCornerShape(20.dp))
                            .background(Color.Black.copy(alpha = 0.6f))
                            .border(
                                1.dp,
                                Color.Red.copy(alpha = 0.4f),
                                RoundedCornerShape(20.dp)
                            )
                            .padding(horizontal = 14.dp, vertical = 7.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp)
                    ) {
                        val blinkingTransition = rememberInfiniteTransition(label = "RecBlink")
                        val recAlpha by blinkingTransition.animateFloat(
                            initialValue = 0.2f,
                            targetValue = 1f,
                            animationSpec = infiniteRepeatable(
                                animation = tween(800, easing = LinearEasing),
                                repeatMode = RepeatMode.Reverse
                            ),
                            label = "RecAlpha"
                        )
                        Box(
                            modifier = Modifier
                                .size(8.dp)
                                .clip(CircleShape)
                                .background(Color.Red.copy(alpha = recAlpha))
                        )
                        Text(
                            text = "EMISSÃO ATIVA",
                            color = Color.White,
                            style = MaterialTheme.typography.labelSmall,
                            fontFamily = JetBrainsMono,
                            fontWeight = FontWeight.Bold
                        )
                    }

                    Row(
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        // Torch quick toggle button
                        IconButton(
                            onClick = { WebcamStreamer.toggleTorch(!isTorchOn) },
                            modifier = Modifier
                                .clip(CircleShape)
                                .background(if (isTorchOn) Color(0xFFFFD700).copy(alpha = 0.3f) else Color.Black.copy(alpha = 0.6f))
                                .border(1.dp, if (isTorchOn) Color(0xFFFFD700) else Color.White.copy(alpha = 0.15f), CircleShape)
                                .size(36.dp)
                        ) {
                            Icon(
                                imageVector = if (isTorchOn) Icons.Default.FlashOn else Icons.Default.FlashOff,
                                contentDescription = "Alternar Lanterna",
                                tint = if (isTorchOn) Color(0xFFFFD700) else Color.White,
                                modifier = Modifier.size(18.dp)
                            )
                        }

                        // Screen-off mode toggle button
                        IconButton(
                            onClick = { isScreenOff = true },
                            modifier = Modifier
                                .clip(CircleShape)
                                .background(Color.Black.copy(alpha = 0.6f))
                                .border(1.dp, Color.White.copy(alpha = 0.15f), CircleShape)
                                .size(36.dp)
                        ) {
                            Icon(
                                imageVector = Icons.Default.Brightness2,
                                contentDescription = "Apagar Ecrã",
                                tint = Color.White,
                                modifier = Modifier.size(18.dp)
                            )
                        }

                        Row(
                            modifier = Modifier
                                .clip(RoundedCornerShape(20.dp))
                                .background(Color.Black.copy(alpha = 0.6f))
                                .border(1.dp, Color.White.copy(alpha = 0.15f), RoundedCornerShape(20.dp))
                                .padding(horizontal = 14.dp, vertical = 7.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(6.dp)
                        ) {
                            Icon(
                                imageVector = Icons.Default.Videocam,
                                contentDescription = "Webcam Status",
                                tint = CyanActive,
                                modifier = Modifier.size(14.dp)
                            )
                            Text(
                                text = lensLabel,
                                color = Color.White,
                                style = MaterialTheme.typography.labelSmall,
                                fontFamily = JetBrainsMono,
                                fontWeight = FontWeight.Bold
                            )
                        }
                    }
                }

                // Bottom floating control panel (translucent glassmorphic style, lifted safely above bottom gestures)
                Box(
                    modifier = Modifier
                        .align(Alignment.BottomCenter)
                        .navigationBarsPadding()
                        .padding(bottom = 56.dp, start = 20.dp, end = 20.dp)
                        .fillMaxWidth()
                        .widthIn(max = 500.dp)
                ) {
                    Column(
                        modifier = Modifier
                            .clip(RoundedCornerShape(24.dp))
                            .background(Color(0xE6050505))
                            .border(1.dp, Color.White.copy(alpha = 0.12f), RoundedCornerShape(24.dp))
                            .padding(20.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.spacedBy(16.dp)
                    ) {
                        Text(
                            text = "CÂMARA ATIVA · $activeCodec",
                            style = MaterialTheme.typography.titleSmall,
                            fontFamily = ArchivoBlack,
                            color = Color.White.copy(alpha = 0.9f)
                        )

                        // Scrollable horizontal row of camera & audio settings
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .horizontalScroll(rememberScrollState()),
                            horizontalArrangement = Arrangement.spacedBy(10.dp)
                        ) {
                            // 1. Lens Selection Button
                            Button(
                                onClick = { WebcamStreamer.nextLens() },
                                colors = ButtonDefaults.buttonColors(
                                    containerColor = Color.White.copy(alpha = 0.08f),
                                    contentColor = Color.White
                                ),
                                border = BorderStroke(1.dp, Color.White.copy(alpha = 0.25f)),
                                shape = RoundedCornerShape(12.dp),
                                modifier = Modifier.height(48.dp)
                            ) {
                                Text(
                                    text = "🔄 $lensLabel",
                                    fontFamily = JetBrainsMono,
                                    fontWeight = FontWeight.Bold,
                                    fontSize = 11.sp,
                                    maxLines = 1
                                )
                            }

                            // 2. Rotate 90 deg Button
                            Button(
                                onClick = { WebcamStreamer.rotate90() },
                                colors = ButtonDefaults.buttonColors(
                                    containerColor = Color.White.copy(alpha = 0.08f),
                                    contentColor = Color.White
                                ),
                                border = BorderStroke(1.dp, Color.White.copy(alpha = 0.25f)),
                                shape = RoundedCornerShape(12.dp),
                                modifier = Modifier.height(48.dp)
                            ) {
                                Text(
                                    text = "📐 ROTAÇÃO: ${rotationDeg}°",
                                    fontFamily = JetBrainsMono,
                                    fontWeight = FontWeight.Bold,
                                    fontSize = 11.sp,
                                    maxLines = 1
                                )
                            }

                            // 3. Mirror toggle Button
                            Button(
                                onClick = { WebcamStreamer.toggleMirror() },
                                colors = ButtonDefaults.buttonColors(
                                    containerColor = if (mirrored) CyanActive.copy(alpha = 0.15f) else Color.White.copy(alpha = 0.08f),
                                    contentColor = if (mirrored) CyanActive else Color.White
                                ),
                                border = BorderStroke(
                                    1.dp,
                                    if (mirrored) CyanActive.copy(alpha = 0.7f) else Color.White.copy(alpha = 0.25f)
                                ),
                                shape = RoundedCornerShape(12.dp),
                                modifier = Modifier.height(48.dp)
                            ) {
                                Text(
                                    text = if (mirrored) "🪞 ESPELHADO" else "🪞 ESPELHAR",
                                    fontFamily = JetBrainsMono,
                                    fontWeight = FontWeight.Bold,
                                    fontSize = 11.sp,
                                    maxLines = 1
                                )
                            }

                            // 4. Torch Button
                            Button(
                                onClick = { WebcamStreamer.toggleTorch(!isTorchOn) },
                                colors = ButtonDefaults.buttonColors(
                                    containerColor = if (isTorchOn) Color(0xFFFFD700).copy(alpha = 0.15f) else Color.White.copy(alpha = 0.08f),
                                    contentColor = if (isTorchOn) Color(0xFFFFD700) else Color.White
                                ),
                                border = BorderStroke(
                                    1.dp,
                                    if (isTorchOn) Color(0xFFFFD700).copy(alpha = 0.7f) else Color.White.copy(alpha = 0.25f)
                                ),
                                shape = RoundedCornerShape(12.dp),
                                modifier = Modifier.height(48.dp)
                            ) {
                                Text(
                                    text = if (isTorchOn) "🔦 LANTERNA: ON" else "🔦 LANTERNA: OFF",
                                    fontFamily = JetBrainsMono,
                                    fontWeight = FontWeight.Bold,
                                    fontSize = 11.sp,
                                    maxLines = 1
                                )
                            }

                            // 5. Microphone toggle Button
                            Button(
                                onClick = {
                                    if (isMicOn) {
                                        WebcamStreamer.stopMic()
                                    } else {
                                        if (ContextCompat.checkSelfPermission(context, android.Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED) {
                                            WebcamStreamer.startMic(context)
                                        } else {
                                            micPermissionLauncher.launch(android.Manifest.permission.RECORD_AUDIO)
                                        }
                                    }
                                },
                                colors = ButtonDefaults.buttonColors(
                                    containerColor = if (isMicOn) Color(0xFF39FF9C).copy(alpha = 0.15f) else Color.White.copy(alpha = 0.08f),
                                    contentColor = if (isMicOn) Color(0xFF39FF9C) else Color.White
                                ),
                                border = BorderStroke(
                                    1.dp,
                                    if (isMicOn) Color(0xFF39FF9C).copy(alpha = 0.7f) else Color.White.copy(alpha = 0.25f)
                                ),
                                shape = RoundedCornerShape(12.dp),
                                modifier = Modifier.height(48.dp)
                            ) {
                                Text(
                                    text = if (isMicOn) "🎙️ MIC: ATIVO" else "🎙️ MIC: INATIVO",
                                    fontFamily = JetBrainsMono,
                                    fontWeight = FontWeight.Bold,
                                    fontSize = 11.sp,
                                    maxLines = 1
                                )
                            }

                            // 6. Codec cycling preference Button
                            Button(
                                onClick = { WebcamStreamer.cycleCodecPreference() },
                                colors = ButtonDefaults.buttonColors(
                                    containerColor = Color.White.copy(alpha = 0.08f),
                                    contentColor = Color.White
                                ),
                                border = BorderStroke(1.dp, Color.White.copy(alpha = 0.25f)),
                                shape = RoundedCornerShape(12.dp),
                                modifier = Modifier.height(48.dp)
                            ) {
                                Text(
                                    text = "📹 PREF. CODEC: ${codecPreference.uppercase()}",
                                    fontFamily = JetBrainsMono,
                                    fontWeight = FontWeight.Bold,
                                    fontSize = 11.sp,
                                    maxLines = 1
                                )
                            }
                        }

                        Spacer(modifier = Modifier.height(4.dp))

                        // Stop transmission Button
                        Button(
                            onClick = { WebcamStreamer.stop() },
                            colors = ButtonDefaults.buttonColors(
                                containerColor = Color(0xFFFF3B5C).copy(alpha = 0.15f),
                                contentColor = Color(0xFFFF3B5C)
                            ),
                            border = BorderStroke(1.dp, Color(0xFFFF3B5C).copy(alpha = 0.7f)),
                            shape = RoundedCornerShape(14.dp),
                            modifier = Modifier
                                .fillMaxWidth()
                                .height(54.dp)
                        ) {
                            Text(
                                text = "PARAR TRANSMISSÃO ⏹",
                                fontFamily = JetBrainsMono,
                                fontWeight = FontWeight.Bold,
                                fontSize = 12.sp,
                                maxLines = 1
                            )
                        }
                    }
                }

                // Screen-off overlay (tap anywhere to re-awaken screen)
                if (isScreenOff) {
                    Box(
                        modifier = Modifier
                            .fillMaxSize()
                            .background(Color.Black)
                            .pointerInput(Unit) {
                                detectTapGestures(
                                    onTap = { isScreenOff = false }
                                )
                            },
                        contentAlignment = Alignment.Center
                    ) {
                        Text(
                            text = "Toque em qualquer lugar para reativar o ecrã",
                            color = Color.White.copy(alpha = 0.3f),
                            style = MaterialTheme.typography.bodySmall,
                            fontFamily = JetBrainsMono,
                            textAlign = TextAlign.Center,
                            modifier = Modifier.padding(24.dp)
                        )
                    }
                }
            }
        }
    }

    if (showWorkstationSelectorDialog) {
        AlertDialog(
            onDismissRequest = { showWorkstationSelectorDialog = false },
            title = { Text("MUDAR ESTAÇÃO DE TRABALHO", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold, color = TextPrimary, fontFamily = JetBrainsMono) },
            text = {
                Column(
                    modifier = Modifier.fillMaxWidth(),
                    verticalArrangement = Arrangement.spacedBy(10.dp)
                ) {
                    if (workstations.isEmpty()) {
                        Text(
                            text = "Nenhuma estação emparelhada.",
                            style = MaterialTheme.typography.bodyMedium,
                            color = TextSecondary
                        )
                    } else {
                        workstations.forEach { ws ->
                            val isSelected = ws.id == activeWorkstationId
                            Row(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .clip(RoundedCornerShape(8.dp))
                                    .background(if (isSelected) DarkOutline else Color.Transparent)
                                    .clickable {
                                        activeWorkstationId = ws.id
                                        showWorkstationSelectorDialog = false
                                    }
                                    .padding(12.dp),
                                horizontalArrangement = Arrangement.SpaceBetween,
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                Column {
                                    Text(
                                        text = ws.deviceName,
                                        style = MaterialTheme.typography.titleMedium,
                                        fontWeight = FontWeight.Bold,
                                        color = if (isSelected) CyanActive else TextPrimary
                                    )
                                    Text(
                                        text = "${ws.host}:${ws.port}",
                                        style = MaterialTheme.typography.labelSmall,
                                        fontFamily = JetBrainsMono,
                                        color = TextSecondary
                                    )
                                }
                                Row(verticalAlignment = Alignment.CenterVertically) {
                                    if (isSelected) {
                                        Icon(
                                            imageVector = Icons.Default.Check,
                                            contentDescription = "Selecionado",
                                            tint = CyanActive,
                                            modifier = Modifier.size(18.dp)
                                        )
                                        Spacer(modifier = Modifier.width(8.dp))
                                    }
                                    IconButton(
                                        onClick = {
                                            coroutineScope.launch {
                                                repository.delete(ws)
                                                if (activeWorkstationId == ws.id) {
                                                    activeWorkstationId = ""
                                                }
                                            }
                                        },
                                        modifier = Modifier.size(24.dp)
                                    ) {
                                        Icon(
                                            imageVector = Icons.Default.Delete,
                                            contentDescription = "Remover Estação",
                                            tint = RedError,
                                            modifier = Modifier.size(18.dp)
                                        )
                                    }
                                }
                            }
                        }
                    }

                    Spacer(modifier = Modifier.height(6.dp))
                    Button(
                        onClick = {
                            showWorkstationSelectorDialog = false
                            showNewPairDialog = true
                        },
                        colors = ButtonDefaults.buttonColors(containerColor = DarkOutline),
                        shape = RoundedCornerShape(8.dp),
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        Text("+ EMPARELHAR NOVA", color = CyanActive, style = MaterialTheme.typography.labelSmall, fontFamily = JetBrainsMono, fontWeight = FontWeight.Bold)
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = { showWorkstationSelectorDialog = false }) {
                    Text("FECHAR", color = TextSecondary, style = MaterialTheme.typography.labelSmall, fontFamily = JetBrainsMono, fontWeight = FontWeight.Bold)
                }
            },
            containerColor = DarkSurface,
            textContentColor = TextPrimary,
            titleContentColor = TextPrimary
        )
    }

    if (showInitialPermissionsDialog) {
        AlertDialog(
            onDismissRequest = {
                showInitialPermissionsDialog = false
                sharedPrefs.edit().putBoolean("has_seen_initial_permissions_dialog", true).apply()
            },
            title = {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    Icon(
                        imageVector = Icons.Default.Security,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.primary
                    )
                    Text(
                        text = "Configuração de Permissões",
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.Bold,
                        color = MaterialTheme.colorScheme.onSurface
                    )
                }
            },
            text = {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .verticalScroll(rememberScrollState()),
                    verticalArrangement = Arrangement.spacedBy(12.dp)
                ) {
                    Text(
                        text = "Para que todas as funções do HyprLink operem em segundo plano com a sua workstation (notificações, áudio remoto e ligação estável), configure os acessos abaixo:",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )

                    // 1. Bateria
                    Card(
                        colors = CardDefaults.cardColors(
                            containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f)
                        ),
                        shape = RoundedCornerShape(12.dp)
                    ) {
                        Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.SpaceBetween,
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                Text(
                                    text = "🔋 Bateria Sem Restrições",
                                    fontWeight = FontWeight.SemiBold,
                                    style = MaterialTheme.typography.bodyMedium,
                                    color = MaterialTheme.colorScheme.onSurface
                                )
                                Text(
                                    text = if (isBatteryOptIgnored) "ATIVO ✓" else "PENDENTE",
                                    style = MaterialTheme.typography.labelSmall,
                                    fontWeight = FontWeight.Bold,
                                    color = if (isBatteryOptIgnored) Color(0xFF4CAF50) else Color(0xFFFF9800)
                                )
                            }
                            Text(
                                text = "Evita que o Android congele a ligação de fundo durante períodos de inatividade.",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                            if (!isBatteryOptIgnored) {
                                Button(
                                    onClick = {
                                        try {
                                            val intent = Intent(android.provider.Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS).apply {
                                                data = Uri.parse("package:${context.packageName}")
                                            }
                                            context.startActivity(intent)
                                        } catch (e: Exception) {
                                            Toast.makeText(context, "Erro: ${e.message}", Toast.LENGTH_SHORT).show()
                                        }
                                    },
                                    modifier = Modifier.fillMaxWidth().height(36.dp),
                                    colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.primary),
                                    contentPadding = PaddingValues(vertical = 4.dp)
                                ) {
                                    Text("Desativar Restrições de Bateria", fontSize = 12.sp)
                                }
                            }
                        }
                    }

                    // 2. Notificações
                    Card(
                        colors = CardDefaults.cardColors(
                            containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f)
                        ),
                        shape = RoundedCornerShape(12.dp)
                    ) {
                        Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.SpaceBetween,
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                Text(
                                    text = "🔔 Espelhar Notificações",
                                    fontWeight = FontWeight.SemiBold,
                                    style = MaterialTheme.typography.bodyMedium,
                                    color = MaterialTheme.colorScheme.onSurface
                                )
                                Text(
                                    text = if (isNotifListenerGranted) "ATIVO ✓" else "PENDENTE",
                                    style = MaterialTheme.typography.labelSmall,
                                    fontWeight = FontWeight.Bold,
                                    color = if (isNotifListenerGranted) Color(0xFF4CAF50) else Color(0xFFFF9800)
                                )
                            }
                            Text(
                                text = "Permite enviar e responder a notificações diretamente no PC.",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                            if (!isNotifListenerGranted) {
                                Button(
                                    onClick = {
                                        try {
                                            val intent = Intent(android.provider.Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS)
                                            context.startActivity(intent)
                                        } catch (e: Exception) {
                                            Toast.makeText(context, "Erro: ${e.message}", Toast.LENGTH_SHORT).show()
                                        }
                                    },
                                    modifier = Modifier.fillMaxWidth().height(36.dp),
                                    colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.primary),
                                    contentPadding = PaddingValues(vertical = 4.dp)
                                ) {
                                    Text("Permitir Acesso a Notificações", fontSize = 12.sp)
                                }
                            }
                        }
                    }

                    // 3. Não Incomodar / Som
                    Card(
                        colors = CardDefaults.cardColors(
                            containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f)
                        ),
                        shape = RoundedCornerShape(12.dp)
                    ) {
                        Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.SpaceBetween,
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                Text(
                                    text = "🔇 Acesso a Não Incomodar",
                                    fontWeight = FontWeight.SemiBold,
                                    style = MaterialTheme.typography.bodyMedium,
                                    color = MaterialTheme.colorScheme.onSurface
                                )
                                Text(
                                    text = if (isDndGranted) "ATIVO ✓" else "PENDENTE",
                                    style = MaterialTheme.typography.labelSmall,
                                    fontWeight = FontWeight.Bold,
                                    color = if (isDndGranted) Color(0xFF4CAF50) else Color(0xFFFF9800)
                                )
                            }
                            Text(
                                text = "Permite ao PC alternar o som do telemóvel para Silencioso, Vibrar ou Normal.",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                            if (!isDndGranted) {
                                Button(
                                    onClick = {
                                        try {
                                            val intent = Intent(android.provider.Settings.ACTION_NOTIFICATION_POLICY_ACCESS_SETTINGS).apply {
                                                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                                            }
                                            context.startActivity(intent)
                                        } catch (e: Exception) {
                                            Toast.makeText(context, "Erro: ${e.message}", Toast.LENGTH_SHORT).show()
                                        }
                                    },
                                    modifier = Modifier.fillMaxWidth().height(36.dp),
                                    colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.primary),
                                    contentPadding = PaddingValues(vertical = 4.dp)
                                ) {
                                    Text("Conceder Permissão DND", fontSize = 12.sp)
                                }
                            }
                        }
                    }
                }
            },
            confirmButton = {
                val allGranted = isBatteryOptIgnored && isNotifListenerGranted && isDndGranted
                TextButton(
                    onClick = {
                        showInitialPermissionsDialog = false
                        sharedPrefs.edit().putBoolean("has_seen_initial_permissions_dialog", true).apply()
                    }
                ) {
                    Text(
                        text = if (allGranted) "Tudo Pronto" else "Concluir",
                        color = MaterialTheme.colorScheme.secondary,
                        fontWeight = FontWeight.Bold
                    )
                }
            },
            dismissButton = {
                TextButton(
                    onClick = {
                        showInitialPermissionsDialog = false
                        sharedPrefs.edit().putBoolean("has_seen_initial_permissions_dialog", true).apply()
                    }
                ) {
                    Text("Mais Tarde", color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        )
    }

    if (showBatteryExemptionDialog) {
        AlertDialog(
            onDismissRequest = { showBatteryExemptionDialog = false },
            title = { Text("Desativar Restrições de Bateria", color = MaterialTheme.colorScheme.onSurface) },
            text = { Text("Para garantir que a ligação de fundo do HyprLink não seja encerrada pelo sistema Android durante períodos de inatividade (Doze mode), é altamente recomendado permitir que o aplicativo seja executado sem restrições de bateria.") },
            confirmButton = {
                TextButton(
                    onClick = {
                        showBatteryExemptionDialog = false
                        hasRequestedBatteryExemptionThisSession = true
                        try {
                            val intent = Intent(android.provider.Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS).apply {
                                data = Uri.parse("package:${context.packageName}")
                            }
                            context.startActivity(intent)
                        } catch (e: Exception) {
                            Toast.makeText(context, "Erro ao abrir configurações: ${e.message}", Toast.LENGTH_SHORT).show()
                        }
                    }
                ) {
                    Text("Configurar", color = MaterialTheme.colorScheme.secondary)
                }
            },
            dismissButton = {
                TextButton(
                    onClick = {
                        showBatteryExemptionDialog = false
                        hasRequestedBatteryExemptionThisSession = true
                    }
                ) {
                    Text("Ignorar", color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        )
    }

    if (showDndDialog) {
        AlertDialog(
            onDismissRequest = {
                showDndDialog = false
                hasRequestedDndThisSession = true
            },
            title = { Text("Acesso a Não Incomodar / Silencioso", color = MaterialTheme.colorScheme.onSurface) },
            text = { Text("Para que o computador consiga alterar remotamente o modo de som do telemóvel (Normal, Vibrar ou Silencioso) via HyprLink, o Android requer autorização de acesso ao modo Não Incomodar.") },
            confirmButton = {
                TextButton(
                    onClick = {
                        showDndDialog = false
                        hasRequestedDndThisSession = true
                        try {
                            val intent = Intent(android.provider.Settings.ACTION_NOTIFICATION_POLICY_ACCESS_SETTINGS).apply {
                                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                            }
                            context.startActivity(intent)
                        } catch (e: Exception) {
                            Toast.makeText(context, "Erro ao abrir definições: ${e.message}", Toast.LENGTH_SHORT).show()
                        }
                    }
                ) {
                    Text("Configurar", color = MaterialTheme.colorScheme.secondary)
                }
            },
            dismissButton = {
                TextButton(
                    onClick = {
                        showDndDialog = false
                        hasRequestedDndThisSession = true
                    }
                ) {
                    Text("Mais Tarde", color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        )
    }

    // 1. Share URL dialog
    if (showUrlShareDialog) {
        var urlInput by remember { mutableStateOf("") }
        Dialog(onDismissRequest = { showUrlShareDialog = false }) {
            Card(
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                shape = RoundedCornerShape(24.dp),
                border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(16.dp)
            ) {
                Column(
                    modifier = Modifier.padding(24.dp),
                    verticalArrangement = Arrangement.spacedBy(16.dp)
                ) {
                    Text(
                        text = "Partilhar URL",
                        style = MaterialTheme.typography.titleLarge,
                        color = MaterialTheme.colorScheme.onSurface
                    )

                    OutlinedTextField(
                        value = urlInput,
                        onValueChange = { urlInput = it },
                        label = { Text("Introduza a URL/Texto", color = MaterialTheme.colorScheme.onSurfaceVariant) },
                        colors = OutlinedTextFieldDefaults.colors(
                            focusedTextColor = MaterialTheme.colorScheme.onSurface,
                            unfocusedTextColor = MaterialTheme.colorScheme.onSurface,
                            focusedBorderColor = MaterialTheme.colorScheme.secondary,
                            unfocusedBorderColor = MaterialTheme.colorScheme.outline
                        ),
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth()
                    )

                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(8.dp)
                    ) {
                        Button(
                            onClick = { showUrlShareDialog = false },
                            colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.outline),
                            shape = RoundedCornerShape(12.dp),
                            modifier = Modifier.weight(1f)
                        ) {
                            Text("Cancelar", color = MaterialTheme.colorScheme.onSurface)
                        }

                        Button(
                            onClick = {
                                if (urlInput.isNotBlank()) {
                                    coroutineScope.launch {
                                        ConnectionRepository.sendUrl(urlInput)
                                        Toast.makeText(context, "URL partilhada!", Toast.LENGTH_SHORT).show()
                                    }
                                }
                                showUrlShareDialog = false
                            },
                            colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.primaryContainer),
                            shape = RoundedCornerShape(12.dp),
                            modifier = Modifier.weight(1.5f)
                        ) {
                            Text("Empurrar", color = MaterialTheme.colorScheme.onPrimary, fontWeight = FontWeight.Bold)
                        }
                    }
                }
            }
        }
    }

    // 1b. Share Choice Dialog
    if (showShareChoiceDialog) {
        Dialog(onDismissRequest = { showShareChoiceDialog = false }) {
            Card(
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                shape = RoundedCornerShape(24.dp),
                border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(16.dp)
            ) {
                Column(
                    modifier = Modifier.padding(24.dp),
                    verticalArrangement = Arrangement.spacedBy(16.dp),
                    horizontalAlignment = Alignment.CenterHorizontally
                ) {
                    Text(
                        text = "Partilhar com o PC",
                        style = MaterialTheme.typography.titleLarge,
                        color = MaterialTheme.colorScheme.onSurface
                    )
                    Text(
                        text = "Escolha o tipo de partilha a enviar para a sua workstation ativa:",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = TextAlign.Center
                    )

                    Button(
                        onClick = {
                            showShareChoiceDialog = false
                            filePickerLauncher.launch("*/*")
                        },
                        colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.primaryContainer),
                        shape = RoundedCornerShape(16.dp),
                        modifier = Modifier.fillMaxWidth().height(52.dp)
                    ) {
                        Row(
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Icon(
                                imageVector = Icons.Default.Share,
                                contentDescription = "Enviar Ficheiro",
                                tint = MaterialTheme.colorScheme.onPrimary
                            )
                            Text("Enviar Ficheiro", color = MaterialTheme.colorScheme.onPrimary, fontWeight = FontWeight.Bold)
                        }
                    }

                    Button(
                        onClick = {
                            showShareChoiceDialog = false
                            showUrlShareDialog = true
                        },
                        colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.secondary),
                        shape = RoundedCornerShape(16.dp),
                        modifier = Modifier.fillMaxWidth().height(52.dp)
                    ) {
                        Row(
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Icon(
                                imageVector = Icons.Default.PlayArrow,
                                contentDescription = "Partilhar URL"
                            )
                            Text("Partilhar URL / Texto", fontWeight = FontWeight.Bold)
                        }
                    }

                    Spacer(modifier = Modifier.height(4.dp))

                    Button(
                        onClick = { showShareChoiceDialog = false },
                        colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.outline),
                        shape = RoundedCornerShape(12.dp),
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        Text("Cancelar", color = MaterialTheme.colorScheme.onSurface)
                    }
                }
            }
        }
    }

    // 1c. Media Controls Dialog
    if (showMediaControlsDialog) {
        Dialog(onDismissRequest = { showMediaControlsDialog = false }) {
            Card(
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                shape = RoundedCornerShape(24.dp),
                border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(16.dp)
            ) {
                Column(
                    modifier = Modifier.padding(24.dp),
                    verticalArrangement = Arrangement.spacedBy(16.dp),
                    horizontalAlignment = Alignment.CenterHorizontally
                ) {
                    Text(
                        text = "Controle de Media",
                        style = MaterialTheme.typography.titleLarge,
                        color = MaterialTheme.colorScheme.onSurface
                    )

                    if (mediaState != null) {
                        val ms = mediaState!!
                        val isPlaying = ms.status.lowercase() == "playing"
                        Column(
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.spacedBy(4.dp)
                        ) {
                            Text(
                                text = ms.title ?: "Sem Título",
                                style = MaterialTheme.typography.titleMedium,
                                fontWeight = FontWeight.Bold,
                                color = MaterialTheme.colorScheme.onSurface,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis
                            )
                            Text(
                                text = ms.artist ?: "Artista Desconhecido",
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis
                            )
                            Text(
                                text = ms.player,
                                style = MaterialTheme.typography.labelSmall,
                                fontFamily = FontFamily.Monospace,
                                color = MaterialTheme.colorScheme.secondary
                            )
                        }
                    } else {
                        Text(
                            text = "Nenhum leitor ativo detetado no PC",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            textAlign = TextAlign.Center
                        )
                    }

                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceEvenly,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        IconButton(onClick = {
                            coroutineScope.launch { ConnectionRepository.sendMediaCommand("previous") }
                        }) {
                            Icon(
                                imageVector = Icons.Default.SkipPrevious,
                                contentDescription = "Anterior",
                                tint = MaterialTheme.colorScheme.onSurface,
                                modifier = Modifier.size(32.dp)
                            )
                        }

                        FloatingActionButton(
                            onClick = {
                                coroutineScope.launch { ConnectionRepository.sendMediaCommand("playpause") }
                            },
                            containerColor = MaterialTheme.colorScheme.secondary,
                            contentColor = TerminalBlack,
                            shape = CircleShape,
                            modifier = Modifier.size(56.dp)
                        ) {
                            Icon(
                                imageVector = if (mediaState?.status?.lowercase() == "playing") Icons.Default.Pause else Icons.Default.PlayArrow,
                                contentDescription = "Play/Pause",
                                modifier = Modifier.size(28.dp)
                            )
                        }

                        IconButton(onClick = {
                            coroutineScope.launch { ConnectionRepository.sendMediaCommand("next") }
                        }) {
                            Icon(
                                imageVector = Icons.Default.SkipNext,
                                contentDescription = "Seguinte",
                                tint = MaterialTheme.colorScheme.onSurface,
                                modifier = Modifier.size(32.dp)
                            )
                        }
                    }

                    Button(
                        onClick = { showMediaControlsDialog = false },
                        colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.outline),
                        shape = RoundedCornerShape(12.dp),
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        Text("Fechar", color = MaterialTheme.colorScheme.onSurface)
                    }
                }
            }
        }
    }

    // 2. Pair Workstation Dialog
    if (showNewPairDialog) {
        var scanTabSelected by remember { mutableStateOf(true) }
        var hasCameraPermission by remember {
            mutableStateOf(
                ContextCompat.checkSelfPermission(
                    context,
                    Manifest.permission.CAMERA
                ) == PackageManager.PERMISSION_GRANTED
            )
        }
        val launcher = rememberLauncherForActivityResult(
            contract = ActivityResultContracts.RequestPermission(),
            onResult = { granted ->
                hasCameraPermission = granted
            }
        )

        var pairingLogs by remember { mutableStateOf(listOf("Scanner active. Scan the QR code from 'hyprlink-daemon --pair'")) }
        var isPairingInProgress by remember { mutableStateOf(false) }

        var inputName by remember { mutableStateOf(if (deviceName.isNotBlank() && deviceName != "Sem Workstation") deviceName else "Workstation-Alpha") }
        var inputHost by remember { mutableStateOf(if (deviceHost.isNotBlank() && deviceHost != "0.0.0.0") deviceHost else "192.168.1.45") }
        var inputPort by remember { mutableStateOf(if (devicePort.isNotBlank() && devicePort != "----") devicePort else ConnectionUtils.HYPRLINK_SERVICE_PORT.toString()) }
        var inputFingerprint by remember { mutableStateOf(if (deviceFingerprint.isNotBlank() && deviceFingerprint != "Nenhum fingerprint configurado") deviceFingerprint else "") }

        Dialog(
            onDismissRequest = { showNewPairDialog = false },
            properties = DialogProperties(usePlatformDefaultWidth = false)
        ) {
            Card(
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                shape = RoundedCornerShape(24.dp),
                border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
                modifier = Modifier
                    .fillMaxWidth(0.95f)
                    .fillMaxHeight(0.92f)
                    .padding(vertical = 12.dp)
            ) {
                Column(
                    modifier = Modifier
                        .fillMaxSize()
                        .padding(20.dp)
                        .verticalScroll(rememberScrollState()),
                    verticalArrangement = Arrangement.spacedBy(16.dp)
                ) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Column {
                            Text(
                                text = "Ligar ao Computador",
                                style = MaterialTheme.typography.titleLarge,
                                fontWeight = FontWeight.Bold,
                                color = MaterialTheme.colorScheme.onSurface
                            )
                            Text(
                                text = "Emparelhamento mTLS seguro via QUIC",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                        }
                        IconButton(onClick = { showNewPairDialog = false }) {
                            Icon(Icons.Default.Close, contentDescription = "Fechar", tint = MaterialTheme.colorScheme.onSurface)
                        }
                    }

                    // Tab selector
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .background(MaterialTheme.colorScheme.background, RoundedCornerShape(12.dp))
                            .padding(4.dp),
                        horizontalArrangement = Arrangement.spacedBy(4.dp)
                    ) {
                        Button(
                            onClick = { scanTabSelected = true },
                            modifier = Modifier.weight(1f).height(40.dp),
                            shape = RoundedCornerShape(8.dp),
                            colors = ButtonDefaults.buttonColors(
                                containerColor = if (scanTabSelected) MaterialTheme.colorScheme.surface else Color.Transparent,
                                contentColor = MaterialTheme.colorScheme.onSurface
                            ),
                            contentPadding = PaddingValues(0.dp)
                        ) {
                            Icon(Icons.Default.QrCodeScanner, contentDescription = null, modifier = Modifier.size(18.dp))
                            Spacer(modifier = Modifier.width(6.dp))
                            Text("Escanear QR Code", style = MaterialTheme.typography.labelMedium)
                        }

                        Button(
                            onClick = { scanTabSelected = false },
                            modifier = Modifier.weight(1f).height(40.dp),
                            shape = RoundedCornerShape(8.dp),
                            colors = ButtonDefaults.buttonColors(
                                containerColor = if (!scanTabSelected) MaterialTheme.colorScheme.surface else Color.Transparent,
                                contentColor = MaterialTheme.colorScheme.onSurface
                            ),
                            contentPadding = PaddingValues(0.dp)
                        ) {
                            Icon(Icons.Default.Edit, contentDescription = null, modifier = Modifier.size(18.dp))
                            Spacer(modifier = Modifier.width(6.dp))
                            Text("Manual", style = MaterialTheme.typography.labelMedium)
                        }
                    }

                    if (scanTabSelected) {
                        // Scan QR Code Tab
                        if (!hasCameraPermission) {
                            Column(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .height(200.dp),
                                horizontalAlignment = Alignment.CenterHorizontally,
                                verticalArrangement = Arrangement.Center
                            ) {
                                Text(
                                    text = "Permissão de câmara necessária para ler o QR Code",
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    style = MaterialTheme.typography.bodyMedium,
                                    textAlign = TextAlign.Center
                                )
                                Spacer(modifier = Modifier.height(12.dp))
                                Button(
                                    onClick = { launcher.launch(Manifest.permission.CAMERA) },
                                    colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.secondary)
                                ) {
                                    Text("Permitir Câmara", color = TerminalBlack, fontWeight = FontWeight.Bold)
                                }
                            }
                        } else {
                            // Camera preview for QR scan
                            Box(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .height(220.dp)
                                    .clip(RoundedCornerShape(16.dp))
                                    .background(TerminalBlack),
                                contentAlignment = Alignment.Center
                            ) {
                                CameraPreview(
                                    onQrCodeScanned = { qrText ->
                                        if (!isPairingInProgress) {
                                            isPairingInProgress = true
                                            pairingLogs = listOf("[INFO] QR Code detectado! A ler metadados...")
                                            coroutineScope.launch {
                                                val logsList = mutableListOf<String>()
                                                fun logPair(msg: String) {
                                                    logsList.add(msg)
                                                    pairingLogs = logsList.toList()
                                                }

                                                try {
                                                    val parsed = parsePairingQr(qrText)
                                                    if (parsed == null) {
                                                        logPair("[ERROR] QR code com formato inválido!")
                                                        logPair("[INFO] Formato esperado: FINGERPRINT|HOST:PORT|TOKEN")
                                                        logPair("[INFO] Conteúdo lido: ${qrText.take(50)}...")
                                                        isPairingInProgress = false
                                                        return@launch
                                                    }
                                                    val host = parsed.host
                                                    val port = parsed.port.toString()
                                                    val fingerprint = parsed.fingerprint
                                                    val token = parsed.pairingToken

                                                    logPair("[INFO] Alvo: $host:$port")
                                                    logPair("[INFO] Fingerprint: ${fingerprint.take(16)}...")
                                                    logPair("[INFO] Handshake pairingToken: ${token.take(12)}...")
                                                    logPair("[INFO] A iniciar handshake criptográfico seguro...")
                                                    val wkName = withContext(Dispatchers.IO) {
                                                        executePairingHandshake(
                                                            context = context,
                                                            localDeviceName = sharedPrefs.getString("local_device_name", "Android-Phone-Client") ?: "Android-Phone-Client",
                                                            data = parsed,
                                                            onLog = { msg -> logPair(msg) }
                                                        )
                                                    }

                                                    if (wkName != null) {
                                                        logPair("[SUCCESS] Workstation '$wkName' emparelhada com sucesso!")
                                                        val newWs = PairedWorkstation(
                                                            id = java.util.UUID.randomUUID().toString(),
                                                            deviceName = wkName,
                                                            host = parsed.host,
                                                            port = ConnectionUtils.HYPRLINK_SERVICE_PORT.toString(),
                                                            fingerprint = parsed.fingerprint,
                                                            pairingTokenHex = parsed.pairingToken
                                                        )
                                                        repository.insert(newWs)
                                                        activeWorkstationId = newWs.id
                                                        delay(1800)
                                                        showNewPairDialog = false
                                                    } else {
                                                        logPair("[ERROR] Handshake rejeitado ou expirado pelo PC.")
                                                    }
                                                } catch (e: Exception) {
                                                    logPair("[ERROR] Falha: ${e.message}")
                                                } finally {
                                                    isPairingInProgress = false
                                                }
                                            }
                                        }
                                    }
                                )

                                // Scanning frame with corner accents
                                Box(
                                    modifier = Modifier
                                        .size(150.dp)
                                        .border(2.dp, MaterialTheme.colorScheme.secondary.copy(alpha = 0.8f), RoundedCornerShape(12.dp))
                                )
                            }
                        }

                        // Terminal Header with Copy/Share actions
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Text(
                                text = "Terminal de Emparelhamento",
                                style = MaterialTheme.typography.labelLarge,
                                fontWeight = FontWeight.SemiBold,
                                color = MaterialTheme.colorScheme.onSurface
                            )
                            Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                                IconButton(
                                    onClick = {
                                        val fullText = pairingLogs.joinToString("\n")
                                        val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                                        clipboard.setPrimaryClip(ClipData.newPlainText("HyprLink Pairing Logs", fullText))
                                        Toast.makeText(context, "Logs copiados para a área de transferência!", Toast.LENGTH_SHORT).show()
                                    },
                                    modifier = Modifier.size(32.dp)
                                ) {
                                    Icon(
                                        imageVector = Icons.Default.ContentCopy,
                                        contentDescription = "Copiar Logs",
                                        tint = MaterialTheme.colorScheme.secondary,
                                        modifier = Modifier.size(18.dp)
                                    )
                                }
                                IconButton(
                                    onClick = {
                                        val fullText = pairingLogs.joinToString("\n")
                                        val sendIntent = Intent().apply {
                                            action = Intent.ACTION_SEND
                                            putExtra(Intent.EXTRA_TITLE, "hyprlink-pairing-logs.txt")
                                            putExtra(Intent.EXTRA_TEXT, fullText)
                                            type = "text/plain"
                                        }
                                        context.startActivity(Intent.createChooser(sendIntent, "Partilhar Logs de Emparelhamento"))
                                    },
                                    modifier = Modifier.size(32.dp)
                                ) {
                                    Icon(
                                        imageVector = Icons.Default.Share,
                                        contentDescription = "Partilhar Logs",
                                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                                        modifier = Modifier.size(18.dp)
                                    )
                                }
                            }
                        }

                        // Real-time pairing console log box (larger, selectable, clearer typography)
                        Box(
                            modifier = Modifier
                                .fillMaxWidth()
                                .height(160.dp)
                                .clip(RoundedCornerShape(12.dp))
                                .background(TerminalBlack)
                                .border(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.5f), RoundedCornerShape(12.dp))
                                .padding(10.dp)
                        ) {
                            val scroll = rememberScrollState()
                            LaunchedEffect(pairingLogs.size) {
                                scroll.animateScrollTo(scroll.maxValue)
                            }
                            SelectionContainer {
                                Column(
                                    modifier = Modifier
                                        .fillMaxSize()
                                        .verticalScroll(scroll),
                                    verticalArrangement = Arrangement.spacedBy(4.dp)
                                ) {
                                    pairingLogs.forEach { log ->
                                        val logColor = when {
                                            log.contains("[ERROR]") -> RedError
                                            log.contains("[SUCCESS]") -> CyanActive
                                            log.contains("[DATA]") -> AmberWarning
                                            else -> TextPrimary
                                        }
                                        Text(
                                            text = log,
                                            color = logColor,
                                            style = MaterialTheme.typography.bodySmall.copy(
                                                fontSize = 12.sp,
                                                lineHeight = 16.sp,
                                                fontFamily = JetBrainsMono
                                            )
                                        )
                                    }
                                }
                            }
                        }
                    } else {
                        // Manual Configuration Tab
                        OutlinedTextField(
                            value = inputName,
                            onValueChange = { inputName = it },
                            label = { Text("Workstation Name", color = MaterialTheme.colorScheme.onSurfaceVariant) },
                            colors = OutlinedTextFieldDefaults.colors(
                                focusedTextColor = MaterialTheme.colorScheme.onSurface,
                                unfocusedTextColor = MaterialTheme.colorScheme.onSurface,
                                focusedBorderColor = MaterialTheme.colorScheme.secondary,
                                unfocusedBorderColor = MaterialTheme.colorScheme.outline
                            ),
                            singleLine = true,
                            modifier = Modifier.fillMaxWidth().testTag("dialog_workstation_name_input")
                        )

                        OutlinedTextField(
                            value = inputHost,
                            onValueChange = { inputHost = it },
                            label = { Text("Workstation IP Host", color = MaterialTheme.colorScheme.onSurfaceVariant) },
                            colors = OutlinedTextFieldDefaults.colors(
                                focusedTextColor = MaterialTheme.colorScheme.onSurface,
                                unfocusedTextColor = MaterialTheme.colorScheme.onSurface,
                                focusedBorderColor = MaterialTheme.colorScheme.secondary,
                                unfocusedBorderColor = MaterialTheme.colorScheme.outline
                            ),
                            singleLine = true,
                            modifier = Modifier.fillMaxWidth().testTag("dialog_workstation_ip_input")
                        )

                        OutlinedTextField(
                            value = inputPort,
                            onValueChange = { inputPort = it },
                            label = { Text("Port", color = MaterialTheme.colorScheme.onSurfaceVariant) },
                            colors = OutlinedTextFieldDefaults.colors(
                                focusedTextColor = MaterialTheme.colorScheme.onSurface,
                                unfocusedTextColor = MaterialTheme.colorScheme.onSurface,
                                focusedBorderColor = MaterialTheme.colorScheme.secondary,
                                unfocusedBorderColor = MaterialTheme.colorScheme.outline
                            ),
                            singleLine = true,
                            modifier = Modifier.fillMaxWidth().testTag("dialog_workstation_port_input")
                        )

                        OutlinedTextField(
                            value = inputFingerprint,
                            onValueChange = { inputFingerprint = it },
                            label = { Text("Daemon fingerprint (SHA-256)", color = MaterialTheme.colorScheme.onSurfaceVariant) },
                            colors = OutlinedTextFieldDefaults.colors(
                                focusedTextColor = MaterialTheme.colorScheme.onSurface,
                                unfocusedTextColor = MaterialTheme.colorScheme.onSurface,
                                focusedBorderColor = MaterialTheme.colorScheme.secondary,
                                unfocusedBorderColor = MaterialTheme.colorScheme.outline
                            ),
                            singleLine = true,
                            modifier = Modifier.fillMaxWidth().testTag("dialog_workstation_fingerprint_input")
                        )

                        Button(
                            onClick = {
                                if (inputName.isNotBlank() && inputHost.isNotBlank() && inputPort.isNotBlank() && inputFingerprint.isNotBlank()) {
                                    coroutineScope.launch {
                                        val newWs = PairedWorkstation(
                                            id = java.util.UUID.randomUUID().toString(),
                                            deviceName = inputName.trim(),
                                            host = inputHost.trim(),
                                            port = inputPort.trim(),
                                            fingerprint = inputFingerprint.trim(),
                                            pairingTokenHex = null
                                        )
                                        repository.insert(newWs)
                                        activeWorkstationId = newWs.id
                                        showNewPairDialog = false
                                        Toast.makeText(context, "Workstation adicionada manualmente!", Toast.LENGTH_SHORT).show()
                                    }
                                } else {
                                    Toast.makeText(context, "Por favor preencha todos os campos!", Toast.LENGTH_SHORT).show()
                                }
                            },
                            colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.secondary),
                            shape = RoundedCornerShape(12.dp),
                            modifier = Modifier.fillMaxWidth().height(48.dp).testTag("dialog_save_workstation_button")
                        ) {
                            Text("Guardar Configuração", color = TerminalBlack, fontWeight = FontWeight.Bold)
                        }
                    }

                    Button(
                        onClick = { showNewPairDialog = false },
                        colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.outline),
                        shape = RoundedCornerShape(12.dp),
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        Text("Fechar", color = MaterialTheme.colorScheme.onSurface)
                    }
                }
            }
        }
    }

    // 3. Device Identity Dialog
    if (showIdentityDialog) {
        val deviceIdentity = remember { DeviceIdentity.getInstance(context) }
        var inputLocalName by remember { mutableStateOf(sharedPrefs.getString("local_device_name", "Android-Phone-Client") ?: "Android-Phone-Client") }
        var localFingerprint by remember { mutableStateOf(formatFingerprint(deviceIdentity.fingerprintHex)) }

        Dialog(onDismissRequest = { showIdentityDialog = false }) {
            Card(
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                shape = RoundedCornerShape(24.dp),
                border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(16.dp)
            ) {
                Column(
                    modifier = Modifier.padding(24.dp),
                    verticalArrangement = Arrangement.spacedBy(16.dp)
                ) {
                    Text(
                        text = "Device Identity",
                        style = MaterialTheme.typography.titleLarge,
                        color = MaterialTheme.colorScheme.onSurface
                    )

                    OutlinedTextField(
                        value = inputLocalName,
                        onValueChange = { inputLocalName = it },
                        label = { Text("Local Identity Name", color = MaterialTheme.colorScheme.onSurfaceVariant) },
                        colors = OutlinedTextFieldDefaults.colors(
                            focusedTextColor = MaterialTheme.colorScheme.onSurface,
                            unfocusedTextColor = MaterialTheme.colorScheme.onSurface,
                            focusedBorderColor = MaterialTheme.colorScheme.secondary,
                            unfocusedBorderColor = MaterialTheme.colorScheme.outline
                        ),
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth()
                    )

                    // Fingerprint box
                    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        Text(
                            text = "Local Fingerprint (SHA-256)",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant
                        )
                        Box(
                            modifier = Modifier
                                .fillMaxWidth()
                                .background(TerminalBlack, RoundedCornerShape(12.dp))
                                .border(1.dp, MaterialTheme.colorScheme.outline, RoundedCornerShape(12.dp))
                                .padding(12.dp)
                        ) {
                            Text(
                                text = localFingerprint,
                                color = MaterialTheme.colorScheme.secondary,
                                style = MaterialTheme.typography.labelSmall,
                                fontFamily = FontFamily.Monospace,
                                lineHeight = 16.sp
                            )
                        }
                    }

                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(8.dp)
                    ) {
                        Button(
                            onClick = { showRegenerateConfirmDialog = true },
                            colors = ButtonDefaults.buttonColors(containerColor = Color.Transparent),
                            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
                            shape = RoundedCornerShape(12.dp),
                            modifier = Modifier.weight(1f).height(46.dp)
                        ) {
                            Text("Gerar Chaves", color = MaterialTheme.colorScheme.onSurface, fontSize = 11.sp, fontWeight = FontWeight.SemiBold)
                        }

                        Button(
                            onClick = {
                                val clipboardManager = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                                val clipData = ClipData.newPlainText("HyprLink Fingerprint", localFingerprint)
                                clipboardManager.setPrimaryClip(clipData)
                                Toast.makeText(context, "Copiado!", Toast.LENGTH_SHORT).show()
                            },
                            colors = ButtonDefaults.buttonColors(containerColor = Color.Transparent),
                            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
                            shape = RoundedCornerShape(12.dp),
                            modifier = Modifier.weight(1f).height(46.dp).testTag("dialog_identity_copy_button")
                        ) {
                            Text("Copiar SHA", color = MaterialTheme.colorScheme.onSurface, fontSize = 11.sp, fontWeight = FontWeight.SemiBold)
                        }
                    }

                    Button(
                        onClick = {
                            if (inputLocalName.isNotBlank()) {
                                sharedPrefs.edit().putString("local_device_name", inputLocalName.trim()).apply()
                            }
                            showIdentityDialog = false
                        },
                        colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.primaryContainer),
                        shape = RoundedCornerShape(12.dp),
                        modifier = Modifier
                            .fillMaxWidth()
                            .height(48.dp)
                            .testTag("dialog_identity_close_button")
                    ) {
                        Text("Aplicar & Fechar", color = MaterialTheme.colorScheme.onPrimary, fontWeight = FontWeight.Bold)
                    }
                }
            }
        }
    }

    if (showRegenerateConfirmDialog) {
        androidx.compose.material3.AlertDialog(
            onDismissRequest = { showRegenerateConfirmDialog = false },
            title = { Text("Recriar Identidade?", color = MaterialTheme.colorScheme.onSurface) },
            text = {
                Text(
                    "Esta acção irá gerar uma nova chave privada e certificado local. Todos os emparelhamentos existentes serão invalidados.",
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        val newIdentity = DeviceIdentity.regenerateInstance(context)
                        showRegenerateConfirmDialog = false
                        Toast.makeText(context, "Identidade recriada!", Toast.LENGTH_SHORT).show()
                    }
                ) {
                    Text("Recriar", color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = {
                TextButton(onClick = { showRegenerateConfirmDialog = false }) {
                    Text("Cancelar", color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            },
            containerColor = MaterialTheme.colorScheme.surface
        )
    }
}

@Composable
fun ModuleCard(
    index: String,
    statusState: String,
    iconText: String,
    title: String,
    description: String,
    onClick: () -> Unit
) {
    val interactionSource = remember { androidx.compose.foundation.interaction.MutableInteractionSource() }
    val isPressed by interactionSource.collectIsPressedAsState()

    val borderColor = if (isPressed) {
        CyanActive
    } else {
        when (statusState) {
            "on" -> CyanActive
            "amber" -> AmberWarning
            else -> DarkOutline
        }
    }
    val statusText = when (statusState) {
        "on" -> "ON"
        "amber" -> "SYNC"
        else -> "OFF"
    }
    val statusColor = when (statusState) {
        "on" -> CyanActive
        "amber" -> AmberWarning
        else -> RedError
    }

    Box(
        modifier = Modifier
            .fillMaxWidth()
            .height(96.dp)
            .clip(RoundedCornerShape(12.dp))
            .background(DarkSurface)
            .border(1.dp, borderColor, RoundedCornerShape(12.dp))
            .clickable(
                interactionSource = interactionSource,
                indication = LocalIndication.current,
                onClick = onClick
            )
            .padding(10.dp)
    ) {
        // Index number top-left (mono style)
        Text(
            text = index,
            color = Color(0xFF6E6E78),
            fontFamily = JetBrainsMono,
            fontSize = 11.sp,
            fontWeight = FontWeight.Bold,
            modifier = Modifier.align(Alignment.TopStart)
        )

        // Status pill top-right
        Row(
            modifier = Modifier.align(Alignment.TopEnd),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(4.dp)
        ) {
            Box(
                modifier = Modifier
                    .size(5.dp)
                    .clip(CircleShape)
                    .background(statusColor)
            )
            Text(
                text = statusText,
                color = statusColor,
                fontFamily = JetBrainsMono,
                fontSize = 9.sp,
                fontWeight = FontWeight.Bold
            )
        }

        // Center visual containing title, icon and description (stacked vertically)
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .align(Alignment.BottomStart),
            verticalArrangement = Arrangement.spacedBy(1.dp)
        ) {
            Text(
                text = iconText,
                color = if (statusState != "idle") statusColor else TextPrimary,
                fontFamily = JetBrainsMono,
                fontSize = 15.sp,
                fontWeight = FontWeight.Bold
            )
            Text(
                text = title.uppercase(),
                color = TextPrimary,
                style = com.example.ui.theme.ModuleTitleTextStyle
            )
            Text(
                text = description,
                color = Color(0xFF6E6E78),
                style = MaterialTheme.typography.bodySmall,
                fontSize = 10.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                lineHeight = 11.sp
            )
        }
    }
}

@Composable
fun QuickCommandChip(
    label: String,
    onClick: () -> Unit
) {
    val interactionSource = remember { androidx.compose.foundation.interaction.MutableInteractionSource() }
    val isPressed by interactionSource.collectIsPressedAsState()
    
    val borderCol = if (isPressed) CyanActive else DarkOutline
    val textCol = if (isPressed) CyanActive else TextPrimary

    Box(
        modifier = Modifier
            .clip(CircleShape)
            .background(DarkSurface)
            .border(1.dp, borderCol, CircleShape)
            .clickable(
                interactionSource = interactionSource,
                indication = LocalIndication.current,
                onClick = onClick
            )
            .padding(horizontal = 14.dp, vertical = 6.dp),
        contentAlignment = Alignment.Center
    ) {
        Text(
            text = label.uppercase(),
            fontFamily = JetBrainsMono,
            fontSize = 9.sp,
            fontWeight = FontWeight.Bold,
            color = textCol
        )
    }
}

@Composable
fun DesktopMainLayout(
    connStatus: ConnectionStatus,
    deviceName: String,
    isServiceActive: Boolean,
    onServiceActiveChange: (Boolean) -> Unit,
    mediaState: MediaState?,
    fileSendingProgress: Float?,
    fileSendingStatus: String?,
    repositoryLogs: List<String>,
    pulseAlpha: Float,
    pulseScale: Float,
    rotationDegrees: Float,
    onPickFile: () -> Unit,
    onShowMediaControls: () -> Unit,
    onShowWorkstationSelector: () -> Unit,
    onNavigateToTab: (String) -> Unit,
    onSendClipboardText: (String) -> Unit,
    onExecuteCommand: (String) -> Unit,
    currentTab: String,
    context: Context
) {
    val linkColor by animateColorAsState(
        targetValue = when (connStatus) {
            ConnectionStatus.CONNECTED -> CyanActive
            ConnectionStatus.CONNECTING -> AmberWarning
            ConnectionStatus.DISCONNECTED -> RedError
        },
        animationSpec = tween(durationMillis = 300, easing = LinearOutSlowInEasing),
        label = "LinkColor"
    )

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(Color.Black)
    ) {
        // Main 3-Column Content (Row)
        Row(
            modifier = Modifier
                .weight(1f)
                .fillMaxWidth()
        ) {
            // -------------------------------------------------------------------------
            // 1. Sidebar Esquerda (lista de módulos numerados) - Width: 240dp
            // -------------------------------------------------------------------------
            Column(
                modifier = Modifier
                    .width(240.dp)
                    .fillMaxHeight()
                    .drawBehind {
                        drawLine(
                            color = DarkOutline,
                            start = Offset(size.width - 0.5.dp.toPx(), 0f),
                            end = Offset(size.width - 0.5.dp.toPx(), size.height),
                            strokeWidth = 1.dp.toPx()
                        )
                    }
                    .background(Color.Black)
                    .padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp)
            ) {
                // Editorial design for Sidebar Header
                Column(
                    modifier = Modifier.fillMaxWidth(),
                    verticalArrangement = Arrangement.spacedBy(2.dp)
                ) {
                    Text(
                        text = "HYPRLINK CORE",
                        style = MaterialTheme.typography.labelSmall,
                        fontFamily = JetBrainsMono,
                        fontWeight = FontWeight.Bold,
                        color = TextSecondary,
                        letterSpacing = 2.sp
                    )
                    Text(
                        text = "MÓDULOS",
                        style = MaterialTheme.typography.titleMedium,
                        fontFamily = ArchivoBlack,
                        color = TextPrimary,
                        letterSpacing = (-0.5).sp
                    )
                    Spacer(modifier = Modifier.height(4.dp))
                    Box(modifier = Modifier.fillMaxWidth().height(1.dp).background(DarkOutline))
                }

                // Modules list styled elegantly in uppercase JetBrains Mono
                val sidebarModules = listOf(
                    Triple("00", "CORE", "DASHBOARD GERAL"),
                    Triple("01", "MEDIA", "MPRIS & VOLUME"),
                    Triple("02", "FILES", "SFTP TRANSFERS"),
                    Triple("03", "TRACK", "TOUCHPAD & RATO"),
                    Triple("04", "CLIP", "CLIPBOARD SYNC"),
                    Triple("05", "CONTROL", "HYPRLAND IPC"),
                    Triple("06", "CONFIG", "DEFINIÇÕES")
                )

                Column(
                    modifier = Modifier.fillMaxWidth(),
                    verticalArrangement = Arrangement.spacedBy(10.dp)
                ) {
                    sidebarModules.forEach { (num, name, subtitle) ->
                        val isSelected = when (num) {
                            "00" -> currentTab == "dashboard"
                            "01" -> currentTab == "audio"
                            "02" -> currentTab == "transfers"
                            "03" -> currentTab == "touchpad"
                            "04" -> false
                            "05" -> currentTab == "desktop"
                            "06" -> currentTab == "notifications"
                            else -> false
                        }
                        val itemBorderColor = if (isSelected) CyanActive else Color.Transparent
                        
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clip(RoundedCornerShape(10.dp))
                                .background(if (isSelected) DarkSurface else Color.Transparent)
                                .border(1.dp, itemBorderColor, RoundedCornerShape(10.dp))
                                .clickable {
                                    when (num) {
                                        "00" -> onNavigateToTab("dashboard")
                                        "01" -> onNavigateToTab("audio")
                                        "02" -> onNavigateToTab("transfers")
                                        "03" -> onNavigateToTab("touchpad")
                                        "04" -> onSendClipboardText("")
                                        "05" -> onNavigateToTab("desktop")
                                        "06" -> onNavigateToTab("notifications")
                                    }
                                }
                                .padding(horizontal = 12.dp, vertical = 10.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(10.dp)
                        ) {
                            Text(
                                text = num,
                                fontFamily = JetBrainsMono,
                                fontSize = 11.sp,
                                fontWeight = FontWeight.Bold,
                                color = if (isSelected) CyanActive else TextSecondary
                            )
                            Column {
                                Text(
                                    text = name,
                                    fontFamily = JetBrainsMono,
                                    fontSize = 12.sp,
                                    fontWeight = FontWeight.Bold,
                                    color = if (isSelected) CyanActive else TextPrimary,
                                    letterSpacing = 1.sp
                                )
                                Text(
                                    text = subtitle,
                                    fontFamily = JetBrainsMono,
                                    fontSize = 8.sp,
                                    color = TextSecondary
                                )
                            }
                        }
                    }
                }
                
                Spacer(modifier = Modifier.weight(1f))
                
                // Active workstation quick selector at the bottom of left sidebar
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(10.dp))
                        .background(DarkSurface)
                        .border(1.dp, DarkOutline, RoundedCornerShape(10.dp))
                        .clickable { onShowWorkstationSelector() }
                        .padding(10.dp),
                    verticalArrangement = Arrangement.spacedBy(4.dp)
                ) {
                    Row(
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        Text(
                            text = "ESTAÇÃO",
                            fontFamily = JetBrainsMono,
                            fontSize = 8.sp,
                            fontWeight = FontWeight.Bold,
                            color = TextSecondary
                        )
                        Box(
                            modifier = Modifier
                                .size(6.dp)
                                .clip(CircleShape)
                                .background(linkColor)
                        )
                    }
                    Text(
                        text = deviceName,
                        style = MaterialTheme.typography.bodyMedium,
                        fontWeight = FontWeight.Bold,
                        color = TextPrimary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis
                    )
                    Text(
                        text = if (connStatus == ConnectionStatus.CONNECTED) "CONECTADO" else "DESCONECTADO",
                        fontFamily = JetBrainsMono,
                        fontSize = 8.sp,
                        fontWeight = FontWeight.Bold,
                        color = linkColor
                    )
                }
            }

            // -------------------------------------------------------------------------
            // 2. Coluna Centro (Masthead + Cartões em grelha 3x2 + Chips de comando)
            // -------------------------------------------------------------------------
            Column(
                modifier = Modifier
                    .weight(1.5f)
                    .fillMaxHeight()
                    .background(Color.Black)
                    .padding(24.dp)
                    .verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(20.dp)
            ) {
                if (currentTab == "dashboard") {
                    // MASTHEAD EDITORIAL
                    Column(
                        modifier = Modifier.fillMaxWidth(),
                        verticalArrangement = Arrangement.spacedBy(4.dp)
                    ) {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Text(
                                text = "WORKSTATION DESKTOP INTEGRATION",
                                style = MaterialTheme.typography.labelSmall,
                                fontFamily = JetBrainsMono,
                                color = TextSecondary,
                                letterSpacing = 2.sp
                            )
                            Text(
                                text = "Nº 001 · PORTUGAL",
                                style = MaterialTheme.typography.labelSmall,
                                fontFamily = JetBrainsMono,
                                color = TextSecondary
                            )
                        }

                        // Big Animated Logo: HYPR[LINK]
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            verticalAlignment = Alignment.Bottom
                        ) {
                            Text(
                                text = "HYPR",
                                style = com.example.ui.theme.LogoTextStyle.copy(
                                    letterSpacing = (-2).sp
                                ),
                                color = TextPrimary,
                                modifier = Modifier.alignByBaseline()
                            )
                            Text(
                                text = "LINK",
                                style = com.example.ui.theme.LogoTextStyle.copy(
                                    letterSpacing = (-2).sp,
                                    shadow = Shadow(
                                        color = linkColor.copy(alpha = 0.5f),
                                        offset = Offset(0f, 0f),
                                        blurRadius = 12f
                                    )
                                ),
                                color = linkColor,
                                modifier = Modifier.alignByBaseline()
                            )
                        }
                        
                        Box(modifier = Modifier.fillMaxWidth().height(1.dp).background(DarkOutline))
                    }

                    // GRELHA 3x2 DE MÓDULOS
                    Column(
                        modifier = Modifier.fillMaxWidth(),
                        verticalArrangement = Arrangement.spacedBy(10.dp)
                    ) {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(10.dp)
                        ) {
                            Box(modifier = Modifier.weight(1f)) {
                                ModuleCard(
                                    index = "01",
                                    statusState = if (mediaState != null) "on" else "idle",
                                    iconText = "▷",
                                    title = "Media",
                                    description = "MPRIS e volume remoto",
                                    onClick = { onNavigateToTab("audio") }
                                )
                            }
                            Box(modifier = Modifier.weight(1f)) {
                                ModuleCard(
                                    index = "02",
                                    statusState = if (fileSendingProgress != null) "on" else "idle",
                                    iconText = "⧉",
                                    title = "Files",
                                    description = "Envio de ficheiros SFTP",
                                    onClick = { onNavigateToTab("transfers") }
                                )
                            }
                            Box(modifier = Modifier.weight(1f)) {
                                ModuleCard(
                                    index = "03",
                                    statusState = if (connStatus == ConnectionStatus.CONNECTED) "on" else "idle",
                                    iconText = "⌖",
                                    title = "Track",
                                    description = "Sensores e rato virtual",
                                    onClick = {
                                        onNavigateToTab("touchpad")
                                    }
                                )
                            }
                        }

                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(10.dp)
                        ) {
                            Box(modifier = Modifier.weight(1f)) {
                                ModuleCard(
                                    index = "04",
                                    statusState = if (connStatus == ConnectionStatus.CONNECTED) "amber" else "idle",
                                    iconText = "⎘",
                                    title = "Clip",
                                    description = "Área de transferência",
                                    onClick = { onSendClipboardText("") }
                                )
                            }
                            Box(modifier = Modifier.weight(1f)) {
                                ModuleCard(
                                    index = "05",
                                    statusState = if (connStatus == ConnectionStatus.CONNECTED) "on" else "idle",
                                    iconText = "⌘",
                                    title = "Control",
                                    description = "Hyprland IPC e atalhos",
                                    onClick = { onNavigateToTab("desktop") }
                                )
                            }
                            Box(modifier = Modifier.weight(1f)) {
                                ModuleCard(
                                    index = "06",
                                    statusState = "idle",
                                    iconText = "⚙",
                                    title = "Config",
                                    description = "Definições e permissões",
                                    onClick = { onNavigateToTab("notifications") }
                                )
                            }
                        }
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(10.dp)
                        ) {
                            Box(modifier = Modifier.weight(1f)) {
                                val isWebcamStreaming by WebcamStreamer.isStreaming.collectAsStateWithLifecycle()
                                ModuleCard(
                                    index = "07",
                                    statusState = if (isWebcamStreaming) "on" else "idle",
                                    iconText = "◎",
                                    title = "Webcam",
                                    description = "Câmara remota do PC",
                                    onClick = {
                                        if (isWebcamStreaming) {
                                            WebcamStreamer.stop()
                                        } else {
                                            // context is available?
                                        }
                                    }
                                )
                            }
                            Box(modifier = Modifier.weight(1f))
                            Box(modifier = Modifier.weight(1f))
                        }
                    }

                    // CHIPS DE COMANDO RÁPIDO
                    Column(
                        modifier = Modifier.fillMaxWidth(),
                        verticalArrangement = Arrangement.spacedBy(8.dp)
                    ) {
                        Text(
                            text = "COMANDOS DE SISTEMA IPC",
                            style = MaterialTheme.typography.labelSmall,
                            fontFamily = JetBrainsMono,
                            color = TextSecondary,
                            fontWeight = FontWeight.Bold
                        )
                        
                        val commands = listOf(
                            "RESTART HYPRLAND" to "dispatch exit",
                            "LOCK SCREEN" to "dispatch exec swaylock",
                            "RELOAD CONFIG" to "dispatch reload",
                            "TOGGLE WAYBAR" to "dispatch exec killall -USR1 waybar",
                            "LAUNCH TERMINAL" to "dispatch exec kitty"
                        )

                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .horizontalScroll(rememberScrollState()),
                            horizontalArrangement = Arrangement.spacedBy(8.dp)
                        ) {
                            commands.forEach { (label, cmd) ->
                                QuickCommandChip(label = label) {
                                    onExecuteCommand(cmd)
                                }
                            }
                        }
                    }

                    // Real-time diagnostics logger inside center column (Desktop version)
                    var showFullScreenConsole by remember { mutableStateOf(false) }
                    
                    if (showFullScreenConsole) {
                        Dialog(onDismissRequest = { showFullScreenConsole = false }) {
                            Surface(
                                modifier = Modifier
                                    .fillMaxWidth(0.8f)
                                    .fillMaxHeight(0.8f)
                                    .clip(RoundedCornerShape(12.dp)),
                                color = DarkSurface,
                                border = BorderStroke(1.dp, DarkOutline)
                            ) {
                                Column(
                                    modifier = Modifier
                                        .fillMaxSize()
                                        .padding(16.dp)
                                ) {
                                    Row(
                                        modifier = Modifier.fillMaxWidth(),
                                        horizontalArrangement = Arrangement.SpaceBetween,
                                        verticalAlignment = Alignment.CenterVertically
                                    ) {
                                        Text(
                                            text = "Terminal Diagnostics Console",
                                            style = MaterialTheme.typography.titleMedium,
                                            fontWeight = FontWeight.Bold,
                                            color = TextPrimary
                                        )
                                        Row(verticalAlignment = Alignment.CenterVertically) {
                                            IconButton(onClick = {
                                                val logText = ConnectionRepository.getFullLogText()
                                                val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                                                clipboard.setPrimaryClip(ClipData.newPlainText("HyprLink Diagnostics Logs", logText))
                                                Toast.makeText(context, "Logs copiados para a área de transferência!", Toast.LENGTH_SHORT).show()
                                            }) {
                                                Icon(Icons.Default.ContentCopy, contentDescription = "Copy Logs", tint = CyanActive)
                                            }
                                            IconButton(onClick = {
                                                val logText = ConnectionRepository.getFullLogText()
                                                val sendIntent = android.content.Intent().apply {
                                                    action = android.content.Intent.ACTION_SEND
                                                    putExtra(android.content.Intent.EXTRA_TITLE, "logs.md")
                                                    putExtra(android.content.Intent.EXTRA_TEXT, logText)
                                                    type = "text/plain"
                                                }
                                                val shareIntent = android.content.Intent.createChooser(sendIntent, "Exportar Logs (.md)")
                                                context.startActivity(shareIntent)
                                            }) {
                                                Icon(Icons.Default.Share, contentDescription = "Export Log", tint = TextPrimary)
                                            }
                                            IconButton(onClick = {
                                                ConnectionRepository.clearLogs()
                                            }) {
                                                Icon(Icons.Default.Delete, contentDescription = "Clear Logs", tint = TextPrimary)
                                            }
                                            IconButton(onClick = { showFullScreenConsole = false }) {
                                                Icon(Icons.Default.Close, contentDescription = "Close", tint = TextPrimary)
                                            }
                                        }
                                    }
                                    Spacer(modifier = Modifier.height(8.dp))
                                    Box(
                                        modifier = Modifier
                                            .fillMaxWidth()
                                            .weight(1f)
                                            .clip(RoundedCornerShape(8.dp))
                                            .background(Color.Black)
                                            .border(1.dp, DarkOutline, RoundedCornerShape(8.dp))
                                            .padding(10.dp)
                                    ) {
                                        val logScrollState = rememberScrollState()
                                        LaunchedEffect(repositoryLogs.size) {
                                            logScrollState.animateScrollTo(logScrollState.maxValue)
                                        }

                                        SelectionContainer {
                                            Column(
                                                modifier = Modifier
                                                    .fillMaxSize()
                                                    .verticalScroll(logScrollState),
                                                verticalArrangement = Arrangement.spacedBy(4.dp)
                                            ) {
                                                if (repositoryLogs.isEmpty()) {
                                                    Text(
                                                        text = "[SYSTEM] Console de diagnóstico pronto. À espera de conexões...",
                                                        color = TextSecondary,
                                                        style = MaterialTheme.typography.bodySmall,
                                                        fontFamily = JetBrainsMono
                                                    )
                                                } else {
                                                    repositoryLogs.forEach { log ->
                                                        val logColor = when {
                                                            log.contains("[ERROR]") || log.contains("failed") -> RedError
                                                            log.contains("[SUCCESS]") || log.contains("Successfully") -> CyanActive
                                                            log.contains("[WARN]") -> AmberWarning
                                                            log.contains("[CLIPBOARD]") -> CyanActive.copy(alpha = 0.8f)
                                                            else -> TextSecondary
                                                        }
                                                        Text(
                                                            text = log,
                                                            color = logColor,
                                                            style = MaterialTheme.typography.bodySmall.copy(fontSize = 12.sp, lineHeight = 16.sp),
                                                            fontFamily = JetBrainsMono
                                                        )
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    Column(
                        modifier = Modifier
                            .fillMaxWidth()
                            .background(DarkSurface, RoundedCornerShape(12.dp))
                            .border(1.dp, DarkOutline, RoundedCornerShape(12.dp))
                            .padding(14.dp)
                    ) {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Text(
                                text = "Consola de Diagnóstico QUIC",
                                style = MaterialTheme.typography.bodyMedium,
                                fontWeight = FontWeight.Bold,
                                color = TextPrimary
                            )
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Text(
                                    text = "cb_mult_listen",
                                    style = MaterialTheme.typography.labelSmall,
                                    fontFamily = JetBrainsMono,
                                    color = linkColor
                                )
                                Spacer(modifier = Modifier.width(8.dp))
                                IconButton(
                                    onClick = { showFullScreenConsole = true },
                                    modifier = Modifier.size(24.dp)
                                ) {
                                    Icon(
                                        imageVector = Icons.Default.List,
                                        contentDescription = "Terminal",
                                        tint = CyanActive,
                                        modifier = Modifier.size(18.dp)
                                    )
                                }
                            }
                        }
                        Spacer(modifier = Modifier.height(8.dp))
                        Box(
                            modifier = Modifier
                                .fillMaxWidth()
                                .height(100.dp)
                                .background(Color.Black, RoundedCornerShape(6.dp))
                                .border(1.dp, DarkOutline, RoundedCornerShape(6.dp))
                                .padding(8.dp)
                        ) {
                            Column(
                                modifier = Modifier
                                    .fillMaxSize()
                                    .verticalScroll(rememberScrollState()),
                                verticalArrangement = Arrangement.spacedBy(2.dp)
                            ) {
                                if (repositoryLogs.isEmpty()) {
                                    Text(
                                        text = "[SYS] À espera de pacotes CBOR...",
                                        color = TextSecondary,
                                        style = MaterialTheme.typography.labelSmall,
                                        fontFamily = JetBrainsMono
                                    )
                                } else {
                                    repositoryLogs.takeLast(10).forEach { log ->
                                        Text(
                                            text = log,
                                            color = when {
                                                log.contains("[ERROR]") -> RedError
                                                log.contains("[SUCCESS]") -> CyanActive
                                                log.contains("[WARN]") -> AmberWarning
                                                else -> TextSecondary
                                            },
                                            style = MaterialTheme.typography.labelSmall,
                                            fontFamily = JetBrainsMono
                                        )
                                    }
                                }
                            }
                        }
                    }
                } else if (currentTab == "desktop") {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Text(
                            text = "HYPRLAND MISSION CONTROL",
                            style = MaterialTheme.typography.titleMedium,
                            fontFamily = ArchivoBlack,
                            color = TextPrimary
                        )
                        TextButton(onClick = { onNavigateToTab("dashboard") }) {
                            Text("< VOLTAR", fontFamily = JetBrainsMono, fontSize = 10.sp, color = CyanActive)
                        }
                    }
                    Box(modifier = Modifier.fillMaxWidth().height(1.dp).background(DarkOutline))
                    MissionControlScreen(modifier = Modifier.fillMaxWidth())
                } else if (currentTab == "transfers") {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Text(
                            text = "GESTOR DE TRANSFERÊNCIAS",
                            style = MaterialTheme.typography.titleMedium,
                            fontFamily = ArchivoBlack,
                            color = TextPrimary
                        )
                        TextButton(onClick = { onNavigateToTab("dashboard") }) {
                            Text("< VOLTAR", fontFamily = JetBrainsMono, fontSize = 10.sp, color = CyanActive)
                        }
                    }
                    Box(modifier = Modifier.fillMaxWidth().height(1.dp).background(DarkOutline))
                    TransfersScreen(onBack = { onNavigateToTab("dashboard") }, modifier = Modifier.fillMaxWidth())
                } else if (currentTab == "notifications") {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Text(
                            text = "NOTIFICAÇÕES & SEGURANÇA",
                            style = MaterialTheme.typography.titleMedium,
                            fontFamily = ArchivoBlack,
                            color = TextPrimary
                        )
                        TextButton(onClick = { onNavigateToTab("dashboard") }) {
                            Text("< VOLTAR", fontFamily = JetBrainsMono, fontSize = 10.sp, color = CyanActive)
                        }
                    }
                    Box(modifier = Modifier.fillMaxWidth().height(1.dp).background(DarkOutline))
                    NotificationsSettingsScreen(onBack = { onNavigateToTab("dashboard") }, modifier = Modifier.fillMaxWidth())
                } else if (currentTab == "touchpad") {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Text(
                            text = "RATO & TECLADO VIRTUAL",
                            style = MaterialTheme.typography.titleMedium,
                            fontFamily = ArchivoBlack,
                            color = TextPrimary
                        )
                        TextButton(onClick = { onNavigateToTab("dashboard") }) {
                            Text("< VOLTAR", fontFamily = JetBrainsMono, fontSize = 10.sp, color = CyanActive)
                        }
                    }
                    Box(modifier = Modifier.fillMaxWidth().height(1.dp).background(DarkOutline))
                    TouchpadScreen(onBack = { onNavigateToTab("dashboard") }, modifier = Modifier.fillMaxWidth())
                } else if (currentTab == "audio") {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Text(
                            text = "CONTROLO DE ÁUDIO",
                            style = MaterialTheme.typography.titleMedium,
                            fontFamily = ArchivoBlack,
                            color = TextPrimary
                        )
                        TextButton(onClick = { onNavigateToTab("dashboard") }) {
                            Text("< VOLTAR", fontFamily = JetBrainsMono, fontSize = 10.sp, color = CyanActive)
                        }
                    }
                    Box(modifier = Modifier.fillMaxWidth().height(1.dp).background(DarkOutline))
                    AudioScreen(onBack = { onNavigateToTab("dashboard") }, modifier = Modifier.fillMaxWidth())
                }
            }

            // -------------------------------------------------------------------------
            // 3. Coluna Direita (Telemetria do dispositivo: bateria, armazenamento, sensores)
            // -------------------------------------------------------------------------
            Column(
                modifier = Modifier
                    .width(260.dp)
                    .fillMaxHeight()
                    .drawBehind {
                        drawLine(
                            color = DarkOutline,
                            start = Offset(0.5.dp.toPx(), 0f),
                            end = Offset(0.5.dp.toPx(), size.height),
                            strokeWidth = 1.dp.toPx()
                        )
                    }
                    .background(Color.Black)
                    .padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(20.dp)
            ) {
                Column(
                    modifier = Modifier.fillMaxWidth(),
                    verticalArrangement = Arrangement.spacedBy(2.dp)
                ) {
                    Text(
                        text = "ESTADO DO HARDWARE",
                        style = MaterialTheme.typography.labelSmall,
                        fontFamily = JetBrainsMono,
                        fontWeight = FontWeight.Bold,
                        color = TextSecondary
                    )
                    Text(
                        text = "TELEMETRIA",
                        style = MaterialTheme.typography.titleMedium,
                        fontFamily = ArchivoBlack,
                        color = TextPrimary
                    )
                    Box(modifier = Modifier.fillMaxWidth().height(1.dp).background(DarkOutline))
                }

                // Battery Status
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(DarkSurface, RoundedCornerShape(10.dp))
                        .border(1.dp, DarkOutline, RoundedCornerShape(10.dp))
                        .padding(12.dp),
                    verticalArrangement = Arrangement.spacedBy(6.dp)
                ) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Text(
                            text = "Bateria",
                            style = MaterialTheme.typography.bodyMedium,
                            color = TextPrimary
                        )
                        Text(
                            text = "100%", // Simulated desktop status
                            fontFamily = JetBrainsMono,
                            fontWeight = FontWeight.Bold,
                            color = CyanActive
                        )
                    }
                    LinearProgressIndicator(
                        progress = 1.0f,
                        modifier = Modifier.fillMaxWidth().height(4.dp).clip(CircleShape),
                        color = CyanActive,
                        trackColor = DarkOutline
                    )
                }

                // Storage Status
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(DarkSurface, RoundedCornerShape(10.dp))
                        .border(1.dp, DarkOutline, RoundedCornerShape(10.dp))
                        .padding(12.dp),
                    verticalArrangement = Arrangement.spacedBy(6.dp)
                ) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Text(
                            text = "Armazenamento",
                            style = MaterialTheme.typography.bodyMedium,
                            color = TextPrimary
                        )
                        Text(
                            text = "42%",
                            fontFamily = JetBrainsMono,
                            fontWeight = FontWeight.Bold,
                            color = TextPrimary
                        )
                    }
                    LinearProgressIndicator(
                        progress = 0.42f,
                        modifier = Modifier.fillMaxWidth().height(4.dp).clip(CircleShape),
                        color = CyanActive,
                        trackColor = DarkOutline
                    )
                    Text(
                        text = "Livre: 243 GB de 512 GB",
                        fontFamily = JetBrainsMono,
                        fontSize = 8.sp,
                        color = TextSecondary
                    )
                }

                // System Active Sensors
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(DarkSurface, RoundedCornerShape(10.dp))
                        .border(1.dp, DarkOutline, RoundedCornerShape(10.dp))
                        .padding(12.dp),
                    verticalArrangement = Arrangement.spacedBy(10.dp)
                ) {
                    Text(
                        text = "Sensores Ativos",
                        style = MaterialTheme.typography.labelSmall,
                        fontFamily = JetBrainsMono,
                        fontWeight = FontWeight.Bold,
                        color = TextSecondary
                    )
                    
                    val sensors = listOf(
                        "Uptime" to "14h 32m",
                        "Cpu Temp" to "44°C",
                        "Gpu Temp" to "41°C",
                        "Fanspeed" to "1200 RPM",
                        "Memória" to "8.2 / 16.0 GB"
                    )

                    sensors.forEach { (name, value) ->
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween
                        ) {
                            Text(name, style = MaterialTheme.typography.bodyMedium, color = TextSecondary)
                            Text(value, fontFamily = JetBrainsMono, fontWeight = FontWeight.Bold, color = TextPrimary)
                        }
                    }
                }
            }
        }

        // -------------------------------------------------------------------------
        // 4. Rodapé Fino (Stats de rede em tempo real: rx/tx, latência, TLS)
        // -------------------------------------------------------------------------
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .drawBehind {
                    drawLine(
                        color = DarkOutline,
                        start = Offset(0f, 0.5.dp.toPx()),
                        end = Offset(size.width, 0.5.dp.toPx()),
                        strokeWidth = 1.dp.toPx()
                    )
                }
                .background(Color.Black)
                .padding(horizontal = 24.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(
                text = "HYPRLINK WORKSTATION INTEGRATION DEPLOYED",
                fontFamily = JetBrainsMono,
                fontSize = 8.sp,
                color = TextSecondary,
                letterSpacing = 1.sp
            )
            
            Row(
                horizontalArrangement = Arrangement.spacedBy(16.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text(
                    text = "RX: 1.4 MB/s",
                    fontFamily = JetBrainsMono,
                    fontSize = 8.sp,
                    color = CyanActive
                )
                Text(
                    text = "TX: 124 KB/s",
                    fontFamily = JetBrainsMono,
                    fontSize = 8.sp,
                    color = CyanActive
                )
                Text(
                    text = "LATÊNCIA: 4 ms",
                    fontFamily = JetBrainsMono,
                    fontSize = 8.sp,
                    color = linkColor
                )
                Text(
                    text = if (connStatus == ConnectionStatus.CONNECTED) "TLS_ECDHE_RSA" else "TLS_INATIVO",
                    fontFamily = JetBrainsMono,
                    fontSize = 8.sp,
                    color = linkColor
                )
            }
        }
    }
}

// Helper query extensions to read SAF metadata in helper functions
private fun getFileName(context: Context, uri: Uri): String? {
    var result: String? = null
    if (uri.scheme == "content") {
        val cursor = context.contentResolver.query(uri, null, null, null, null)
        try {
            if (cursor != null && cursor.moveToFirst()) {
                val index = cursor.getColumnIndex(android.provider.OpenableColumns.DISPLAY_NAME)
                if (index != -1) {
                    result = cursor.getString(index)
                }
            }
        } finally {
            cursor?.close()
        }
    }
    if (result == null) {
        result = uri.path
        val cut = result?.lastIndexOf('/')
        if (cut != null && cut != -1) {
            result = result.substring(cut + 1)
        }
    }
    return result
}

private fun getFileSize(context: Context, uri: Uri): Long? {
    var result: Long? = null
    if (uri.scheme == "content") {
        val cursor = context.contentResolver.query(uri, null, null, null, null)
        try {
            if (cursor != null && cursor.moveToFirst()) {
                val index = cursor.getColumnIndex(android.provider.OpenableColumns.SIZE)
                if (index != -1) {
                    result = cursor.getLong(index)
                }
            }
        } finally {
            cursor?.close()
        }
    }
    return result
}

data class HyprWorkspace(
    val id: Int,
    val name: String,
    val monitor: String,
    val windows: Int,
    val hasFullscreen: Boolean,
    val lastWindow: String,
    val lastWindowTitle: String
)

data class HyprClient(
    val address: String,
    val title: String,
    val clazz: String,
    val workspaceId: Int,
    val workspaceName: String,
    val floating: Boolean,
    val fullscreen: Boolean,
    val pid: Int,
    val hidden: Boolean,
    val mapped: Boolean
)

@Composable
fun MissionControlScreen(modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val coroutineScope = rememberCoroutineScope()
    
    val connStatus by ConnectionRepository.connectionStatus.collectAsStateWithLifecycle()
    val isDemo = connStatus != ConnectionStatus.CONNECTED

    // Demo Mode States
    var demoWorkspaces by remember {
        mutableStateOf(
            listOf(
                HyprWorkspace(1, "1", "DP-1", 2, false, "firefox", "Firefox - Google Search"),
                HyprWorkspace(2, "2", "DP-1", 1, false, "code", "Visual Studio Code - main.rs"),
                HyprWorkspace(3, "3", "DP-1", 1, false, "alacritty", "Terminal"),
                HyprWorkspace(4, "4", "DP-1", 0, false, "", "")
            )
        )
    }
    
    var demoClients by remember {
        mutableStateOf(
            listOf(
                HyprClient("0x555555abcd10", "Firefox - Google Search", "firefox", 1, "1", false, false, 1234, false, true),
                HyprClient("0x555555abcd20", "Terminal - cargo run", "alacritty", 1, "1", false, false, 1235, false, true),
                HyprClient("0x555555abcd30", "Visual Studio Code - main.rs", "code", 2, "2", false, false, 1236, false, true),
                HyprClient("0x555555abcd40", "Spotify - Lo-Fi Beats", "spotify", 3, "3", false, false, 1237, false, true)
            )
        )
    }

    var realWorkspaces by remember { mutableStateOf<List<HyprWorkspace>>(emptyList()) }
    var realClients by remember { mutableStateOf<List<HyprClient>>(emptyList()) }
    
    val workspaces = if (isDemo) {
        demoWorkspaces.map { ws ->
            ws.copy(windows = demoClients.count { it.workspaceId == ws.id })
        }
    } else {
        realWorkspaces
    }
    
    val clients = if (isDemo) demoClients else realClients

    var isLoading by remember { mutableStateOf(false) }
    var errorMessage by remember { mutableStateOf<String?>(null) }
    var clientToClose by remember { mutableStateOf<HyprClient?>(null) }
    
    // Parser helpers
    fun parseWorkspaces(jsonStr: String): List<HyprWorkspace> {
        val list = mutableListOf<HyprWorkspace>()
        if (jsonStr.isBlank()) return list
        try {
            val array = org.json.JSONArray(jsonStr)
            for (i in 0 until array.length()) {
                val obj = array.getJSONObject(i)
                list.add(
                    HyprWorkspace(
                        id = obj.optInt("id", 0),
                        name = obj.optString("name", ""),
                        monitor = obj.optString("monitor", ""),
                        windows = obj.optInt("windows", 0),
                        hasFullscreen = obj.optBoolean("hasfullscreen", false),
                        lastWindow = obj.optString("lastwindow", ""),
                        lastWindowTitle = obj.optString("lastwindowtitle", "")
                    )
                )
            }
        } catch (e: Exception) {
            e.printStackTrace()
        }
        return list
    }

    fun parseClients(jsonStr: String): List<HyprClient> {
        val list = mutableListOf<HyprClient>()
        if (jsonStr.isBlank()) return list
        try {
            val array = org.json.JSONArray(jsonStr)
            for (i in 0 until array.length()) {
                val obj = array.getJSONObject(i)
                val mapped = obj.optBoolean("mapped", true)
                if (!mapped) continue
                
                val wsObj = obj.optJSONObject("workspace")
                val wsId = wsObj?.optInt("id", 0) ?: 0
                val wsName = wsObj?.optString("name", "") ?: ""
                
                list.add(
                    HyprClient(
                        address = obj.optString("address", ""),
                        title = obj.optString("title", ""),
                        clazz = obj.optString("class", ""),
                        workspaceId = wsId,
                        workspaceName = wsName,
                        floating = obj.optBoolean("floating", false),
                        fullscreen = obj.optInt("fullscreen", 0) > 0,
                        pid = obj.optInt("pid", 0),
                        hidden = obj.optBoolean("hidden", false),
                        mapped = mapped
                    )
                )
            }
        } catch (e: Exception) {
            e.printStackTrace()
        }
        return list
    }

    fun refreshData() {
        if (isDemo) {
            // reset demo data to initial states
            demoWorkspaces = listOf(
                HyprWorkspace(1, "1", "DP-1", 2, false, "firefox", "Firefox - Google Search"),
                HyprWorkspace(2, "2", "DP-1", 1, false, "code", "Visual Studio Code - main.rs"),
                HyprWorkspace(3, "3", "DP-1", 1, false, "alacritty", "Terminal"),
                HyprWorkspace(4, "4", "DP-1", 0, false, "", "")
            )
            demoClients = listOf(
                HyprClient("0x555555abcd10", "Firefox - Google Search", "firefox", 1, "1", false, false, 1234, false, true),
                HyprClient("0x555555abcd20", "Terminal - cargo run", "alacritty", 1, "1", false, false, 1235, false, true),
                HyprClient("0x555555abcd30", "Visual Studio Code - main.rs", "code", 2, "2", false, false, 1236, false, true),
                HyprClient("0x555555abcd40", "Spotify - Lo-Fi Beats", "spotify", 3, "3", false, false, 1237, false, true)
            )
            return
        }
        coroutineScope.launch {
            isLoading = true
            try {
                val wsJson = ConnectionRepository.fetchHyprWorkspaces()
                val clJson = ConnectionRepository.fetchHyprClients()
                realWorkspaces = parseWorkspaces(wsJson).sortedBy { it.id }
                realClients = parseClients(clJson)
                errorMessage = null
            } catch (e: Exception) {
                if (realWorkspaces.isEmpty() && realClients.isEmpty()) {
                    errorMessage = e.message ?: "Erro ao obter estado do Hyprland"
                } else {
                    errorMessage = null
                }
                ConnectionRepository.appendLog("[HYPRLAND] Fetch error: ${e.message}")
            } finally {
                isLoading = false
            }
        }
    }
    
    fun executeCommand(cmd: String) {
        if (isDemo) {
            if (cmd.startsWith("closewindow address:")) {
                val addr = cmd.substringAfter("closewindow address:")
                val client = demoClients.find { it.address == addr }
                demoClients = demoClients.filter { it.address != addr }
                android.widget.Toast.makeText(context, "Janela '${client?.title ?: addr}' fechada (Simulado)", android.widget.Toast.LENGTH_SHORT).show()
            } else if (cmd.startsWith("focuswindow address:")) {
                val addr = cmd.substringAfter("focuswindow address:")
                val clientName = demoClients.find { it.address == addr }?.title ?: "Janela"
                android.widget.Toast.makeText(context, "Janela '$clientName' focada (Simulado)", android.widget.Toast.LENGTH_SHORT).show()
            } else if (cmd.startsWith("workspace ")) {
                val idStr = cmd.substringAfter("workspace ")
                android.widget.Toast.makeText(context, "Mudou para Workspace $idStr (Simulado)", android.widget.Toast.LENGTH_SHORT).show()
            }
            return
        }
        coroutineScope.launch {
            try {
                ConnectionRepository.appendLog("[HYPRLAND] Sending dispatch command: $cmd")
                val success = ConnectionRepository.dispatchHyprCommand(cmd)
                if (success) {
                    delay(600)
                    refreshData()
                }
            } catch (e: Exception) {
                errorMessage = e.message ?: "Erro ao executar comando"
                ConnectionRepository.appendLog("[HYPRLAND] Dispatch error: ${e.message}")
            }
        }
    }
    
    // Fetch initial data or update when mode changes with auto-refresh (polling) and real-time push events
    LaunchedEffect(isDemo) {
        if (isDemo) {
            refreshData()
        } else {
            // Fetch initial data
            refreshData()
            
            // Listen for real-time push events
            launch {
                ConnectionRepository.hyprEvents.collect { event ->
                    if (!isLoading) {
                        ConnectionRepository.appendLog("[HYPRLAND] Real-time event triggered refresh: $event")
                        refreshData()
                    }
                }
            }

            // Fallback poll every 5 seconds
            launch {
                while (isActive) {
                    delay(5_000)
                    if (!isLoading) {
                        refreshData()
                    }
                }
            }
        }
    }
    
    if (clientToClose != null) {
        AlertDialog(
            onDismissRequest = { clientToClose = null },
            title = { Text("FECHAR JANELA?", color = TextPrimary, fontFamily = JetBrainsMono, fontWeight = FontWeight.Bold) },
            text = { Text("Tem a certeza que deseja fechar a janela '${clientToClose?.title}'?", color = TextSecondary) },
            confirmButton = {
                TextButton(onClick = {
                    val client = clientToClose
                    if (client != null) {
                        executeCommand("closewindow address:${client.address}")
                    }
                    clientToClose = null
                }) {
                    Text("FECHAR", color = RedError, fontFamily = JetBrainsMono, fontWeight = FontWeight.Bold)
                }
            },
            dismissButton = {
                TextButton(onClick = { clientToClose = null }) {
                    Text("CANCELAR", color = TextSecondary, fontFamily = JetBrainsMono, fontWeight = FontWeight.Bold)
                }
            },
            containerColor = DarkSurface,
            textContentColor = TextSecondary,
            titleContentColor = TextPrimary
        )
    }
    
    Column(
        modifier = modifier
            .fillMaxSize()
            .testTag("mission_control_screen"),
        verticalArrangement = Arrangement.spacedBy(16.dp)
    ) {
        // Refresh & Status row
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Column {
                Text(
                    text = "MISSION CONTROL",
                    style = MaterialTheme.typography.titleLarge,
                    fontFamily = ArchivoBlack,
                    color = TextPrimary
                )
                Text(
                    text = "Gere os workspaces e janelas ativas",
                    style = MaterialTheme.typography.labelSmall,
                    fontFamily = JetBrainsMono,
                    color = TextSecondary
                )
            }
            
            IconButton(
                onClick = { refreshData() },
                modifier = Modifier
                    .background(DarkSurface, CircleShape)
                    .border(1.dp, DarkOutline, CircleShape)
                    .testTag("refresh_hypr_button")
            ) {
                if (isLoading) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(18.dp),
                        strokeWidth = 2.dp,
                        color = CyanActive
                    )
                } else {
                    Icon(
                        imageVector = Icons.Default.Refresh,
                        contentDescription = "Refresh",
                        tint = CyanActive,
                        modifier = Modifier.size(18.dp)
                    )
                }
            }
        }
        
        if (isDemo) {
            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(containerColor = AmberWarning.copy(alpha = 0.1f)),
                border = BorderStroke(1.dp, AmberWarning)
            ) {
                Row(
                    modifier = Modifier.padding(12.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Icon(
                        imageVector = Icons.Default.Info,
                        contentDescription = "Demo Mode",
                        tint = AmberWarning
                    )
                    Column {
                        Text(
                            text = "MODO DE DEMONSTRAÇÃO ATIVO",
                            fontWeight = FontWeight.Bold,
                            fontFamily = JetBrainsMono,
                            color = AmberWarning,
                            style = MaterialTheme.typography.titleSmall
                        )
                        Text(
                            text = "Workstation desligada. Interaja para testar.",
                            color = TextSecondary,
                            fontFamily = JetBrainsMono,
                            style = MaterialTheme.typography.bodySmall
                        )
                    }
                }
            }
        }
        
        if (errorMessage != null && !isDemo) {
            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(containerColor = RedError.copy(alpha = 0.1f)),
                border = BorderStroke(1.dp, RedError)
            ) {
                Row(
                    modifier = Modifier.padding(16.dp),
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Icon(
                        imageVector = Icons.Default.Warning,
                        contentDescription = "Erro",
                        tint = RedError
                    )
                    Column(modifier = Modifier.weight(1f)) {
                        Text(
                            text = "ERRO DE LIGAÇÃO",
                            fontWeight = FontWeight.Bold,
                            fontFamily = JetBrainsMono,
                            color = RedError,
                            style = MaterialTheme.typography.titleMedium
                        )
                        Text(
                            text = errorMessage ?: "",
                            color = TextSecondary,
                            fontFamily = JetBrainsMono,
                            style = MaterialTheme.typography.bodyMedium
                        )
                    }
                    Button(
                        onClick = { refreshData() },
                        colors = ButtonDefaults.buttonColors(
                            containerColor = DarkSurface,
                            contentColor = RedError
                        ),
                        border = BorderStroke(1.dp, RedError),
                        contentPadding = PaddingValues(horizontal = 12.dp, vertical = 6.dp),
                        shape = RoundedCornerShape(8.dp)
                    ) {
                        Text("REPETIR", style = MaterialTheme.typography.labelSmall, fontFamily = JetBrainsMono, fontWeight = FontWeight.Bold)
                    }
                }
            }
        }
        
        // Horizontal Workspaces list
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(
                text = "WORKSPACES",
                style = MaterialTheme.typography.labelSmall,
                fontFamily = JetBrainsMono,
                color = CyanActive,
                fontWeight = FontWeight.Bold
            )
            
            if (workspaces.isEmpty() && !isLoading) {
                Text(
                    text = "Nenhum workspace ativo.",
                    style = MaterialTheme.typography.bodyMedium,
                    fontFamily = JetBrainsMono,
                    color = TextSecondary
                )
            } else {
                LazyRow(
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                    modifier = Modifier.fillMaxWidth()
                ) {
                    items(workspaces) { ws ->
                        val hasWindows = ws.windows > 0
                        Card(
                            modifier = Modifier
                                .widthIn(min = 100.dp)
                                .clickable {
                                    executeCommand("workspace ${ws.id}")
                                }
                                .testTag("workspace_chip_${ws.id}"),
                            colors = CardDefaults.cardColors(
                                containerColor = if (hasWindows) DarkSurface else Color.Transparent
                            ),
                            border = BorderStroke(
                                1.dp,
                                if (hasWindows) CyanActive else DarkOutline
                            ),
                            shape = RoundedCornerShape(12.dp)
                        ) {
                            Column(
                                modifier = Modifier.padding(12.dp),
                                horizontalAlignment = Alignment.Start
                            ) {
                                Text(
                                    text = "WS ${ws.name}",
                                    style = MaterialTheme.typography.titleMedium,
                                    fontFamily = ArchivoBlack,
                                    color = if (hasWindows) CyanActive else TextPrimary
                                )
                                Text(
                                    text = if (ws.windows == 1) "1 janela" else "${ws.windows} janelas",
                                    style = MaterialTheme.typography.labelSmall,
                                    fontFamily = JetBrainsMono,
                                    color = TextSecondary,
                                    modifier = Modifier.padding(top = 2.dp)
                                )
                            }
                        }
                    }
                }
            }
        }
        
        // Grouped Clients list
        Column(
            modifier = Modifier.weight(1f),
            verticalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            Text(
                text = "JANELAS ATIVAS",
                style = MaterialTheme.typography.labelSmall,
                fontFamily = JetBrainsMono,
                color = CyanActive,
                fontWeight = FontWeight.Bold
            )
            
            if (clients.isEmpty() && !isLoading) {
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .weight(1f),
                    contentAlignment = Alignment.Center
                ) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Icon(
                            imageVector = Icons.Default.Info,
                            contentDescription = "Sem janelas",
                            tint = TextSecondary,
                            modifier = Modifier.size(48.dp)
                        )
                        Text(
                            text = "Nenhuma janela aberta",
                            style = MaterialTheme.typography.bodyLarge,
                            fontFamily = JetBrainsMono,
                            color = TextSecondary,
                            modifier = Modifier.padding(top = 8.dp)
                        )
                    }
                }
            } else {
                androidx.compose.foundation.lazy.LazyColumn(
                    modifier = Modifier.fillMaxSize(),
                    verticalArrangement = Arrangement.spacedBy(10.dp)
                ) {
                    val clientsByWorkspace = clients.groupBy { it.workspaceId }
                    // Sort workspaces by ID
                    val sortedWsIds = clientsByWorkspace.keys.sorted()
                    
                    sortedWsIds.forEach { wsId ->
                        val wsClients = clientsByWorkspace[wsId] ?: emptyList()
                        val wsName = wsClients.firstOrNull()?.workspaceName ?: "Workspace $wsId"
                        
                        item {
                            Text(
                                text = "WORKSPACE $wsName",
                                style = MaterialTheme.typography.labelSmall,
                                fontFamily = JetBrainsMono,
                                color = TextSecondary,
                                fontWeight = FontWeight.Bold,
                                modifier = Modifier.padding(top = 8.dp, bottom = 4.dp)
                            )
                        }
                        
                        items(wsClients) { client ->
                            Card(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .clickable {
                                        executeCommand("focuswindow address:${client.address}")
                                    }
                                    .testTag("client_item_${client.address}"),
                                colors = CardDefaults.cardColors(containerColor = DarkSurface),
                                border = BorderStroke(1.dp, DarkOutline),
                                shape = RoundedCornerShape(14.dp)
                            ) {
                                Row(
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .padding(14.dp),
                                    horizontalArrangement = Arrangement.SpaceBetween,
                                    verticalAlignment = Alignment.CenterVertically
                                ) {
                                    Column(modifier = Modifier.weight(1f)) {
                                        Text(
                                            text = client.title.ifEmpty { "Sem título" },
                                            style = MaterialTheme.typography.titleMedium,
                                            color = TextPrimary,
                                            fontWeight = FontWeight.Bold,
                                            maxLines = 1,
                                            overflow = TextOverflow.Ellipsis
                                        )
                                        Text(
                                            text = client.clazz,
                                            fontFamily = JetBrainsMono,
                                            style = MaterialTheme.typography.labelSmall,
                                            color = CyanActive,
                                            modifier = Modifier.padding(top = 2.dp)
                                        )
                                    }
                                    
                                    IconButton(
                                        onClick = { clientToClose = client },
                                        modifier = Modifier.size(36.dp)
                                    ) {
                                        Icon(
                                            imageVector = Icons.Default.Close,
                                            contentDescription = "Fechar Janela",
                                            tint = RedError,
                                            modifier = Modifier.size(18.dp)
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
fun TouchpadScreen(onBack: () -> Unit, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val coroutineScope = rememberCoroutineScope()
    val connStatus by ConnectionRepository.connectionStatus.collectAsStateWithLifecycle()
    val isDemo = connStatus != ConnectionStatus.CONNECTED

    // Touch accumulation states
    var accumulatedMoveX by remember { mutableStateOf(0f) }
    var accumulatedMoveY by remember { mutableStateOf(0f) }

    var accumulatedScrollX by remember { mutableStateOf(0f) }
    var accumulatedScrollY by remember { mutableStateOf(0f) }

    val sensitivity = 1.5f

    // Throttle loop for moving (~60 Hz)
    LaunchedEffect(isDemo) {
        if (!isDemo) {
            while (isActive) {
                delay(16)
                val dx = accumulatedMoveX
                val dy = accumulatedMoveY
                if (dx != 0f || dy != 0f) {
                    accumulatedMoveX = 0f
                    accumulatedMoveY = 0f
                    coroutineScope.launch {
                        ConnectionRepository.sendInputMove((dx * sensitivity).toInt(), (dy * sensitivity).toInt())
                    }
                }
            }
        }
    }

    // Throttle loop for scrolling (~60 Hz)
    LaunchedEffect(isDemo) {
        if (!isDemo) {
            while (isActive) {
                delay(16)
                val dx = accumulatedScrollX
                val dy = accumulatedScrollY
                if (dx != 0f || dy != 0f) {
                    accumulatedScrollX = 0f
                    accumulatedScrollY = 0f
                    coroutineScope.launch {
                        ConnectionRepository.sendInputScroll(dx.toInt(), dy.toInt())
                    }
                }
            }
        }
    }

    // Keyboard states
    val sentinelKeyboardValue = androidx.compose.ui.text.input.TextFieldValue(
        text = " ",
        selection = androidx.compose.ui.text.TextRange(1)
    )
    var keyboardText by remember { mutableStateOf(sentinelKeyboardValue) }
    val focusRequester = remember { FocusRequester() }
    val keyboardController = androidx.compose.ui.platform.LocalSoftwareKeyboardController.current

    Column(
        modifier = modifier
            .fillMaxSize()
            .background(Color.Black),
        verticalArrangement = Arrangement.spacedBy(16.dp)
    ) {
        // Upper Toolbar
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Column {
                Text(
                    text = "RATO & TECLADO VIRTUAL",
                    style = MaterialTheme.typography.titleMedium,
                    fontFamily = ArchivoBlack,
                    color = TextPrimary
                )
                Text(
                    text = if (isDemo) "MODO SIMULADO" else "TOUCHPAD SENSÍVEL",
                    fontFamily = JetBrainsMono,
                    fontSize = 10.sp,
                    color = if (isDemo) AmberWarning else CyanActive
                )
            }
            Row(
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                // Keyboard Toggle Button
                IconButton(
                    onClick = {
                        focusRequester.requestFocus()
                        keyboardController?.show()
                    },
                    modifier = Modifier
                        .size(40.dp)
                        .background(DarkSurface, CircleShape)
                        .border(1.dp, DarkOutline, CircleShape)
                        .testTag("keyboard_toggle_button")
                ) {
                    Icon(
                        imageVector = Icons.Default.Keyboard,
                        contentDescription = "Abrir Teclado",
                        tint = CyanActive,
                        modifier = Modifier.size(20.dp)
                    )
                }

                // Back Button
                TextButton(onClick = onBack) {
                    Text("< VOLTAR", fontFamily = JetBrainsMono, fontSize = 10.sp, color = CyanActive)
                }
            }
        }

        Box(modifier = Modifier.fillMaxWidth().height(1.dp).background(DarkOutline))

        if (isDemo) {
            // Disconnected Placeholder
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .weight(1f)
                    .clip(RoundedCornerShape(16.dp))
                    .background(DarkSurface)
                    .border(1.dp, RedError.copy(alpha = 0.3f), RoundedCornerShape(16.dp))
                    .padding(24.dp),
                contentAlignment = Alignment.Center
            ) {
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(12.dp)
                ) {
                    Icon(
                        imageVector = Icons.Default.Keyboard,
                        contentDescription = "Desconectado",
                        tint = RedError,
                        modifier = Modifier.size(64.dp)
                    )
                    Text(
                        text = "Liga a uma workstation primeiro.",
                        style = MaterialTheme.typography.titleMedium,
                        color = TextPrimary,
                        fontFamily = JetBrainsMono
                    )
                    Text(
                        text = "Os comandos de touchpad e teclado necessitam de uma ligação ativa.",
                        style = MaterialTheme.typography.bodySmall,
                        color = TextSecondary,
                        textAlign = TextAlign.Center
                    )
                }
            }
        } else {
            // Invisible BasicTextField to capture IME keyboard inputs
            androidx.compose.foundation.text.BasicTextField(
                value = keyboardText,
                onValueChange = { newVal ->
                    if (newVal.text.isEmpty()) {
                        coroutineScope.launch {
                            ConnectionRepository.sendInputKey("backspace")
                        }
                        keyboardText = sentinelKeyboardValue
                    } else if (newVal.text == " ") {
                        keyboardText = sentinelKeyboardValue
                    } else {
                        val typed = newVal.text.removePrefix(" ")
                        coroutineScope.launch {
                            ConnectionRepository.sendInputType(typed)
                        }
                        keyboardText = sentinelKeyboardValue
                    }
                },
                modifier = Modifier
                    .size(1.dp)
                    .alpha(0.01f)
                    .focusRequester(focusRequester)
                    .testTag("invisible_input_field"),
                keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(
                    capitalization = androidx.compose.ui.text.input.KeyboardCapitalization.None,
                    autoCorrect = false,
                    imeAction = androidx.compose.ui.text.input.ImeAction.Done
                ),
                keyboardActions = androidx.compose.foundation.text.KeyboardActions(
                    onDone = {
                        coroutineScope.launch {
                            ConnectionRepository.sendInputKey("enter")
                        }
                    }
                )
            )

            // Touchpad Surface
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .weight(1f)
                    .clip(RoundedCornerShape(16.dp))
                    .background(DarkSurface)
                    .border(1.dp, CyanActive.copy(alpha = 0.4f), RoundedCornerShape(16.dp))
                    .pointerInput(Unit) {
                        awaitPointerEventScope {
                            var isTracking = false
                            var maxFingers = 1
                            var totalDistance = 0f
                            var startTime = 0L
                            
                            while (true) {
                                val event = awaitPointerEvent()
                                val changes = event.changes
                                
                                if (!isTracking) {
                                    isTracking = true
                                    maxFingers = changes.size
                                    totalDistance = 0f
                                    startTime = System.currentTimeMillis()
                                } else {
                                    maxFingers = maxOf(maxFingers, changes.size)
                                }
                                
                                var deltaX = 0f
                                var deltaY = 0f
                                var hasMovement = false
                                for (change in changes) {
                                    if (change.pressed && change.previousPosition != null) {
                                        val dx = change.position.x - change.previousPosition.x
                                        val dy = change.position.y - change.previousPosition.y
                                        deltaX += dx
                                        deltaY += dy
                                        totalDistance += kotlin.math.sqrt(dx * dx + dy * dy)
                                        hasMovement = true
                                    }
                                }
                                
                                if (hasMovement) {
                                    if (maxFingers == 1) {
                                        accumulatedMoveX += deltaX
                                        accumulatedMoveY += deltaY
                                    } else {
                                        accumulatedScrollX += deltaX
                                        accumulatedScrollY += deltaY
                                    }
                                }
                                
                                changes.forEach { it.consume() }
                                
                                val allUp = changes.all { !it.pressed }
                                if (allUp) {
                                    val elapsed = System.currentTimeMillis() - startTime
                                    if (totalDistance < 15f && elapsed < 300) {
                                        if (maxFingers == 1) {
                                            coroutineScope.launch {
                                                ConnectionRepository.sendInputClick("left")
                                            }
                                        } else if (maxFingers >= 2) {
                                            coroutineScope.launch {
                                                ConnectionRepository.sendInputClick("right")
                                            }
                                        }
                                    }
                                    isTracking = false
                                }
                            }
                        }
                    }
                    .testTag("touchpad_surface")
                    .padding(24.dp),
                contentAlignment = Alignment.Center
            ) {
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    Icon(
                        imageVector = Icons.Default.Keyboard,
                        contentDescription = "Superfície Touchpad",
                        tint = CyanActive.copy(alpha = 0.6f),
                        modifier = Modifier.size(48.dp)
                    )
                    Text(
                        text = "Superfície de Toque",
                        style = MaterialTheme.typography.titleMedium,
                        color = TextPrimary,
                        fontFamily = JetBrainsMono
                    )
                    Text(
                        text = "Arraste 1 dedo para mover o cursor.\nTap curto = Clique esquerdo.\nTap com 2 dedos = Clique direito.\nArraste 2 dedos para scroll.",
                        style = MaterialTheme.typography.bodySmall,
                        color = TextSecondary,
                        textAlign = TextAlign.Center,
                        lineHeight = 16.sp
                    )
                }
            }
        }

        // Horizontal Row of special keys above bottom mouse buttons
        val specialKeys = listOf(
            "ESC" to "escape", "TAB" to "tab", "ENTER" to "enter", "BKSP" to "backspace", "ESP" to "space", "DEL" to "delete",
            "↑" to "up", "↓" to "down", "←" to "left", "→" to "right"
        )
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.spacedBy(6.dp)
        ) {
            specialKeys.forEach { (label, key) ->
                Box(
                    modifier = Modifier
                        .clip(RoundedCornerShape(8.dp))
                        .background(DarkSurface)
                        .border(1.dp, DarkOutline, RoundedCornerShape(8.dp))
                        .clickable(enabled = !isDemo) {
                            coroutineScope.launch {
                                ConnectionRepository.sendInputKey(key)
                            }
                        }
                        .testTag("special_key_${key}")
                        .padding(horizontal = 14.dp, vertical = 8.dp),
                    contentAlignment = Alignment.Center
                ) {
                    Text(
                        text = label,
                        fontFamily = JetBrainsMono,
                        fontSize = 11.sp,
                        fontWeight = FontWeight.Bold,
                        color = if (isDemo) TextSecondary else CyanActive
                    )
                }
            }
        }

        // Bottom Mouse Buttons
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(10.dp)
        ) {
            val buttons = listOf(
                "ESQUERDO" to "left",
                "MEIO" to "middle",
                "DIREITO" to "right"
            )
            buttons.forEach { (label, btn) ->
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .height(48.dp)
                        .clip(RoundedCornerShape(12.dp))
                        .background(DarkSurface)
                        .border(1.dp, if (isDemo) DarkOutline else CyanActive.copy(alpha = 0.5f), RoundedCornerShape(12.dp))
                        .clickable(enabled = !isDemo) {
                            coroutineScope.launch {
                                ConnectionRepository.sendInputClick(btn)
                            }
                        }
                        .testTag("mouse_click_${btn}"),
                    contentAlignment = Alignment.Center
                ) {
                    Text(
                        text = label,
                        fontFamily = JetBrainsMono,
                        fontSize = 11.sp,
                        fontWeight = FontWeight.Bold,
                        color = if (isDemo) TextSecondary else TextPrimary
                    )
                }
            }
        }
    }
}

data class HyprFeature(
    val id: Int,
    val title: String,
    val subtitle: String,
    val icon: ImageVector,
    val isEnabled: Boolean,
    val isDemoAllowed: Boolean = true
)

@Composable
fun FeatureGrid(
    connStatus: ConnectionStatus,
    isDemo: Boolean,
    onFeatureClick: (Int) -> Unit,
    modifier: Modifier = Modifier
) {
    val features = remember {
        listOf(
            HyprFeature(1, "Desktop", "Workspaces e janelas do PC", Icons.Default.Monitor, isEnabled = true),
            HyprFeature(2, "Partilha", "Enviar ficheiros e links", Icons.Default.Share, isEnabled = true),
            HyprFeature(3, "Clipboard", "Sincronizar área de transferência", Icons.Default.ContentPaste, isEnabled = true),
            HyprFeature(4, "Media", "Controlar o leitor do PC", Icons.Default.PlayCircle, isEnabled = true),
            HyprFeature(5, "Transferências", "Progresso e histórico de envios", Icons.Default.SwapVert, isEnabled = true),
            HyprFeature(6, "Touchpad & Teclado", "Telemóvel como input do PC", Icons.Default.Keyboard, isEnabled = true),
            HyprFeature(7, "Notificações", "Espelhar e responder no PC", Icons.Default.Notifications, isEnabled = true),
            HyprFeature(8, "Webcam", "Telemóvel como câmara do PC", Icons.Default.Videocam, isEnabled = false),
            HyprFeature(9, "Chamadas", "Atender com o áudio do PC", Icons.Default.Call, isEnabled = false),
            HyprFeature(10, "Espelhar Ecrã", "Ver o desktop no telemóvel", Icons.Default.Cast, isEnabled = false)
        )
    }

    val isInteractive = connStatus == ConnectionStatus.CONNECTED || isDemo

    Column(
        modifier = modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(10.dp)
    ) {
        val rows = features.chunked(2)
        rows.forEach { rowFeatures ->
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(10.dp)
            ) {
                rowFeatures.forEach { feature ->
                    val isFeatureEnabled = feature.isEnabled
                    val alphaValue = if (!isInteractive) 0.4f else if (!isFeatureEnabled) 0.5f else 1.0f

                    Card(
                        modifier = Modifier
                            .weight(1f)
                            .height(95.dp)
                            .alpha(alphaValue)
                            .clickable {
                                onFeatureClick(feature.id)
                            },
                        colors = CardDefaults.cardColors(
                            containerColor = MaterialTheme.colorScheme.surface
                        ),
                        shape = RoundedCornerShape(16.dp),
                        border = BorderStroke(
                            1.dp,
                            if (isInteractive && isFeatureEnabled) MaterialTheme.colorScheme.outline.copy(alpha = 0.8f) else MaterialTheme.colorScheme.outline.copy(alpha = 0.3f)
                        )
                    ) {
                        Box(modifier = Modifier.fillMaxSize()) {
                            Column(
                                modifier = Modifier
                                    .fillMaxSize()
                                    .padding(12.dp),
                                verticalArrangement = Arrangement.SpaceBetween
                            ) {
                                Row(
                                    modifier = Modifier.fillMaxWidth(),
                                    horizontalArrangement = Arrangement.SpaceBetween,
                                    verticalAlignment = Alignment.Top
                                ) {
                                    Icon(
                                        imageVector = feature.icon,
                                        contentDescription = feature.title,
                                        tint = if (isInteractive && isFeatureEnabled) MaterialTheme.colorScheme.secondary else MaterialTheme.colorScheme.onSurfaceVariant,
                                        modifier = Modifier.size(24.dp)
                                    )
                                    
                                    if (!isFeatureEnabled) {
                                        Box(
                                            modifier = Modifier
                                                .background(
                                                    color = MaterialTheme.colorScheme.outline.copy(alpha = 0.2f),
                                                    shape = RoundedCornerShape(4.dp)
                                                )
                                                .padding(horizontal = 4.dp, vertical = 2.dp)
                                        ) {
                                            Text(
                                                text = "EM BREVE",
                                                style = MaterialTheme.typography.labelSmall,
                                                fontSize = 7.5.sp,
                                                fontWeight = FontWeight.Bold,
                                                color = MaterialTheme.colorScheme.onSurfaceVariant
                                            )
                                        }
                                    }
                                }
                                
                                Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                                    Text(
                                        text = feature.title,
                                        style = MaterialTheme.typography.titleSmall,
                                        fontWeight = FontWeight.Bold,
                                        color = MaterialTheme.colorScheme.onSurface,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis
                                    )
                                    Text(
                                        text = feature.subtitle,
                                        style = MaterialTheme.typography.labelSmall,
                                        fontSize = 9.sp,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis
                                    )
                                }
                             }
                        }
                    }
                }
                if (rowFeatures.size < 2) {
                    Spacer(modifier = Modifier.weight(1f))
                }
            }
        }
    }
}

@Composable
fun CategoryHeader(
    title: String,
    code: String,
    icon: ImageVector,
    modifier: Modifier = Modifier,
    action: (@Composable () -> Unit)? = null
) {
    Row(
        modifier = modifier
            .fillMaxWidth()
            .padding(top = 18.dp, bottom = 10.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            Icon(
                imageVector = icon,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.secondary,
                modifier = Modifier.size(16.dp)
            )
            Text(
                text = title,
                style = MaterialTheme.typography.labelMedium,
                fontWeight = FontWeight.Bold,
                color = MaterialTheme.colorScheme.onSurface,
                letterSpacing = 1.sp
            )
            Text(
                text = "[$code]",
                style = MaterialTheme.typography.labelSmall,
                fontFamily = FontFamily.Monospace,
                color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f),
                fontSize = 9.sp
            )
        }
        action?.invoke()
    }
}

@Composable
fun TransfersScreen(
    onBack: () -> Unit,
    modifier: Modifier = Modifier
) {
    val context = LocalContext.current
    val coroutineScope = rememberCoroutineScope()
    val transfers by ConnectionRepository.transfers.collectAsStateWithLifecycle()
    val persistentHistory by remember(context) {
        ConnectionRepository.getPersistentHistory(context)
    }.collectAsStateWithLifecycle(initialValue = emptyList())

    val filePickerLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.GetContent()
    ) { uri ->
        if (uri != null) {
            coroutineScope.launch {
                val name = getFileName(context, uri) ?: "file.bin"
                val size = getFileSize(context, uri) ?: 0L
                ConnectionRepository.sendFile(context, uri, name, size)
            }
        }
    }

    Box(
        modifier = modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
    ) {
        Column(
            modifier = Modifier.fillMaxSize()
        ) {
            // Toolbar / Header
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(vertical = 12.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    IconButton(
                        onClick = onBack,
                        modifier = Modifier.testTag("transfers_back_button")
                    ) {
                        Icon(
                            imageVector = Icons.Default.ArrowBack,
                            contentDescription = "Voltar",
                            tint = MaterialTheme.colorScheme.onBackground
                        )
                    }
                    Spacer(modifier = Modifier.width(8.dp))
                    Column {
                        Text(
                            text = "TRANSFERÊNCIAS",
                            style = MaterialTheme.typography.titleMedium,
                            fontWeight = FontWeight.Bold,
                            color = MaterialTheme.colorScheme.onBackground
                        )
                        Text(
                            text = "Ficheiros partilhados na sessão",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant
                        )
                    }
                }
                
                TextButton(
                    onClick = { ConnectionRepository.clearTransfers() },
                    enabled = transfers.isNotEmpty(),
                    modifier = Modifier.testTag("transfers_clear_button")
                ) {
                    Icon(
                        imageVector = Icons.Default.Delete,
                        contentDescription = null,
                        modifier = Modifier.size(16.dp)
                    )
                    Spacer(modifier = Modifier.width(4.dp))
                    Text(
                        "Limpar",
                        style = MaterialTheme.typography.labelMedium
                    )
                }
            }

            if (transfers.isEmpty() && persistentHistory.isEmpty()) {
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .fillMaxWidth(),
                    contentAlignment = Alignment.Center
                ) {
                    Column(
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.spacedBy(12.dp)
                    ) {
                        Icon(
                            imageVector = Icons.Default.SwapVert,
                            contentDescription = null,
                            tint = MaterialTheme.colorScheme.outline.copy(alpha = 0.5f),
                            modifier = Modifier.size(64.dp)
                        )
                        Text(
                            text = "Nenhuma transferência registada",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            textAlign = TextAlign.Center
                        )
                    }
                }
            } else {
                androidx.compose.foundation.lazy.LazyColumn(
                    modifier = Modifier
                        .weight(1f)
                        .fillMaxWidth()
                        .padding(horizontal = 16.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                    contentPadding = PaddingValues(bottom = 80.dp) // extra padding to avoid overlapping the FAB
                ) {
                    if (transfers.isNotEmpty()) {
                        item {
                            Text(
                                text = "SESSÃO ATUAL",
                                style = MaterialTheme.typography.titleSmall,
                                fontWeight = FontWeight.Bold,
                                color = MaterialTheme.colorScheme.primary,
                                modifier = Modifier.padding(vertical = 8.dp)
                            )
                        }
                        items(transfers, key = { "session_${it.id}" }) { item ->
                            TransferCard(item = item)
                        }
                    }

                    if (persistentHistory.isNotEmpty()) {
                        item {
                            Row(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .padding(top = 16.dp, bottom = 8.dp),
                                horizontalArrangement = Arrangement.SpaceBetween,
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                Text(
                                    text = "HISTÓRICO PERSISTIDO",
                                    style = MaterialTheme.typography.titleSmall,
                                    fontWeight = FontWeight.Bold,
                                    color = MaterialTheme.colorScheme.secondary
                                )
                                TextButton(
                                    onClick = { ConnectionRepository.clearPersistentHistory(context) },
                                    modifier = Modifier.testTag("clear_persistent_history_button")
                                ) {
                                    Icon(
                                        imageVector = Icons.Default.Delete,
                                        contentDescription = null,
                                        modifier = Modifier.size(16.dp)
                                    )
                                    Spacer(modifier = Modifier.width(4.dp))
                                    Text(
                                        "Limpar Histórico",
                                        style = MaterialTheme.typography.labelMedium
                                    )
                                }
                            }
                        }
                        items(persistentHistory, key = { "history_${it.id}" }) { record ->
                            val item = TransferItem(
                                id = record.id,
                                name = record.name,
                                size = record.size,
                                direction = if (record.direction == "UPLOAD") TransferDirection.UPLOAD else TransferDirection.DOWNLOAD,
                                status = when (record.status) {
                                    "VERIFICADO" -> TransferStatus.VERIFICADO
                                    "NAO_VERIFICADO" -> TransferStatus.NAO_VERIFICADO
                                    else -> TransferStatus.ERRO
                                },
                                progress = 1f,
                                bytesTransferred = record.size,
                                error = record.error,
                                sha256Local = record.sha256Local,
                                sha256Remote = record.sha256Remote,
                                timestamp = record.timestamp
                            )
                            TransferCard(item = item)
                        }
                    }
                }
            }
        }

        ExtendedFloatingActionButton(
            onClick = { filePickerLauncher.launch("*/*") },
            icon = { Icon(Icons.Default.Add, contentDescription = "Adicionar ficheiro", tint = Color.Black) },
            text = { Text("ENVIAR FICHEIRO", fontFamily = JetBrainsMono, fontWeight = FontWeight.Bold, color = Color.Black) },
            containerColor = CyanActive,
            contentColor = Color.Black,
            shape = RoundedCornerShape(12.dp),
            modifier = Modifier
                .align(Alignment.BottomEnd)
                .padding(16.dp)
                .testTag("send_file_fab")
        )
    }
}

@Composable
fun TransferCard(
    item: TransferItem,
    modifier: Modifier = Modifier
) {
    val isUpload = item.direction == TransferDirection.UPLOAD
    val directionText = if (isUpload) "↑ Enviado" else "↓ Recebido"
    val directionColor = if (isUpload) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.secondary
    
    Card(
        modifier = modifier
            .fillMaxWidth()
            .testTag("transfer_item_${item.id}"),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surface
        ),
        shape = RoundedCornerShape(16.dp),
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.6f))
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp)
        ) {
            // Header: Name & Direction icon
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                Row(
                    modifier = Modifier.weight(1f),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(10.dp)
                ) {
                    Box(
                        modifier = Modifier
                            .size(36.dp)
                            .background(
                                color = directionColor.copy(alpha = 0.15f),
                                shape = CircleShape
                            ),
                        contentAlignment = Alignment.Center
                    ) {
                        Icon(
                            imageVector = if (isUpload) Icons.Default.Upload else Icons.Default.Download,
                            contentDescription = null,
                            tint = directionColor,
                            modifier = Modifier.size(18.dp)
                        )
                    }
                    Column {
                        Text(
                            text = item.name,
                            style = MaterialTheme.typography.bodyMedium,
                            fontWeight = FontWeight.Bold,
                            color = MaterialTheme.colorScheme.onSurface,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis
                        )
                        Row(
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(8.dp)
                        ) {
                            Text(
                                text = directionText,
                                style = MaterialTheme.typography.labelSmall,
                                color = directionColor,
                                fontWeight = FontWeight.SemiBold
                            )
                            Text(
                                text = "•",
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.5f)
                            )
                            Text(
                                text = formatTransferSize(item.size),
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                        }
                    }
                }
                
                // Status Chip/Badge
                TransferStatusBadge(status = item.status)
            }

            // Progress bar & label (if active)
            if (item.status == TransferStatus.EM_CURSO) {
                Column(
                    modifier = Modifier.fillMaxWidth(),
                    verticalArrangement = Arrangement.spacedBy(4.dp)
                ) {
                    LinearProgressIndicator(
                        progress = { item.progress },
                        modifier = Modifier
                            .fillMaxWidth()
                            .height(6.dp)
                            .clip(RoundedCornerShape(3.dp)),
                        color = directionColor,
                        trackColor = MaterialTheme.colorScheme.outline.copy(alpha = 0.2f)
                    )
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Text(
                            text = "${(item.progress * 100).toInt()}% completo",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant
                        )
                        Text(
                            text = "${formatTransferSize(item.bytesTransferred)} de ${formatTransferSize(item.size)}",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant
                        )
                    }
                }
            }

            // Error display
            if (item.status == TransferStatus.ERRO && item.error != null) {
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(
                            color = MaterialTheme.colorScheme.error.copy(alpha = 0.1f),
                            shape = RoundedCornerShape(8.dp)
                        )
                        .padding(8.dp)
                ) {
                    Text(
                        text = item.error,
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.error
                    )
                }
            }

            // Hashes display (local and remote)
            if (item.sha256Local != null || item.sha256Remote != null) {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(
                            color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.4f),
                            shape = RoundedCornerShape(8.dp)
                        )
                        .padding(8.dp),
                    verticalArrangement = Arrangement.spacedBy(4.dp)
                ) {
                    if (item.sha256Local != null) {
                        Text(
                            text = "SHA-256 (local): ${abbreviateHash(item.sha256Local)}",
                            style = MaterialTheme.typography.labelSmall,
                            fontFamily = FontFamily.Monospace,
                            color = MaterialTheme.colorScheme.onSurfaceVariant
                        )
                    }
                    if (item.sha256Remote != null) {
                        Text(
                            text = "SHA-256 (remoto): ${abbreviateHash(item.sha256Remote)}",
                            style = MaterialTheme.typography.labelSmall,
                            fontFamily = FontFamily.Monospace,
                            color = MaterialTheme.colorScheme.onSurfaceVariant
                        )
                    }
                    // If matched and verified
                    if (item.status == TransferStatus.VERIFICADO) {
                        Row(
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(4.dp)
                        ) {
                            Icon(
                                imageVector = Icons.Default.CheckCircle,
                                contentDescription = null,
                                tint = Color(0xFF4CAF50),
                                modifier = Modifier.size(12.dp)
                            )
                            Text(
                                text = "Integridade verificada ✓",
                                style = MaterialTheme.typography.labelSmall,
                                fontWeight = FontWeight.Bold,
                                color = Color(0xFF4CAF50)
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
fun TransferStatusBadge(status: TransferStatus) {
    val (text, color) = when (status) {
        TransferStatus.EM_CURSO -> "EM CURSO" to MaterialTheme.colorScheme.primary
        TransferStatus.VERIFICADO -> "VERIFICADO ✓" to Color(0xFF4CAF50)
        TransferStatus.NAO_VERIFICADO -> "NÃO VERIFICADO" to MaterialTheme.colorScheme.outline
        TransferStatus.ERRO -> "ERRO" to MaterialTheme.colorScheme.error
    }
    
    Box(
        modifier = Modifier
            .background(
                color = color.copy(alpha = 0.15f),
                shape = RoundedCornerShape(6.dp)
            )
            .border(1.dp, color.copy(alpha = 0.5f), RoundedCornerShape(6.dp))
            .padding(horizontal = 8.dp, vertical = 4.dp)
    ) {
        Text(
            text = text,
            style = MaterialTheme.typography.labelSmall,
            fontWeight = FontWeight.Bold,
            color = color,
            fontSize = 9.sp
        )
    }
}

private fun formatTransferSize(bytes: Long): String {
    if (bytes < 1024) return "$bytes B"
    val exp = (Math.log(bytes.toDouble()) / Math.log(1024.0)).toInt()
    val pre = "KMGTPE"[exp - 1] + "B"
    return String.format(java.util.Locale.US, "%.1f %s", bytes / Math.pow(1024.0, exp.toDouble()), pre)
}

private fun abbreviateHash(hash: String): String {
    if (hash.length <= 16) return hash
    return hash.take(8) + "..." + hash.takeLast(8)
}

@Composable
fun NotificationsSettingsScreen(
    onBack: () -> Unit,
    modifier: Modifier = Modifier
) {
    val context = LocalContext.current
    val sharedPrefs = remember { context.getSharedPreferences("hyprlink_prefs", Context.MODE_PRIVATE) }
    var isMirrorEnabled by remember { mutableStateOf(sharedPrefs.getBoolean("mirror_notifications", true)) }
    
    val lifecycleOwner = androidx.lifecycle.compose.LocalLifecycleOwner.current
    var hasPermission by remember { mutableStateOf(false) }
    var hasDndPermission by remember { mutableStateOf(false) }

    DisposableEffect(lifecycleOwner) {
        val observer = androidx.lifecycle.LifecycleEventObserver { _, event ->
            if (event == androidx.lifecycle.Lifecycle.Event.ON_RESUME) {
                hasPermission = isNotificationListenerEnabled(context)
                val nm = context.getSystemService(Context.NOTIFICATION_SERVICE) as? android.app.NotificationManager
                hasDndPermission = nm?.isNotificationPolicyAccessGranted ?: false
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
        }
    }

    val mirroredCount by ConnectionRepository.mirroredNotificationsCount.collectAsStateWithLifecycle()

    Column(
        modifier = modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            .verticalScroll(rememberScrollState())
    ) {
        // Toolbar / Header
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            IconButton(
                onClick = onBack,
                modifier = Modifier.testTag("notifications_back_button")
            ) {
                Icon(
                    imageVector = Icons.Default.ArrowBack,
                    contentDescription = "Voltar",
                    tint = MaterialTheme.colorScheme.onBackground
                )
            }
            Spacer(modifier = Modifier.width(8.dp))
            Column {
                Text(
                    text = "NOTIFICAÇÕES",
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.Bold,
                    color = MaterialTheme.colorScheme.onBackground
                )
                Text(
                    text = "Espelhar alertas no PC",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
            }
        }

        Spacer(modifier = Modifier.height(16.dp))

        // Card 1: Permissão de Acesso
        Card(
            modifier = Modifier
                .fillMaxWidth()
                .padding(bottom = 16.dp)
                .testTag("permission_card"),
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.surface
            ),
            shape = RoundedCornerShape(16.dp),
            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.6f))
        ) {
            Column(
                modifier = Modifier.padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp)
            ) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        text = "Acesso ao Sistema",
                        style = MaterialTheme.typography.titleSmall,
                        fontWeight = FontWeight.Bold,
                        color = MaterialTheme.colorScheme.onSurface
                    )
                    
                    // Permission badge
                    val badgeColor = if (hasPermission) Color(0xFF4CAF50) else Color(0xFFFF9800)
                    val badgeText = if (hasPermission) "ATIVADO" else "PENDENTE"
                    
                    Box(
                        modifier = Modifier
                            .background(badgeColor.copy(alpha = 0.15f), RoundedCornerShape(6.dp))
                            .border(1.dp, badgeColor.copy(alpha = 0.5f), RoundedCornerShape(6.dp))
                            .padding(horizontal = 8.dp, vertical = 4.dp)
                    ) {
                        Text(
                            text = badgeText,
                            style = MaterialTheme.typography.labelSmall,
                            fontWeight = FontWeight.Bold,
                            color = badgeColor
                        )
                    }
                }

                Text(
                    text = "O HyprLink necessita de permissão de acesso às notificações para poder sincronizar os alertas recebidos com a sua workstation de forma segura.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )

                Button(
                    onClick = {
                        try {
                            val intent = Intent(android.provider.Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS)
                            context.startActivity(intent)
                        } catch (e: Exception) {
                            Toast.makeText(context, "Não foi possível abrir as definições.", Toast.LENGTH_LONG).show()
                        }
                    },
                    modifier = Modifier
                        .fillMaxWidth()
                        .testTag("open_settings_button"),
                    colors = ButtonDefaults.buttonColors(
                        containerColor = if (hasPermission) MaterialTheme.colorScheme.secondary else MaterialTheme.colorScheme.primary,
                        contentColor = if (hasPermission) MaterialTheme.colorScheme.onSecondary else MaterialTheme.colorScheme.onPrimary
                    )
                ) {
                    Icon(
                        imageVector = Icons.Default.Settings,
                        contentDescription = null,
                        modifier = Modifier.size(18.dp)
                    )
                    Spacer(modifier = Modifier.width(8.dp))
                    Text(if (hasPermission) "Gerir Acesso" else "Conceder Permissão")
                }
            }
        }

        // Card 1b: Permissão de Não Incomodar / Modo Silencioso
        Card(
            modifier = Modifier
                .fillMaxWidth()
                .padding(bottom = 16.dp)
                .testTag("dnd_permission_card"),
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.surface
            ),
            shape = RoundedCornerShape(16.dp),
            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.6f))
        ) {
            Column(
                modifier = Modifier.padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp)
            ) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        text = "Acesso a Não Incomodar (DND)",
                        style = MaterialTheme.typography.titleSmall,
                        fontWeight = FontWeight.Bold,
                        color = MaterialTheme.colorScheme.onSurface
                    )

                    val badgeColor = if (hasDndPermission) Color(0xFF4CAF50) else Color(0xFFFF9800)
                    val badgeText = if (hasDndPermission) "ATIVADO" else "PENDENTE"

                    Box(
                        modifier = Modifier
                            .background(badgeColor.copy(alpha = 0.15f), RoundedCornerShape(6.dp))
                            .border(1.dp, badgeColor.copy(alpha = 0.5f), RoundedCornerShape(6.dp))
                            .padding(horizontal = 8.dp, vertical = 4.dp)
                    ) {
                        Text(
                            text = badgeText,
                            style = MaterialTheme.typography.labelSmall,
                            fontWeight = FontWeight.Bold,
                            color = badgeColor
                        )
                    }
                }

                Text(
                    text = "Permite que a sua workstation ajuste os volumes e altere remotamente o telemóvel entre Silencioso, Vibrar e Som Normal.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )

                Button(
                    onClick = {
                        try {
                            val intent = Intent(android.provider.Settings.ACTION_NOTIFICATION_POLICY_ACCESS_SETTINGS).apply {
                                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                            }
                            context.startActivity(intent)
                        } catch (e: Exception) {
                            Toast.makeText(context, "Não foi possível abrir as definições.", Toast.LENGTH_LONG).show()
                        }
                    },
                    modifier = Modifier
                        .fillMaxWidth()
                        .testTag("open_dnd_settings_button"),
                    colors = ButtonDefaults.buttonColors(
                        containerColor = if (hasDndPermission) MaterialTheme.colorScheme.secondary else MaterialTheme.colorScheme.primary,
                        contentColor = if (hasDndPermission) MaterialTheme.colorScheme.onSecondary else MaterialTheme.colorScheme.onPrimary
                    )
                ) {
                    Icon(
                        imageVector = Icons.Default.VolumeUp,
                        contentDescription = null,
                        modifier = Modifier.size(18.dp)
                    )
                    Spacer(modifier = Modifier.width(8.dp))
                    Text(if (hasDndPermission) "Gerir Permissão DND" else "Conceder Permissão DND")
                }
            }
        }

        // Card 2: Interruptor / Toggle
        Card(
            modifier = Modifier
                .fillMaxWidth()
                .padding(bottom = 16.dp)
                .testTag("toggle_card"),
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.surface
            ),
            shape = RoundedCornerShape(16.dp),
            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.6f))
        ) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(16.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        text = "Espelhar notificações no PC",
                        style = MaterialTheme.typography.titleSmall,
                        fontWeight = FontWeight.Bold,
                        color = MaterialTheme.colorScheme.onSurface
                    )
                    Text(
                        text = "Enviar alertas de apps quando recebidos no telemóvel.",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(top = 4.dp)
                    )
                }
                
                Switch(
                    checked = isMirrorEnabled,
                    onCheckedChange = { newValue ->
                        isMirrorEnabled = newValue
                        sharedPrefs.edit().putBoolean("mirror_notifications", newValue).apply()
                    },
                    modifier = Modifier.testTag("mirror_switch")
                )
            }
        }

        // Card 3: Estatísticas de Transmissão da Sessão
        Card(
            modifier = Modifier
                .fillMaxWidth()
                .padding(bottom = 16.dp)
                .testTag("stats_card"),
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.surface
            ),
            shape = RoundedCornerShape(16.dp),
            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.6f))
        ) {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(16.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(12.dp)
            ) {
                Text(
                    text = "Sessão Ativa",
                    style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.Bold,
                    color = MaterialTheme.colorScheme.onSurface,
                    modifier = Modifier.align(Alignment.Start)
                )

                // Large Glowing Counter
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    modifier = Modifier.padding(vertical = 12.dp)
                ) {
                    Text(
                        text = "$mirroredCount",
                        style = MaterialTheme.typography.displayLarge,
                        fontWeight = FontWeight.ExtraBold,
                        color = MaterialTheme.colorScheme.primary
                    )
                    Text(
                        text = "Notificações espelhadas",
                        style = MaterialTheme.typography.labelMedium,
                        fontWeight = FontWeight.Bold,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }

                Text(
                    text = "Nota: Para sua conveniência e privacidade, as notificações em curso (como reprodutores multimédia ou downloads ativos), alertas do próprio HyprLink e notificações de grupos resumo são automaticamente filtrados.",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.8f),
                    textAlign = TextAlign.Center
                )
            }
        }
    }
}

private fun isNotificationListenerEnabled(context: Context): Boolean {
    val pkgName = context.packageName
    try {
        val enabledPackages = androidx.core.app.NotificationManagerCompat.getEnabledListenerPackages(context)
        if (enabledPackages.contains(pkgName)) {
            return true
        }
    } catch (_: Exception) {
    }
    val flat = android.provider.Settings.Secure.getString(
        context.contentResolver,
        "enabled_notification_listeners"
    )
    if (!flat.isNullOrEmpty()) {
        val names = flat.split(":")
        for (name in names) {
            val cn = android.content.ComponentName.unflattenFromString(name)
            if (cn != null && cn.packageName == pkgName) {
                return true
            }
        }
    }
    return false
}

@Composable
fun AudioScreen(onBack: () -> Unit, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val coroutineScope = rememberCoroutineScope()
    val connStatus by ConnectionRepository.connectionStatus.collectAsStateWithLifecycle()
    val mediaState by ConnectionRepository.mediaState.collectAsStateWithLifecycle()
    val isPlayingAudioTap by AudioStreamPlayer.isPlaying.collectAsStateWithLifecycle()

    var audioState by remember { mutableStateOf<AudioState?>(null) }
    var errorMsg by remember { mutableStateOf<String?>(null) }
    var previousDefaultSinkName by rememberSaveable { mutableStateOf("") }

    // Sliders drag states so they don't jump during active drag/interaction
    val activeSliders = remember { mutableStateMapOf<String, Float>() } // Key: "sink_<id>" or "app_<id>" -> volume value

    // Debounce state for sending volume changes
    val volumeChannels = remember { mutableStateMapOf<String, kotlinx.coroutines.Job>() }

    // Stop playback if disconnected
    LaunchedEffect(connStatus) {
        if (connStatus != ConnectionStatus.CONNECTED) {
            AudioStreamPlayer.stop()
        }
    }

    // Periodic state refresh (every ~3s)
    LaunchedEffect(connStatus) {
        if (connStatus == ConnectionStatus.CONNECTED) {
            while (isActive) {
                try {
                    val state = ConnectionRepository.fetchAudioState()
                    audioState = state
                    errorMsg = null
                } catch (e: Exception) {
                    errorMsg = e.message
                }
                delay(3000)
            }
        } else {
            audioState = null
        }
    }

    Column(
        modifier = modifier
            .fillMaxSize()
            .background(Color.Black)
            .padding(16.dp)
            .verticalScroll(rememberScrollState()),
        verticalArrangement = Arrangement.spacedBy(16.dp)
    ) {
        // Back Button & Title
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            IconButton(onClick = onBack) {
                Icon(
                    imageVector = Icons.Default.ArrowBack,
                    contentDescription = "Voltar",
                    tint = TextPrimary
                )
            }
            Text(
                text = "ÁUDIO",
                style = MaterialTheme.typography.titleLarge,
                fontFamily = ArchivoBlack,
                color = TextPrimary
            )
        }

        if (connStatus != ConnectionStatus.CONNECTED) {
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(vertical = 32.dp),
                contentAlignment = Alignment.Center
            ) {
                Text(
                    text = "Liga a uma workstation primeiro.",
                    color = TextSecondary,
                    fontFamily = JetBrainsMono,
                    textAlign = TextAlign.Center
                )
            }
            return@Column
        }

        // 1. A TOCAR CARD (reused design from MediaState)
        if (mediaState != null) {
            val ms = mediaState!!
            val isPlaying = ms.status.lowercase() == "playing"
            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(containerColor = DarkSurface),
                shape = RoundedCornerShape(12.dp),
                border = BorderStroke(1.dp, DarkOutline)
            ) {
                Column(
                    modifier = Modifier.padding(16.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp)
                ) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Row(
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(6.dp)
                        ) {
                            Icon(
                                imageVector = Icons.Default.PlayArrow,
                                contentDescription = "Media Player Icon",
                                tint = CyanActive,
                                modifier = Modifier.size(18.dp)
                            )
                            Text(
                                text = "A REPRODUZIR NO PC",
                                style = MaterialTheme.typography.labelSmall,
                                fontWeight = FontWeight.Bold,
                                color = TextSecondary
                            )
                        }
                        Text(
                            text = ms.player,
                            style = MaterialTheme.typography.labelSmall,
                            fontFamily = JetBrainsMono,
                            color = CyanActive
                        )
                    }

                    Column {
                        Text(
                            text = ms.title ?: "Sem Título",
                            style = MaterialTheme.typography.titleMedium,
                            fontWeight = FontWeight.Bold,
                            color = TextPrimary,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis
                        )
                        Text(
                            text = ms.artist ?: "Artista Desconhecido",
                            style = MaterialTheme.typography.bodyMedium,
                            color = TextSecondary,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis
                        )
                    }

                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceEvenly,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        IconButton(onClick = {
                            coroutineScope.launch { ConnectionRepository.sendMediaCommand("previous") }
                        }) {
                            Icon(
                                imageVector = Icons.Default.SkipPrevious,
                                contentDescription = "Anterior",
                                tint = TextPrimary,
                                modifier = Modifier.size(28.dp)
                            )
                        }

                        FloatingActionButton(
                            onClick = {
                                coroutineScope.launch { ConnectionRepository.sendMediaCommand("playpause") }
                            },
                            containerColor = CyanActive,
                            contentColor = Color.Black,
                            shape = CircleShape,
                            modifier = Modifier.size(52.dp)
                        ) {
                            Icon(
                                imageVector = if (isPlaying) Icons.Default.Pause else Icons.Default.PlayArrow,
                                contentDescription = "Play/Pause",
                                modifier = Modifier.size(28.dp)
                            )
                        }

                        IconButton(onClick = {
                            coroutineScope.launch { ConnectionRepository.sendMediaCommand("next") }
                        }) {
                            Icon(
                                imageVector = Icons.Default.SkipNext,
                                contentDescription = "Seguinte",
                                tint = TextPrimary,
                                modifier = Modifier.size(28.dp)
                            )
                        }
                    }
                }
            }
        } else {
            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(containerColor = DarkSurface),
                shape = RoundedCornerShape(12.dp),
                border = BorderStroke(1.dp, DarkOutline)
            ) {
                Column(
                    modifier = Modifier.padding(16.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp)
                ) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Row(
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(6.dp)
                        ) {
                            Icon(
                                imageVector = Icons.Default.PlayArrow,
                                contentDescription = "Media Player Icon",
                                tint = TextSecondary,
                                modifier = Modifier.size(18.dp)
                            )
                            Text(
                                text = "CONTROLO DE MÉDIA",
                                style = MaterialTheme.typography.labelSmall,
                                fontWeight = FontWeight.Bold,
                                color = TextSecondary
                            )
                        }
                        Text(
                            text = "Inativo",
                            style = MaterialTheme.typography.labelSmall,
                            fontFamily = JetBrainsMono,
                            color = TextSecondary
                        )
                    }

                    Column {
                        Text(
                            text = "Nenhum leitor ativo detetado",
                            style = MaterialTheme.typography.titleMedium,
                            fontWeight = FontWeight.Bold,
                            color = TextSecondary,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis
                        )
                        Text(
                            text = "Envia comandos de reprodução globais",
                            style = MaterialTheme.typography.bodyMedium,
                            color = TextSecondary,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis
                        )
                    }

                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceEvenly,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        IconButton(onClick = {
                            coroutineScope.launch { ConnectionRepository.sendMediaCommand("previous") }
                        }) {
                            Icon(
                                imageVector = Icons.Default.SkipPrevious,
                                contentDescription = "Anterior",
                                tint = TextPrimary,
                                modifier = Modifier.size(28.dp)
                            )
                        }

                        FloatingActionButton(
                            onClick = {
                                coroutineScope.launch { ConnectionRepository.sendMediaCommand("playpause") }
                            },
                            containerColor = DarkOutline,
                            contentColor = TextPrimary,
                            shape = CircleShape,
                            modifier = Modifier.size(52.dp)
                        ) {
                            Icon(
                                imageVector = Icons.Default.PlayArrow,
                                contentDescription = "Play/Pause",
                                modifier = Modifier.size(28.dp)
                            )
                        }

                        IconButton(onClick = {
                            coroutineScope.launch { ConnectionRepository.sendMediaCommand("next") }
                        }) {
                            Icon(
                                imageVector = Icons.Default.SkipNext,
                                contentDescription = "Seguinte",
                                tint = TextPrimary,
                                modifier = Modifier.size(28.dp)
                            )
                        }
                    }
                }
            }
        }

        // 2. OUVIR NO TELEMÓVEL CARD
        Card(
            modifier = Modifier.fillMaxWidth(),
            colors = CardDefaults.cardColors(containerColor = DarkSurface),
            shape = RoundedCornerShape(12.dp),
            border = BorderStroke(1.dp, DarkOutline)
        ) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(16.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        text = "OUVIR NO TELEMÓVEL",
                        style = MaterialTheme.typography.titleSmall,
                        fontWeight = FontWeight.Bold,
                        color = TextPrimary
                    )
                    Text(
                        text = "Toca o áudio do PC diretamente no telemóvel",
                        style = MaterialTheme.typography.bodySmall,
                        color = TextSecondary
                    )
                }
                Switch(
                    checked = isPlayingAudioTap,
                    onCheckedChange = { checked ->
                        coroutineScope.launch {
                            if (checked) {
                                audioState?.let { state ->
                                    val currentDefault = state.sinks.firstOrNull { it.name == state.default_sink }
                                    if (currentDefault != null && !currentDefault.is_phone) {
                                        previousDefaultSinkName = currentDefault.name
                                    }
                                }
                                val phoneSink = audioState?.sinks?.firstOrNull { it.is_phone }
                                val targetSinkName = phoneSink?.name ?: "hyprlink-speaker"
                                val successId = ConnectionRepository.startAudioTap()
                                if (successId != null) {
                                    ConnectionRepository.setAudioDefaultSink(targetSinkName)
                                    val freshState = ConnectionRepository.fetchAudioState()
                                    audioState = freshState
                                } else {
                                    Toast.makeText(context, "Erro ao iniciar transmissão", Toast.LENGTH_SHORT).show()
                                }
                            } else {
                                ConnectionRepository.stopAudioTap()
                                AudioStreamPlayer.stop()
                                if (previousDefaultSinkName.isNotEmpty()) {
                                    ConnectionRepository.setAudioDefaultSink(previousDefaultSinkName)
                                    previousDefaultSinkName = ""
                                }
                                val freshState = ConnectionRepository.fetchAudioState()
                                audioState = freshState
                            }
                        }
                    },
                    colors = SwitchDefaults.colors(
                        checkedThumbColor = Color.Black,
                        checkedTrackColor = CyanActive,
                        uncheckedThumbColor = TextSecondary,
                        uncheckedTrackColor = DarkOutline
                    )
                )
            }
        }

        // 3. SAÍDA DE SOM CARD
        Text(
            text = "SAÍDA DE SOM",
            style = MaterialTheme.typography.labelMedium,
            fontWeight = FontWeight.Bold,
            color = TextSecondary,
            fontFamily = JetBrainsMono
        )

        val state = audioState
        if (state == null) {
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(vertical = 16.dp),
                contentAlignment = Alignment.Center
            ) {
                CircularProgressIndicator(color = CyanActive)
            }
        } else {
            state.sinks.forEach { sink ->
                val isSelected = sink.name == state.default_sink
                val borderCol = if (isSelected) CyanActive else DarkOutline
                
                Card(
                    modifier = Modifier.fillMaxWidth(),
                    colors = CardDefaults.cardColors(containerColor = DarkSurface),
                    shape = RoundedCornerShape(12.dp),
                    border = BorderStroke(1.dp, borderCol)
                ) {
                    Column(
                        modifier = Modifier.padding(16.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp)
                    ) {
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clickable {
                                    coroutineScope.launch {
                                        val ok = ConnectionRepository.setAudioDefaultSink(sink.name)
                                        if (ok) {
                                            audioState = ConnectionRepository.fetchAudioState()
                                        }
                                    }
                                },
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Row(
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(8.dp)
                            ) {
                                RadioButton(
                                    selected = isSelected,
                                    onClick = {
                                        coroutineScope.launch {
                                            val ok = ConnectionRepository.setAudioDefaultSink(sink.name)
                                            if (ok) {
                                                audioState = ConnectionRepository.fetchAudioState()
                                            }
                                        }
                                    },
                                    colors = RadioButtonDefaults.colors(
                                        selectedColor = CyanActive,
                                        unselectedColor = TextSecondary
                                    )
                                )
                                Text(
                                    text = if (sink.is_phone) "📱 Este telemóvel" else sink.description,
                                    style = MaterialTheme.typography.titleSmall,
                                    fontWeight = FontWeight.Bold,
                                    color = if (isSelected) CyanActive else TextPrimary
                                )
                            }
                            if (isSelected) {
                                Text(
                                    text = "ATIVO",
                                    style = MaterialTheme.typography.labelSmall,
                                    fontFamily = JetBrainsMono,
                                    color = CyanActive
                                )
                            }
                        }

                        val sliderKey = "sink_${sink.id}"
                        val currentVol = activeSliders[sliderKey] ?: sink.volume.toFloat()

                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(8.dp)
                        ) {
                            IconButton(onClick = {
                                coroutineScope.launch {
                                    val newMuted = !sink.muted
                                    val ok = ConnectionRepository.setAudioMute("sink", sink.id, newMuted)
                                    if (ok) {
                                        audioState = ConnectionRepository.fetchAudioState()
                                    }
                                }
                            }) {
                                Icon(
                                    imageVector = if (sink.muted) Icons.Default.Close else Icons.Default.Share,
                                    contentDescription = "Mute Toggle",
                                    tint = if (sink.muted) RedError else TextPrimary
                                )
                            }

                            Slider(
                                value = currentVol,
                                onValueChange = { value ->
                                    activeSliders[sliderKey] = value
                                    volumeChannels[sliderKey]?.cancel()
                                    volumeChannels[sliderKey] = coroutineScope.launch {
                                        delay(150)
                                        ConnectionRepository.setAudioVolume("sink", sink.id, value.toInt())
                                    }
                                },
                                onValueChangeFinished = {
                                    activeSliders.remove(sliderKey)
                                    coroutineScope.launch {
                                        ConnectionRepository.setAudioVolume("sink", sink.id, currentVol.toInt())
                                        audioState = ConnectionRepository.fetchAudioState()
                                    }
                                },
                                valueRange = 0f..150f,
                                modifier = Modifier.weight(1f),
                                colors = SliderDefaults.colors(
                                    thumbColor = CyanActive,
                                    activeTrackColor = CyanActive,
                                    inactiveTrackColor = DarkOutline
                                )
                            )

                            Text(
                                text = "${currentVol.toInt()}%",
                                style = MaterialTheme.typography.bodySmall,
                                fontFamily = JetBrainsMono,
                                color = TextPrimary,
                                modifier = Modifier.width(42.dp),
                                textAlign = TextAlign.End
                            )
                        }
                    }
                }
            }

            Spacer(modifier = Modifier.height(8.dp))
            Text(
                text = "APLICAÇÕES",
                style = MaterialTheme.typography.labelMedium,
                fontWeight = FontWeight.Bold,
                color = TextSecondary,
                fontFamily = JetBrainsMono
            )

            if (state.apps.isEmpty()) {
                Card(
                    modifier = Modifier.fillMaxWidth(),
                    colors = CardDefaults.cardColors(containerColor = DarkSurface),
                    shape = RoundedCornerShape(12.dp),
                    border = BorderStroke(1.dp, DarkOutline)
                ) {
                    Box(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(24.dp),
                        contentAlignment = Alignment.Center
                    ) {
                        Text(
                            text = "Nada a tocar no PC.",
                            style = MaterialTheme.typography.bodyMedium,
                            color = TextSecondary,
                            fontFamily = JetBrainsMono
                        )
                    }
                }
            } else {
                state.apps.forEach { app ->
                    Card(
                        modifier = Modifier.fillMaxWidth(),
                        colors = CardDefaults.cardColors(containerColor = DarkSurface),
                        shape = RoundedCornerShape(12.dp),
                        border = BorderStroke(1.dp, DarkOutline)
                    ) {
                        Column(
                            modifier = Modifier.padding(16.dp),
                            verticalArrangement = Arrangement.spacedBy(8.dp)
                        ) {
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.SpaceBetween,
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                Column {
                                    Text(
                                        text = app.name,
                                        style = MaterialTheme.typography.titleSmall,
                                        fontWeight = FontWeight.Bold,
                                        color = TextPrimary
                                    )
                                    if (!app.media.isNullOrEmpty()) {
                                        Text(
                                            text = app.media,
                                            style = MaterialTheme.typography.bodySmall,
                                            color = CyanActive
                                        )
                                    }
                                }
                            }

                            val sliderKey = "app_${app.id}"
                            val currentVol = activeSliders[sliderKey] ?: app.volume.toFloat()

                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(8.dp)
                            ) {
                                IconButton(onClick = {
                                    coroutineScope.launch {
                                        val newMuted = !app.muted
                                        val ok = ConnectionRepository.setAudioMute("app", app.id, newMuted)
                                        if (ok) {
                                            audioState = ConnectionRepository.fetchAudioState()
                                        }
                                    }
                                }) {
                                    Icon(
                                        imageVector = if (app.muted) Icons.Default.Close else Icons.Default.Share,
                                        contentDescription = "Mute Toggle",
                                        tint = if (app.muted) RedError else TextPrimary
                                    )
                                }

                                Slider(
                                    value = currentVol,
                                    onValueChange = { value ->
                                        activeSliders[sliderKey] = value
                                        volumeChannels[sliderKey]?.cancel()
                                        volumeChannels[sliderKey] = coroutineScope.launch {
                                            delay(150)
                                            ConnectionRepository.setAudioVolume("app", app.id, value.toInt())
                                        }
                                    },
                                    onValueChangeFinished = {
                                        activeSliders.remove(sliderKey)
                                        coroutineScope.launch {
                                            ConnectionRepository.setAudioVolume("app", app.id, currentVol.toInt())
                                            audioState = ConnectionRepository.fetchAudioState()
                                        }
                                    },
                                    valueRange = 0f..150f,
                                    modifier = Modifier.weight(1f),
                                    colors = SliderDefaults.colors(
                                        thumbColor = CyanActive,
                                        activeTrackColor = CyanActive,
                                        inactiveTrackColor = DarkOutline
                                    )
                                )

                                Text(
                                    text = "${currentVol.toInt()}%",
                                    style = MaterialTheme.typography.bodySmall,
                                    fontFamily = JetBrainsMono,
                                    color = TextPrimary,
                                    modifier = Modifier.width(42.dp),
                                    textAlign = TextAlign.End
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}




