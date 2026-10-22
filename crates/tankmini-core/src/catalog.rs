//! Human-facing catalogue: module roles, effect variants and parameter names.
//!
//! Sources, in order of strength:
//!
//! - **verified**: the 6 module slots, their on/off byte (`0x08 + slot`), their
//!   unit/type byte (`0x0e + slot`) and the 6 parameter words per slot
//!   (`0x14 + slot*12 + param*2`) come straight from the app's accessors
//!   (`TankMiniData.getEffOnAt / getEffUnitAt / getEffParaAt`, recovered with
//!   Blutter) and were confirmed against the device dump.
//! - **from app**: the variant lists and their parameter names are the lists the
//!   official app builds (`compact_effect_params.dart`).
//! - **inferred**: the binding of each slot index to a role (amp, cab, mod…),
//!   derived from the value ranges observed in the 21 factory presets matching
//!   the counts in the manual (20 preamps, 7 modulations, 5 delays, 6 reverbs,
//!   20 IR cab slots).

/// What a module slot does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Gate / boost / compressor (unit ids 0..2).
    Fx,
    /// Preamp model, 20 models (unit id = index into the amp/cab name table).
    Amp,
    /// Modulation, 7 variants.
    Modulation,
    /// Delay, 5 variants.
    Delay,
    /// Reverb, 6 variants.
    Reverb,
    /// IR cabinet, 20 slots (unit id = index into the cab name table).
    Cabinet,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::Fx => "Dinâmica",
            Role::Amp => "Amp",
            Role::Modulation => "Modulação",
            Role::Delay => "Delay",
            Role::Reverb => "Reverb",
            Role::Cabinet => "Cabinet",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Role::Fx => "Portão de ruído, boost ou compressor",
            Role::Amp => "Modelo de pré-amplificador",
            Role::Modulation => "Chorus, flanger, phaser, tremolo…",
            Role::Delay => "Eco com diferentes caracteres",
            Role::Reverb => "Ambiente: sala, hall, spring…",
            Role::Cabinet => "Caixa simulada (IR)",
        }
    }

    /// Longer, plain-language explanation for people who never used a pedal.
    pub fn description(self) -> &'static str {
        match self {
            Role::Fx => "Fica no começo do caminho do som. O Noise Gate corta o chiado quando você não está tocando, o Boost dá um empurrão e o Compress iguala as notas fortes e fracas.",
            Role::Amp => "É o coração do timbre: imita o pré de um amplificador. Pouco Gain soa limpo; muito Gain distorce.",
            Role::Modulation => "Faz o som ondular ou \"nadar\": Chorus engorda, Phaser e Flanger giram, Tremolo pulsa o volume.",
            Role::Delay => "Repete o que você toca, como um eco. Time é a distância entre as repetições, Fb quantas vezes repete.",
            Role::Reverb => "Coloca o som dentro de um ambiente: sala, salão, mola de amplificador antigo ou nuvens longas.",
            Role::Cabinet => "Simula a caixa e o alto-falante do amplificador (IR). Sem ela a distorção soa áspera no fone.",
        }
    }

    /// Short code used in compact chain strips.
    pub fn code(self) -> &'static str {
        match self {
            Role::Fx => "DYN",
            Role::Amp => "AMP",
            Role::Modulation => "MOD",
            Role::Delay => "DLY",
            Role::Reverb => "RVB",
            Role::Cabinet => "CAB",
        }
    }

    /// Role of a slot index on this pedal (inferred, see module docs).
    pub fn for_slot(slot: usize) -> Role {
        match slot {
            0 => Role::Fx,
            1 => Role::Amp,
            2 => Role::Modulation,
            3 => Role::Delay,
            4 => Role::Reverb,
            _ => Role::Cabinet,
        }
    }
}

/// One selectable variant of a module, with its parameter names in wire order.
pub struct Variant {
    pub name: &'static str,
    pub params: &'static [&'static str],
}

// All lists below are **from app** (`compact_effect_params.dart`,
// `BlackboxData._initIrAmpDs`), see `docs/protocol/app-findings.md`.

const FX_VARIANTS: &[Variant] = &[
    Variant { name: "Noise Gate", params: &["Gate"] },
    Variant { name: "Boost", params: &["Gate", "Gain"] },
    Variant { name: "Compress", params: &["Gate", "Sustain", "Attack", "Level"] },
];

const MOD_VARIANTS: &[Variant] = &[
    Variant { name: "Chorus", params: &["Speed", "Depth", "Mix"] },
    Variant { name: "Phaser", params: &["Speed", "ParamA", "ParamB", "Fb"] },
    Variant { name: "Tremolo", params: &["Speed", "Depth", "Level"] },
    Variant { name: "Flanger", params: &["Speed", "Depth", "Fb", "Mix"] },
    Variant { name: "Vibrato", params: &["Speed", "Depth"] },
    Variant { name: "Univibe", params: &["Speed", "Depth", "Mix"] },
    Variant { name: "Autofilter", params: &["Speed", "Min", "Max", "Mix", "Fb"] },
];

const DLY_VARIANTS: &[Variant] = &[
    Variant { name: "Analog", params: &["Time", "Fb", "Mix", "Phaser", "Pitch"] },
    Variant { name: "Duck", params: &["Time", "Fb", "Mix", "Unpack", "Speed", "Depth"] },
    Variant { name: "dTape", params: &["Time", "Fb", "Mix", "Grit", "Speed", "Depth"] },
    Variant { name: "Dual", params: &["Time", "Fb", "Mix", "T-Mode", "Speed", "Depth"] },
    Variant { name: "Lofi", params: &["Time", "Fb", "Mix", "Grit", "Speed", "Depth"] },
];

const RVB_VARIANTS: &[Variant] = &[
    Variant { name: "Room", params: &["Decay", "Mix", "HPass", "LPass", "Depth"] },
    Variant { name: "Hall", params: &["Decay", "Mix", "HPass", "LPass", "Depth"] },
    Variant { name: "Swell", params: &["Decay", "Mix", "HPass", "LPass", "RiseT"] },
    Variant { name: "Spring", params: &["Decay", "Mix", "HPass", "LPass", "Combs"] },
    Variant { name: "Shimmer", params: &["Decay", "Mix", "Tone", "Pitch", "Amt"] },
    Variant { name: "Cloud", params: &["Decay", "Mix", "HPass", "LPass", "Diff"] },
];

const AMP_PARAMS: &[&str] = &["Gain", "Level", "Bass", "Mid", "Treble"];
const CAB_PARAMS: &[&str] = &["Level", "Low Cut", "High Cut"];

/// Variants available for a role. Amp and cab index the pedal's name table, so
/// their names come from the device, not from here.
pub fn variants(role: Role) -> &'static [Variant] {
    match role {
        Role::Fx => FX_VARIANTS,
        Role::Modulation => MOD_VARIANTS,
        Role::Delay => DLY_VARIANTS,
        Role::Reverb => RVB_VARIANTS,
        Role::Amp | Role::Cabinet => &[],
    }
}

/// Name of parameter `index` of a module variant.
///
/// `unit` is the module's type byte; `cab_name` lets amp/cab slots show the real
/// model name from the device table.
pub fn param_name(role: Role, unit: u8, index: usize) -> Option<&'static str> {
    match role {
        Role::Amp => AMP_PARAMS.get(index).copied(),
        Role::Cabinet => CAB_PARAMS.get(index).copied(),
        _ => {
            let list = variants(role);
            list.get(unit as usize)
                .and_then(|v| v.params.get(index))
                .copied()
        }
    }
}

/// Human name of a module variant (or `None` for amp/cab, whose names are on the
/// device).
pub fn variant_name(role: Role, unit: u8) -> Option<&'static str> {
    variants(role).get(unit as usize).map(|v| v.name)
}

/// pt-BR label for a technical parameter name (industry terms stay when there is
/// no better translation).
pub fn display_name(name: &str) -> &'static str {
    match name {
        "Gain" => "Ganho",
        "Bass" => "Graves",
        "Mid" | "Middle" => "Médios",
        "Treble" => "Agudos",
        "Pres" => "Presença",
        "Gate" => "Gate",
        "Low Cut" => "Corte de graves",
        "High Cut" => "Corte de agudos",
        "T-Mode" => "Modo",
        "Level" | "Output" => "Volume",
        "Threshold" => "Limiar",
        "Release" => "Liberação",
        "Sustain" => "Sustentação",
        "Attack" => "Ataque",
        "Time" => "Tempo",
        "Fb" => "Repetição",
        "Mix" => "Mistura",
        "Decay" => "Decaimento",
        "Depth" => "Profundidade",
        "Speed" => "Velocidade",
        "Tone" => "Timbre",
        "Grit" => "Aspereza",
        "Diff" => "Difusão",
        "Combs" => "Ressonância",
        "RiseT" => "Subida",
        "Amt" => "Quantidade",
        "Pitch" => "Tom",
        "Unpack" => "Abertura",
        "ParamA" => "Ajuste A",
        "ParamB" => "Ajuste B",
        "Min" => "Mínimo",
        "Max" => "Máximo",
        "HPass" => "Corte de graves",
        "LPass" => "Corte de agudos",
        "Phaser" => "Phaser",
        "Input" => "Entrada",
        _ => "Parâmetro",
    }
}

/// One-line, plain-language help for a parameter (by its technical name).
pub fn param_help(name: &str) -> &'static str {
    match name {
        "Gain" => "Quanto o amplificador distorce. Baixo = limpo, alto = pesado.",
        "Bass" => "Graves: corpo e peso do som.",
        "Mid" | "Middle" => "Médios: presença na mistura com a banda.",
        "Treble" => "Agudos: brilho e ataque das notas.",
        "Pres" => "Presença: o brilho mais alto, deixa o som mais definido.",
        "Level" | "Output" => "Volume de saída deste bloco.",
        "Gate" => "Corta o chiado abaixo deste nível quando você não está tocando.",
        "Low Cut" => "Tira graves embolados da caixa (20 a 300 Hz).",
        "High Cut" => "Tira agudos ásperos da caixa (5 a 18 kHz).",
        "T-Mode" => "Como as duas repetições do Dual se combinam.",
        "Threshold" => "A partir de que volume o gate deixa o som passar.",
        "Release" => "Quanto tempo o gate demora para fechar.",
        "Sustain" => "Quanto o compressor segura as notas soando.",
        "Attack" => "Quão rápido o compressor reage ao ataque da palheta.",
        "Time" => "Distância entre as repetições do eco.",
        "Fb" => "Quantas vezes o eco/efeito se repete.",
        "Mix" => "Quanto do efeito se mistura ao som original.",
        "Decay" => "Duração da cauda do reverb.",
        "Depth" => "Intensidade da ondulação.",
        "Speed" => "Velocidade da ondulação.",
        "Tone" => "Mais escuro ou mais brilhante.",
        "Grit" => "Sujeira de fita antiga nas repetições.",
        "Diff" => "Quão espalhada e suave fica a cauda.",
        "Combs" => "Ressonância metálica da mola.",
        "RiseT" => "Tempo que o reverb leva para crescer.",
        "Amt" => "Quantidade do brilho de oitava do Shimmer.",
        "Pitch" => "Afinação das repetições.",
        "HPass" => "Corta graves da cauda (deixa mais limpo).",
        "LPass" => "Corta agudos da cauda (deixa mais escuro).",
        "Phaser" => "Movimento de phaser nas repetições.",
        "Unpack" => "Quanto o eco volta quando você para de tocar.",
        "Min" | "Max" => "Faixa de frequência do filtro automático.",
        "ParamA" | "ParamB" => "Ajuste específico deste modelo.",
        _ => "Parâmetro ainda não identificado. O valor é gravado como está.",
    }
}

/// A parameter value in real units when the app documents a scale
/// (**from app**: cab Low Cut 20–300 Hz, High Cut 5–18 kHz). `None` means the
/// pedal's own 0..100 is all we know.
pub fn format_value(name: &str, value: u16) -> Option<String> {
    let v = value.min(100) as f64;
    match name {
        "Low Cut" => Some(format!("{:.0} Hz", 20.0 + v * 280.0 / 100.0)),
        "High Cut" => Some(format!("{:.1} kHz", 5.0 + v * 13.0 / 100.0)),
        _ => None,
    }
}

/// Instrument a sound is meant for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Instrument {
    Guitar,
    Acoustic,
    Bass,
}

impl Instrument {
    pub const ALL: [Instrument; 3] = [Instrument::Guitar, Instrument::Acoustic, Instrument::Bass];

    pub fn label(self) -> &'static str {
        match self {
            Instrument::Guitar => "Guitarra",
            Instrument::Acoustic => "Violão",
            Instrument::Bass => "Baixo",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Instrument::Guitar => "guitar",
            Instrument::Acoustic => "acoustic",
            Instrument::Bass => "bass",
        }
    }

    pub fn from_key(key: &str) -> Option<Instrument> {
        Instrument::ALL.into_iter().find(|i| i.key() == key)
    }
}

/// Model indices 15..=19 are the bass amps/cabs (`BassADAtube` … `BassMarkT501`
/// and `BassAgula25` … `BassEDN300` in the device tables; factory banks 6–7).
pub const FIRST_BASS_MODEL: u8 = 15;

/// Instrument a preset is voiced for, judged by its amp model.
pub fn instrument_for_patch(patch: &crate::patch::Patch<'_>) -> Instrument {
    let amp_on = patch.is_on(1);
    if !amp_on && !patch.is_on(5) {
        return Instrument::Acoustic;
    }
    if patch.unit(1) >= FIRST_BASS_MODEL {
        Instrument::Bass
    } else {
        Instrument::Guitar
    }
}

/// Display name for a variant, falling back to "Role N" when unknown.
pub fn catalog_name(role: Role, unit: u8) -> String {
    variant_name(role, unit)
        .map(str::to_string)
        .unwrap_or_else(|| format!("{} {}", role.label(), unit as usize + 1))
}

/// Number of variants for a role; amp and cab use the 20-entry device table.
pub fn variant_count(role: Role) -> usize {
    match role {
        Role::Amp | Role::Cabinet => 20,
        other => variants(other).len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_cover_six_slots() {
        assert_eq!(Role::for_slot(0), Role::Fx);
        assert_eq!(Role::for_slot(1), Role::Amp);
        assert_eq!(Role::for_slot(5), Role::Cabinet);
    }

    #[test]
    fn variant_tables_are_consistent() {
        assert_eq!(variants(Role::Modulation).len(), 7);
        assert_eq!(variants(Role::Reverb).len(), 6);
        assert_eq!(variants(Role::Delay).len(), 5);
        assert_eq!(variants(Role::Fx).len(), 3);
        for role in [Role::Fx, Role::Modulation, Role::Delay, Role::Reverb] {
            for v in variants(role) {
                assert!(v.params.len() <= 6, "{} has too many params", v.name);
            }
        }
    }

    #[test]
    fn param_names_follow_the_variant() {
        assert_eq!(param_name(Role::Reverb, 0, 0), Some("Decay"));
        assert_eq!(param_name(Role::Reverb, 4, 2), Some("Tone")); // Shimmer
        assert_eq!(param_name(Role::Modulation, 1, 1), Some("ParamA")); // Phaser
        assert_eq!(param_name(Role::Amp, 7, 1), Some("Level"));
        assert_eq!(param_name(Role::Amp, 7, 4), Some("Treble"));
        assert_eq!(param_name(Role::Cabinet, 0, 2), Some("High Cut"));
        assert_eq!(param_name(Role::Delay, 2, 3), Some("Grit")); // dTape
        assert_eq!(format_value("Low Cut", 0).as_deref(), Some("20 Hz"));
        assert_eq!(format_value("High Cut", 100).as_deref(), Some("18.0 kHz"));
        assert_eq!(format_value("Gain", 10), None);
        assert_eq!(param_name(Role::Reverb, 0, 5), None);
    }
}
