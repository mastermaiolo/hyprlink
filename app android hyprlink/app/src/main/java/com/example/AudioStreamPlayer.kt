package com.example

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import tech.kwik.core.QuicStream

object AudioStreamPlayer {
    private val playerScope = CoroutineScope(Dispatchers.Default + SupervisorJob())
    private var audioTrack: AudioTrack? = null
    private var activeStream: QuicStream? = null
    private var playJob: Job? = null

    private val _isPlaying = MutableStateFlow(false)
    val isPlaying = _isPlaying.asStateFlow()

    @Synchronized
    fun playStream(stream: QuicStream) {
        stop()

        activeStream = stream
        _isPlaying.value = true
        playJob = playerScope.launch(Dispatchers.IO) {
            val inp = stream.inputStream
            var track: AudioTrack? = null
            try {
                val sampleRate = 48000
                val channelConfig = AudioFormat.CHANNEL_OUT_STEREO
                val audioFormat = AudioFormat.ENCODING_PCM_16BIT
                
                val minBufferSize = AudioTrack.getMinBufferSize(sampleRate, channelConfig, audioFormat)
                val bufferSize = maxOf(minBufferSize * 2, 16384)

                track = AudioTrack.Builder()
                    .setAudioAttributes(
                        AudioAttributes.Builder()
                            .setUsage(AudioAttributes.USAGE_MEDIA)
                            .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                            .build()
                    )
                    .setAudioFormat(
                        AudioFormat.Builder()
                            .setEncoding(audioFormat)
                            .setSampleRate(sampleRate)
                            .setChannelMask(channelConfig)
                            .build()
                    )
                    .setBufferSizeInBytes(bufferSize)
                    .setTransferMode(AudioTrack.MODE_STREAM)
                    .build()

                synchronized(this@AudioStreamPlayer) {
                    audioTrack = track
                }

                track.setVolume(1.0f)
                track.play()
                ConnectionRepository.appendLog("[AUDIO] AudioTrack started playing (bufferSize=$bufferSize, state=${track.state})")

                val buffer = ByteArray(bufferSize)
                var totalBytesRead = 0L
                var intervalBytes = 0
                var intervalPeak = 0
                var pendingLowByte = -1
                var firstReadLogged = false
                val loopStartMs = System.currentTimeMillis()

                while (isActive) {
                    val n = inp.read(buffer)
                    if (n == -1) {
                        ConnectionRepository.appendLog("[AUDIO] PCM stream reached EOF (total read: $totalBytesRead bytes)")
                        break
                    }
                    if (n > 0) {
                        if (!firstReadLogged) {
                            firstReadLogged = true
                            ConnectionRepository.appendLog("[AUDIO] Primeira leitura: $n bytes em ${System.currentTimeMillis() - loopStartMs}ms")
                        }
                        totalBytesRead += n
                        intervalBytes += n

                        // Compute peak amplitude across 16-bit little-endian samples
                        var i = 0
                        if (pendingLowByte != -1) {
                            val sample = ((buffer[0].toInt() shl 8) or (pendingLowByte and 0xFF)).toShort()
                            val absVal = kotlin.math.abs(sample.toInt())
                            if (absVal > intervalPeak) {
                                intervalPeak = absVal
                            }
                            pendingLowByte = -1
                            i = 1
                        }

                        while (i + 1 < n) {
                            val sample = ((buffer[i + 1].toInt() shl 8) or (buffer[i].toInt() and 0xFF)).toShort()
                            val absVal = kotlin.math.abs(sample.toInt())
                            if (absVal > intervalPeak) {
                                intervalPeak = absVal
                            }
                            i += 2
                        }

                        if (i < n) {
                            pendingLowByte = buffer[i].toInt() and 0xFF
                        }

                        // Every ~1 second of 48kHz stereo 16-bit audio (48000 * 2 * 2 = 192000 bytes)
                        if (intervalBytes >= 192000) {
                            val pct = (intervalPeak * 100) / 32767
                            ConnectionRepository.appendLog("[AUDIO] chunk peak amplitude: $intervalPeak / 32767 ($pct%) | Total: ${totalBytesRead / 1024} KB")
                            intervalBytes = 0
                            intervalPeak = 0
                        }

                        var written = 0
                        while (written < n && isActive) {
                            val writeStartMs = System.currentTimeMillis()
                            val result = track.write(buffer, written, n - written)
                            val writeMs = System.currentTimeMillis() - writeStartMs
                            if (writeMs > 200) {
                                ConnectionRepository.appendLog("[AUDIO] track.write demorou ${writeMs}ms (pediu ${n - written} bytes, devolveu $result)")
                            }
                            if (result < 0) {
                                ConnectionRepository.appendLog("[AUDIO] Error writing to AudioTrack: $result")
                                break
                            }
                            written += result
                        }
                    }
                }
            } catch (e: Exception) {
                ConnectionRepository.appendLog("[AUDIO] Playback error: ${e.message}")
            } finally {
                ConnectionRepository.appendLog("[AUDIO] Cleaning up AudioTrack...")
                synchronized(this@AudioStreamPlayer) {
                    if (audioTrack == track) {
                        audioTrack = null
                    }
                    if (activeStream == stream) {
                        activeStream = null
                    }
                }
                try {
                    track?.stop()
                } catch (e: Exception) {}
                try {
                    track?.release()
                } catch (e: Exception) {}
                try {
                    stream.closeInput(0)
                } catch (e: Exception) {}
                _isPlaying.value = false
            }
        }
    }

    @Synchronized
    fun stop() {
        val caller = Thread.currentThread().stackTrace.getOrNull(3)?.let {
            "${it.className}.${it.methodName}:${it.lineNumber}"
        } ?: "desconhecido"
        ConnectionRepository.appendLog("[AUDIO] stop() chamado de: $caller")
        playJob?.cancel()
        playJob = null
        val streamToClose = activeStream
        activeStream = null
        try {
            streamToClose?.closeInput(0)
        } catch (e: Exception) {}
        val track = audioTrack
        audioTrack = null
        if (track != null) {
            try {
                track.stop()
            } catch (e: Exception) {}
            try {
                track.release()
            } catch (e: Exception) {}
        }
        _isPlaying.value = false
    }
}
