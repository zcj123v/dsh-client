use super::*;

#[cfg(test)]
const TICK: Duration = Duration::from_millis(10);
#[cfg(not(test))]
const TICK: Duration = Duration::from_millis(200);

fn notify(child: &Arc<Mutex<Tunnel>>, status: &str, attempt: u8) {
    let callback = child
        .lock()
        .ok()
        .and_then(|t| if t.stopped { None } else { t.callback.clone() });
    if let Some(callback) = callback {
        callback(status, attempt);
    }
}
fn cancelled(child: &Arc<Mutex<Tunnel>>) -> bool {
    child.lock().map(|t| t.stopped).unwrap_or(true)
}
fn delay(child: &Arc<Mutex<Tunnel>>, duration: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < duration {
        if cancelled(child) {
            return false;
        }
        std::thread::sleep(TICK);
    }
    !cancelled(child)
}

pub(super) fn supervise(child: Arc<Mutex<Tunnel>>, port: u16) {
    // Three attempts per open/retry session. Repeated flapping cannot create an unbounded loop.
    let mut attempts = 0;
    loop {
        if !delay(&child, TICK) {
            return;
        }
        let needs_recovery = match child.lock() {
            Ok(mut tunnel) => tunnel.activated && !matches!(tunnel.process.try_wait(), Ok(None)),
            Err(_) => return,
        };
        if !needs_recovery {
            continue;
        }
        let mut recovered = false;
        while attempts < 3 {
            attempts += 1;
            notify(&child, "reconnecting", attempts);
            #[cfg(test)]
            let backoff = Duration::from_millis(20 * (1 << (attempts - 1)));
            #[cfg(not(test))]
            let backoff = Duration::from_secs(1 << (attempts - 1));
            if !delay(&child, backoff) {
                return;
            }
            let started = {
                let Ok(mut tunnel) = child.lock() else { return };
                if tunnel.stopped {
                    return;
                }
                let _ = tunnel.process.kill();
                let _ = tunnel.process.wait();
                // Never accept a service that took our fixed port while SSH was down.
                if let Ok(reservation) = TcpListener::bind(("127.0.0.1", port)) {
                    drop(reservation);
                    match tunnel.command.spawn() {
                        Ok(process) => {
                            tunnel.process = process;
                            true
                        }
                        Err(_) => false,
                    }
                } else {
                    false
                }
            };
            if started && ssh_ready(child.clone(), port).is_ok() {
                if cancelled(&child) {
                    return;
                }
                recovered = true;
                notify(&child, "ready", attempts);
                break;
            }
        }
        if !recovered {
            let Ok(mut tunnel) = child.lock() else { return };
            if tunnel.stopped {
                return;
            }
            let _ = tunnel.process.kill();
            let _ = tunnel.process.wait();
            tunnel.exhausted = true;
            drop(tunnel);
            notify(&child, "failed", attempts);
            return;
        }
    }
}
