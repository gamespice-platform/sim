/*
 * dot-sim patch test (see README.dot-sim.md): the guards' drops notify only when someone waits, so a thread blocked on
 * the cell must still wake when the borrow it waits on ends, with the patch and with NOTIFY_ALWAYS (upstream behaviour).
 */

use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;

use godot_cell::blocking::GdCell;
use godot_cell::NOTIFY_ALWAYS;

const ROUNDS: usize = 200;
const WAKE_LIMIT: Duration = Duration::from_secs(10);

/// One thread holds a borrow; another blocks on the opposite kind; the holder drops after a short hold. The blocked
/// thread must get its borrow (a lost wakeup would hang it past the limit).
fn blocked_thread_wakes(holder_mut: bool, notify_always: bool) {
    NOTIFY_ALWAYS.store(notify_always, Ordering::Relaxed);
    for round in 0..ROUNDS {
        let cell = GdCell::new(0_i64);
        let (tx, rx) = mpsc::channel();
        std::thread::scope(|scope| {
            let (ready_tx, ready_rx) = mpsc::channel();
            let holder = {
                let cell = &cell;
                scope.spawn(move || {
                    if holder_mut {
                        let mut guard = cell.borrow_mut().unwrap();
                        *guard += 1;
                        ready_tx.send(()).unwrap();
                        std::thread::sleep(Duration::from_micros((round % 7) as u64 * 50));
                    } else {
                        let guard = cell.borrow().unwrap();
                        assert!(*guard >= 0);
                        ready_tx.send(()).unwrap();
                        std::thread::sleep(Duration::from_micros((round % 7) as u64 * 50));
                    }
                })
            };
            ready_rx.recv().unwrap();
            let cell = &cell;
            scope.spawn(move || {
                if holder_mut {
                    let value = *cell.borrow().unwrap();
                    tx.send(value).unwrap();
                } else {
                    let mut guard = cell.borrow_mut().unwrap();
                    *guard += 10;
                    tx.send(*guard).unwrap();
                }
            });
            let got = rx
                .recv_timeout(WAKE_LIMIT)
                .expect("blocked thread never woke: lost wakeup");
            assert_eq!(got, if holder_mut { 1 } else { 10 });
            holder.join().unwrap();
        });
    }
}

// One test, run in sequence, because NOTIFY_ALWAYS is process-wide.
#[test]
#[cfg_attr(
    all(target_family = "wasm", not(target_feature = "atomics")),
    ignore = "Threading not available"
)]
fn blocking_wakeups() {
    for notify_always in [false, true] {
        blocked_thread_wakes(true, notify_always);
        blocked_thread_wakes(false, notify_always);
    }
    NOTIFY_ALWAYS.store(false, Ordering::Relaxed);
}
