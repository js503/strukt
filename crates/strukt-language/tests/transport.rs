use std::path::PathBuf;
use std::time::Duration;

use strukt_language::{
    LanguageProcess, LanguageTransport, ProcessExit, SpawnRequest, StdioTransport, TransportError,
};

#[test]
fn transport_contract_is_object_safe_and_sendable() {
    fn accept_transport(_transport: &dyn LanguageTransport) {}
    fn assert_send<T: Send>() {}

    accept_transport(&StdioTransport);
    assert_send::<Box<dyn LanguageProcess>>();
}

#[test]
fn spawn_request_requires_an_absolute_working_directory() {
    let command =
        strukt_language::ResolvedCommand::new(absolute_fixture_path(), Vec::new()).unwrap();
    assert!(SpawnRequest::new(command.clone(), PathBuf::from("relative")).is_err());
    assert!(SpawnRequest::new(command, absolute_fixture_path()).is_ok());
}

#[test]
fn process_exit_and_transport_errors_are_bounded_values() {
    let exit = ProcessExit::new(Some(7), false);
    assert_eq!(exit.code(), Some(7));
    assert!(!exit.success());
    let error = TransportError::protocol("x".repeat(20_000));
    assert!(error.to_string().len() < 5_000);
    let _grace = Duration::from_secs(2);
}

fn absolute_fixture_path() -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(r"C:\Windows\System32\cmd.exe")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/bin/sh")
    }
}
