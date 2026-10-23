//! React to the system waking from sleep, and to the network changing.
//!
//! Tunnels through the workspace proxies can die while asleep without either
//! end noticing: the page's side is loopback, so a hung connection only clears
//! on a kernel timeout, and pages that wait on one look frozen. Waking cuts
//! them all so pages reconnect, and starts DNS afresh. It also lifts any hover
//! guard whose exit event was lost across the sleep, which would otherwise
//! leave tabs ignoring clicks.
//!
//! The engine never sees the network change behind the loopback proxy, so it
//! cannot drop the connections the change killed itself. A path monitor does
//! the same cut once the network is usable again with a different path. Off
//! macOS a two-second poll stands in for both: it compares the addresses the
//! machine sends from, and reads a sleep from a tick that ran far too long.

use std::net::{IpAddr, Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use crate::state::AppState;

pub fn install(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    imp::install(app.clone());

    #[cfg(not(target_os = "macos"))]
    poll::spawn(app.clone());
}

fn on_wake(app: &AppHandle) {
    crate::chrome::hover_guard::release_all();
    reconnect(app, "woke from sleep");
}

/// When [`reconnect`] last ran.
static LAST_DISRUPTION: Mutex<Option<Instant>> = Mutex::new(None);

/// Whether a wake or a network change happened within `window`, so a
/// connection quiet since then may be dead rather than idle.
pub fn disrupted_within(window: Duration) -> bool {
    LAST_DISRUPTION
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_some_and(|at| at.elapsed() < window)
}

/// Start DNS afresh and cut every proxy's tunnels, restarting their dials, so
/// pages reconnect over the network as it is now.
fn reconnect(app: &AppHandle, why: &'static str) {
    *LAST_DISRUPTION
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(Instant::now());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let doh = state.browser.lock().await.settings.doh_resolver.clone();
        state
            .resolver
            .set(crate::state::build_resolver(doh.as_deref()));
        let proxies = state.proxies.lock().await;
        for server in proxies.values() {
            server.network_changed();
        }
        log::info!("{why}; cut the tunnels of {} proxies", proxies.len());
    });
}

/// Path updates arrive in bursts as a network comes back; reconnect once the
/// burst has been quiet this long.
const PATH_SETTLE: Duration = Duration::from_secs(1);

static PATH_CHANGES: AtomicU64 = AtomicU64::new(0);

fn reconnect_when_settled(app: &AppHandle) {
    let change = PATH_CHANGES.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(PATH_SETTLE).await;
        if PATH_CHANGES.load(Ordering::SeqCst) == change {
            reconnect(&app, "the network changed");
        }
    });
}

/// The network as connections see it: whether it is usable, the interfaces
/// it uses and where the machine sends from, each sorted and named once.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PathPrint {
    usable: bool,
    interfaces: Vec<String>,
    addresses: Vec<IpAddr>,
}

/// Whether moving from `previous` to `next` should cut the connections: the
/// network is usable again and is not the one they were opened over. The
/// first report is the network at launch, so it never does.
fn reconnects_for(previous: Option<&PathPrint>, next: &PathPrint) -> bool {
    next.usable && previous.is_some_and(|previous| previous != next)
}

/// Where the machine would send from to reach the internet, per address
/// family that has a route. IPv6 keeps just its /64, since privacy addresses
/// rotate within one network and the connections on the old one keep working.
fn sending_addresses() -> Vec<IpAddr> {
    let remotes = [
        SocketAddr::from(([1, 1, 1, 1], 53)),
        SocketAddr::from((
            Ipv6Addr::new(0x2606, 0x4700, 0x4700, 0, 0, 0, 0, 0x1111),
            53,
        )),
    ];
    let mut addresses: Vec<IpAddr> = remotes
        .into_iter()
        .filter_map(|remote| {
            let any = if remote.is_ipv4() {
                SocketAddr::from(([0, 0, 0, 0], 0))
            } else {
                SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0))
            };
            let socket = UdpSocket::bind(any).ok()?;
            socket.connect(remote).ok()?;
            socket
                .local_addr()
                .ok()
                .map(|local| network_part(local.ip()))
        })
        .collect();
    addresses.sort();
    addresses
}

fn network_part(address: IpAddr) -> IpAddr {
    match address {
        IpAddr::V4(_) => address,
        IpAddr::V6(v6) => {
            let mut segments = v6.segments();
            segments[4..].fill(0);
            IpAddr::V6(Ipv6Addr::from(segments))
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::cell::RefCell;
    use std::ffi::{c_void, CStr, CString};
    use std::os::raw::{c_char, c_int};
    use std::rc::Rc;
    use std::sync::{Mutex, OnceLock, PoisonError};

    use block2::{Block, RcBlock};
    use objc2::rc::Id;
    use objc2::runtime::{AnyObject, Bool, NSObject};
    use objc2::{
        class, declare_class, msg_send, msg_send_id, mutability, sel, ClassType, DeclaredClass,
    };
    use tauri::AppHandle;

    static APP: OnceLock<AppHandle> = OnceLock::new();

    declare_class!(
        struct WakeObserver;

        unsafe impl ClassType for WakeObserver {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisWakeObserver";
        }

        impl DeclaredClass for WakeObserver {
            type Ivars = ();
        }

        unsafe impl WakeObserver {
            #[method(didWake:)]
            fn did_wake(&self, _note: *mut AnyObject) {
                if let Some(app) = APP.get() {
                    super::on_wake(app);
                }
            }
        }
    );

    pub(super) fn install(app: AppHandle) {
        if APP.set(app).is_err() {
            return;
        }
        unsafe { register() }
        watch_path(|print| {
            let mut last = LAST_PATH.lock().unwrap_or_else(PoisonError::into_inner);
            let changed = super::reconnects_for(last.as_ref(), &print);
            *last = Some(print);
            if changed {
                if let Some(app) = APP.get() {
                    super::reconnect_when_settled(app);
                }
            }
        });
    }

    static LAST_PATH: Mutex<Option<super::PathPrint>> = Mutex::new(None);

    #[link(name = "Network", kind = "framework")]
    extern "C" {
        fn nw_path_monitor_create() -> *mut c_void;
        fn nw_path_monitor_set_queue(monitor: *mut c_void, queue: *mut c_void);
        fn nw_path_monitor_set_update_handler(
            monitor: *mut c_void,
            handler: &Block<dyn Fn(*mut c_void)>,
        );
        fn nw_path_monitor_start(monitor: *mut c_void);
        #[cfg(test)]
        fn nw_path_monitor_cancel(monitor: *mut c_void);
        fn nw_path_get_status(path: *mut c_void) -> c_int;
        fn nw_path_enumerate_interfaces(
            path: *mut c_void,
            each: &Block<dyn Fn(*mut c_void) -> Bool>,
        );
        fn nw_interface_get_name(interface: *mut c_void) -> *const c_char;
    }

    extern "C" {
        fn dispatch_queue_create(label: *const c_char, attr: *mut c_void) -> *mut c_void;
    }

    /// Start a monitor that hands `on_path` each path the system reports, the
    /// current one first, on a serial queue of its own. Returns the monitor,
    /// which lives as long as the app unless cancelled.
    ///
    /// `on_path` runs on that queue's thread, which the `Send + Sync` bound
    /// keeps sound, since `RcBlock` does not require it.
    pub(super) fn watch_path(
        on_path: impl Fn(super::PathPrint) + Send + Sync + 'static,
    ) -> *mut c_void {
        let handler = RcBlock::new(move |path: *mut c_void| {
            // SAFETY: the path is valid for the length of the callback and is
            // not kept past it.
            on_path(unsafe { path_print(path) });
        });
        // SAFETY: a NULL attribute makes a serial queue; the monitor copies the
        // handler block, so `handler` may drop here. The queue and monitor are
        // never released, living as long as the app.
        unsafe {
            let queue =
                dispatch_queue_create(c"app.wis.network-path".as_ptr(), std::ptr::null_mut());
            let monitor = nw_path_monitor_create();

            nw_path_monitor_set_queue(monitor, queue);
            nw_path_monitor_set_update_handler(monitor, &handler);
            nw_path_monitor_start(monitor);
            monitor
        }
    }

    #[cfg(test)]
    pub(super) unsafe fn stop_watching(monitor: *mut c_void) {
        nw_path_monitor_cancel(monitor);
    }

    unsafe fn path_print(path: *mut c_void) -> super::PathPrint {
        let status = nw_path_get_status(path);
        let names = Rc::new(RefCell::new(Vec::new()));
        let sink = names.clone();
        let each = RcBlock::new(move |interface: *mut c_void| {
            // SAFETY: the interface is valid during the enumeration, and its
            // name is a NUL-terminated string it owns, copied out at once.
            let name = unsafe { nw_interface_get_name(interface) };
            if !name.is_null() {
                let name = unsafe { CStr::from_ptr(name) };
                sink.borrow_mut().push(name.to_string_lossy().into_owned());
            }
            Bool::YES
        });

        nw_path_enumerate_interfaces(path, &each);
        drop(each);

        let mut interfaces = names.take();
        interfaces.sort();
        interfaces.dedup();

        super::PathPrint {
            usable: status == PATH_SATISFIED,
            interfaces,
            addresses: super::sending_addresses(),
        }
    }

    /// `nw_path_status_satisfied`: the path has a usable route.
    const PATH_SATISFIED: c_int = 1;

    pub(super) unsafe fn wake_name() -> *mut AnyObject {
        let name = CString::new("NSWorkspaceDidWakeNotification").unwrap_or_default();
        msg_send![class!(NSString), stringWithUTF8String: name.as_ptr()]
    }

    pub(super) unsafe fn workspace_center() -> *mut AnyObject {
        let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        msg_send![workspace, notificationCenter]
    }

    pub(super) unsafe fn register() {
        let observer: Id<WakeObserver> = msg_send_id![WakeObserver::alloc(), init];
        let _: () = msg_send![
            workspace_center(),
            addObserver: &*observer,
            selector: sel!(didWake:),
            name: wake_name(),
            object: std::ptr::null_mut::<AnyObject>(),
        ];
        // The center does not retain its observers, and this one lives as long
        // as the app.
        std::mem::forget(observer);
    }
}

/// Off macOS: a poll for what the path monitor and the wake notification give
/// macOS.
#[cfg(not(target_os = "macos"))]
mod poll {
    use std::time::{Duration, Instant, SystemTime};

    use tauri::AppHandle;

    const TICK: Duration = Duration::from_secs(2);

    pub(super) fn spawn(app: AppHandle) {
        tauri::async_runtime::spawn(async move {
            let mut last = None;
            loop {
                let (wall, monotonic) = (SystemTime::now(), Instant::now());
                tokio::time::sleep(TICK).await;

                let addresses = super::sending_addresses();
                let print = super::PathPrint {
                    usable: !addresses.is_empty(),
                    interfaces: Vec::new(),
                    addresses,
                };
                let wall = wall.elapsed().unwrap_or_default();
                if super::slept_through(TICK, wall, monotonic.elapsed()) {
                    super::on_wake(&app);
                } else if super::reconnects_for(last.as_ref(), &print) {
                    super::reconnect_when_settled(&app);
                }

                last = Some(print);
            }
        });
    }
}

/// How much longer than asked a tick must run to mean the machine slept.
#[cfg_attr(target_os = "macos", allow(dead_code))]
const SLEPT: Duration = Duration::from_secs(10);

/// Whether a `tick` that took `wall` by the wall clock and `monotonic` by the
/// monotonic one spanned a sleep. Platforms differ on whether the monotonic
/// clock runs while asleep, so either clock running long is enough.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn slept_through(tick: Duration, wall: Duration, monotonic: Duration) -> bool {
    wall > tick + SLEPT || monotonic > tick + SLEPT
}

#[cfg(test)]
mod path_tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use std::time::Duration;

    use super::{network_part, reconnects_for, slept_through, PathPrint};

    fn print(usable: bool, interfaces: &[&str], addresses: &[IpAddr]) -> PathPrint {
        PathPrint {
            usable,
            interfaces: interfaces.iter().map(|n| n.to_string()).collect(),
            addresses: addresses.to_vec(),
        }
    }

    const HOME: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20));
    const CAFE: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7));

    #[test]
    fn the_path_at_launch_never_reconnects() {
        assert!(!reconnects_for(None, &print(true, &["en0"], &[HOME])));
    }

    #[test]
    fn coming_back_or_switching_reconnects_once_usable() {
        let wifi = print(true, &["en0"], &[HOME]);
        let down = print(false, &[], &[]);
        assert!(
            !reconnects_for(Some(&wifi), &down),
            "nothing to reconnect over"
        );
        assert!(reconnects_for(Some(&down), &wifi), "back after a wake");
        assert!(
            reconnects_for(Some(&wifi), &print(true, &["en0", "utun4"], &[HOME])),
            "a VPN"
        );
        assert!(
            reconnects_for(Some(&wifi), &print(true, &["en1"], &[HOME])),
            "another interface"
        );
        assert!(
            reconnects_for(Some(&wifi), &print(true, &["en0"], &[CAFE])),
            "another network on the same interface"
        );
    }

    #[test]
    fn the_same_path_again_keeps_the_connections() {
        let wifi = print(true, &["en0"], &[HOME]);
        assert!(!reconnects_for(Some(&wifi), &wifi.clone()));
    }

    #[test]
    fn a_rotated_ipv6_privacy_address_is_the_same_network() {
        let before: Ipv6Addr = "2001:db8:1:2:aaaa:bbbb:cccc:dddd".parse().unwrap();
        let after: Ipv6Addr = "2001:db8:1:2:1111:2222:3333:4444".parse().unwrap();
        let elsewhere: Ipv6Addr = "2001:db8:9:9:aaaa:bbbb:cccc:dddd".parse().unwrap();
        assert_eq!(network_part(before.into()), network_part(after.into()));
        assert_ne!(network_part(before.into()), network_part(elsewhere.into()));
        assert_eq!(network_part(HOME), HOME);
    }

    #[test]
    fn a_tick_far_longer_than_asked_on_either_clock_is_a_sleep() {
        let tick = Duration::from_secs(2);
        let late = Duration::from_secs(60);
        assert!(!slept_through(tick, tick, tick));
        assert!(!slept_through(tick, tick + Duration::from_secs(1), tick));
        assert!(
            slept_through(tick, late, tick),
            "a monotonic clock that stops asleep"
        );
        assert!(slept_through(tick, tick, late), "one that runs on");
    }

    #[test]
    fn the_sending_addresses_are_read_without_sending() {
        for address in super::sending_addresses() {
            assert!(!address.is_unspecified(), "{address}");
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    /// A wrong message type aborts at runtime only, so drive every send once.
    #[test]
    fn the_observer_registers_and_handles_a_wake() {
        unsafe {
            super::imp::register();
            let center = super::imp::workspace_center();
            let _: () = msg_send![
                center,
                postNotificationName: super::imp::wake_name(),
                object: std::ptr::null_mut::<AnyObject>(),
            ];
        }
    }

    #[test]
    fn the_path_monitor_reports_the_current_path() {
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        let monitor = super::imp::watch_path(move |print| {
            let _ = tx.lock().unwrap().send(print);
        });
        let print = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the monitor reports the current path first");
        unsafe { super::imp::stop_watching(monitor) };
        assert!(print.interfaces.iter().all(|name| !name.is_empty()));
        assert!(
            print.usable || print.addresses.is_empty(),
            "an unusable path has nowhere to send from"
        );
    }
}
