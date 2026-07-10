use std::fs::{self, OpenOptions};
use std::io::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use grm_service_api::{
    AuditStoreFailurePoint, AuditStoreInstrumentation, DurableSecurityAuditStore,
    SECURITY_AUDIT_SCHEMA_VERSION, SecurityAction, SecurityAuditActorAssertion,
    SecurityAuditDecision, SecurityAuditDeliveryOutcome, SecurityAuditDurabilityOutcome,
    SecurityAuditEvent, SecurityAuditMode, SecurityAuditOperation, SecurityAuditPrincipal,
    SecurityAuditReason, SecurityAuditRuntimeOutcome, SecurityAuditSink, SecurityAuditSinkError,
    SecurityAuditSinkHealth, SecurityAuditStage, SecurityAuditStoreOpenError,
    SecurityAuditStoreOpenState, SecurityAuditTransportPeer, SecurityResourceKind,
};

type PersistedRecordMutation = Box<dyn FnOnce(&mut serde_json::Value)>;

fn event(
    store: &DurableSecurityAuditStore,
    request_id: u64,
    stage: SecurityAuditStage,
) -> SecurityAuditEvent {
    let (event_id, service_sequence) = store.allocate_event_identity().unwrap();
    SecurityAuditEvent {
        schema_version: SECURITY_AUDIT_SCHEMA_VERSION,
        event_id,
        request_id,
        service_sequence,
        timestamp: SystemTime::now(),
        service_identity: "grm-local-workspace-service".into(),
        mode: SecurityAuditMode::Mandatory,
        stage,
        transport_peer: SecurityAuditTransportPeer {
            remote_address_present: true,
            client_certificate_present: true,
        },
        authenticated_principal: None,
        asserted_actor: None,
        workspace: Some("opaque-workspace-ref".into()),
        operations: vec![SecurityAuditOperation {
            action: SecurityAction::NodeCreate,
            resource_kind: SecurityResourceKind::NodeModel,
            workspace: "opaque-workspace-ref".into(),
            model: Some("PublicModelName".into()),
        }],
        policy_version: Some("policy-v1".into()),
        decision: SecurityAuditDecision::Allow,
        reason: SecurityAuditReason::ExplicitPolicyAllow,
        runtime_outcome: SecurityAuditRuntimeOutcome::NotReached,
        durability_outcome: SecurityAuditDurabilityOutcome::NotApplicable,
        delivery_outcome: SecurityAuditDeliveryOutcome::NotReached,
    }
}

fn persisted_record_with(
    mutate: impl FnOnce(&mut serde_json::Value),
) -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let store = DurableSecurityAuditStore::open_default(temp.path()).unwrap();
    let request_id = store.allocate_request_id().unwrap();
    store
        .append(event(&store, request_id, SecurityAuditStage::Attempt))
        .unwrap();
    let log = store.audit_directory().join("security-audit.log");
    drop(store);
    let mut value: serde_json::Value =
        serde_json::from_slice(fs::read(&log).unwrap().trim_ascii()).unwrap();
    mutate(&mut value);
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    fs::write(&log, bytes).unwrap();
    (temp, log)
}

#[test]
fn initializes_reopens_and_recovers_compound_identity_counters() {
    let temp = tempfile::tempdir().unwrap();
    let store =
        DurableSecurityAuditStore::open(temp.path(), 32, 64 * 1024, Duration::from_secs(3600))
            .unwrap();
    assert_eq!(
        store.recovery().open_state,
        SecurityAuditStoreOpenState::Initialized
    );
    let generation = store.store_generation_id().to_owned();
    let request_id = store.allocate_request_id().unwrap();
    let first = event(&store, request_id, SecurityAuditStage::Attempt);
    let first_event_id = first.event_id;
    let first_sequence = first.service_sequence;
    store.append(first).unwrap();
    drop(store);

    let reopened =
        DurableSecurityAuditStore::open(temp.path(), 32, 64 * 1024, Duration::from_secs(3600))
            .unwrap();
    assert_eq!(
        reopened.recovery().open_state,
        SecurityAuditStoreOpenState::Reopened
    );
    assert_eq!(reopened.store_generation_id(), generation);
    assert_eq!(reopened.retained_events().len(), 1);
    assert!(reopened.allocate_request_id().unwrap() > request_id);
    let next = event(
        &reopened,
        reopened.allocate_request_id().unwrap(),
        SecurityAuditStage::Authentication,
    );
    assert!(next.event_id > first_event_id);
    assert!(next.service_sequence > first_sequence);
}

#[test]
fn ignores_only_a_torn_final_record_and_rejects_malformed_complete_records() {
    let temp = tempfile::tempdir().unwrap();
    let store =
        DurableSecurityAuditStore::open(temp.path(), 32, 64 * 1024, Duration::from_secs(3600))
            .unwrap();
    let request_id = store.allocate_request_id().unwrap();
    store
        .append(event(&store, request_id, SecurityAuditStage::Attempt))
        .unwrap();
    let log = store.audit_directory().join("security-audit.log");
    OpenOptions::new()
        .append(true)
        .open(&log)
        .unwrap()
        .write_all(b"{torn")
        .unwrap();
    drop(store);
    let reopened =
        DurableSecurityAuditStore::open(temp.path(), 32, 64 * 1024, Duration::from_secs(3600))
            .unwrap();
    assert!(reopened.recovery().ignored_torn_final_record);
    assert_eq!(reopened.retained_events().len(), 1);
    drop(reopened);

    fs::write(&log, b"{malformed}\n").unwrap();
    assert!(matches!(
        DurableSecurityAuditStore::open(temp.path(), 32, 64 * 1024, Duration::from_secs(3600)),
        Err(SecurityAuditStoreOpenError::MalformedRecord)
    ));
}

#[test]
fn torn_only_log_is_compacted_empty_before_a_later_append() {
    let temp = tempfile::tempdir().unwrap();
    let store = DurableSecurityAuditStore::open_default(temp.path()).unwrap();
    let log = store.audit_directory().join("security-audit.log");
    fs::write(&log, b"{torn-only").unwrap();
    drop(store);

    let reopened = DurableSecurityAuditStore::open_default(temp.path()).unwrap();
    assert!(reopened.recovery().ignored_torn_final_record);
    assert!(reopened.retained_events().is_empty());
    assert_eq!(fs::metadata(&log).unwrap().len(), 0);

    let request_id = reopened.allocate_request_id().unwrap();
    reopened
        .append(event(&reopened, request_id, SecurityAuditStage::Attempt))
        .unwrap();
    drop(reopened);

    let reopened_again = DurableSecurityAuditStore::open_default(temp.path()).unwrap();
    assert_eq!(reopened_again.retained_events().len(), 1);
}

#[test]
fn startup_expiry_compacts_all_records_to_empty_and_allows_reopen_and_append() {
    let temp = tempfile::tempdir().unwrap();
    let store = DurableSecurityAuditStore::open(temp.path(), 32, 64 * 1024, Duration::MAX).unwrap();
    let log = store.audit_directory().join("security-audit.log");
    let request_id = store.allocate_request_id().unwrap();
    let mut expired_event = event(&store, request_id, SecurityAuditStage::Attempt);
    expired_event.timestamp = UNIX_EPOCH;
    store.append(expired_event).unwrap();
    assert!(fs::metadata(&log).unwrap().len() > 0);
    drop(store);

    let expired =
        DurableSecurityAuditStore::open(temp.path(), 32, 64 * 1024, Duration::from_secs(1))
            .unwrap();
    assert!(expired.retained_events().is_empty());
    assert_eq!(fs::metadata(&log).unwrap().len(), 0);
    drop(expired);

    let reopened =
        DurableSecurityAuditStore::open(temp.path(), 32, 64 * 1024, Duration::from_secs(3600))
            .unwrap();
    let request_id = reopened.allocate_request_id().unwrap();
    reopened
        .append(event(
            &reopened,
            request_id,
            SecurityAuditStage::Authentication,
        ))
        .unwrap();
    drop(reopened);

    let reopened_again =
        DurableSecurityAuditStore::open(temp.path(), 32, 64 * 1024, Duration::from_secs(3600))
            .unwrap();
    assert_eq!(reopened_again.retained_events().len(), 1);
    assert_eq!(
        reopened_again.retained_events()[0].stage,
        SecurityAuditStage::Authentication
    );
}

#[test]
fn compaction_physically_bounds_the_log_and_preserves_the_retained_suffix() {
    let temp = tempfile::tempdir().unwrap();
    let store =
        DurableSecurityAuditStore::open(temp.path(), 2, 16 * 1024, Duration::from_secs(3600))
            .unwrap();
    let request_id = store.allocate_request_id().unwrap();
    for stage in [
        SecurityAuditStage::Attempt,
        SecurityAuditStage::Authentication,
        SecurityAuditStage::Authorization,
    ] {
        store.append(event(&store, request_id, stage)).unwrap();
    }
    let retained = store.retained_events();
    assert_eq!(retained.len(), 2);
    assert_eq!(retained[0].stage, SecurityAuditStage::Authentication);
    assert_eq!(retained[1].stage, SecurityAuditStage::Authorization);
    let log = store.audit_directory().join("security-audit.log");
    assert!(fs::metadata(log).unwrap().len() < 16 * 1024);
    assert!(
        !store
            .audit_directory()
            .join("security-audit.log.compacting")
            .exists()
    );
}

#[test]
fn durable_audit_is_service_owned_and_separate_from_workspace_artifacts() {
    let temp = tempfile::tempdir().unwrap();
    let store = DurableSecurityAuditStore::open_default(temp.path()).unwrap();
    assert_eq!(store.audit_directory(), temp.path().join("audit"));
    assert!(
        !store
            .audit_directory()
            .starts_with(temp.path().join("workspaces"))
    );
    assert!(store.audit_directory().join("store-metadata").is_file());
}

#[test]
fn rejects_invalid_retention_and_a_second_in_process_writer() {
    let temp = tempfile::tempdir().unwrap();
    assert!(matches!(
        DurableSecurityAuditStore::open(temp.path(), 0, 1024, Duration::from_secs(1)),
        Err(SecurityAuditStoreOpenError::InvalidRetention)
    ));
    assert!(matches!(
        DurableSecurityAuditStore::open(temp.path(), 1, 0, Duration::from_secs(1)),
        Err(SecurityAuditStoreOpenError::InvalidRetention)
    ));
    assert!(matches!(
        DurableSecurityAuditStore::open(temp.path(), 1, 1024, Duration::ZERO),
        Err(SecurityAuditStoreOpenError::InvalidRetention)
    ));
    let first = DurableSecurityAuditStore::open_default(temp.path()).unwrap();
    assert!(matches!(
        DurableSecurityAuditStore::open_default(temp.path()),
        Err(SecurityAuditStoreOpenError::AlreadyOpen)
    ));
    drop(first);
    DurableSecurityAuditStore::open_default(temp.path()).unwrap();
}

#[test]
fn counter_ranges_combine_identity_reservation_and_reduce_metadata_churn() {
    let temp = tempfile::tempdir().unwrap();
    let instrumentation = AuditStoreInstrumentation::default();
    let store = DurableSecurityAuditStore::open_with_instrumentation(
        temp.path(),
        256,
        256 * 1024,
        Duration::from_secs(3600),
        instrumentation.clone(),
    )
    .unwrap();
    let baseline = instrumentation.stats().metadata_replacements;
    for _ in 0..64 {
        store.allocate_request_id().unwrap();
    }
    assert_eq!(instrumentation.stats().metadata_replacements - baseline, 1);
    store.allocate_request_id().unwrap();
    assert_eq!(instrumentation.stats().metadata_replacements - baseline, 2);

    let before_events = instrumentation.stats().metadata_replacements;
    for _ in 0..64 {
        store.allocate_event_identity().unwrap();
    }
    assert_eq!(
        instrumentation.stats().metadata_replacements - before_events,
        1
    );
    store.allocate_event_identity().unwrap();
    assert_eq!(
        instrumentation.stats().metadata_replacements - before_events,
        2
    );
    drop(store);

    let reopened = DurableSecurityAuditStore::open_default(temp.path()).unwrap();
    assert!(reopened.allocate_request_id().unwrap() > 65);
    let (event_id, sequence) = reopened.allocate_event_identity().unwrap();
    assert!(event_id > 65);
    assert!(sequence > 65);
}

#[test]
fn representative_seven_stage_request_uses_nine_metadata_replacements() {
    let temp = tempfile::tempdir().unwrap();
    let instrumentation = AuditStoreInstrumentation::default();
    let store = DurableSecurityAuditStore::open_with_instrumentation(
        temp.path(),
        32,
        64 * 1024,
        Duration::from_secs(3600),
        instrumentation.clone(),
    )
    .unwrap();
    let baseline = instrumentation.stats().metadata_replacements;
    let request_id = store.allocate_request_id().unwrap();
    for stage in [
        SecurityAuditStage::Attempt,
        SecurityAuditStage::Authentication,
        SecurityAuditStage::Authorization,
        SecurityAuditStage::Admission,
        SecurityAuditStage::Runtime,
        SecurityAuditStage::Durability,
        SecurityAuditStage::Delivery,
    ] {
        store.append(event(&store, request_id, stage)).unwrap();
    }
    let stats = instrumentation.stats();
    assert_eq!(stats.metadata_replacements - baseline, 9);
    assert_eq!(stats.audit_log_writes, 7);
    assert_eq!(stats.audit_log_syncs, 7);
}

#[test]
fn injected_append_and_metadata_failures_are_not_accepted() {
    for point in [
        AuditStoreFailurePoint::MetadataTempWrite,
        AuditStoreFailurePoint::MetadataTempSync,
        AuditStoreFailurePoint::MetadataRename,
        AuditStoreFailurePoint::MetadataDirectorySync,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let hooks = AuditStoreInstrumentation::default();
        let store = DurableSecurityAuditStore::open_with_instrumentation(
            temp.path(),
            32,
            64 * 1024,
            Duration::from_secs(3600),
            hooks.clone(),
        )
        .unwrap();
        hooks.fail_next(point);
        assert!(store.allocate_request_id().is_err(), "{point:?}");
    }

    for point in [
        AuditStoreFailurePoint::AuditLogWrite,
        AuditStoreFailurePoint::AuditLogSync,
        AuditStoreFailurePoint::FirstAuditDirectorySync,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let hooks = AuditStoreInstrumentation::default();
        let store = DurableSecurityAuditStore::open_with_instrumentation(
            temp.path(),
            32,
            64 * 1024,
            Duration::from_secs(3600),
            hooks.clone(),
        )
        .unwrap();
        let request_id = store.allocate_request_id().unwrap();
        let audit_event = event(&store, request_id, SecurityAuditStage::Attempt);
        hooks.fail_next(point);
        assert!(store.append(audit_event).is_err(), "{point:?}");
    }
}

#[test]
fn failed_range_reservation_is_retried_before_identifiers_are_issued() {
    let temp = tempfile::tempdir().unwrap();
    let hooks = AuditStoreInstrumentation::default();
    let store = DurableSecurityAuditStore::open_with_instrumentation(
        temp.path(),
        32,
        64 * 1024,
        Duration::from_secs(3600),
        hooks.clone(),
    )
    .unwrap();
    hooks.fail_next(AuditStoreFailurePoint::MetadataTempSync);
    assert!(store.allocate_event_identity().is_err());
    let issued = store.allocate_event_identity().unwrap();
    drop(store);
    let reopened = DurableSecurityAuditStore::open_default(temp.path()).unwrap();
    let next = reopened.allocate_event_identity().unwrap();
    assert!(next.0 > issued.0);
    assert!(next.1 > issued.1);
}

#[test]
fn injected_initialization_sync_failures_prevent_open() {
    for point in [
        AuditStoreFailurePoint::ServiceRootSync,
        AuditStoreFailurePoint::InitialMetadataWrite,
        AuditStoreFailurePoint::InitialMetadataSync,
        AuditStoreFailurePoint::MetadataDirectorySync,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join(format!("{point:?}"));
        let hooks = AuditStoreInstrumentation::default();
        hooks.fail_next(point);
        assert!(
            DurableSecurityAuditStore::open_with_instrumentation(
                root,
                32,
                64 * 1024,
                Duration::from_secs(3600),
                hooks,
            )
            .is_err()
        );
    }
}

#[test]
fn retained_bytes_and_maximum_event_size_are_physically_enforced() {
    let temp = tempfile::tempdir().unwrap();
    let store =
        DurableSecurityAuditStore::open(temp.path(), 32, 2 * 1024, Duration::from_secs(3600))
            .unwrap();
    let request_id = store.allocate_request_id().unwrap();
    for stage in [
        SecurityAuditStage::Attempt,
        SecurityAuditStage::Authentication,
        SecurityAuditStage::Authorization,
        SecurityAuditStage::Admission,
    ] {
        store.append(event(&store, request_id, stage)).unwrap();
    }
    assert!(
        fs::metadata(store.audit_directory().join("security-audit.log"))
            .unwrap()
            .len()
            <= 2 * 1024
    );

    let mut oversized = event(&store, request_id, SecurityAuditStage::Runtime);
    oversized.service_identity = "x".repeat(129);
    assert!(matches!(
        store.append(oversized),
        Err(SecurityAuditSinkError::EventTooLarge)
    ));
}

#[test]
fn persisted_representation_excludes_unmodeled_sensitive_content() {
    let temp = tempfile::tempdir().unwrap();
    let store = DurableSecurityAuditStore::open_default(temp.path()).unwrap();
    let request_id = store.allocate_request_id().unwrap();
    store
        .append(event(&store, request_id, SecurityAuditStage::Attempt))
        .unwrap();
    let bytes = fs::read(store.audit_directory().join("security-audit.log")).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    for forbidden in [
        "graph-property-secret",
        "bearer-token-secret",
        "raw-certificate-secret",
        "/private/server/path",
        "policy-document-secret",
        "unbounded-request-body",
    ] {
        assert!(!text.contains(forbidden));
    }
}

#[test]
fn recovery_revalidates_all_bounded_event_fields() {
    let mutations: Vec<PersistedRecordMutation> = vec![
        Box::new(|value| value["event"]["service_identity"] = "x".repeat(129).into()),
        Box::new(|value| {
            let operation = value["event"]["operations"][0].clone();
            value["event"]["operations"] = serde_json::Value::Array(vec![operation; 65]);
        }),
        Box::new(|value| value["event"]["operations"][0]["workspace"] = "w".repeat(129).into()),
        Box::new(|value| value["event"]["schema_version"] = 2.into()),
        Box::new(|value| {
            let operation = value["event"]["operations"][0].clone();
            value["event"]["operations"] = serde_json::Value::Array(vec![operation; 64]);
        }),
    ];
    for mutate in mutations {
        let (temp, _) = persisted_record_with(mutate);
        assert!(matches!(
            DurableSecurityAuditStore::open_default(temp.path()),
            Err(SecurityAuditStoreOpenError::MalformedRecord)
        ));
    }
}

#[test]
fn valid_individual_field_boundaries_reopen_successfully() {
    let temp = tempfile::tempdir().unwrap();
    let store = DurableSecurityAuditStore::open_default(temp.path()).unwrap();
    let request_id = store.allocate_request_id().unwrap();
    let mut boundary = event(&store, request_id, SecurityAuditStage::Attempt);
    let field = "x".repeat(128);
    boundary.service_identity = field.clone();
    boundary.workspace = Some(field.clone());
    boundary.policy_version = Some(field.clone());
    boundary.asserted_actor = Some(SecurityAuditActorAssertion {
        actor_id: field.clone(),
        authenticated: false,
    });
    boundary.authenticated_principal = Some(SecurityAuditPrincipal {
        issuer: field.clone(),
        subject: field.clone(),
        authentication_provider: field.clone(),
        authentication_method: field.clone(),
    });
    boundary.operations[0].workspace = field.clone();
    boundary.operations[0].model = Some(field);
    store.append(boundary).unwrap();
    drop(store);
    assert_eq!(
        DurableSecurityAuditStore::open_default(temp.path())
            .unwrap()
            .retained_events()
            .len(),
        1
    );
}

#[test]
fn generation_conflict_metadata_corruption_and_counter_exhaustion_fail_closed() {
    let (temp, _) = persisted_record_with(|value| {
        value["store_generation_id"] = "ffffffffffffffffffffffffffffffff".into();
    });
    assert!(matches!(
        DurableSecurityAuditStore::open_default(temp.path()),
        Err(SecurityAuditStoreOpenError::InconsistentStore)
    ));

    let temp = tempfile::tempdir().unwrap();
    let store = DurableSecurityAuditStore::open_default(temp.path()).unwrap();
    let metadata = store.audit_directory().join("store-metadata");
    drop(store);
    fs::write(&metadata, b"not-json\n").unwrap();
    assert!(matches!(
        DurableSecurityAuditStore::open_default(temp.path()),
        Err(SecurityAuditStoreOpenError::MalformedMetadata)
    ));

    let temp = tempfile::tempdir().unwrap();
    let store = DurableSecurityAuditStore::open_default(temp.path()).unwrap();
    let metadata = store.audit_directory().join("store-metadata");
    drop(store);
    let mut value: serde_json::Value =
        serde_json::from_slice(fs::read(&metadata).unwrap().trim_ascii()).unwrap();
    value["reserved_event_id"] = u64::MAX.into();
    fs::write(&metadata, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(matches!(
        DurableSecurityAuditStore::open_default(temp.path()),
        Err(SecurityAuditStoreOpenError::CounterExhausted)
    ));
}

#[test]
fn compaction_failures_preserve_a_readable_active_log_and_leftovers_are_ignored() {
    for point in [
        AuditStoreFailurePoint::CompactionWrite,
        AuditStoreFailurePoint::CompactionSync,
        AuditStoreFailurePoint::CompactionRename,
        AuditStoreFailurePoint::CompactionDirectorySync,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let hooks = AuditStoreInstrumentation::default();
        let store = DurableSecurityAuditStore::open_with_instrumentation(
            temp.path(),
            1,
            64 * 1024,
            Duration::from_secs(3600),
            hooks.clone(),
        )
        .unwrap();
        let request_id = store.allocate_request_id().unwrap();
        store
            .append(event(&store, request_id, SecurityAuditStage::Attempt))
            .unwrap();
        hooks.fail_next(point);
        assert!(
            store
                .append(event(
                    &store,
                    request_id,
                    SecurityAuditStage::Authentication,
                ))
                .is_err(),
            "{point:?}"
        );
        drop(store);
        let reopened =
            DurableSecurityAuditStore::open(temp.path(), 1, 64 * 1024, Duration::from_secs(3600))
                .unwrap();
        assert!(!reopened.retained_events().is_empty(), "{point:?}");
    }

    let temp = tempfile::tempdir().unwrap();
    let store = DurableSecurityAuditStore::open_default(temp.path()).unwrap();
    let leftover = store
        .audit_directory()
        .join("security-audit.log.compacting");
    fs::write(&leftover, b"malformed temporary bytes").unwrap();
    drop(store);
    DurableSecurityAuditStore::open_default(temp.path()).unwrap();
    assert!(!leftover.exists());
}

#[test]
fn bounded_probe_recovers_transient_failure_and_repairs_torn_tail() {
    let temp = tempfile::tempdir().unwrap();
    let hooks = AuditStoreInstrumentation::default();
    let store = DurableSecurityAuditStore::open_with_instrumentation(
        temp.path(),
        32,
        64 * 1024,
        Duration::from_secs(3600),
        hooks.clone(),
    )
    .unwrap();
    let request_id = store.allocate_request_id().unwrap();
    store
        .append(event(&store, request_id, SecurityAuditStage::Attempt))
        .unwrap();
    let retained = store.retained_events();
    hooks.fail_next(AuditStoreFailurePoint::AuditLogWrite);
    assert!(
        store
            .append(event(
                &store,
                request_id,
                SecurityAuditStage::Authentication
            ))
            .is_err()
    );
    assert_eq!(store.health(), SecurityAuditSinkHealth::Degraded);
    assert_eq!(
        store.try_recover().unwrap(),
        SecurityAuditSinkHealth::Healthy
    );
    assert_eq!(store.retained_events(), retained);
    let log = store.audit_directory().join("security-audit.log");
    OpenOptions::new()
        .append(true)
        .open(&log)
        .unwrap()
        .write_all(b"{torn")
        .unwrap();
    assert_eq!(
        store.try_recover().unwrap(),
        SecurityAuditSinkHealth::Healthy
    );
    assert!(fs::read(log).unwrap().ends_with(b"\n"));
    let next = store.allocate_event_identity().unwrap();
    assert!(next.0 > 64 && next.1 > 64);
}

#[test]
fn failed_probe_keeps_previous_memory_and_degraded_health() {
    let temp = tempfile::tempdir().unwrap();
    let hooks = AuditStoreInstrumentation::default();
    let store = DurableSecurityAuditStore::open_with_instrumentation(
        temp.path(),
        32,
        64 * 1024,
        Duration::from_secs(3600),
        hooks.clone(),
    )
    .unwrap();
    let request_id = store.allocate_request_id().unwrap();
    store
        .append(event(&store, request_id, SecurityAuditStage::Attempt))
        .unwrap();
    let retained = store.retained_events();
    hooks.fail_next(AuditStoreFailurePoint::AuditLogWrite);
    assert!(
        store
            .append(event(
                &store,
                request_id,
                SecurityAuditStage::Authentication
            ))
            .is_err()
    );
    hooks.fail_next(AuditStoreFailurePoint::RecoveryDirectorySync);
    assert!(store.try_recover().is_err());
    assert_eq!(store.health(), SecurityAuditSinkHealth::Degraded);
    assert_eq!(store.retained_events(), retained);
}

#[test]
fn probe_fails_closed_on_durable_corruption_without_replacing_memory() {
    for corruption in ["metadata", "generation", "record"] {
        let temp = tempfile::tempdir().unwrap();
        let hooks = AuditStoreInstrumentation::default();
        let store = DurableSecurityAuditStore::open_with_instrumentation(
            temp.path(),
            32,
            64 * 1024,
            Duration::from_secs(3600),
            hooks.clone(),
        )
        .unwrap();
        let request_id = store.allocate_request_id().unwrap();
        store
            .append(event(&store, request_id, SecurityAuditStage::Attempt))
            .unwrap();
        let retained = store.retained_events();
        match corruption {
            "metadata" => {
                fs::write(store.audit_directory().join("store-metadata"), b"bad\n").unwrap()
            }
            "generation" => {
                let path = store.audit_directory().join("store-metadata");
                let mut value: serde_json::Value =
                    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                value["store_generation_id"] = "ffffffffffffffffffffffffffffffff".into();
                fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
            }
            "record" => fs::write(
                store.audit_directory().join("security-audit.log"),
                b"{bad}\n",
            )
            .unwrap(),
            _ => unreachable!(),
        }
        hooks.fail_next(AuditStoreFailurePoint::AuditLogWrite);
        let _ = store.append(event(&store, request_id, SecurityAuditStage::Runtime));
        assert!(store.try_recover().is_err(), "{corruption}");
        assert_eq!(store.health(), SecurityAuditSinkHealth::Degraded);
        assert_eq!(store.retained_events(), retained);
    }
}

#[test]
fn clock_rollback_recovery_retains_and_reports_future_records() {
    let (temp, _) = persisted_record_with(|value| {
        value["event"]["timestamp"] =
            serde_json::to_value(SystemTime::now() + Duration::from_secs(60 * 60)).unwrap();
    });
    let store =
        DurableSecurityAuditStore::open(temp.path(), 1, 64 * 1024, Duration::from_secs(1)).unwrap();
    assert_eq!(store.retained_events().len(), 1);
    assert_eq!(store.recovery().future_dated_records, 1);
    assert_eq!(store.future_dated_records(), 1);
}

#[test]
fn future_records_still_obey_count_and_byte_retention() {
    for (max_events, max_bytes) in [(1, 64 * 1024), (32, 1_500)] {
        let temp = tempfile::tempdir().unwrap();
        let store =
            DurableSecurityAuditStore::open(temp.path(), 32, 64 * 1024, Duration::from_secs(3600))
                .unwrap();
        let request_id = store.allocate_request_id().unwrap();
        for stage in [
            SecurityAuditStage::Attempt,
            SecurityAuditStage::Authentication,
            SecurityAuditStage::Authorization,
        ] {
            store.append(event(&store, request_id, stage)).unwrap();
        }
        let log = store.audit_directory().join("security-audit.log");
        drop(store);
        let future = serde_json::to_value(SystemTime::now() + Duration::from_secs(3600)).unwrap();
        let mut output = Vec::new();
        for line in fs::read_to_string(&log).unwrap().lines() {
            let mut value: serde_json::Value = serde_json::from_str(line).unwrap();
            value["event"]["timestamp"] = future.clone();
            output.extend(serde_json::to_vec(&value).unwrap());
            output.push(b'\n');
        }
        fs::write(&log, output).unwrap();
        let reopened = DurableSecurityAuditStore::open(
            temp.path(),
            max_events,
            max_bytes,
            Duration::from_secs(1),
        )
        .unwrap();
        assert!(reopened.retained_events().len() < 3);
        assert!(reopened.future_dated_records() >= reopened.retained_events().len());
        assert!(fs::metadata(&log).unwrap().len() <= max_bytes as u64);
    }
}

#[test]
fn append_rejects_excessive_future_skew_and_pre_epoch_timestamps() {
    let temp = tempfile::tempdir().unwrap();
    let store = DurableSecurityAuditStore::open_default(temp.path()).unwrap();
    let request_id = store.allocate_request_id().unwrap();
    let mut future = event(&store, request_id, SecurityAuditStage::Attempt);
    future.timestamp = SystemTime::now() + Duration::from_secs(60 * 60);
    assert!(store.append(future).is_err());
    let mut before_epoch = event(&store, request_id, SecurityAuditStage::Attempt);
    before_epoch.timestamp = UNIX_EPOCH.checked_sub(Duration::from_secs(1)).unwrap();
    assert!(store.append(before_epoch).is_err());
}

#[test]
fn recovery_syncs_complete_unaccepted_record_and_advances_accepted_high_water() {
    let temp = tempfile::tempdir().unwrap();
    let hooks = AuditStoreInstrumentation::default();
    let store = DurableSecurityAuditStore::open_with_instrumentation(
        temp.path(),
        32,
        64 * 1024,
        Duration::from_secs(3600),
        hooks.clone(),
    )
    .unwrap();
    let request_id = store.allocate_request_id().unwrap();
    let unaccepted = event(&store, request_id, SecurityAuditStage::Attempt);
    let event_id = unaccepted.event_id;
    let sequence = unaccepted.service_sequence;
    hooks.fail_next(AuditStoreFailurePoint::AuditLogSync);
    assert!(store.append(unaccepted).is_err());
    let metadata_path = store.audit_directory().join("store-metadata");
    let before: serde_json::Value =
        serde_json::from_slice(&fs::read(&metadata_path).unwrap()).unwrap();
    assert!(before["accepted_event_id"].as_u64().unwrap() < event_id);
    assert_eq!(
        store.try_recover().unwrap(),
        SecurityAuditSinkHealth::Healthy
    );
    let after: serde_json::Value =
        serde_json::from_slice(&fs::read(&metadata_path).unwrap()).unwrap();
    assert_eq!(after["accepted_event_id"].as_u64(), Some(event_id));
    assert_eq!(after["accepted_request_id"].as_u64(), Some(request_id));
    assert_eq!(after["accepted_service_sequence"].as_u64(), Some(sequence));
    drop(store);
    let reopened = DurableSecurityAuditStore::open_default(temp.path()).unwrap();
    assert_eq!(reopened.retained_events().len(), 1);
    assert_eq!(reopened.retained_events()[0].event_id, event_id);
}

#[test]
fn recovery_log_or_metadata_sync_failure_remains_degraded() {
    for point in [
        AuditStoreFailurePoint::RecoveryLogSync,
        AuditStoreFailurePoint::MetadataTempSync,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let hooks = AuditStoreInstrumentation::default();
        let store = DurableSecurityAuditStore::open_with_instrumentation(
            temp.path(),
            32,
            64 * 1024,
            Duration::from_secs(3600),
            hooks.clone(),
        )
        .unwrap();
        let request_id = store.allocate_request_id().unwrap();
        hooks.fail_next(AuditStoreFailurePoint::AuditLogSync);
        assert!(
            store
                .append(event(&store, request_id, SecurityAuditStage::Attempt))
                .is_err()
        );
        let retained = store.retained_events();
        hooks.fail_next(point);
        assert!(store.try_recover().is_err(), "{point:?}");
        assert_eq!(store.health(), SecurityAuditSinkHealth::Degraded);
        assert_eq!(store.retained_events(), retained);
    }
}
