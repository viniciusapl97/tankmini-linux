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

    /// Machine-readable output (show, patch, backups, names, library, templates)
    #[arg(long, global = true)]
    json: bool,

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
    /// Print the amp and cab model names stored on the pedal
    Names,
    /// Global bypass: all effects off (`on`) or back on (`off`)
    Bypass { state: String },
    /// Local preset library (~/.local/share/tankmini/library.json)
    Library {
        #[command(subcommand)]
        op: LibraryOp,
    },
    /// Built-in starting sounds ("Sons prontos")
    Templates {
        #[command(subcommand)]
        op: TemplateOp,
    },
    /// Raw protocol access: read/write a memory region
    Raw {
        #[command(subcommand)]
        op: RawOp,
    },
    /// Cabinet (IR) slots stored on the pedal: list, or save one to a file
    Cab {
        #[command(subcommand)]
        op: CabOp,
    },
    /// Live tuner: reads the pedal's USB audio and prints the detected note
    Tune {
        /// Reference pitch for A4 (default 440)
        #[arg(long, default_value_t = 440.0)]
        a4: f32,
        /// PipeWire/Pulse source name; auto-detected (the pedal) when omitted
        #[arg(long)]
        source: Option<String>,
        /// Keep running until Ctrl+C instead of stopping after the first stable note
        #[arg(long = "watch")]
        watch: bool,
    },
}

/// Cabinet slots live in type 5 at `0x70028000 + slot * 0x1000` (4096 bytes each),
/// per `docs/protocol/app-findings.md`. Reading them is safe; writing is not
/// implemented because the AM2 container format is still unknown.
#[derive(Subcommand)]
enum CabOp {
    /// List the 20 cabinet slots with their name on the pedal
    List,
    /// Save one cabinet slot (4096 bytes) to a file
    Read {
        /// Slot 0-19
        slot: usize,
        file: PathBuf,
    },
}

#[derive(Subcommand)]
enum LibraryOp {
    /// List entries (optionally filtered)
    List { query: Option<String> },
    /// Save preset `index` from the pedal into the library
    Add { index: usize, name: String },
    /// Write a library entry to pedal slot `index` and commit it
    Apply { id: String, index: usize },
    /// Export an entry as a 92-byte .tankmini file
    Export { id: String, file: PathBuf },
}

#[derive(Subcommand)]
enum TemplateOp {
    /// List the built-in sounds
    List,
    /// Write a built-in sound to pedal slot `index` and commit it
    Apply { id: String, index: usize },
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

/// Backup → write one preset → verify → commit, like the GUI's Salvar.
fn store_patch(pedal: &mut Pedal, index: usize, bytes: &[u8], note: &str) -> Result<(), String> {
    if index >= tankmini_core::PATCH_COUNT {
        return Err("índice fora da faixa 0-20".into());
    }
    let store = BackupStore::new(BackupStore::default_root());
    if let Some(current) = pedal.read_table().map_err(|e| e.to_string())? {
        let path = store.save(&current.to_preset(), Some(index as u8), note).map_err(|e| e.to_string())?;
        eprintln!("backup: {}", path.display());
    }
    let ack = pedal.write_patch(index as u8, bytes).map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_millis(120));
    let back = pedal.read_patch(index as u8).map_err(|e| e.to_string())?;
    if ack != 0 || back.as_deref() != Some(bytes) {
        return Err(format!("gravação não confirmada (ack 0x{ack:02x})"));
    }
    match pedal.commit().map_err(|e| e.to_string())? {
        Some(0) => Ok(()),
        other => Err(format!("commit recusado: {other:?}")),
    }
}

/// One preset as JSON, with the catalog's names.
fn patch_json(patch: &tankmini_core::Patch<'_>, amps: &[String], cabs: &[String]) -> serde_json::Value {
    use tankmini_core::catalog::{self, Role};
    let modules: Vec<serde_json::Value> = (0..6)
        .map(|m| {
            let role = Role::for_slot(m);
            let unit = patch.unit(m);
            let params: Vec<serde_json::Value> = (0..6)
                .map(|k| {
                    serde_json::json!({
                        "name": catalog::param_name(role, unit, k),
                        "value": patch.param(m, k).unwrap_or(0),
                    })
                })
                .collect();
            serde_json::json!({
                "module": m,
                "role": role.code(),
                "unit": unit,
                "name": patch.effect_name(m, amps, cabs),
                "on": patch.is_on(m),
                "params": params,
            })
        })
        .collect();
    serde_json::json!({
        "index": patch.index,
        "slot": patch.label(),
        "vol": patch.vol(),
        "chain": patch.chain(),
        "instrument": catalog::instrument_for_patch(patch).key(),
        "modules": modules,
        "hex": tankmini_core::library::to_hex(patch.raw()),
    })
}

fn run(cli: Cli) -> Result<(), String> {
    let json = cli.json;
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
        Command::Show if json => {
            let (mut pedal, _) = open_device(cli.dev.clone())?;
            let active = pedal.active_slot().map_err(|e| e.to_string())?;
            let table = pedal.read_table().map_err(|e| e.to_string())?.ok_or("sem tabela")?;
            let amps = pedal.amp_names().unwrap_or_default();
            let cabs = pedal.cab_names().unwrap_or_default();
            let names = tankmini_core::PresetNames::load_default();
            let presets: Vec<serde_json::Value> = table
                .patches()
                .map(|p| {
                    let mut v = patch_json(&p, &amps, &cabs);
                    v["local_name"] = serde_json::json!(names.get(p.index));
                    v
                })
                .collect();
            println!(
                "{}",
                serde_json::json!({ "active": active, "presets": presets })
            );
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
            if json {
                let list = store.list_info().map_err(|e| e.to_string())?;
                let items: Vec<serde_json::Value> = list
                    .iter()
                    .map(|b| {
                        serde_json::json!({
                            "path": b.path, "timestamp": b.timestamp, "slot": b.slot, "note": b.note
                        })
                    })
                    .collect();
                println!("{}", serde_json::Value::Array(items));
                return Ok(());
            }
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
            if json && set.is_none() && !write {
                let amps = pedal.amp_names().unwrap_or_default();
                let cabs = pedal.cab_names().unwrap_or_default();
                println!("{}", patch_json(&patch, &amps, &cabs));
                return Ok(());
            }
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
            let amps = pedal.amp_names().map_err(|e| e.to_string())?;
            let cabs = pedal.cab_names().map_err(|e| e.to_string())?;
            if json {
                println!("{}", serde_json::json!({ "amps": amps, "cabs": cabs }));
                return Ok(());
            }
            println!("    {:<16} cab", "amp");
            for i in 0..amps.len().max(cabs.len()) {
                println!(
                    "{i:02}: {:<16} {}",
                    amps.get(i).map(String::as_str).unwrap_or("-"),
                    cabs.get(i).map(String::as_str).unwrap_or("-")
                );
            }
            Ok(())
        }
        Command::Bypass { state } => {
            let on = match state.as_str() {
                "on" | "1" | "true" => true,
                "off" | "0" | "false" => false,
                _ => return Err("use: tankmini bypass on|off".into()),
            };
            let (mut pedal, _) = open_device(cli.dev.clone())?;
            if !pedal.set_bypass(on).map_err(|e| e.to_string())? {
                return Err("a pedaleira recusou o bypass".into());
            }
            println!("bypass {}", if on { "ligado (sem efeitos)" } else { "desligado" });
            Ok(())
        }
        Command::Library { op } => {
            use tankmini_core::library::Library;
            let path = Library::default_path();
            let mut lib = Library::load(&path)?;
            match op {
                LibraryOp::List { query } => {
                    let found = lib.search(query.as_deref().unwrap_or(""), None);
                    if json {
                        println!("{}", serde_json::to_string(&found).map_err(|e| e.to_string())?);
                    } else if found.is_empty() {
                        println!("biblioteca vazia ({})", path.display());
                    } else {
                        for e in found {
                            println!(
                                "{}  {}{:<28} {:<9} {}",
                                e.id,
                                if e.favorite { "★ " } else { "  " },
                                e.name,
                                e.instrument,
                                e.tags.join(",")
                            );
                        }
                    }
                }
                LibraryOp::Add { index, name } => {
                    let (mut pedal, _) = open_device(cli.dev.clone())?;
                    let bytes = pedal.read_patch(index as u8).map_err(|e| e.to_string())?.ok_or("sem resposta")?;
                    let table = tankmini_core::PresetTable::factory();
                    let mut probe = table.clone();
                    probe.set_patch_raw(0, &bytes);
                    let inst = tankmini_core::catalog::instrument_for_patch(&probe.patch(0).unwrap());
                    let id = lib.add(&name, inst.key(), Vec::new(), &bytes)?;
                    lib.save(&path).map_err(|e| e.to_string())?;
                    println!("{id}");
                }
                LibraryOp::Apply { id, index } => {
                    let entry = lib.get(&id).ok_or("id não encontrado")?;
                    let bytes = entry.patch_bytes().ok_or("entrada corrompida")?;
                    let (mut pedal, _) = open_device(cli.dev.clone())?;
                    store_patch(&mut pedal, index, &bytes, "antes de aplicar da biblioteca")?;
                    let mut names = tankmini_core::PresetNames::load_default();
                    names.set(index, &entry.name);
                    names.save_default().map_err(|e| e.to_string())?;
                    println!("{} → {}", entry.name, tankmini_core::SLOT_LABELS[index]);
                }
                LibraryOp::Export { id, file } => {
                    let entry = lib.get(&id).ok_or("id não encontrado")?;
                    let bytes = entry.patch_bytes().ok_or("entrada corrompida")?;
                    std::fs::write(&file, bytes).map_err(|e| e.to_string())?;
                    println!("{} → {}", entry.name, file.display());
                }
            }
            Ok(())
        }
        Command::Templates { op } => {
            use tankmini_core::templates::{by_id, TEMPLATES};
            match op {
                TemplateOp::List => {
                    if json {
                        let items: Vec<serde_json::Value> = TEMPLATES
                            .iter()
                            .map(|t| serde_json::json!({
                                "id": t.id, "name": t.name, "instrument": t.instrument.key(),
                                "description": t.description, "hex": tankmini_core::library::to_hex(&t.bytes()),
                            }))
                            .collect();
                        println!("{}", serde_json::Value::Array(items));
                    } else {
                        for t in TEMPLATES {
                            println!("{:<13} {:<18} {:<9} {}", t.id, t.name, t.instrument.key(), t.description);
                        }
                    }
                }
                TemplateOp::Apply { id, index } => {
                    let t = by_id(&id).ok_or("som desconhecido (veja `tankmini templates list`)")?;
                    let (mut pedal, _) = open_device(cli.dev.clone())?;
                    store_patch(&mut pedal, index, &t.bytes(), "antes de aplicar som pronto")?;
                    println!("{} → {}", t.name, tankmini_core::SLOT_LABELS[index]);
                }
            }
            Ok(())
        }
        Command::Cab { op } => {
            let (mut pedal, _) = open_device(cli.dev.clone())?;
            const BASE: u32 = 0x7002_8000;
            const SLOT_LEN: usize = 0x1000;
            let names: Vec<String> = pedal
                .read(5, 0x8000_0000, 20 * 14)
                .map_err(|e| e.to_string())?
                .map(|raw| {
                    (0..20)
                        .map(|i| {
                            let chunk = &raw[i * 14..(i + 1) * 14];
                            let end = chunk.iter().position(|&b| b == 0).unwrap_or(14);
                            String::from_utf8_lossy(&chunk[..end]).trim().to_string()
                        })
                        .collect()
                })
                .unwrap_or_default();
            match op {
                CabOp::List => {
                    println!("slot  nome            primeiros bytes");
                    for slot in 0..20usize {
                        let addr = BASE + (slot as u32) * SLOT_LEN as u32;
                        let head = pedal
                            .read(5, addr, 12)
                            .map_err(|e| e.to_string())?
                            .map(|d| d.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "))
                            .unwrap_or_else(|| "error ACK".into());
                        let name = names.get(slot).cloned().unwrap_or_default();
                        println!("{slot:>4}  {name:<15} {head}");
                    }
                }
                CabOp::Read { slot, file } => {
                    if slot >= 20 {
                        return Err("slot deve ser 0-19".into());
                    }
                    let addr = BASE + (slot as u32) * SLOT_LEN as u32;
                    // The pedal answers with `error ACK` for large and for small
                    // requests depending on the state it is in, so walk a ladder of
                    // sizes and keep whatever it agrees to serve. What worked on the
                    // bench is recorded in docs/protocol/app-findings.md.
                    let mut data: Vec<u8> = Vec::with_capacity(SLOT_LEN);
                    let mut chunk_size = 512usize;
                    let mut off = 0usize;
                    while off < SLOT_LEN {
                        let want = chunk_size.min(SLOT_LEN - off);
                        match pedal.read(5, addr + off as u32, want).map_err(|e| e.to_string())? {
                            Some(chunk) if !chunk.is_empty() => {
                                data.extend_from_slice(&chunk);
                                off += chunk.len();
                                std::thread::sleep(std::time::Duration::from_millis(30));
                            }
                            _ => {
                                if chunk_size > 64 {
                                    chunk_size /= 2;
                                    continue;
                                }
                                return Err(format!(
                                    "a pedaleira recusou a leitura em {} bytes (lidos até 0x{off:x}); \
                                     o slot não está servindo leitura agora",
                                    want
                                )
                                .into());
                            }
                        }
                    }
                    std::fs::write(&file, &data).map_err(|e| e.to_string())?;
                    let non_zero = data.iter().filter(|&&b| b != 0).count();
                    println!(
                        "slot {slot} ({}): {} bytes, {non_zero} não-zero -> {}",
                        names.get(slot).cloned().unwrap_or_default(),
                        data.len(),
                        file.display()
                    );
                }
            }
            Ok(())
        }
        Command::Tune { a4, source, watch } => {
            tune(a4, source, watch).map_err(|e| e.to_string())?;
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

/// Name of the pedal's capture source in PipeWire/PulseAudio, if it is there.
fn pedal_source() -> Option<String> {
    let out = std::process::Command::new("pactl")
        .args(["list", "short", "sources"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .find(|l| l.contains("Jieli") && !l.contains(".monitor"))
        .and_then(|l| l.split_whitespace().nth(1))
        .map(str::to_string)
}

/// Capture 44.1 kHz mono f32 from the pedal and print the detected note.
fn tune(a4: f32, source: Option<String>, watch: bool) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Read;

    let rate = 44_100u32;
    let src = source.or_else(pedal_source).ok_or(
        "fonte de áudio da pedaleira não encontrada (o app precisa dela como interface USB; \
         veja tools/tankmini-audio.sh status)",
    )?;

    let mut child = std::process::Command::new("pw-record")
        .args([
            "--raw",
            "--format=f32",
            "--rate=44100",
            "--channels=1",
            "--latency=4096/44100",
            &format!("--target={src}"),
            "-",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    println!("afinador: capturando de {src} (A4 = {a4} Hz) — Ctrl+C para sair");
    let mut stdout = child.stdout.take().expect("piped stdout");
    let window = tankmini_core::pitch::WINDOW;
    let mut bytes = vec![0u8; window * 4];
    let mut stable = 0;
    let mut last = String::new();

    loop {
        let mut filled = 0;
        while filled < bytes.len() {
            match stdout.read(&mut bytes[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(e) => return Err(e.into()),
            }
        }
        if filled < bytes.len() {
            break;
        }
        let samples: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let Some(mut pitch) = tankmini_core::pitch::detect(&samples, rate) else {
            print!("\r   — sem sinal —                                    ");
            use std::io::Write;
            let _ = std::io::stdout().flush();
            stable = 0;
            continue;
        };
        pitch = tankmini_core::pitch::pitch_from_hz(pitch.hz, a4, pitch.rms);
        let string = tankmini_core::pitch::nearest_string(&pitch)
            .map(|s| format!(" (corda {s})"))
            .unwrap_or_default();
        let advice = pitch.advice().unwrap_or("afinado");
        let line = format!(
            "{:>3}{}  {:>7.2} Hz  {:>+6.1} cents  {}{}",
            pitch.note,
            pitch.octave,
            pitch.hz,
            pitch.cents,
            advice,
            string
        );
        print!("\r{line:<60}");
        use std::io::Write;
        let _ = std::io::stdout().flush();
        if line == last {
            stable += 1;
        } else {
            stable = 0;
            last = line.clone();
        }
        if !watch && stable >= 4 {
            println!("\n{line}");
            break;
        }
    }
    let _ = child.kill();
    Ok(())
}
