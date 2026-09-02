package com.example

import android.content.Context
import android.util.Base64
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKeys
import org.bouncycastle.asn1.x500.X500Name
import org.bouncycastle.cert.jcajce.JcaX509v3CertificateBuilder
import org.bouncycastle.jce.provider.BouncyCastleProvider
import org.bouncycastle.operator.jcajce.JcaContentSignerBuilder
import java.io.ByteArrayInputStream
import java.math.BigInteger
import java.security.*
import java.security.cert.CertificateFactory
import java.security.cert.X509Certificate
import java.security.spec.ECGenParameterSpec
import java.security.spec.PKCS8EncodedKeySpec
import java.util.*

class DeviceIdentity private constructor(context: Context) {

    val certificate: X509Certificate
    val privateKey: PrivateKey
    val fingerprintHex: String

    init {
        // Register Bouncy Castle Provider
        Security.addProvider(BouncyCastleProvider())

        val sharedPrefs = getSecurePreferences(context)

        val cachedKey = sharedPrefs.getString("device_private_key", null)
        val cachedCert = sharedPrefs.getString("device_certificate", null)

        var tempPrivateKey: PrivateKey? = null
        var tempCertificate: X509Certificate? = null

        if (cachedKey != null && cachedCert != null) {
            try {
                val privateKeyBytes = Base64.decode(cachedKey, Base64.DEFAULT)
                val certBytes = Base64.decode(cachedCert, Base64.DEFAULT)

                val kf = KeyFactory.getInstance("EC")
                tempPrivateKey = kf.generatePrivate(PKCS8EncodedKeySpec(privateKeyBytes))

                val cf = CertificateFactory.getInstance("X.509")
                tempCertificate = cf.generateCertificate(ByteArrayInputStream(certBytes)) as X509Certificate
            } catch (e: Exception) {
                // If parsing fails for any reason, let them remain null so they are regenerated
            }
        }

        if (tempPrivateKey != null && tempCertificate != null) {
            privateKey = tempPrivateKey
            certificate = tempCertificate
        } else {
            val identity = generateNewIdentity(context)
            privateKey = identity.privateKey
            certificate = identity.certificate
            saveIdentity(sharedPrefs, privateKey, certificate)
        }

        // Calculate SHA-256 fingerprint of the full DER cert
        val md = MessageDigest.getInstance("SHA-256")
        val fingerprintBytes = md.digest(certificate.encoded)
        fingerprintHex = fingerprintBytes.joinToString("") { "%02X".format(it) }
    }

    private data class GeneratedKeyPairAndCert(val privateKey: PrivateKey, val certificate: X509Certificate)

    companion object {
        @Volatile
        private var INSTANCE: DeviceIdentity? = null

        fun getInstance(context: Context): DeviceIdentity {
            return INSTANCE ?: synchronized(this) {
                INSTANCE ?: DeviceIdentity(context.applicationContext).also { INSTANCE = it }
            }
        }

        private fun getSecurePreferences(context: Context): android.content.SharedPreferences {
            return try {
                val masterKeyAlias = MasterKeys.getOrCreate(MasterKeys.AES256_GCM_SPEC)
                EncryptedSharedPreferences.create(
                    "hyprlink_secure_prefs",
                    masterKeyAlias,
                    context,
                    EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
                    EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
                )
            } catch (e: Exception) {
                context.getSharedPreferences("hyprlink_secure_prefs_fallback", Context.MODE_PRIVATE)
            }
        }

        // Function to force identity regeneration
        fun regenerateInstance(context: Context): DeviceIdentity {
            synchronized(this) {
                val appContext = context.applicationContext
                val sharedPrefs = getSecurePreferences(appContext)
                
                val identity = generateNewIdentity(appContext)
                saveIdentity(sharedPrefs, identity.privateKey, identity.certificate)
                
                val newIdentity = DeviceIdentity(appContext)
                INSTANCE = newIdentity
                return newIdentity
            }
        }

        private fun generateNewIdentity(context: Context): GeneratedKeyPairAndCert {
            // Get local device name from settings/prefs
            val sharedPrefs = context.getSharedPreferences("hyprlink_prefs", Context.MODE_PRIVATE)
            val deviceName = sharedPrefs.getString("local_device_name", "Android-Phone-Client") ?: "Android-Phone-Client"

            // 1. Generate ECDSA P-256 keypair (secp256r1)
            val g = KeyPairGenerator.getInstance("EC")
            g.initialize(ECGenParameterSpec("secp256r1"))
            val keyPair = g.generateKeyPair()

            // 2. Generate self-signed X.509 Certificate with BouncyCastle
            val subject = X500Name("CN=$deviceName")
            val serial = BigInteger(64, SecureRandom())
            val notBefore = Date()
            val notAfter = Date(notBefore.time + 10L * 365 * 24 * 60 * 60 * 1000) // 10 years

            val certBuilder = JcaX509v3CertificateBuilder(
                subject, // issuer
                serial,
                notBefore,
                notAfter,
                subject, // subject (self-signed)
                keyPair.public
            )

            val signer = JcaContentSignerBuilder("SHA256withECDSA")
                .build(keyPair.private)

            val certHolder = certBuilder.build(signer)
            val certificate = CertificateFactory.getInstance("X.509")
                .generateCertificate(ByteArrayInputStream(certHolder.encoded)) as X509Certificate

            return GeneratedKeyPairAndCert(keyPair.private, certificate)
        }

        private fun saveIdentity(sharedPrefs: android.content.SharedPreferences, privateKey: PrivateKey, certificate: X509Certificate) {
            val privateKeyBase64 = Base64.encodeToString(privateKey.encoded, Base64.DEFAULT)
            val certBase64 = Base64.encodeToString(certificate.encoded, Base64.DEFAULT)

            sharedPrefs.edit()
                .putString("device_private_key", privateKeyBase64)
                .putString("device_certificate", certBase64)
                .apply()
        }
    }
}
