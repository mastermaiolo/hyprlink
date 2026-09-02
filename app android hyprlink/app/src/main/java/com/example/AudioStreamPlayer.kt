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
    private var playJob: Job? = null

    private val _isPlaying = MutableStateFlow(false)
    val isPlaying = _isPlaying.asStateFlow()

    @Synchronized
    fun playStream(stream: QuicStream) {
        stop()

        _isPlaying.value = true
        playJob = playerScope.launch(Dispatchers.IO) {
            val inp = stream.inputStream
            var track: AudioTrack? = null
            try {
                val sampleRate = 48000
                val channelConfig = AudioFormat.CHANNEL_OUT_STEREO
                val audioFormat = AudioFormat.ENCODING_PCM_16BIT
                
                val minBufferSize = AudioTrack.getMinBufferSize(sampleRate, channelConfig, audioFormat)
                val bufferSize = maxOf(minBufferSize * 4, 16384)

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

                track.play()
                ConnectionRepository.appendLog("[AUDIO] AudioTrack started playing")

                val buffer = ByteArray(bufferSize)
                while (isActive) {
                    val n = inp.read(buffer)
                    if (n == -1) {
                        ConnectionRepository.appendLog("[AUDIO] PCM stream reached EOF")
                        break
                    }
                    if (n > 0) {
                        var written = 0
                        while (written < n && isActive) {
                            val result = track.write(buffer, written, n - written)
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
        playJob?.cancel()
        playJob = null
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
