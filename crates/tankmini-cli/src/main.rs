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
    /// Inspect (and optionally edit) one preset of the table
    Patch {
        /// Preset index 0-20 (1A..7C is index 0..20)
        index: usize,
        /// Edit in the form module:param=value (module 1-6, param 1-6)
        #[arg(long = "set")]
        set: Option<String>,
        /// Write the preset back to the pedal after editing
        #[arg(long)]
        write: bool,
    },
    /// Commit the working preset table to the pedal's flash
    Save,
    /// Set or show the local name of a preset (names are stored on the computer)
    Name {
        index: usize,
        /// New name; omit to clear it
        name: Option<String>,
    },
    /// Export one preset (92 bytes) or the whole table to a file
    Export {
        /// Preset index 0-20, or "all" for the whole table
        index: String,
        file: PathBuf,
    },
    /// Import a preset file (92 bytes) into a slot, or a table (1942 bytes)
    Import { index: usize, file: PathBuf },
    /// Copy a preset to another slot (in RAM; use `save` to persist)
    Copy { from: usize, to: usize },
    /// Restore the factory preset table and commit it to flash
    Factory,
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
        Command::Patch { index, set, write } => {
            let (mut pedal, _) = open_device(cli.dev.clone())?;
            let table = pedal
                .read_table()
                .map_err(|e| e.to_string())?
                .ok_or("sem tabela de presets")?;
            let mut table = table;
            let patch = table.patch(index).ok_or("índice fora da faixa 0-20")?;
            println!(
                "preset {} (index {}) id=0x{:02x} cadeia={:?} flags={:?}",
                patch.label(),
                index,
                patch.id(),
                patch.chain(),
                patch.module_flags()
            );
            for module in 0..6 {
                let params: Vec<String> = (0..6)
                    .map(|i| format!("{:>3}", patch.param(module, i).unwrap_or(0)))
                    .collect();
                println!("  módulo {}: {}", module + 1, params.join(" "));
            }
            if let Some(spec) = set {
                let (target, value) = spec.split_once('=').ok_or("use módulo:param=valor")?;
                let (module, param) = target.split_once(':').ok_or("use módulo:param=valor")?;
                let module: usize = module.parse().map_err(|_| "módulo inválido")?;
                let param: usize = param.parse().map_err(|_| "param inválido")?;
                let value: u16 = value.parse().map_err(|_| "valor inválido")?;
                if module < 1 || module > 6 || param < 1 || param > 6 || value > 100 {
                    return Err("faixa: módulo 1-6, param 1-6, valor 0-100".into());
                }
                table.set_param(index, module - 1, param - 1, value);
                println!("editado: módulo {module} param {param} = {value}");
            }
            if write {
                let bytes = table.patch(index).unwrap().raw().to_vec();
                let store = BackupStore::new(BackupStore::default_root());
                let current = pedal.read_table().map_err(|e| e.to_string())?;
                if let Some(current) = current {
                    let path = store
                        .save(&current.to_preset(), Some(index as u8), "antes de gravar preset")
                        .map_err(|e| e.to_string())?;
                    println!("backup: {}", path.display());
                }
                let ack = pedal.write_patch(index as u8, &bytes).map_err(|e| e.to_string())?;
                std::thread::sleep(Duration::from_millis(150));
                let back = pedal
                    .read_patch(index as u8)
                    .map_err(|e| e.to_string())?
                    .unwrap_or_default();
                println!(
                    "gravado: ack=0x{ack:02x} verificado={}",
                    back == bytes
                );
            }
            Ok(())
        }
        Command::Save => {
            let (mut pedal, _) = open_device(cli.dev.clone())?;
            match pedal.commit().map_err(|e| e.to_string())? {
                Some(0) => println!("tabela de presets gravada na flash (ack 0x00)"),
                Some(other) => println!("flash recusou a gravação (ack 0x{other:02x})"),
                None => println!("sem resposta ao comando de gravação"),
            }
            Ok(())
        }
        Command::Name { index, name } => {
            let mut names = tankmini_core::PresetNames::load_default();
            match name {
                Some(n) => {
                    names.set(index, &n);
                    names.save_default().map_err(|e| e.to_string())?;
                    println!("{} = {}", names.label(index), n);
                }
                None => println!("{}", names.label(index)),
            }
            Ok(())
        }
        Command::Export { index, file } => {
            let (mut pedal, _) = open_device(cli.dev.clone())?;
            let table = pedal.read_table().map_err(|e| e.to_string())?.ok_or("sem tabela")?;
            let bytes = if index == "all" {
                table.raw().to_vec()
            } else {
                let i: usize = index.parse().map_err(|_| "índice inválido")?;
                table.patch(i).ok_or("índice fora da faixa")?.raw().to_vec()
            };
            std::fs::write(&file, &bytes).map_err(|e| e.to_string())?;
            println!("{} bytes -> {}", bytes.len(), file.display());
            Ok(())
        }
        Command::Import { index, file } => {
            let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
            let (mut pedal, _) = open_device(cli.dev.clone())?;
            let mut table = pedal.read_table().map_err(|e| e.to_string())?.ok_or("sem tabela")?;
            table.import(index, &bytes)?;
            let store = BackupStore::new(BackupStore::default_root());
            let path = store
                .save(&pedal.read_table().map_err(|e| e.to_string())?.ok_or("sem tabela")?.to_preset(), None, "antes de importar")
                .map_err(|e| e.to_string())?;
            println!("backup: {}", path.display());
            let report = pedal.write_table(&table).map_err(|e| e.to_string())?;
            let commit = pedal.commit().map_err(|e| e.to_string())?.unwrap_or(0xff);
            println!(
                "importado: {} bytes, verificado={}, commit=0x{commit:02x}",
                bytes.len(),
                report.verified
            );
            Ok(())
        }
        Command::Copy { from, to } => {
            let (mut pedal, _) = open_device(cli.dev.clone())?;
            let mut table = pedal.read_table().map_err(|e| e.to_string())?.ok_or("sem tabela")?;
            if !table.copy_patch(from, to) {
                return Err("índices inválidos (0-20, origem ≠ destino)".into());
            }
            let report = pedal.write_table(&table).map_err(|e| e.to_string())?;
            let commit = pedal.commit().map_err(|e| e.to_string())?.unwrap_or(0xff);
            println!(
                "copiado {} -> {}: verificado={}, commit=0x{commit:02x}",
                from, to, report.verified
            );
            Ok(())
        }
        Command::Factory => {
            let (mut pedal, _) = open_device(cli.dev.clone())?;
            let factory = tankmini_core::PresetTable::factory();
            let store = BackupStore::new(BackupStore::default_root());
            if let Some(current) = pedal.read_table().map_err(|e| e.to_string())? {
                let path = store
                    .save(&current.to_preset(), None, "antes de restaurar de fábrica")
                    .map_err(|e| e.to_string())?;
                println!("backup: {}", path.display());
            }
            let report = pedal.write_table(&factory).map_err(|e| e.to_string())?;
            let commit = pedal.commit().map_err(|e| e.to_string())?.unwrap_or(0xff);
            println!(
                "fábrica: verificado={}, commit=0x{commit:02x}, igual à referência={}",
                report.verified,
                factory.is_factory()
            );
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