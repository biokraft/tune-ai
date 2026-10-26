//! The modules a song may use, looked up by name.

use crate::builtin::{ACID303, CLAP, DISTORTION, EQ, HAT_CLOSED, HAT_OPEN, KICK, RIDE};
use crate::descriptor::ModuleDescriptor;

/// The registration list: one line per module, in the order listings show
/// them.
pub static BUILTIN: [&ModuleDescriptor; 8] = [
    &ACID303,
    &KICK,
    &CLAP,
    &HAT_CLOSED,
    &HAT_OPEN,
    &RIDE,
    &DISTORTION,
    &EQ,
];

/// A set of modules with unique names, in registration order.
#[derive(Clone, Debug)]
pub struct Registry {
    modules: Vec<&'static ModuleDescriptor>,
}

impl Registry {
    /// Every module in [`BUILTIN`].
    pub fn builtin() -> Self {
        BUILTIN.iter().fold(
            Self {
                modules: Vec::new(),
            },
            |registry, module| registry.with(module),
        )
    }

    /// Adds `module` at the end.
    ///
    /// # Panics
    ///
    /// On a duplicate name ("module `x` is already registered"), an
    /// instrument whose prefix is not its name, or a param id without
    /// `"<prefix>."`.
    pub fn with(mut self, module: &'static ModuleDescriptor) -> Self {
        let name = module.name;
        assert!(
            self.get(name).is_none(),
            "module `{name}` is already registered"
        );
        assert!(
            !module.is_instrument() || module.prefix == name,
            "instrument `{name}` has the prefix `{}`, not its name",
            module.prefix
        );
        for spec in module.params {
            assert!(
                spec.id
                    .strip_prefix(module.prefix)
                    .is_some_and(|rest| rest.starts_with('.')),
                "param `{}` of module `{name}` lacks the prefix `{}.`",
                spec.id,
                module.prefix
            );
        }
        self.modules.push(module);
        self
    }

    /// The module called `name`.
    pub fn get(&self, name: &str) -> Option<&'static ModuleDescriptor> {
        self.modules.iter().copied().find(|m| m.name == name)
    }

    /// Every instrument, in registration order.
    pub fn instruments(&self) -> impl Iterator<Item = &'static ModuleDescriptor> + '_ {
        self.modules.iter().copied().filter(|m| m.is_instrument())
    }

    /// Every effect, in registration order.
    pub fn effects(&self) -> impl Iterator<Item = &'static ModuleDescriptor> + '_ {
        self.modules.iter().copied().filter(|m| !m.is_instrument())
    }
}
