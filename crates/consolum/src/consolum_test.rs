use super::*;
use host_kernel::ProviderContent;

#[test]
fn manifest_omits_fundet_alias_and_registers_canonical_routes() {
    let mut kernel = Kernel::new();
    register(&mut kernel).expect("register consolum");
    let calls = &kernel.manifest().providers[0].calls;
    assert_eq!(calls.len(), 16);
    assert!(calls.iter().any(|call| call.route == "consolum:funde"));
    assert!(!calls.iter().any(|call| call.route == "consolum:fundet"));
}

#[test]
fn terminal_predicate_returns_one_boolean_item() {
    let provider = Consolum::new().expect("provider");
    let reply = provider
        .dispatch(
            &RequestFrame {
                conversation_id: "audit".into(),
                route: "consolum:audit".into(),
                opener: Valor::Nihil,
                target: None,
            },
            &DispatchContext {
                cancellation: host_kernel::CancellationProbe::new(|| false),
            },
        )
        .expect("audit");
    assert!(matches!(
        reply.contents.as_slice(),
        [ProviderContent::Item(Valor::Bivalens(_))]
    ));
}

#[test]
fn byte_and_string_arguments_decode_from_ordered_openers() {
    assert_eq!(
        bytes_arg(&Valor::Octeti(vec![1, 2]), 0, "data").unwrap(),
        vec![1, 2]
    );
    assert_eq!(
        string_arg(&Valor::Lista(vec![Valor::Textus("ok".into())]), 0, "msg").unwrap(),
        "ok"
    );
    assert!(i64_arg(&Valor::Textus("bad".into()), 0, "n").is_err());
}

#[cfg(unix)]
#[test]
fn fd_wait_honors_cancellation() {
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};

    let (reader, _writer) = UnixStream::pair().expect("socket pair");
    let cancelled = Arc::new(AtomicBool::new(false));
    let trigger = Arc::clone(&cancelled);
    let fd = reader.as_raw_fd();
    let started = Instant::now();
    let waiter = thread::spawn(move || {
        let _reader = reader;
        wait_for_fd(
            fd,
            libc::POLLIN as libc::c_short,
            &DispatchContext {
                cancellation: host_kernel::CancellationProbe::from_flag(cancelled),
            },
            "test:read",
        )
    });
    thread::sleep(Duration::from_millis(25));
    trigger.store(true, Ordering::SeqCst);
    let error = waiter
        .join()
        .expect("waiter thread")
        .expect_err("blocked fd wait must cancel");
    assert_eq!(error.code, "E_CANCELLED");
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[cfg(unix)]
#[test]
fn nonblocking_write_honors_cancellation_and_restores_flags() {
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};

    let (writer, _reader) = UnixStream::pair().expect("socket pair");
    let fd = writer.as_raw_fd();
    let original_flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    assert!(original_flags >= 0, "read original fd flags");
    let cancelled = Arc::new(AtomicBool::new(false));
    let trigger = Arc::clone(&cancelled);
    let timer = thread::spawn(move || {
        thread::sleep(Duration::from_millis(25));
        trigger.store(true, Ordering::SeqCst);
    });
    let started = Instant::now();
    let error = write_fd_cancellable(
        fd,
        &vec![b'x'; 8 * 1024 * 1024],
        &DispatchContext {
            cancellation: host_kernel::CancellationProbe::from_flag(cancelled),
        },
        "test:write",
    )
    .expect_err("blocked fd write must cancel");
    timer.join().expect("cancellation timer");
    assert_eq!(error.code, "E_CANCELLED");
    assert!(started.elapsed() < Duration::from_secs(1));
    let restored_flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    assert_eq!(
        restored_flags & libc::O_NONBLOCK,
        original_flags & libc::O_NONBLOCK
    );
}
