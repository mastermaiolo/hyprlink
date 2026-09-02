package com.example

import android.content.Context
import androidx.room.*
import kotlinx.coroutines.flow.Flow

// --- 1. Entity ---
@Entity(tableName = "paired_workstations")
data class PairedWorkstation(
    @PrimaryKey val id: String, // Fingerprint in clean hex/uppercase
    val deviceName: String,
    val host: String,
    val port: String,
    val fingerprint: String,
    val pairingTokenHex: String?,
    val pairedAt: Long = System.currentTimeMillis()
)

@Entity(tableName = "transfer_records")
data class TransferRecord(
    @PrimaryKey(autoGenerate = true) val id: Long = 0,
    val name: String,
    val size: Long,
    val direction: String, // "UPLOAD" or "DOWNLOAD"
    val status: String,    // "VERIFICADO", "NAO_VERIFICADO", "ERRO"
    val sha256Local: String?,
    val sha256Remote: String?,
    val error: String?,
    val timestamp: Long = System.currentTimeMillis()
)

// --- 2. DAO ---
@Dao
interface WorkstationDao {
    @Query("SELECT * FROM paired_workstations ORDER BY pairedAt DESC")
    fun getAll(): Flow<List<PairedWorkstation>>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun insert(workstation: PairedWorkstation)

    @Delete
    suspend fun delete(workstation: PairedWorkstation)

    @Query("DELETE FROM paired_workstations WHERE id = :id")
    suspend fun deleteById(id: String)

    @Query("SELECT * FROM paired_workstations WHERE id = :id LIMIT 1")
    suspend fun getById(id: String): PairedWorkstation?
}

@Dao
interface TransferRecordDao {
    @Query("SELECT * FROM transfer_records ORDER BY timestamp DESC LIMIT 50")
    fun getRecent(): Flow<List<TransferRecord>>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun insert(record: TransferRecord)

    @Query("DELETE FROM transfer_records")
    suspend fun clearAll()
}

// --- 3. Database Singleton ---
@Database(entities = [PairedWorkstation::class, TransferRecord::class], version = 2, exportSchema = false)
abstract class AppDatabase : RoomDatabase() {
    abstract fun workstationDao(): WorkstationDao
    abstract fun transferRecordDao(): TransferRecordDao

    companion object {
        @Volatile
        private var INSTANCE: AppDatabase? = null

        val MIGRATION_1_2 = object : androidx.room.migration.Migration(1, 2) {
            override fun migrate(db: androidx.sqlite.db.SupportSQLiteDatabase) {
                db.execSQL("""
                    CREATE TABLE IF NOT EXISTS `transfer_records` (
                        `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                        `name` TEXT NOT NULL,
                        `size` INTEGER NOT NULL,
                        `direction` TEXT NOT NULL,
                        `status` TEXT NOT NULL,
                        `sha256Local` TEXT,
                        `sha256Remote` TEXT,
                        `error` TEXT,
                        `timestamp` INTEGER NOT NULL
                    )
                """.trimIndent())
            }
        }

        fun getDatabase(context: Context): AppDatabase {
            return INSTANCE ?: synchronized(this) {
                val instance = Room.databaseBuilder(
                    context.applicationContext,
                    AppDatabase::class.java,
                    "hyprlink_database"
                )
                .addMigrations(MIGRATION_1_2)
                .build()
                INSTANCE = instance
                instance
            }
        }
    }
}

// --- 4. Repository Pattern ---
class WorkstationRepository(private val workstationDao: WorkstationDao) {
    val allWorkstations: Flow<List<PairedWorkstation>> = workstationDao.getAll()

    suspend fun insert(workstation: PairedWorkstation) {
        workstationDao.insert(workstation)
    }

    suspend fun delete(workstation: PairedWorkstation) {
        workstationDao.delete(workstation)
    }

    suspend fun deleteById(id: String) {
        workstationDao.deleteById(id)
    }

    suspend fun getById(id: String): PairedWorkstation? {
        return workstationDao.getById(id)
    }
}
