//! What a module is: its name, default device id, params, settings and the
//! factory that builds it.

use std::fmt;

use tune_dsp::ParamSpec;

use crate::instrument::{Effect, Instrument};

/// The kind of clip a track plays, and so the step notation it is written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClipKind {
    /// 303 notation (`C2 C2! C3~ .`).
    Acid,
    /// Drum notation (`x X .`).
    Drum,
    /// Timed notes (`kind = "notes"`); every instrument plays them.
    Notes,
}

impl ClipKind {
    /// Every kind, in the order error messages list them.
    pub const ALL: [Self; 3] = [Self::Acid, Self::Drum, Self::Notes];

    /// The clip `kind` value in a song file.
    pub fn name(self) -> &'static str {
        match self {
            Self::Acid => "acid",
            Self::Drum => "drum",
            Self::Notes => "notes",
        }
    }

    /// Parses [`name`](Self::name).
    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == s)
    }
}

/// A named choice that is fixed when the module is built (not a param).
#[derive(Clone, Copy, Debug)]
pub struct SettingSpec {
    pub name: &'static str,
    /// Every value a song file may give, in the order help lists them.
    pub values: &'static [&'static str],
    /// One of `values`.
    pub default: &'static str,
    pub help: &'static str,
}

/// Builds an instrument; `settings` holds one value per
/// [`ModuleDescriptor::settings`] entry, each one of its `values`.
pub type MakeInstrument = fn(sample_rate: f64, settings: &[&'static str]) -> Box<dyn Instrument>;

/// Builds an effect; `settings` as for [`MakeInstrument`].
pub type MakeEffect = fn(sample_rate: f64, settings: &[&'static str]) -> Box<dyn Effect>;

/// Whether a module is a track's sound source or an effect in its chain.
pub enum Role {
    Instrument {
        /// The clip kinds this instrument plays.
        plays: &'static [ClipKind],
        /// Whether the first track with this instrument ducks the others by
        /// default.
        default_duck_source: bool,
        make: MakeInstrument,
    },
    Effect {
        make: MakeEffect,
    },
}

/// One module: everything the validator, the CLI and the engine need to
/// know about it.
pub struct ModuleDescriptor {
    /// The module name a song file uses (`"distortion"`).
    pub name: &'static str,
    /// The default device id (`"dist"`); equals `name` for instruments.
    pub prefix: &'static str,
    /// Every id is `"<prefix>.<param>"`.
    pub params: &'static [ParamSpec],
    pub settings: &'static [SettingSpec],
    pub role: Role,
}

impl ModuleDescriptor {
    pub fn is_instrument(&self) -> bool {
        matches!(self.role, Role::Instrument { .. })
    }

    /// Whether this module plays clips of `kind`; false for effects. Every
    /// instrument plays note clips, whatever its `plays` list holds.
    pub fn plays(&self, kind: ClipKind) -> bool {
        match self.role {
            Role::Instrument { plays, .. } => kind == ClipKind::Notes || plays.contains(&kind),
            Role::Effect { .. } => false,
        }
    }

    /// Whether this module is the default duck source; false for effects.
    pub fn default_duck_source(&self) -> bool {
        match self.role {
            Role::Instrument {
                default_duck_source,
                ..
            } => default_duck_source,
            Role::Effect { .. } => false,
        }
    }

    /// The param whose id is `"<prefix>.<local>"`, with its index.
    pub fn param(&self, local: &str) -> Option<(usize, &'static ParamSpec)> {
        self.params
            .iter()
            .enumerate()
            .find(|(_, spec)| self.strip_prefix(spec.id) == Some(local))
    }

    /// The local name of `id` when it is `"<device_id>.<local>"` and `local`
    /// names one of this module's params: how a device called `device_id`
    /// reads a track-relative param id (`eq2.low_cut` on device `eq2` is
    /// `low_cut`).
    pub fn local_param<'a>(&self, device_id: &str, id: &'a str) -> Option<&'a str> {
        let local = id.strip_prefix(device_id)?.strip_prefix('.')?;
        self.param(local).map(|_| local)
    }

    /// The id of the param at `index` without `"<prefix>."`.
    ///
    /// # Panics
    ///
    /// If `index` is out of range or the id lacks the prefix.
    pub fn local_name(&self, index: usize) -> &'static str {
        let id = self.params[index].id;
        self.strip_prefix(id)
            .unwrap_or_else(|| panic!("param `{id}` lacks the prefix `{}.`", self.prefix))
    }

    /// The setting called `name`, with its index.
    pub fn setting(&self, name: &str) -> Option<(usize, &'static SettingSpec)> {
        self.settings
            .iter()
            .enumerate()
            .find(|(_, s)| s.name == name)
    }

    /// Every setting at its default, in [`settings`](Self::settings) order.
    pub fn default_settings(&self) -> Vec<&'static str> {
        self.settings.iter().map(|s| s.default).collect()
    }

    /// A fresh instrument with every param at its default; `None` for an
    /// effect.
    pub fn make_instrument(
        &self,
        sample_rate: f64,
        settings: &[&'static str],
    ) -> Option<Box<dyn Instrument>> {
        match self.role {
            Role::Instrument { make, .. } => Some(make(sample_rate, settings)),
            Role::Effect { .. } => None,
        }
    }

    /// A fresh effect with every param at its default; `None` for an
    /// instrument.
    pub fn make_effect(
        &self,
        sample_rate: f64,
        settings: &[&'static str],
    ) -> Option<Box<dyn Effect>> {
        match self.role {
            Role::Effect { make } => Some(make(sample_rate, settings)),
            Role::Instrument { .. } => None,
        }
    }

    /// `id` without `"<prefix>."`, if it has that prefix.
    fn strip_prefix<'a>(&self, id: &'a str) -> Option<&'a str> {
        id.strip_prefix(self.prefix)?.strip_prefix('.')
    }
}

impl PartialEq for ModuleDescriptor {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl fmt::Debug for ModuleDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ModuleDescriptor({:?})", self.name)
    }
}
