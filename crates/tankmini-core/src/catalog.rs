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

const FX_VARIANTS: &[Variant] = &[
    Variant { name: "Gate", params: &["Threshold", "Release", "Level"] },
    Variant { name: "Boost", params: &["Level", "Tone"] },
    Variant { name: "Comp", params: &["Sustain", "Attack", "Level"] },
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
    Variant { name: "Duck", params: &["Time", "Fb", "Mix", "Unpack", "Speed"] },
    Variant { name: "Dual", params: &["Time", "Fb", "Mix", "Speed", "Depth"] },
    Variant { name: "Lofi", params: &["Time", "Fb", "Mix", "Grit", "Speed"] },
];

const RVB_VARIANTS: &[Variant] = &[
    Variant { name: "Room", params: &["Decay", "Mix", "HPass", "LPass", "Depth"] },
    Variant { name: "Hall", params: &["Decay", "Mix", "HPass", "LPass", "Depth"] },
    Variant { name: "Swell", params: &["Decay", "Mix", "HPass", "LPass", "RiseT"] },
    Variant { name: "Spring", params: &["Decay", "Mix", "HPass", "LPass", "Combs"] },
    Variant { name: "Shimmer", params: &["Decay", "Mix", "Tone", "Pitch", "Amt"] },
    Variant { name: "Cloud", params: &["Decay", "Mix", "HPass", "LPass", "Diff"] },
];

const AMP_PARAMS: &[&str] = &["Gain", "Bass", "Mid", "Treble", "Pres", "Level"];
const CAB_PARAMS: &[&str] = &["Level"];

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
        "Input" => "Entrada",
        _ => "Parâmetro",
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
        assert_eq!(variants(Role::Delay).len(), 4);
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
        assert_eq!(param_name(Role::Amp, 7, 4), Some("Pres"));
        assert_eq!(param_name(Role::Reverb, 0, 5), None);
    }
}
