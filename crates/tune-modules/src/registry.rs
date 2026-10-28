//! The modules a song may use, looked up by name.

use std::sync::LazyLock;

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
    /// Every module in [`BUILTIN`], built once and shared.
    pub fn builtin() -> &'static Self {
        static BUILTIN_REGISTRY: LazyLock<Registry> = LazyLock::new(|| {
            let mut registry = Registry {
                modules: Vec::new(),
            };
            for module in BUILTIN {
                registry.push(module);
            }
            registry
        });
        &BUILTIN_REGISTRY
    }

    /// A copy of this registry with `module` added at the end.
    ///
    /// # Panics
    ///
    /// On a duplicate name ("module `x` is already registered"), an
    /// instrument whose prefix is not its name, or a param id without
    /// `"<prefix>."`.
    pub fn with(&self, module: &'static ModuleDescriptor) -> Self {
        let mut registry = self.clone();
        registry.push(module);
        registry
    }

    /// Adds `module` at the end, with the checks of [`with`](Self::with).
    fn push(&mut self, module: &'static ModuleDescriptor) {
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
