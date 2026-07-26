use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use arboard::Clipboard;
use zeroize::Zeroizing;

use crate::error::{Result, TermKeyError};

#[derive(Default)]
struct ClipboardOwnership {
    generation: u64,
    current: Option<u64>,
}

type ClipboardCoordinator = Arc<Mutex<ClipboardOwnership>>;

fn clipboard_coordinator() -> ClipboardCoordinator {
    static COORDINATOR: OnceLock<ClipboardCoordinator> = OnceLock::new();
    Arc::clone(COORDINATOR.get_or_init(Default::default))
}

pub trait ClipboardBackend: Send + 'static {
    fn get_text(&mut self) -> std::result::Result<String, String>;
    fn set_text(&mut self, value: String) -> std::result::Result<(), String>;
}

struct ArboardBackend(Clipboard);

impl ArboardBackend {
    fn new() -> Result<Self> {
        Clipboard::new()
            .map(Self)
            .map_err(|error| TermKeyError::Clipboard(error.to_string()))
    }
}

impl ClipboardBackend for ArboardBackend {
    fn get_text(&mut self) -> std::result::Result<String, String> {
        self.0.get_text().map_err(|error| error.to_string())
    }

    fn set_text(&mut self, value: String) -> std::result::Result<(), String> {
        self.0.set_text(value).map_err(|error| error.to_string())
    }
}

#[derive(Debug)]
pub struct ClipboardCleanup {
    receiver: mpsc::Receiver<()>,
}

impl ClipboardCleanup {
    /// Wait until the cleanup worker has attempted to clear or preserve the clipboard.
    pub fn wait(self) -> Result<()> {
        self.receiver.recv().map_err(|error| {
            TermKeyError::Clipboard(format!(
                "clipboard cleanup worker stopped before completion: {error}"
            ))
        })
    }
}

/// Copy a secret and later clear it only while this operation remains the current TermKey owner
/// and the clipboard still contains this exact value.
///
/// On Linux/X11, the background worker also keeps the clipboard backend alive so it can continue
/// serving selection requests until the clear timeout.
///
/// TermKey operations in this process are serialized around publication and cleanup so an older
/// timer cannot clear a newer TermKey copy, including an identical secret. This is best-effort
/// relative to external clipboard writers: `arboard` exposes separate read and write operations,
/// so an external application can still replace the clipboard between the OS read and clear.
pub fn copy_and_clear(text: Zeroizing<String>, clear_after: Duration) -> Result<ClipboardCleanup> {
    copy_and_clear_with_factory(ArboardBackend::new, text, clear_after)
}

fn copy_and_clear_with_factory<B, F>(
    factory: F,
    text: Zeroizing<String>,
    clear_after: Duration,
) -> Result<ClipboardCleanup>
where
    B: ClipboardBackend,
    F: FnOnce() -> Result<B>,
{
    copy_and_clear_with_backend(factory()?, text, clear_after)
}

pub fn copy_and_clear_with_backend<B: ClipboardBackend>(
    backend: B,
    text: Zeroizing<String>,
    clear_after: Duration,
) -> Result<ClipboardCleanup> {
    copy_and_clear_with_coordinator(clipboard_coordinator(), backend, text, clear_after)
}

fn copy_and_clear_with_coordinator<B: ClipboardBackend>(
    coordinator: ClipboardCoordinator,
    backend: B,
    text: Zeroizing<String>,
    clear_after: Duration,
) -> Result<ClipboardCleanup> {
    copy_and_clear_with_coordinator_and_wait(coordinator, backend, text, clear_after, thread::sleep)
}

fn copy_and_clear_with_coordinator_and_wait<B, W>(
    coordinator: ClipboardCoordinator,
    mut backend: B,
    text: Zeroizing<String>,
    clear_after: Duration,
    wait_for_timeout: W,
) -> Result<ClipboardCleanup>
where
    B: ClipboardBackend,
    W: FnOnce(Duration) + Send + 'static,
{
    let generation = {
        let mut ownership = coordinator.lock().map_err(|_| {
            TermKeyError::Clipboard("clipboard ownership coordinator is poisoned".to_string())
        })?;
        let generation = ownership.generation.checked_add(1).ok_or_else(|| {
            TermKeyError::Clipboard("clipboard ownership generation exhausted".to_string())
        })?;
        backend
            .set_text(text.as_str().to_owned())
            .map_err(TermKeyError::Clipboard)?;
        ownership.generation = generation;
        ownership.current = Some(generation);
        generation
    };

    let (completion_sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        wait_for_timeout(clear_after);
        if let Ok(mut ownership) = coordinator.lock() {
            if ownership.current == Some(generation) {
                if matches!(backend.get_text(), Ok(current) if current == text.as_str()) {
                    let _ = backend.set_text(String::new());
                }
                ownership.current = None;
            }
        }
        drop(backend);
        drop(text);
        let _ = completion_sender.send(());
    });

    Ok(ClipboardCleanup { receiver })
}

#[cfg(test)]
mod tests {
    use super::{
        copy_and_clear_with_backend, copy_and_clear_with_coordinator_and_wait,
        copy_and_clear_with_factory, ClipboardBackend, ClipboardCoordinator,
    };
    use std::sync::{mpsc, Arc, Barrier, Mutex, MutexGuard, OnceLock};
    use std::thread;
    use std::time::Duration;
    use zeroize::Zeroizing;

    #[derive(Default)]
    struct ClipboardState {
        text: String,
        writes: Vec<String>,
        publications: Vec<&'static str>,
    }

    struct FakeBackend {
        id: &'static str,
        state: Arc<Mutex<ClipboardState>>,
        read_gate: mpsc::Receiver<()>,
        dropped: Option<mpsc::Sender<()>>,
        fail_initial_set: bool,
        fail_read: bool,
    }

    impl ClipboardBackend for FakeBackend {
        fn get_text(&mut self) -> std::result::Result<String, String> {
            self.read_gate.recv().map_err(|error| error.to_string())?;
            if self.fail_read {
                return Err("read failed".to_string());
            }
            Ok(self.state.lock().unwrap().text.clone())
        }

        fn set_text(&mut self, value: String) -> std::result::Result<(), String> {
            if self.fail_initial_set {
                return Err("set failed".to_string());
            }
            let mut state = self.state.lock().unwrap();
            state.text = value.clone();
            if !value.is_empty() {
                state.publications.push(self.id);
            }
            state.writes.push(value);
            Ok(())
        }
    }

    impl Drop for FakeBackend {
        fn drop(&mut self) {
            if let Some(dropped) = self.dropped.take() {
                let _ = dropped.send(());
            }
        }
    }

    struct FakeControl {
        allow_read: mpsc::Sender<()>,
        dropped: mpsc::Receiver<()>,
    }

    struct CleanupControl {
        allow_cleanup: mpsc::Sender<()>,
        backend: FakeControl,
    }

    fn fake_backend(
        id: &'static str,
        state: Arc<Mutex<ClipboardState>>,
        fail_initial_set: bool,
        fail_read: bool,
    ) -> (FakeBackend, FakeControl) {
        let (allow_read, read_gate) = mpsc::channel();
        let (dropped, backend_dropped) = mpsc::channel();
        (
            FakeBackend {
                id,
                state,
                read_gate,
                dropped: Some(dropped),
                fail_initial_set,
                fail_read,
            },
            FakeControl {
                allow_read,
                dropped: backend_dropped,
            },
        )
    }

    fn test_guard() -> MutexGuard<'static, ()> {
        static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn test_coordinator() -> ClipboardCoordinator {
        Arc::new(Mutex::new(Default::default()))
    }

    fn copy_with_gated_cleanup(
        coordinator: ClipboardCoordinator,
        backend: FakeBackend,
        control: FakeControl,
        text: &str,
    ) -> crate::error::Result<CleanupControl> {
        let (allow_cleanup, cleanup_gate) = mpsc::channel();
        copy_and_clear_with_coordinator_and_wait(
            coordinator,
            backend,
            Zeroizing::new(text.to_string()),
            Duration::ZERO,
            move |_| {
                let _ = cleanup_gate.recv();
            },
        )?;
        Ok(CleanupControl {
            allow_cleanup,
            backend: control,
        })
    }

    fn wait_for_cleanup(control: FakeControl) {
        // A stale generation exits before reading, which drops this gate.
        let _ = control.allow_read.send(());
        control
            .dropped
            .recv_timeout(Duration::from_secs(2))
            .expect("cleanup worker did not finish within two seconds");
    }

    fn release_cleanup(control: CleanupControl) {
        control
            .allow_cleanup
            .send(())
            .expect("cleanup worker exited before its timeout gate was released");
        wait_for_cleanup(control.backend);
    }

    #[test]
    fn does_not_clear_replaced_clipboard_text() {
        let _guard = test_guard();
        let state = Arc::new(Mutex::new(ClipboardState::default()));
        let (backend, control) = fake_backend("operation", Arc::clone(&state), false, false);

        copy_and_clear_with_backend(
            backend,
            Zeroizing::new("termkey secret".to_string()),
            Duration::ZERO,
        )
        .unwrap();
        state.lock().unwrap().text = "replacement".to_string();

        wait_for_cleanup(control);

        let state = state.lock().unwrap();
        assert_eq!(state.text, "replacement");
        assert_eq!(state.writes, ["termkey secret"]);
    }

    #[test]
    fn older_timer_does_not_clear_newer_termkey_copy() {
        let _guard = test_guard();
        let coordinator = test_coordinator();
        let state = Arc::new(Mutex::new(ClipboardState::default()));
        let (older_backend, older_control) =
            fake_backend("older", Arc::clone(&state), false, false);
        let (newer_backend, newer_control) =
            fake_backend("newer", Arc::clone(&state), false, false);

        let older_control = copy_with_gated_cleanup(
            Arc::clone(&coordinator),
            older_backend,
            older_control,
            "older secret",
        )
        .unwrap();
        let newer_control =
            copy_with_gated_cleanup(coordinator, newer_backend, newer_control, "newer secret")
                .unwrap();

        release_cleanup(older_control);
        assert_eq!(state.lock().unwrap().text, "newer secret");

        release_cleanup(newer_control);
        let state = state.lock().unwrap();
        assert_eq!(state.text, "");
        assert_eq!(state.writes, ["older secret", "newer secret", ""]);
    }

    #[test]
    fn concurrent_older_timer_does_not_clear_newer_same_secret_copy() {
        let _guard = test_guard();
        let coordinator = test_coordinator();
        let state = Arc::new(Mutex::new(ClipboardState::default()));
        let start = Arc::new(Barrier::new(3));
        let (one_backend, one_control) = fake_backend("one", Arc::clone(&state), false, false);
        let (two_backend, two_control) = fake_backend("two", Arc::clone(&state), false, false);

        let one_start = Arc::clone(&start);
        let one_coordinator = Arc::clone(&coordinator);
        let one = thread::spawn(move || {
            one_start.wait();
            copy_with_gated_cleanup(one_coordinator, one_backend, one_control, "same secret")
        });
        let two_start = Arc::clone(&start);
        let two = thread::spawn(move || {
            two_start.wait();
            copy_with_gated_cleanup(coordinator, two_backend, two_control, "same secret")
        });

        start.wait();
        let one_control = one.join().unwrap().unwrap();
        let two_control = two.join().unwrap().unwrap();

        let publication_order = state.lock().unwrap().publications.clone();
        let (older_control, newer_control) = match publication_order.as_slice() {
            ["one", "two"] => (one_control, two_control),
            ["two", "one"] => (two_control, one_control),
            other => panic!("unexpected publication order: {other:?}"),
        };

        release_cleanup(older_control);
        {
            let state = state.lock().unwrap();
            assert_eq!(state.text, "same secret");
            assert_eq!(state.writes, ["same secret", "same secret"]);
        }

        release_cleanup(newer_control);
        let state = state.lock().unwrap();
        assert_eq!(state.text, "");
        assert_eq!(state.writes, ["same secret", "same secret", ""]);
    }

    #[test]
    fn initial_backend_set_failure_is_returned() {
        let _guard = test_guard();
        let state = Arc::new(Mutex::new(ClipboardState::default()));
        let (backend, _control) = fake_backend("failed", state, true, false);

        let error = copy_and_clear_with_backend(
            backend,
            Zeroizing::new("secret".to_string()),
            Duration::ZERO,
        )
        .unwrap_err();

        assert!(error.to_string().contains("set failed"));
    }

    #[test]
    fn initial_backend_construction_failure_is_returned() {
        let _guard = test_guard();
        let error = copy_and_clear_with_factory(
            || -> crate::error::Result<FakeBackend> {
                Err(crate::error::TermKeyError::Clipboard(
                    "construction failed".to_string(),
                ))
            },
            Zeroizing::new("secret".to_string()),
            Duration::ZERO,
        )
        .unwrap_err();

        assert!(error.to_string().contains("construction failed"));
    }

    #[test]
    fn clipboard_read_failure_at_timeout_never_triggers_a_clear() {
        let _guard = test_guard();
        let state = Arc::new(Mutex::new(ClipboardState::default()));
        let (backend, control) = fake_backend("operation", Arc::clone(&state), false, true);

        let cleanup = copy_and_clear_with_backend(
            backend,
            Zeroizing::new("termkey secret".to_string()),
            Duration::ZERO,
        )
        .unwrap();
        let waiter = thread::spawn(move || cleanup.wait());
        control.allow_read.send(()).unwrap();
        assert!(
            waiter.join().unwrap().is_ok(),
            "cleanup completion should be signalled after the backend read attempt"
        );
        control
            .dropped
            .recv_timeout(Duration::from_secs(2))
            .expect("cleanup worker did not finish within two seconds");

        let state = state.lock().unwrap();
        assert_eq!(state.text, "termkey secret");
        assert_eq!(state.writes, ["termkey secret"]);
    }

    #[test]
    fn cleanup_wait_blocks_until_delayed_worker_finishes() {
        let _guard = test_guard();
        let coordinator = test_coordinator();
        let state = Arc::new(Mutex::new(ClipboardState::default()));
        let (backend, control) = fake_backend("operation", Arc::clone(&state), false, false);
        let (allow_timeout, timeout_gate) = mpsc::channel();

        let cleanup = copy_and_clear_with_coordinator_and_wait(
            coordinator,
            backend,
            Zeroizing::new("secret".to_string()),
            Duration::ZERO,
            move |_| {
                timeout_gate.recv().unwrap();
            },
        )
        .unwrap();

        assert_eq!(
            cleanup.receiver.try_recv(),
            Err(mpsc::TryRecvError::Empty),
            "completion was signalled before the delayed worker was released"
        );
        allow_timeout.send(()).unwrap();
        control.allow_read.send(()).unwrap();
        assert!(cleanup.wait().is_ok());
        control
            .dropped
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        assert_eq!(state.lock().unwrap().text, "");
    }

    #[test]
    fn zero_timeout_waits_for_actual_cleanup_attempt() {
        let _guard = test_guard();
        let state = Arc::new(Mutex::new(ClipboardState::default()));
        let (backend, control) = fake_backend("operation", Arc::clone(&state), false, false);

        let cleanup = copy_and_clear_with_backend(
            backend,
            Zeroizing::new("secret".to_string()),
            Duration::ZERO,
        )
        .unwrap();
        assert_eq!(
            cleanup.receiver.try_recv(),
            Err(mpsc::TryRecvError::Empty),
            "completion was signalled before the backend cleanup attempt"
        );
        control.allow_read.send(()).unwrap();
        assert!(cleanup.wait().is_ok());
        assert_eq!(state.lock().unwrap().text, "");
    }

    #[test]
    fn failed_newer_write_does_not_supersede_prior_owner() {
        let _guard = test_guard();
        let coordinator = test_coordinator();
        let state = Arc::new(Mutex::new(ClipboardState::default()));
        let (prior_backend, prior_control) =
            fake_backend("prior", Arc::clone(&state), false, false);
        let (failed_backend, _failed_control) =
            fake_backend("failed", Arc::clone(&state), true, false);

        let prior_control = copy_with_gated_cleanup(
            Arc::clone(&coordinator),
            prior_backend,
            prior_control,
            "prior secret",
        )
        .unwrap();
        let (_allow_failed_cleanup, failed_cleanup_gate) = mpsc::channel::<()>();
        let error = copy_and_clear_with_coordinator_and_wait(
            coordinator,
            failed_backend,
            Zeroizing::new("newer secret".to_string()),
            Duration::ZERO,
            move |_| {
                let _ = failed_cleanup_gate.recv();
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("set failed"));

        release_cleanup(prior_control);
        let state = state.lock().unwrap();
        assert_eq!(state.text, "");
        assert_eq!(state.writes, ["prior secret", ""]);
        assert_eq!(state.publications, ["prior"]);
    }
}
