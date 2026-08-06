use std::io::{Read, Write};
use std::sync::Arc;
use std::thread;

use strukt_session::{
    AuthenticatedListener, EndpointIdentity, EndpointQueue, EndpointTransport, LocalEndpoint,
    PROTOCOL_VERSION, RendezvousRecord, RendezvousStatus, RendezvousStore, ServiceInstanceId,
    ServiceLock, ServiceSecret,
};

#[test]
fn endpoint_identity_is_generated_inside_the_application_namespace() {
    let data = tempfile::tempdir().expect("temporary application data");
    let instance = ServiceInstanceId::new().expect("service instance");

    let identity = EndpointIdentity::for_service(data.path(), instance).expect("endpoint identity");

    assert!(identity.belongs_to(data.path(), instance));
    assert!(!identity.identity().contains("tcp"));
    assert!(!identity.identity().contains("127.0.0.1"));
    assert!(!identity.identity().contains("localhost"));
    #[cfg(unix)]
    {
        assert_eq!(identity.transport(), EndpointTransport::UnixDomainSocket);
        assert!(
            identity
                .native_path()
                .expect("Unix socket path")
                .starts_with(data.path().canonicalize().expect("canonical data root"))
        );
    }
    #[cfg(windows)]
    assert_eq!(identity.transport(), EndpointTransport::WindowsNamedPipe);

    assert!(EndpointIdentity::from_record(data.path(), instance, "outside.sock").is_err());
}

#[test]
fn authenticated_round_trip_survives_a_rejected_client() {
    let data = tempfile::tempdir().expect("temporary application data");
    let instance = ServiceInstanceId::new().expect("service instance");
    let identity = EndpointIdentity::for_service(data.path(), instance).expect("endpoint identity");
    let secret = Arc::new(ServiceSecret::from_bytes([7; 32]));
    let wrong_secret = ServiceSecret::from_bytes([9; 32]);
    let listener =
        AuthenticatedListener::bind(identity.clone(), Arc::clone(&secret)).expect("bind listener");

    let server = thread::spawn(move || {
        assert!(listener.accept().is_err(), "wrong secret must be isolated");
        let mut stream = listener.accept().expect("next authenticated client");
        let mut request = [0_u8; 4];
        stream.read_exact(&mut request).expect("read request");
        assert_eq!(&request, b"ping");
        stream.write_all(b"pong").expect("write response");
    });

    assert!(LocalEndpoint::connect_authenticated(&identity, instance, &wrong_secret).is_err());
    let mut client = LocalEndpoint::connect_authenticated(&identity, instance, &secret)
        .expect("authenticated client");
    client.write_all(b"ping").expect("write request");
    let mut response = [0_u8; 4];
    client.read_exact(&mut response).expect("read response");
    assert_eq!(&response, b"pong");
    server.join().expect("server thread");
}

#[test]
fn endpoint_queue_never_exceeds_byte_or_event_limits() {
    let mut queue = EndpointQueue::new(8, 2);
    queue.push(vec![1, 2, 3, 4]).expect("first event");
    queue.push(vec![5, 6, 7, 8]).expect("second event");
    assert!(queue.push(vec![9]).is_err());
    assert_eq!(queue.len(), 2);
    assert_eq!(queue.queued_bytes(), 8);
    assert_eq!(queue.pop(), Some(vec![1, 2, 3, 4]));
    assert_eq!(queue.queued_bytes(), 4);
}

#[test]
fn service_lock_is_exclusive_and_released_with_its_handle() {
    let data = tempfile::tempdir().expect("temporary application data");
    let first = ServiceLock::acquire(data.path()).expect("first service lock");
    assert!(ServiceLock::acquire(data.path()).is_err());
    drop(first);
    ServiceLock::acquire(data.path()).expect("lock released after owner drop");
}

#[test]
fn rendezvous_is_owner_only_and_validates_generated_endpoint() {
    let data = tempfile::tempdir().expect("temporary application data");
    let instance = ServiceInstanceId::new().expect("service instance");
    let identity = EndpointIdentity::for_service(data.path(), instance).expect("endpoint identity");
    let record =
        RendezvousRecord::new(&identity, instance, "service.secret").expect("rendezvous record");
    let store = RendezvousStore::at(data.path());

    store.publish(&record).expect("publish rendezvous");
    assert_eq!(store.load().expect("load rendezvous"), Some(record));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(store.record_path())
            .expect("rendezvous metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[test]
fn discovery_clears_stale_records_but_keeps_authenticated_owners() {
    let data = tempfile::tempdir().expect("temporary application data");
    let instance = ServiceInstanceId::new().expect("service instance");
    let identity = EndpointIdentity::for_service(data.path(), instance).expect("endpoint identity");
    let record =
        RendezvousRecord::new(&identity, instance, "service.secret").expect("rendezvous record");
    let store = RendezvousStore::at(data.path());
    store.publish(&record).expect("publish rendezvous");

    assert_eq!(
        store.discover(|_| false).expect("stale discovery"),
        RendezvousStatus::StaleRemoved
    );
    assert_eq!(store.load().expect("load after cleanup"), None);

    store.publish(&record).expect("republish rendezvous");
    let lock = ServiceLock::acquire(data.path()).expect("service lock");
    assert_eq!(
        store
            .discover(|candidate| candidate == &record)
            .expect("live discovery"),
        RendezvousStatus::Live(record)
    );
    drop(lock);
}

#[test]
fn cleanup_cannot_remove_another_service_instances_record() {
    let data = tempfile::tempdir().expect("temporary application data");
    let owner = ServiceInstanceId::new().expect("owner instance");
    let other = ServiceInstanceId::new().expect("other instance");
    let identity = EndpointIdentity::for_service(data.path(), owner).expect("endpoint identity");
    let record =
        RendezvousRecord::new(&identity, owner, "service.secret").expect("rendezvous record");
    let store = RendezvousStore::at(data.path());
    store.publish(&record).expect("publish rendezvous");

    assert!(!store.clear_if_owner(other).expect("foreign cleanup"));
    assert_eq!(store.load().expect("record remains"), Some(record.clone()));
    assert!(store.clear_if_owner(owner).expect("owner cleanup"));
    assert_eq!(store.load().expect("record removed"), None);
    assert_eq!(record.protocol_version(), PROTOCOL_VERSION);
}
