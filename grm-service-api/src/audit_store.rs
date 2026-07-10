use std::collections::{BTreeSet, VecDeque};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};

use crate::{
    SECURITY_AUDIT_MAX_EVENT_BYTES, SECURITY_AUDIT_SCHEMA_VERSION, SecurityAuditEvent,
    SecurityAuditSink, SecurityAuditSinkError, SecurityAuditSinkHealth, security_audit_event_size,
};

const METADATA_VERSION: u16 = 1;
const RECORD_VERSION: u16 = 1;
const AUDIT_DIR: &str = "audit";
const METADATA_FILE: &str = "store-metadata";
const LOG_FILE: &str = "security-audit.log";
const COMPACTION_FILE: &str = "security-audit.log.compacting";
const COUNTER_RESERVATION_RANGE: u64 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditStoreFailurePoint {
    ServiceRootSync,
    InitialMetadataWrite,
    InitialMetadataSync,
    MetadataTempWrite,
    MetadataTempSync,
    MetadataRename,
    MetadataDirectorySync,
    AuditLogWrite,
    AuditLogSync,
    FirstAuditDirectorySync,
    CompactionWrite,
    CompactionSync,
    CompactionRename,
    CompactionDirectorySync,
    RecoveryDirectorySync,
    RecoveryLogSync,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AuditStoreIoStats {
    pub metadata_replacements: u64,
    pub metadata_file_syncs: u64,
    pub metadata_directory_syncs: u64,
    pub audit_log_writes: u64,
    pub audit_log_syncs: u64,
    pub compactions: u64,
}

#[derive(Debug, Default)]
struct AuditStoreInstrumentationState {
    fail_next: Option<AuditStoreFailurePoint>,
    stats: AuditStoreIoStats,
}

#[derive(Debug, Clone, Default)]
pub struct AuditStoreInstrumentation(Arc<Mutex<AuditStoreInstrumentationState>>);

impl AuditStoreInstrumentation {
    pub fn fail_next(&self, point: AuditStoreFailurePoint) {
        if let Ok(mut state) = self.0.lock() {
            state.fail_next = Some(point);
        }
    }

    pub fn stats(&self) -> AuditStoreIoStats {
        self.0.lock().map(|state| state.stats).unwrap_or_default()
    }

    fn check(&self, point: AuditStoreFailurePoint) -> io::Result<()> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| io::Error::other("audit instrumentation unavailable"))?;
        if state.fail_next == Some(point) {
            state.fail_next = None;
            return Err(io::Error::other("injected audit store failure"));
        }
        Ok(())
    }

    fn update(&self, update: impl FnOnce(&mut AuditStoreIoStats)) {
        if let Ok(mut state) = self.0.lock() {
            update(&mut state.stats);
        }
    }
}

#[derive(Debug)]
struct AuditStoreOwnership {
    path: PathBuf,
}

impl Drop for AuditStoreOwnership {
    fn drop(&mut self) {
        if let Ok(mut paths) = open_audit_stores().lock() {
            paths.remove(&self.path);
        }
    }
}

fn open_audit_stores() -> &'static Mutex<BTreeSet<PathBuf>> {
    static OPEN: OnceLock<Mutex<BTreeSet<PathBuf>>> = OnceLock::new();
    OPEN.get_or_init(|| Mutex::new(BTreeSet::new()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityAuditStoreOpenState {
    Initialized,
    Reopened,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityAuditStoreRecovery {
    pub open_state: SecurityAuditStoreOpenState,
    pub store_generation_id: String,
    pub retained_records: usize,
    pub ignored_torn_final_record: bool,
    pub next_event_id: u64,
    pub next_request_id: u64,
    pub next_service_sequence: u64,
    pub future_dated_records: usize,
}

#[derive(Debug)]
pub enum SecurityAuditStoreOpenError {
    Unavailable,
    MalformedMetadata,
    UnsupportedMetadataVersion,
    MalformedRecord,
    UnsupportedRecordVersion,
    InconsistentStore,
    CounterExhausted,
    InvalidRetention,
    AlreadyOpen,
}

impl fmt::Display for SecurityAuditStoreOpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "audit store unavailable",
            Self::MalformedMetadata => "audit store metadata is malformed",
            Self::UnsupportedMetadataVersion => "audit store metadata version is unsupported",
            Self::MalformedRecord => "audit store contains a malformed complete record",
            Self::UnsupportedRecordVersion => "audit store record version is unsupported",
            Self::InconsistentStore => "audit store metadata and records are inconsistent",
            Self::CounterExhausted => "audit store identifier space is exhausted",
            Self::InvalidRetention => "audit store retention configuration is invalid",
            Self::AlreadyOpen => "audit store is already open in this process",
        })
    }
}

impl std::error::Error for SecurityAuditStoreOpenError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoreMetadata {
    version: u16,
    store_generation_id: String,
    // Reserved high-water marks are synchronized before identifiers are issued.
    // A crash can leave gaps, but a restart never reuses an issued identifier.
    reserved_event_id: u64,
    reserved_request_id: u64,
    reserved_service_sequence: u64,
    // Accepted high-water marks describe only records accepted in the log.
    accepted_event_id: u64,
    accepted_request_id: u64,
    accepted_service_sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedAuditRecord {
    version: u16,
    store_generation_id: String,
    event: SecurityAuditEvent,
}

#[derive(Debug)]
struct StoreState {
    metadata: StoreMetadata,
    events: VecDeque<SecurityAuditEvent>,
    record_bytes: VecDeque<usize>,
    retained_bytes: usize,
    degraded: bool,
    next_event_id: u64,
    next_request_id: u64,
    next_service_sequence: u64,
    future_dated_records: usize,
}

#[derive(Debug, Clone)]
pub struct DurableSecurityAuditStore {
    audit_dir: PathBuf,
    metadata_path: PathBuf,
    log_path: PathBuf,
    max_events: usize,
    max_bytes: usize,
    max_age: Duration,
    state: Arc<Mutex<StoreState>>,
    recovery: SecurityAuditStoreRecovery,
    instrumentation: AuditStoreInstrumentation,
    _ownership: Arc<AuditStoreOwnership>,
}

impl DurableSecurityAuditStore {
    pub fn open_default(
        service_root: impl AsRef<Path>,
    ) -> Result<Self, SecurityAuditStoreOpenError> {
        Self::open(
            service_root,
            crate::DEFAULT_SECURITY_AUDIT_MAX_EVENTS,
            crate::DEFAULT_SECURITY_AUDIT_MAX_BYTES,
            crate::DEFAULT_SECURITY_AUDIT_MAX_AGE,
        )
    }

    pub fn open(
        service_root: impl AsRef<Path>,
        max_events: usize,
        max_bytes: usize,
        max_age: Duration,
    ) -> Result<Self, SecurityAuditStoreOpenError> {
        Self::open_with_instrumentation(
            service_root,
            max_events,
            max_bytes,
            max_age,
            AuditStoreInstrumentation::default(),
        )
    }

    pub fn open_with_instrumentation(
        service_root: impl AsRef<Path>,
        max_events: usize,
        max_bytes: usize,
        max_age: Duration,
        instrumentation: AuditStoreInstrumentation,
    ) -> Result<Self, SecurityAuditStoreOpenError> {
        if max_events == 0 || max_bytes == 0 || max_age.is_zero() {
            return Err(SecurityAuditStoreOpenError::InvalidRetention);
        }
        let service_root = service_root.as_ref().to_path_buf();
        let audit_dir = service_root.join(AUDIT_DIR);
        let metadata_path = audit_dir.join(METADATA_FILE);
        let log_path = audit_dir.join(LOG_FILE);
        let audit_dir_existed = audit_dir.exists();
        create_private_dir(&service_root)?;
        create_private_dir(&audit_dir)?;
        let ownership_path = audit_dir
            .canonicalize()
            .map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
        {
            let mut paths = open_audit_stores()
                .lock()
                .map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
            if !paths.insert(ownership_path.clone()) {
                return Err(SecurityAuditStoreOpenError::AlreadyOpen);
            }
        }
        let ownership = Arc::new(AuditStoreOwnership {
            path: ownership_path,
        });
        if !audit_dir_existed {
            instrumentation
                .check(AuditStoreFailurePoint::ServiceRootSync)
                .map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
            sync_dir(&service_root).map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
        }
        let (metadata, open_state) = if metadata_path.exists() {
            (
                read_metadata(&metadata_path)?,
                SecurityAuditStoreOpenState::Reopened,
            )
        } else {
            let metadata = StoreMetadata {
                version: METADATA_VERSION,
                store_generation_id: random_generation_id()?,
                reserved_event_id: 0,
                reserved_request_id: 0,
                reserved_service_sequence: 0,
                accepted_event_id: 0,
                accepted_request_id: 0,
                accepted_service_sequence: 0,
            };
            write_new_metadata(&metadata_path, &metadata, &instrumentation)?;
            instrumentation
                .check(AuditStoreFailurePoint::MetadataDirectorySync)
                .map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
            sync_dir(&audit_dir).map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
            (metadata, SecurityAuditStoreOpenState::Initialized)
        };
        if metadata.version != METADATA_VERSION {
            return Err(SecurityAuditStoreOpenError::UnsupportedMetadataVersion);
        }
        if metadata.store_generation_id.len() != 32
            || !metadata
                .store_generation_id
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
        {
            return Err(SecurityAuditStoreOpenError::MalformedMetadata);
        }
        let (records, ignored_torn_final_record) = read_records(&log_path, &metadata, max_bytes)?;
        let mut events = VecDeque::new();
        let mut record_bytes = VecDeque::new();
        let mut retained_bytes = 0usize;
        let mut max_event = metadata.accepted_event_id;
        let mut max_request = metadata.accepted_request_id;
        let mut max_sequence = metadata.accepted_service_sequence;
        let mut previous_event = 0;
        let mut previous_sequence = 0;
        let mut future_dated_records = 0usize;
        for (event, bytes) in records {
            if validate_event_for_recovery(&event, bytes, max_bytes)? {
                future_dated_records = future_dated_records.saturating_add(1);
            }
            if event.event_id <= previous_event || event.service_sequence <= previous_sequence {
                return Err(SecurityAuditStoreOpenError::InconsistentStore);
            }
            previous_event = event.event_id;
            previous_sequence = event.service_sequence;
            max_event = max_event.max(event.event_id);
            max_request = max_request.max(event.request_id);
            max_sequence = max_sequence.max(event.service_sequence);
            retained_bytes = retained_bytes
                .checked_add(bytes)
                .ok_or(SecurityAuditStoreOpenError::InconsistentStore)?;
            events.push_back(event);
            record_bytes.push_back(bytes);
        }
        let recovered_record_count = events.len();
        let recovered_retained_bytes = retained_bytes;
        if max_event > metadata.reserved_event_id
            || max_request > metadata.reserved_request_id
            || max_sequence > metadata.reserved_service_sequence
        {
            return Err(SecurityAuditStoreOpenError::InconsistentStore);
        }
        for value in [
            metadata.reserved_event_id,
            metadata.reserved_request_id,
            metadata.reserved_service_sequence,
        ] {
            value
                .checked_add(1)
                .ok_or(SecurityAuditStoreOpenError::CounterExhausted)?;
        }
        let next_event_id = metadata.reserved_event_id.saturating_add(1);
        let next_request_id = metadata.reserved_request_id.saturating_add(1);
        let next_service_sequence = metadata.reserved_service_sequence.saturating_add(1);
        let mut state = StoreState {
            metadata,
            events,
            record_bytes,
            retained_bytes,
            degraded: false,
            next_event_id,
            next_request_id,
            next_service_sequence,
            future_dated_records,
        };
        prune_retention(
            &mut state,
            max_events,
            max_bytes,
            max_age,
            SystemTime::now(),
        );
        let recovery_requires_compaction = ignored_torn_final_record
            || recovered_record_count != state.events.len()
            || recovered_retained_bytes != state.retained_bytes;
        let recovery = SecurityAuditStoreRecovery {
            open_state,
            store_generation_id: state.metadata.store_generation_id.clone(),
            retained_records: state.events.len(),
            ignored_torn_final_record,
            next_event_id: state.metadata.reserved_event_id + 1,
            next_request_id: state.metadata.reserved_request_id + 1,
            next_service_sequence: state.metadata.reserved_service_sequence + 1,
            future_dated_records,
        };
        let store = Self {
            audit_dir,
            metadata_path,
            log_path,
            max_events,
            max_bytes,
            max_age,
            state: Arc::new(Mutex::new(state)),
            recovery,
            instrumentation,
            _ownership: ownership,
        };
        // Remove an uncommitted compaction artifact. The active log remains authoritative.
        let _ = fs::remove_file(store.audit_dir.join(COMPACTION_FILE));
        if recovery_requires_compaction {
            store.compact_if_pruned()?;
        }
        Ok(store)
    }

    pub fn recovery(&self) -> &SecurityAuditStoreRecovery {
        &self.recovery
    }

    pub fn store_generation_id(&self) -> &str {
        &self.recovery.store_generation_id
    }

    pub fn retained_events(&self) -> Vec<SecurityAuditEvent> {
        self.state
            .lock()
            .map(|state| state.events.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn future_dated_records(&self) -> usize {
        self.state
            .lock()
            .map(|state| state.future_dated_records)
            .unwrap_or_default()
    }

    pub fn audit_directory(&self) -> &Path {
        &self.audit_dir
    }

    fn compact_if_pruned(&self) -> Result<(), SecurityAuditStoreOpenError> {
        let state = self
            .state
            .lock()
            .map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
        if state.events.len() == state.record_bytes.len() {
            compact(
                &self.audit_dir,
                &self.log_path,
                &state.metadata,
                &state.events,
                &self.instrumentation,
            )?;
        }
        Ok(())
    }
}

impl SecurityAuditSink for DurableSecurityAuditStore {
    fn allocate_request_id(&self) -> Result<u64, SecurityAuditSinkError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| SecurityAuditSinkError::Unavailable)?;
        if state.next_request_id > state.metadata.reserved_request_id {
            let previous_reserved = state.metadata.reserved_request_id;
            state.metadata.reserved_request_id = state
                .metadata
                .reserved_request_id
                .checked_add(COUNTER_RESERVATION_RANGE)
                .ok_or(SecurityAuditSinkError::Unavailable)?;
            if replace_metadata(
                &self.metadata_path,
                &self.audit_dir,
                &state.metadata,
                &self.instrumentation,
            )
            .is_err()
            {
                state.metadata.reserved_request_id = previous_reserved;
                state.degraded = true;
                return Err(SecurityAuditSinkError::Unavailable);
            }
        }
        let id = state.next_request_id;
        state.next_request_id = id
            .checked_add(1)
            .ok_or(SecurityAuditSinkError::Unavailable)?;
        Ok(id)
    }

    fn allocate_event_identity(&self) -> Result<(u64, u64), SecurityAuditSinkError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| SecurityAuditSinkError::Unavailable)?;
        if state.next_event_id > state.metadata.reserved_event_id
            || state.next_service_sequence > state.metadata.reserved_service_sequence
        {
            let previous_event_reserved = state.metadata.reserved_event_id;
            let previous_sequence_reserved = state.metadata.reserved_service_sequence;
            state.metadata.reserved_event_id = state
                .metadata
                .reserved_event_id
                .checked_add(COUNTER_RESERVATION_RANGE)
                .ok_or(SecurityAuditSinkError::Unavailable)?;
            state.metadata.reserved_service_sequence = state
                .metadata
                .reserved_service_sequence
                .checked_add(COUNTER_RESERVATION_RANGE)
                .ok_or(SecurityAuditSinkError::Unavailable)?;
            if replace_metadata(
                &self.metadata_path,
                &self.audit_dir,
                &state.metadata,
                &self.instrumentation,
            )
            .is_err()
            {
                state.metadata.reserved_event_id = previous_event_reserved;
                state.metadata.reserved_service_sequence = previous_sequence_reserved;
                state.degraded = true;
                return Err(SecurityAuditSinkError::Unavailable);
            }
        }
        let event_id = state.next_event_id;
        let sequence = state.next_service_sequence;
        state.next_event_id = event_id
            .checked_add(1)
            .ok_or(SecurityAuditSinkError::Unavailable)?;
        state.next_service_sequence = sequence
            .checked_add(1)
            .ok_or(SecurityAuditSinkError::Unavailable)?;
        Ok((event_id, sequence))
    }

    fn append(&self, event: SecurityAuditEvent) -> Result<(), SecurityAuditSinkError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| SecurityAuditSinkError::Unavailable)?;
        validate_event_for_append(&event).map_err(|_| SecurityAuditSinkError::EventTooLarge)?;
        let record = PersistedAuditRecord {
            version: RECORD_VERSION,
            store_generation_id: state.metadata.store_generation_id.clone(),
            event,
        };
        let mut line =
            serde_json::to_vec(&record).map_err(|_| SecurityAuditSinkError::Unavailable)?;
        line.push(b'\n');
        if line.len() > SECURITY_AUDIT_MAX_EVENT_BYTES || line.len() > self.max_bytes {
            return Err(SecurityAuditSinkError::EventTooLarge);
        }
        if record.event.event_id > state.metadata.reserved_event_id
            || record.event.request_id > state.metadata.reserved_request_id
            || record.event.service_sequence > state.metadata.reserved_service_sequence
        {
            state.degraded = true;
            return Err(SecurityAuditSinkError::Unavailable);
        }
        let log_existed = self.log_path.exists();
        if append_and_sync(&self.log_path, &line, &self.instrumentation).is_err()
            || (!log_existed
                && (self
                    .instrumentation
                    .check(AuditStoreFailurePoint::FirstAuditDirectorySync)
                    .is_err()
                    || sync_dir(&self.audit_dir).is_err()))
        {
            state.degraded = true;
            return Err(SecurityAuditSinkError::Unavailable);
        }
        state.metadata.accepted_event_id =
            state.metadata.accepted_event_id.max(record.event.event_id);
        state.metadata.accepted_request_id = state
            .metadata
            .accepted_request_id
            .max(record.event.request_id);
        state.metadata.accepted_service_sequence = state
            .metadata
            .accepted_service_sequence
            .max(record.event.service_sequence);
        state.retained_bytes += line.len();
        state.record_bytes.push_back(line.len());
        state.events.push_back(record.event);
        let before = state.events.len();
        prune_retention(
            &mut state,
            self.max_events,
            self.max_bytes,
            self.max_age,
            SystemTime::now(),
        );
        if state.events.len() != before
            && compact(
                &self.audit_dir,
                &self.log_path,
                &state.metadata,
                &state.events,
                &self.instrumentation,
            )
            .is_err()
        {
            state.degraded = true;
            return Err(SecurityAuditSinkError::Unavailable);
        }
        if replace_metadata(
            &self.metadata_path,
            &self.audit_dir,
            &state.metadata,
            &self.instrumentation,
        )
        .is_err()
        {
            state.degraded = true;
            return Err(SecurityAuditSinkError::Unavailable);
        }
        state.degraded = false;
        Ok(())
    }

    fn health(&self) -> SecurityAuditSinkHealth {
        self.state
            .lock()
            .map(|state| {
                if state.degraded {
                    SecurityAuditSinkHealth::Degraded
                } else {
                    SecurityAuditSinkHealth::Healthy
                }
            })
            .unwrap_or(SecurityAuditSinkHealth::Degraded)
    }

    fn try_recover(&self) -> Result<SecurityAuditSinkHealth, SecurityAuditSinkError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| SecurityAuditSinkError::Unavailable)?;
        let recovered = (|| -> Result<StoreState, SecurityAuditStoreOpenError> {
            let metadata = read_metadata(&self.metadata_path)?;
            if metadata.version != METADATA_VERSION
                || metadata.store_generation_id != state.metadata.store_generation_id
                || metadata.store_generation_id.len() != 32
                || !metadata
                    .store_generation_id
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
                || metadata.accepted_event_id > metadata.reserved_event_id
                || metadata.accepted_request_id > metadata.reserved_request_id
                || metadata.accepted_service_sequence > metadata.reserved_service_sequence
            {
                return Err(SecurityAuditStoreOpenError::InconsistentStore);
            }
            let next_reserved_event = metadata
                .reserved_event_id
                .checked_add(1)
                .ok_or(SecurityAuditStoreOpenError::CounterExhausted)?;
            let next_reserved_request = metadata
                .reserved_request_id
                .checked_add(1)
                .ok_or(SecurityAuditStoreOpenError::CounterExhausted)?;
            let next_reserved_sequence = metadata
                .reserved_service_sequence
                .checked_add(1)
                .ok_or(SecurityAuditStoreOpenError::CounterExhausted)?;
            let (records, torn) = read_records(&self.log_path, &metadata, self.max_bytes)?;
            let recovered_count = records.len();
            let mut events = VecDeque::new();
            let mut record_bytes = VecDeque::new();
            let mut retained_bytes = 0usize;
            let mut previous_event = 0u64;
            let mut previous_sequence = 0u64;
            let mut maximum_event = metadata.accepted_event_id;
            let mut maximum_request = metadata.accepted_request_id;
            let mut maximum_sequence = metadata.accepted_service_sequence;
            let mut future_dated_records = 0usize;
            for (event, bytes) in records {
                if validate_event_for_recovery(&event, bytes, self.max_bytes)? {
                    future_dated_records = future_dated_records.saturating_add(1);
                }
                if event.event_id <= previous_event || event.service_sequence <= previous_sequence {
                    return Err(SecurityAuditStoreOpenError::InconsistentStore);
                }
                previous_event = event.event_id;
                previous_sequence = event.service_sequence;
                maximum_event = maximum_event.max(event.event_id);
                maximum_request = maximum_request.max(event.request_id);
                maximum_sequence = maximum_sequence.max(event.service_sequence);
                retained_bytes = retained_bytes
                    .checked_add(bytes)
                    .ok_or(SecurityAuditStoreOpenError::InconsistentStore)?;
                events.push_back(event);
                record_bytes.push_back(bytes);
            }
            if maximum_event > metadata.reserved_event_id
                || maximum_request > metadata.reserved_request_id
                || maximum_sequence > metadata.reserved_service_sequence
            {
                return Err(SecurityAuditStoreOpenError::InconsistentStore);
            }
            let recovered_bytes = retained_bytes;
            let mut candidate = StoreState {
                metadata,
                events,
                record_bytes,
                retained_bytes,
                degraded: true,
                next_event_id: state.next_event_id.max(next_reserved_event),
                next_request_id: state.next_request_id.max(next_reserved_request),
                next_service_sequence: state.next_service_sequence.max(next_reserved_sequence),
                future_dated_records,
            };
            prune_retention(
                &mut candidate,
                self.max_events,
                self.max_bytes,
                self.max_age,
                SystemTime::now(),
            );
            let compacted = torn
                || recovered_count != candidate.events.len()
                || recovered_bytes != candidate.retained_bytes;
            if compacted {
                compact(
                    &self.audit_dir,
                    &self.log_path,
                    &candidate.metadata,
                    &candidate.events,
                    &self.instrumentation,
                )?;
            } else {
                match File::open(&self.log_path) {
                    Ok(file) => {
                        self.instrumentation
                            .check(AuditStoreFailurePoint::RecoveryLogSync)
                            .map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
                        file.sync_all()
                            .map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
                    }
                    Err(error)
                        if error.kind() == io::ErrorKind::NotFound
                            && candidate.events.is_empty() => {}
                    Err(_) => return Err(SecurityAuditStoreOpenError::Unavailable),
                }
            }
            candidate.metadata.accepted_event_id = maximum_event;
            candidate.metadata.accepted_request_id = maximum_request;
            candidate.metadata.accepted_service_sequence = maximum_sequence;
            replace_metadata(
                &self.metadata_path,
                &self.audit_dir,
                &candidate.metadata,
                &self.instrumentation,
            )
            .map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
            self.instrumentation
                .check(AuditStoreFailurePoint::RecoveryDirectorySync)
                .map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
            sync_dir(&self.audit_dir).map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
            candidate.degraded = false;
            Ok(candidate)
        })();
        match recovered {
            Ok(candidate) => {
                *state = candidate;
                Ok(SecurityAuditSinkHealth::Healthy)
            }
            Err(_) => {
                state.degraded = true;
                Err(SecurityAuditSinkError::Unavailable)
            }
        }
    }
}

fn random_generation_id() -> Result<String, SecurityAuditStoreOpenError> {
    let mut bytes = [0u8; 16];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn create_private_dir(path: &Path) -> Result<(), SecurityAuditStoreOpenError> {
    fs::create_dir_all(path).map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
    }
    Ok(())
}

fn sync_dir(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

fn write_new_metadata(
    path: &Path,
    metadata: &StoreMetadata,
    instrumentation: &AuditStoreInstrumentation,
) -> Result<(), SecurityAuditStoreOpenError> {
    let bytes =
        serde_json::to_vec(metadata).map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
    instrumentation
        .check(AuditStoreFailurePoint::InitialMetadataWrite)
        .map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
    file.write_all(&bytes)
        .and_then(|_| file.write_all(b"\n"))
        .map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
    instrumentation
        .check(AuditStoreFailurePoint::InitialMetadataSync)
        .map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
    file.sync_all()
        .map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
    instrumentation.update(|stats| stats.metadata_file_syncs += 1);
    Ok(())
}

fn read_metadata(path: &Path) -> Result<StoreMetadata, SecurityAuditStoreOpenError> {
    let bytes = fs::read(path).map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
    serde_json::from_slice(&bytes).map_err(|_| SecurityAuditStoreOpenError::MalformedMetadata)
}

fn replace_metadata(
    path: &Path,
    dir: &Path,
    metadata: &StoreMetadata,
    instrumentation: &AuditStoreInstrumentation,
) -> io::Result<()> {
    let temp = dir.join("store-metadata.tmp");
    let bytes = serde_json::to_vec(metadata).map_err(io::Error::other)?;
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temp)?;
    instrumentation.check(AuditStoreFailurePoint::MetadataTempWrite)?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    instrumentation.check(AuditStoreFailurePoint::MetadataTempSync)?;
    file.sync_all()?;
    instrumentation.update(|stats| stats.metadata_file_syncs += 1);
    instrumentation.check(AuditStoreFailurePoint::MetadataRename)?;
    fs::rename(temp, path)?;
    instrumentation.check(AuditStoreFailurePoint::MetadataDirectorySync)?;
    sync_dir(dir)?;
    instrumentation.update(|stats| {
        stats.metadata_replacements += 1;
        stats.metadata_directory_syncs += 1;
    });
    Ok(())
}

fn append_and_sync(
    path: &Path,
    line: &[u8],
    instrumentation: &AuditStoreInstrumentation,
) -> io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    instrumentation.check(AuditStoreFailurePoint::AuditLogWrite)?;
    file.write_all(line)?;
    instrumentation.update(|stats| stats.audit_log_writes += 1);
    instrumentation.check(AuditStoreFailurePoint::AuditLogSync)?;
    file.sync_all()?;
    instrumentation.update(|stats| stats.audit_log_syncs += 1);
    Ok(())
}

fn read_records(
    path: &Path,
    metadata: &StoreMetadata,
    max_bytes: usize,
) -> Result<(Vec<(SecurityAuditEvent, usize)>, bool), SecurityAuditStoreOpenError> {
    let maximum_physical_bytes = max_bytes
        .checked_add(SECURITY_AUDIT_MAX_EVENT_BYTES)
        .ok_or(SecurityAuditStoreOpenError::Unavailable)?;
    match fs::metadata(path) {
        Ok(file) if file.len() > maximum_physical_bytes as u64 => {
            return Err(SecurityAuditStoreOpenError::MalformedRecord);
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((Vec::new(), false)),
        Err(_) => return Err(SecurityAuditStoreOpenError::Unavailable),
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return Err(SecurityAuditStoreOpenError::Unavailable),
    };
    let mut records = Vec::new();
    let mut start = 0;
    let mut torn = false;
    while start < bytes.len() {
        let Some(relative_end) = bytes[start..].iter().position(|byte| *byte == b'\n') else {
            torn = true;
            break;
        };
        let end = start + relative_end;
        let line = &bytes[start..end];
        start = end + 1;
        if line.is_empty() {
            continue;
        }
        let record: PersistedAuditRecord = serde_json::from_slice(line)
            .map_err(|_| SecurityAuditStoreOpenError::MalformedRecord)?;
        if record.version != RECORD_VERSION {
            return Err(SecurityAuditStoreOpenError::UnsupportedRecordVersion);
        }
        if record.store_generation_id != metadata.store_generation_id {
            return Err(SecurityAuditStoreOpenError::InconsistentStore);
        }
        records.push((record.event, line.len() + 1));
    }
    Ok((records, torn))
}

fn validate_event_for_recovery(
    event: &SecurityAuditEvent,
    record_bytes: usize,
    max_bytes: usize,
) -> Result<bool, SecurityAuditStoreOpenError> {
    if validate_event_common(event).is_err()
        || record_bytes > SECURITY_AUDIT_MAX_EVENT_BYTES
        || record_bytes > max_bytes
    {
        return Err(SecurityAuditStoreOpenError::MalformedRecord);
    }
    Ok(event.timestamp > SystemTime::now())
}

fn validate_event_for_append(event: &SecurityAuditEvent) -> Result<(), ()> {
    validate_event_common(event)?;
    let latest_timestamp = SystemTime::now()
        .checked_add(Duration::from_secs(5 * 60))
        .ok_or(())?;
    if event.timestamp > latest_timestamp {
        return Err(());
    }
    Ok(())
}

fn validate_event_common(event: &SecurityAuditEvent) -> Result<(), ()> {
    let bounded = |value: &str| value.len() <= crate::SECURITY_AUDIT_MAX_FIELD_BYTES;
    if event.schema_version != SECURITY_AUDIT_SCHEMA_VERSION
        || event.operations.len() > crate::SECURITY_AUDIT_MAX_OPERATIONS
        || !bounded(&event.service_identity)
        || event.timestamp.duration_since(UNIX_EPOCH).is_err()
        || security_audit_event_size(event) > SECURITY_AUDIT_MAX_EVENT_BYTES
        || event
            .workspace
            .as_deref()
            .is_some_and(|value| !bounded(value))
        || event
            .policy_version
            .as_deref()
            .is_some_and(|value| !bounded(value))
        || event
            .asserted_actor
            .as_ref()
            .is_some_and(|actor| !bounded(&actor.actor_id))
        || event
            .authenticated_principal
            .as_ref()
            .is_some_and(|principal| {
                !bounded(&principal.issuer)
                    || !bounded(&principal.subject)
                    || !bounded(&principal.authentication_provider)
                    || !bounded(&principal.authentication_method)
            })
        || event.operations.iter().any(|operation| {
            !bounded(&operation.workspace)
                || operation
                    .model
                    .as_deref()
                    .is_some_and(|value| !bounded(value))
        })
    {
        return Err(());
    }
    Ok(())
}

fn prune_retention(
    state: &mut StoreState,
    max_events: usize,
    max_bytes: usize,
    max_age: Duration,
    now: SystemTime,
) {
    let cutoff = now.checked_sub(max_age);
    while state.events.len() > max_events
        || state.retained_bytes > max_bytes
        || cutoff.is_some_and(|cutoff| {
            state
                .events
                .front()
                .is_some_and(|event| event.timestamp < cutoff)
        })
    {
        state.events.pop_front();
        if let Some(bytes) = state.record_bytes.pop_front() {
            state.retained_bytes = state.retained_bytes.saturating_sub(bytes);
        }
    }
}

fn compact(
    dir: &Path,
    log_path: &Path,
    metadata: &StoreMetadata,
    events: &VecDeque<SecurityAuditEvent>,
    instrumentation: &AuditStoreInstrumentation,
) -> Result<(), SecurityAuditStoreOpenError> {
    let temp_path = dir.join(COMPACTION_FILE);
    let mut temp = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temp_path)
        .map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
    for event in events {
        let record = PersistedAuditRecord {
            version: RECORD_VERSION,
            store_generation_id: metadata.store_generation_id.clone(),
            event: event.clone(),
        };
        let line =
            serde_json::to_vec(&record).map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
        instrumentation
            .check(AuditStoreFailurePoint::CompactionWrite)
            .map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
        temp.write_all(&line)
            .and_then(|_| temp.write_all(b"\n"))
            .map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
    }
    instrumentation
        .check(AuditStoreFailurePoint::CompactionSync)
        .map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
    temp.sync_all()
        .map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
    instrumentation
        .check(AuditStoreFailurePoint::CompactionRename)
        .map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
    fs::rename(&temp_path, log_path).map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
    instrumentation
        .check(AuditStoreFailurePoint::CompactionDirectorySync)
        .map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
    sync_dir(dir).map_err(|_| SecurityAuditStoreOpenError::Unavailable)?;
    instrumentation.update(|stats| stats.compactions += 1);
    Ok(())
}
