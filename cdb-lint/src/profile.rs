//! Turning a [`ProfileChoice`] into the yardstick a run is measured against.
//!
//! The standard makes the application profile the implementable unit (§5.1),
//! so this module answers exactly one question — *which* profile — and never
//! the question honesty rule 5 forbids: it does not look at the datastore.
//! Both halves of a built-in yardstick come from the command line, and a
//! datastore that disagrees with them draws a finding rather than changing
//! the yardstick.

use rusty_cdb::profiles::ApplicationProfile;
use rusty_cdb::{GnosisProfile, SimulationProfile};

use crate::cli::{BuiltinProfile, Encoding, ProfileChoice, UsageError};

/// Resolves the profile a run declared.
///
/// The return is boxed and `dyn` rather than an enum of the two built-ins,
/// because a descriptor profile (design §9, Task 6) is a third implementation
/// that cannot be enumerated here: it is read at runtime. Callers already
/// pass `&*profile` to `CdbDatastore::validate`, which takes
/// `&dyn ApplicationProfile`, so the indirection costs nothing and the
/// descriptor arm lands here without touching them.
///
/// A yardstick this build cannot construct is a **usage** error, never an
/// operational one: nothing was judged, and the reason is the request rather
/// than the datastore (design §4.1).
pub fn resolve(choice: &ProfileChoice) -> Result<Box<dyn ApplicationProfile>, UsageError> {
    match choice {
        ProfileChoice::Builtin { profile, encoding } => Ok(builtin(*profile, *encoding)),
        ProfileChoice::File(path) => Err(UsageError::new(format!(
            "`--profile-file` is not implemented yet, so `{}` cannot be used as a \
             yardstick; state a built-in profile with `--profile <simulation|gnosis>` \
             and `--encoding <json|xml>`",
            path.display()
        ))),
    }
}

/// The four built-in pairings. Each is a distinct constructor rather than one
/// constructor taking an encoding, because that is the shape the library
/// offers: `SimulationProfile` makes the standard's third encoding, `gpkg`,
/// unrepresentable by refusing to accept an encoding value at all, and
/// cdb-lint's own `--encoding` vocabulary is narrowed to match (design §9.2).
fn builtin(profile: BuiltinProfile, encoding: Encoding) -> Box<dyn ApplicationProfile> {
    match (profile, encoding) {
        (BuiltinProfile::Simulation, Encoding::Json) => Box::new(SimulationProfile::json()),
        (BuiltinProfile::Simulation, Encoding::Xml) => Box::new(SimulationProfile::xml()),
        (BuiltinProfile::Gnosis, Encoding::Json) => Box::new(GnosisProfile::json()),
        (BuiltinProfile::Gnosis, Encoding::Xml) => Box::new(GnosisProfile::xml()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    use rusty_cdb::metadata::MetadataEncoding;

    /// The resolved profile for a built-in pairing that must resolve.
    fn builtin_profile(profile: BuiltinProfile, encoding: Encoding) -> Box<dyn ApplicationProfile> {
        match resolve(&ProfileChoice::Builtin { profile, encoding }) {
            Ok(resolved) => resolved,
            Err(error) => panic!("{profile:?}/{encoding:?} should resolve: {error}"),
        }
    }

    /// Every one of the four pairings resolves, and each resolves to the
    /// profile it names rather than to a default that happens to be close.
    /// The encoding is checked through the profile's own declaration, which
    /// is what `validate` will compare the datastore against.
    #[test]
    fn cli_profile_every_builtin_pairing_resolves_to_what_it_names() {
        for (profile, encoding, name, declared) in [
            (
                BuiltinProfile::Simulation,
                Encoding::Json,
                "simulation",
                MetadataEncoding::Json,
            ),
            (
                BuiltinProfile::Simulation,
                Encoding::Xml,
                "simulation",
                MetadataEncoding::Xml,
            ),
            (
                BuiltinProfile::Gnosis,
                Encoding::Json,
                "gnosis",
                MetadataEncoding::Json,
            ),
            (
                BuiltinProfile::Gnosis,
                Encoding::Xml,
                "gnosis",
                MetadataEncoding::Xml,
            ),
        ] {
            let resolved = builtin_profile(profile, encoding);

            assert_eq!(resolved.name(), name, "{profile:?}/{encoding:?}");
            assert_eq!(
                resolved.metadata_encoding(),
                declared,
                "{profile:?}/{encoding:?}"
            );
        }
    }

    /// The two profiles differ in the tiling scheme they pin, which is the
    /// whole reason the second one exists. Resolving them to the same
    /// yardstick would make `--profile` decorative.
    #[test]
    fn cli_profile_the_two_builtins_are_different_yardsticks() {
        let simulation = builtin_profile(BuiltinProfile::Simulation, Encoding::Json);
        let gnosis = builtin_profile(BuiltinProfile::Gnosis, Encoding::Json);

        assert_ne!(simulation.tiling_scheme(), gnosis.tiling_scheme());
    }

    /// Task 6 owns the descriptor. Until then the flag parses and then says
    /// plainly that it does nothing, which beats accepting it and quietly
    /// measuring against a built-in profile the user did not ask for.
    #[test]
    fn cli_profile_descriptor_is_not_implemented_yet() {
        let error = match resolve(&ProfileChoice::File(PathBuf::from("acme.json"))) {
            Err(error) => error,
            Ok(profile) => panic!("a descriptor should not resolve, got {}", profile.name()),
        };

        assert!(
            error
                .message
                .contains("`--profile-file` is not implemented yet"),
            "{}",
            error.message
        );
        assert!(error.message.contains("acme.json"), "{}", error.message);
    }
}
