//-
// Copyright (c) 2017, 2021, Jason Lingle
//
// This file is part of Ensync.
//
// Ensync is free software: you can  redistribute it and/or modify it under the
// terms of  the GNU General Public  License as published by  the Free Software
// Foundation, either version  3 of the License, or (at  your option) any later
// version.
//
// Ensync is distributed  in the hope that  it will be useful,  but WITHOUT ANY
// WARRANTY; without  even the implied  warranty of MERCHANTABILITY  or FITNESS
// FOR  A PARTICULAR  PURPOSE.  See the  GNU General  Public  License for  more
// details.
//
// You should have received a copy of the GNU General Public License along with
// Ensync. If not, see <http://www.gnu.org/licenses/>.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::{sync::mpsc, thread};

use crate::replica::WatchHandle;
use signal_hook::{
    consts::{SIGINT, SIGTERM},
    flag,
    iterator::Signals,
};

static INTERRUPTED: LazyLock<Arc<AtomicBool>> = LazyLock::new(Default::default);
static WATCH: Mutex<Weak<WatchHandle>> = Mutex::new(Weak::new());

pub fn is_interrupted() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

/// Install once, before entering the client's reconnect loop.
pub fn install_signal_handler() -> io::Result<mpsc::Receiver<()>> {
    for signal in [SIGINT, SIGTERM] {
        // The conditional action must run before setting the flag. The next
        // signal can then terminate even if the worker thread is blocked.
        flag::register_conditional_default(signal, INTERRUPTED.clone())?;
        flag::register(signal, INTERRUPTED.clone())?;
    }
    let mut signals = Signals::new([SIGINT, SIGTERM])?;
    let (stop, interrupted) = mpsc::channel();
    thread::Builder::new().name("interrupt".into()).spawn(move || {
        for _ in signals.forever() {
            let _ = stop.send(());
            let watch = WATCH.lock().unwrap().upgrade();
            let _ = writeln!(io::stderr(), "\nSync interrupted, stopping gracefully.\nPress ^C again to terminate immediately.");
            if let Some(watch) = watch { watch.notify(); }
        }
    })?;
    Ok(interrupted)
}

pub fn notify_on_signal(watch: &Arc<WatchHandle>) {
    *WATCH.lock().unwrap() = Arc::downgrade(watch);
}

pub fn clear_notify() {
    *WATCH.lock().unwrap() = Weak::new();
}
