//! Hardware check for live edit: drive the worker like the GUI does, nudge
//! one parameter of the playing preset through the edit buffer, verify it,
//! then put the original bytes back. Touches only the edit buffer (no table
//! write, no flash commit).
//!
//!     cargo run -p tankmini-core --example live_probe

use std::sync::mpsc;
use std::time::{Duration, Instant};

use tankmini_core::discovery::find_pedal;
use tankmini_core::worker::{Command, Event, Worker};
use tankmini_core::Pedal;

fn main() {
    let (tx, rx) = mpsc::channel();
    let worker = Worker::spawn(move |ev| {
        let _ = tx.send(ev);
    });
    let t0 = Instant::now();
    let (table, active) = loop {
        match rx.recv_timeout(Duration::from_secs(5)).expect("no event from worker") {
            Event::Connected { table, active, firmware, amp_names, bypass, .. } => {
                println!("bypass: {bypass}");
                println!("connected in {:?}: {firmware}, {} amp names", t0.elapsed(), amp_names.len());
                break (table, active.expect("active slot"));
            }
            Event::Disconnected(d) => panic!("disconnected: {d:?}"),
            _ => {}
        }
    };
    let slot = active as usize;
    let original = table.patch(slot).unwrap().raw().to_vec();
    let mut edited = table.clone();
    let mix = edited.patch(slot).unwrap().param(4, 1).unwrap();
    edited.set_param(slot, 4, 1, if mix < 90 { mix + 5 } else { mix - 5 });
    let bytes = edited.patch(slot).unwrap().raw().to_vec();

    // A burst like a knob drag: the worker should coalesce it.
    let t1 = Instant::now();
    for _ in 0..20 {
        worker.send(Command::Preview { slot: active, bytes: bytes.clone() });
    }
    let mut previews = 0;
    while let Ok(ev) = rx.recv_timeout(Duration::from_millis(500)) {
        if let Event::Previewed { ok, .. } = ev {
            assert!(ok, "preview refused");
            previews += 1;
        }
    }
    println!("20 queued previews → {previews} sent, {:?}", t1.elapsed());

    drop(worker);
    let dev = find_pedal().unwrap();
    let mut pedal = Pedal::open(&dev.midi_path).unwrap();
    let buf = pedal.read_edit().unwrap().unwrap();
    assert_eq!(buf, bytes, "edit buffer should hold the edited preset");
    let stored = pedal.read_patch(active).unwrap().unwrap();
    assert_eq!(stored, original, "stored table must be untouched");
    println!("edit buffer matches; stored preset untouched");

    // Restore.
    for w in tankmini_core::worker::edit_writes(&buf, &original) {
        assert_eq!(pedal.write_edit(w.offset, &w.data).unwrap(), 0);
    }
    assert_eq!(pedal.read_edit().unwrap().unwrap(), original);
    println!("restored");
}
