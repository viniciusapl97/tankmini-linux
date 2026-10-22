//! `tankmini` — command line control for the M-VAVE TANK MINI.

use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};
use tankmini_core::backup::BackupStore;
use tankmini_core::discovery::{find_devices, find_pedal, Device};
use tankmini_core::preset::{Preset, RECORD_COUNT, RECORD_LEN};
use tankmini_core::Pedal;

#[derive(Parser)]
#[command(name = "tankmini", version, about = "Control the M-VAVE TANK MINI over USB")]
struct Cli {
    /// Rawmidi node, e.g. /dev/snd/midiC2D0 (auto-detected by default)
    #[arg(long, global = true)]
    dev: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List USB MIDI devices and their identity
    Devices,
    /// Query firmware identity (cmd 0x11)
    Identify,
    /// Read the active preset block and print a hexdump
    Dump {
        /// Also write the raw 1942-byte block to this file
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Decode the active preset block into its 21 records
    Show,
    /// Select a preset slot (0-20)
    Slot { index: u8 },
    /// Write a preset block from a file, with backup + read-back verification
    Apply {
        file: PathBuf,
        /// Skip the automatic backup (not recommended)
        #[arg(long)]
        no_backup: bool,
    },
    /// Write the newest local backup back to the pedal
    Restore {
        /// Backup file; "latest" picks the newest one
        file: String,
        #[arg(long)]
        no_backup: bool,
    },
    /// List local backups
    Backups,
    /// Print the 32 cab/amp names stored on the pedal
    Names,
    /// Raw protocol access: read/write a memory region
    Raw {
        #[command(subcommand)]
        op: RawOp,
    },
}

#[derive(Subcommand)]
enum RawOp {
    Read { rtype: String, addr: String, len: usize },
    Write { rtype: String, addr: String, hex: String },
}

fn parse_u8_auto(s: &str) -> Result<u8, String> {
    let t = s.trim();
    u8::from_str_radix(t.trim_start_matches("0x"), if t.starts_with("0x") { 16 } else { 10 })
        .map_err(|e| e.to_string())
}

fn parse_u32_auto(s: &str) -> Result<u32, String> {
    let t = s.trim();
    u32::from_str_radix(t.trim_start_matches("0x"), if t.starts_with("0x") { 16 } else { 10 })
        .map_err(|e| e.to_string())
}

fn hex_to_bytes(s: &str) -> Vec<u8> {
    let clean: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    clean
        .as_bytes()
        .chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap_or("00"), 16).unwrap_or(0))
        .collect()
}

fn open_device(explicit: Option<PathBuf>) -> Result<(Pedal, Option<Device>), String> {
    let detected = find_devices();
    let dev = match &explicit {
        Some(p) => detected.into_iter().find(|d| &d.midi_path == p),
        None => find_pedal(),
    };
    let path = match (&dev, &explicit) {
        (Some(d), _) => d.midi_path.clone(),
        (None, Some(p)) => p.clone(),
        (None, None) => {
            return Err(
                "no TANK MINI found: plug the pedal in and check `tankmini devices` / permissions on /dev/snd/midiC*"
                    .into(),
            )
        }
    };
    if let Some(d) = &dev {
        if !d.known {
            eprintln!(
                "warning: {:04x}:{:04x} {} is not the known pedal VID/PID",
                d.vid,
                d.pid,
                d.display_name()
            );
        }
    }
    let pedal = Pedal::open(&path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    Ok((pedal, dev))
}

fn report_identity(pedal: &mut Pedal) -> Result<(), String> {
    match pedal.identify() {
        Ok(Some(info)) => {
            println!("firmware: {}", info.firmware);
            Ok(())
        }
        Ok(None) => Err("no reply to identity query (cmd 0x11)".into()),
        Err(e) => Err(e.to_string()),
    }
}

fn print_preset(preset: &Preset) {
    println!(
        "preset block: {} bytes, {} records of {} bytes, trailer ok: {}",
        preset.raw().len(),
        RECORD_COUNT,
        RECORD_LEN,
        preset.has_trailer()
    );
    for record in preset.records() {
        let params: Vec<String> = record.params().iter().map(|v| format!("{v:>4}")).collect();
        println!(
            "  [{:02}] off=0x{:04x} id=0x{:02x} kind=0x{:02x} flags={} params: {}",
            record.index,
            record.offset(),
            record.id(),
            record.kind(),
            record
                .flags()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<Vec<_>>()
                .join(" "),
            params.join(" ")
        );
    }
}

fn run(cli: Cli) -> Result<(), String> {
    let result = match cli.cmd {
        Command::Devices => {
            let devices = find_devices();
            if devices.is_empty() {
                println!("no USB MIDI devices found under /dev/snd");
            }
            for d in devices {
                println!(
                    "{:04x}:{:04x} {:<28} serial={:<20} card={} node={}{}",
                    d.vid,
                    d.pid,
                    d.display_name(),
                    d.serial.clone().unwrap_or_else(|| "-".into()),
                    d.card,
                    d.midi_path.display(),
                    if d.known { "  <- TANK MINI family" } else { "" }
                );
            }
            Ok(())
        }
        Command::Identify => {
            let (mut pedal, dev) = open_device(cli.dev.clone())?;
            if let Some(d) = &dev {
                println!("device: {:04x}:{:04x} {} ({})", d.vid, d.pid, d.display_name(), d.midi_path.display());
            }
            report_identity(&mut pedal)
        }
        Command::Dump { out } => {
            let (mut pedal, _) = open_device(cli.dev.clone())?;
            let preset = pedal
                .read_preset()
                .map_err(|e| e.to_string())?
                .ok_or("device returned no preset block")?;
            if let Some(path) = out {
                std::fs::write(&path, preset.raw()).map_err(|e| e.to_string())?;
                println!("wrote {} bytes to {}", preset.raw().len(), path.display());
            }
            print!("{}", preset.hexdump());
            Ok(())
        }
        Command::Show => {
            let (mut pedal, _) = open_device(cli.dev.clone())?;
            let slot = pedal.active_slot().map_err(|e| e.to_string())?;
            let preset = pedal
                .read_preset()
                .map_err(|e| e.to_string())?
                .ok_or("device returned no preset block")?;
            println!("active slot: {:?}", slot.map(|s| s + 1));
            print_preset(&preset);
            Ok(())
        }
        Command::Slot { index } => {
            let (mut pedal, _) = open_device(cli.dev.clone())?;
            let ok = pedal.select_slot(index).map_err(|e| e.to_string())?;
            std::thread::sleep(Duration::from_millis(200));
            let now = pedal.active_slot().map_err(|e| e.to_string())?;
            println!("select slot {index}: acked={ok}, active now={now:?}");
            Ok(())
        }
        Command::Apply { file, no_backup } => {
            let preset = BackupStore::load(&file)?;
            let (mut pedal, _) = open_device(cli.dev.clone())?;
            let slot = pedal.active_slot().map_err(|e| e.to_string())?;
            let current = pedal.read_preset().map_err(|e| e.to_string())?.ok_or("no current preset")?;
            if !no_backup {
                let store = BackupStore::new(BackupStore::default_root());
                let path = store.save(&current, slot, "auto before apply").map_err(|e| e.to_string())?;
                println!("backup: {}", path.display());
            }
            let report = pedal.write_preset(&preset).map_err(|e| e.to_string())?;
            println!(
                "write: {}/{} chunks acked, read-back verified: {}",
                report.chunks_ok, report.chunks, report.verified
            );
            if !report.verified {
                return Err(
                    "read-back does not match the file; the pedal may be in an inconsistent state".into(),
                );
            }
            Ok(())
        }
        Command::Restore { file, no_backup } => {
            let path = if file == "latest" {
                BackupStore::new(BackupStore::default_root())
                    .list()
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .next()
                    .ok_or("no backups found")?
            } else {
                PathBuf::from(file)
            };
            let preset = BackupStore::load(&path)?;
            let (mut pedal, _) = open_device(cli.dev.clone())?;
            if !no_backup {
                if let Some(current) = pedal.read_preset().map_err(|e| e.to_string())? {
                    let store = BackupStore::new(BackupStore::default_root());
                    let p = store.save(&current, None, "auto before restore").map_err(|e| e.to_string())?;
                    println!("backup: {}", p.display());
                }
            }
            let report = pedal.write_preset(&preset).map_err(|e| e.to_string())?;
            println!(
                "restored {}: {}/{} chunks acked, verified: {}",
                path.display(),
                report.chunks_ok,
                report.chunks,
                report.verified
            );
            Ok(())
        }
        Command::Backups => {
            let store = BackupStore::new(BackupStore::default_root());
            let list = store.list().map_err(|e| e.to_string())?;
            if list.is_empty() {
                println!("no backups in {}", store.root().display());
            }
            for p in list {
                println!("{}", p.display());
            }
            Ok(())
        }
        Command::Names => {
            let (mut pedal, _) = open_device(cli.dev.clone())?;
            for (i, name) in pedal.cab_names().map_err(|e| e.to_string())?.iter().enumerate() {
                println!("{i:02}: {name}");
            }
            Ok(())
        }
        Command::Raw { op } => {
            let (mut pedal, _) = open_device(cli.dev.clone())?;
            match op {
                RawOp::Read { rtype, addr, len } => {
                    let t = parse_u8_auto(&rtype)?;
                    let a = parse_u32_auto(&addr)?;
                    match pedal.read(t, a, len).map_err(|e| e.to_string())? {
                        Some(data) => println!("{}", tankmini_core::preset::hexdump(&data)),
                        None => println!("error ACK (region not served)"),
                    }
                }
                RawOp::Write { rtype, addr, hex } => {
                    let t = parse_u8_auto(&rtype)?;
                    let a = parse_u32_auto(&addr)?;
                    let data = hex_to_bytes(&hex);
                    let ack = pedal.write(t, a, &data).map_err(|e| e.to_string())?;
                    println!("ack: {ack:?}");
                }
            }
            Ok(())
        }
    };

    result
}

fn main() {
    // Behave like a normal Unix CLI when stdout closes early (`tankmini show | head`).
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}