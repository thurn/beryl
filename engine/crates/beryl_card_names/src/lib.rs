//! Every card name as a constant, so that a misspelled card is a compile error, and the
//! decklists, which are only names.
//!
//! Code that names cards, such as the search's heuristics, depends on this crate rather than
//! on `beryl_cards`, the registry over every card file. Cargo recompiles every crate that
//! depends on a changed crate, so a crate that depended on `beryl_cards` for its names
//! recompiled after an edit to any card file. This crate's only input is the card store.

/// A constant for each card of the card store, by its own name: `names::COUNTERSPELL` is
/// `"Counterspell"` (`beryl_card_build`'s `names_source`). A name two cards would spell as one
/// constant has none.
pub mod names {
    include!(concat!(env!("OUT_DIR"), "/names.rs"));
}

/// Decklists.
pub mod decks {
    use crate::names;

    /// The deck a game starts with when its request names none, as owned name strings. Callers
    /// outside the card crates use this instead of naming a deck.
    #[must_use]
    pub fn default_deck() -> Vec<(u8, String)> {
        WU_CONTROL
            .iter()
            .map(|&(count, name)| (count, name.to_owned()))
            .collect()
    }

    /// The 40-card main deck of `docs/sample-decks/2025-11-18-17lands-cb8d5d5f-wu-control.txt`, the
    /// Milestone 1 mirror: a mythic drafter's deck from an Arena run of 2025-10-30.
    pub const WU_CONTROL: &[(u8, &str)] = &[
        (1, names::JACE_VRYNS_PRODIGY),
        (1, names::SNAPCASTER_MAGE),
        (1, names::TRINKET_MAGE),
        (1, names::ABHORRENT_OCULUS),
        (1, names::BEZA_THE_BOUNDING_SPRING),
        (1, names::SUBTLETY),
        (1, names::COUNTERSPELL),
        (1, names::NO_MORE_LIES),
        (1, names::REPRIEVE),
        (1, names::OUST),
        (1, names::THIRST_FOR_DISCOVERY),
        (1, names::STOCK_UP),
        (1, names::DAMN),
        (1, names::WINDS_OF_ABANDON),
        (1, names::FRACTURED_IDENTITY),
        (1, names::TEFERI_TIME_RAVELER),
        (1, names::THE_WANDERING_EMPEROR),
        (1, names::JACE_WIELDER_OF_MYSTERIES),
        (1, names::JACE_THE_MIND_SCULPTOR),
        (1, names::TEFERI_HERO_OF_DOMINARIA),
        (1, names::MANA_CRYPT),
        (1, names::PORTABLE_HOLE),
        (1, names::CURRENCY_CONVERTER),
        (1, names::STAFF_OF_THE_STORYTELLER),
        (1, names::TUNDRA),
        (1, names::SEACHROME_COAST),
        (1, names::FLOODFARM_VERGE),
        (1, names::SPARAS_HEADQUARTERS),
        (6, names::PLAINS),
        (6, names::ISLAND),
    ];

    /// The 40-card main deck of `docs/sample-decks/2026-10-17-PrinceJools-wu-control.txt`, the
    /// MTGO deck that was the mirror before. It plays implemented cards the mirror
    /// lacks, which UI fixtures reach in play.
    pub const VINTAGE_WU_CONTROL: &[(u8, &str)] = &[
        (1, names::MANA_TITHE),
        (1, names::SWIFT_RECONFIGURATION),
        (1, names::OUST),
        (1, names::THUNDERTRAP_TRAINER),
        (1, names::JACE_VRYNS_PRODIGY),
        (1, names::THIEVING_SKYDIVER),
        (1, names::MALCOLM_ALLURING_SCOUNDREL),
        (1, names::OVERLORD_OF_THE_MISTMOORS),
        (1, names::ELITE_SPELLBINDER),
        (1, names::TEFERI_TIME_RAVELER),
        (1, names::AANGS_ICEBERG),
        (1, names::ADELINE_RESPLENDENT_CATHAR),
        (1, names::THE_WANDERING_EMPEROR),
        (1, names::JACE_THE_MIND_SCULPTOR),
        (1, names::SPHINX_OF_FALSE_CONCLUSIONS),
        (1, names::TIME_SPIRAL),
        (1, names::STRIP_MINE),
        (1, names::MANA_CRYPT),
        (1, names::SEACHROME_COAST),
        (1, names::SPARAS_HEADQUARTERS),
        (1, names::TUNDRA),
        (7, names::PLAINS),
        (6, names::ISLAND),
        (1, names::STAFF_OF_THE_STORYTELLER),
        (1, names::GET_LOST),
        (1, names::NO_MORE_LIES),
        (1, names::MANA_DRAIN),
        (1, names::SINK_INTO_STUPOR),
        (1, names::TIMETWISTER),
    ];

    /// The rules keystones (`docs/rules-coverage.md`, "The keystone cards") by family, each a name
    /// and the keystones and partners played together in it. The names match the
    /// present-cost benchmarks where those exist. Random and agent games deal fuzz decks
    /// from one family's cards and the basic lands (`beryl_cards::decks::fuzz_deck_from`), so that
    /// the cards of a family meet each other far more often than in decks drawn from every card.
    pub const KEYSTONE_FAMILIES: &[(&str, &[&str])] = &[
        (
            "designations",
            &[
                names::WIZARD_CLASS,
                names::CASE_OF_THE_CRIMSON_PULSE,
                names::STUDENT_OF_WARFARE,
                names::VAMPIRE_HEXMAGE,
                names::DIMINISH,
            ],
        ),
        (
            "combat",
            &[
                names::PALACE_GUARD,
                names::FENCING_ACE,
                names::BENALISH_HERO,
                names::BENALISH_INFANTRY,
            ],
        ),
        (
            "split",
            &[
                names::FIRE_ICE,
                names::WEAR_TEAR,
                names::LIQUIMETAL_COATING,
                names::BLOOD_MOON,
                names::FORK,
                names::FORCE_OF_WILL,
                names::SPELL_SNARE,
            ],
        ),
        ("face-down", &[names::EXALTED_ANGEL, names::GATHAN_RAIDERS]),
        (
            "turn-in-place",
            &[
                names::DELVER_OF_SECRETS,
                names::ERAYO_SORATAMI_ASCENDANT,
                names::LIGHTNING_BOLT,
                names::HOLY_STRENGTH,
                names::PHANTASMAL_IMAGE,
            ],
        ),
        (
            "casting",
            &[
                names::BLOODBRAID_ELF,
                names::KNOWLEDGE_POOL,
                names::POSSIBILITY_STORM,
                names::BONECRUSHER_GIANT,
                names::PANGLACIAL_WURM,
                names::TRINKET_MAGE,
                names::ADVENTUROUS_EATER_HAVE_A_BITE,
                names::LIGHTNING_BOLT,
                names::COUNTERSPELL,
                names::VALAKUT_AWAKENING,
                names::SYLVAN_SCRYING,
            ],
        ),
        ("outside-the-game", &[names::BURNING_WISH]),
        ("subgames", &[names::SHAHRAZAD]),
        (
            "attachments",
            &[names::SWIFT_RECONFIGURATION, names::LEONIN_SCIMITAR],
        ),
        (
            "layers",
            &[
                names::BLOOD_MOON,
                names::URBORG_TOMB_OF_YAWGMOTH,
                names::DRYAD_ARBOR,
                names::GLORIOUS_ANTHEM,
                names::HUMILITY,
                names::TARMOGOYF,
                names::DRESS_DOWN,
                names::OPALESCENCE,
                names::OKO_THIEF_OF_CROWNS,
                names::LEONIN_SCIMITAR,
            ],
        ),
        (
            "replacement",
            &[
                names::CHAINS_OF_MEPHISTOPHELES,
                names::NOTION_THIEF,
                names::GRISELBRAND,
                names::LIFE_FROM_THE_LOAM,
                names::REST_IN_PEACE,
                names::TARMOGOYF,
                names::CONTAINMENT_PRIEST,
                names::FLICKERWISP,
                names::MOTHER_OF_RUNES,
                names::CIRCLE_OF_PROTECTION_RED,
                names::FURNACE_OF_RATH,
                names::LIGHTNING_BOLT,
            ],
        ),
        (
            "zones-and-text",
            &[
                names::PAINTERS_SERVANT,
                names::FORCE_OF_WILL,
                names::CIRCLE_OF_PROTECTION_RED,
                names::MIND_BEND,
                names::DANDAN,
                names::CRYSTAL_SPRAY,
                names::LORD_OF_ATLANTIS,
                names::UNDERWORLD_BREACH,
                names::LIONS_EYE_DIAMOND,
                names::BRAIN_FREEZE,
            ],
        ),
        (
            "copies",
            &[
                names::PHANTASMAL_IMAGE,
                names::QUICKSILVER_GARGANTUAN,
                names::THESPIANS_STAGE,
                names::DARK_DEPTHS,
                names::FORK,
                names::TENDRILS_OF_AGONY,
                names::BRAINGEYSER,
            ],
        ),
        (
            "costs",
            &[
                names::TRINISPHERE,
                names::SHRAPNEL_BLAST,
                names::THALIA_GUARDIAN_OF_THRABEN,
                names::FORCE_OF_WILL,
                names::GURMAG_ANGLER,
                names::PITHING_NEEDLE,
                names::MISTER_IMMORTAL,
                names::CAVERN_OF_SOULS,
            ],
        ),
        (
            "identity",
            &[
                names::STIFLE,
                names::ANIMATE_DEAD,
                names::WORLDGORGER_DRAGON,
                names::GEMRAZER,
                names::TEFERIS_PROTECTION,
            ],
        ),
        (
            "turn",
            &[
                names::TIME_STOP,
                names::SUNDIAL_OF_THE_INFINITE,
                names::PARADOX_HAZE,
                names::STASIS,
                names::TIME_VAULT,
                names::FINAL_FORTUNE,
                names::KARN_LIBERATED,
            ],
        ),
        ("sagas", &[names::THE_ELDEST_REBORN]),
        (
            "decisions",
            &[names::SHOW_AND_TELL, names::GRISELBRAND, names::MINDSLAVER],
        ),
    ];
}
