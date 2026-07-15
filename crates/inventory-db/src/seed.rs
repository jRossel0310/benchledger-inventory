//! Built-in category and attribute seeds. Idempotent and insert-only: rows
//! are matched by deterministic ID; existing rows are NEVER updated or
//! deleted, so user customizations survive re-seeding and upgrades.

use rusqlite::Connection;

use crate::DbError;

#[derive(Debug, Default)]
pub struct SeedReport {
    pub categories_inserted: usize,
    pub attributes_inserted: usize,
    pub links_inserted: usize,
    pub choices_inserted: usize,
}

/// Deterministic 26-char id: 22 zeros + tag (1 char) + 3-digit base-10 index.
/// Uses only Crockford-valid characters.
fn det_id(tag: char, index: usize) -> String {
    format!("{}{}{:03}", "0".repeat(22), tag, index)
}

const GROUPS: [&str; 5] = [
    "Passive components",
    "Semiconductors",
    "Interconnect and electromechanical",
    "Modules and reusable items",
    "Mechanical and miscellaneous",
];

/// (name, group index). Order is stable — indexes feed det_id and must never
/// be reordered or removed; append only.
const CATEGORIES: &[(&str, usize)] = &[
    ("Resistor", 0),
    ("Resistor network", 0),
    ("Potentiometer", 0),
    ("Capacitor", 0),
    ("Inductor", 0),
    ("Transformer", 0),
    ("Ferrite bead", 0),
    ("Diode", 1),
    ("Zener diode", 1),
    ("Schottky diode", 1),
    ("Photodiode", 1),
    ("LED", 1),
    ("BJT", 1),
    ("MOSFET", 1),
    ("JFET", 1),
    ("Op amp", 1),
    ("Comparator", 1),
    ("Voltage regulator", 1),
    ("Logic IC", 1),
    ("ADC", 1),
    ("DAC", 1),
    ("Memory", 1),
    ("Interface IC", 1),
    ("Driver IC", 1),
    ("Timing IC", 1),
    ("Microcontroller", 1),
    ("Processor", 1),
    ("FPGA or CPLD", 1),
    ("Sensor", 1),
    ("Optocoupler", 1),
    ("Crystal", 1),
    ("Oscillator", 1),
    ("Connector", 2),
    ("Terminal block", 2),
    ("Header", 2),
    ("Socket", 2),
    ("Switch", 2),
    ("Button", 2),
    ("Relay", 2),
    ("Fuse", 2),
    ("Cable", 2),
    ("Adapter", 2),
    ("Wire", 2),
    ("Development board", 3),
    ("Breakout board", 3),
    ("Communication module", 3),
    ("Power module", 3),
    ("Sensor module", 3),
    ("Programmer", 3),
    ("Probe", 3),
    ("Reusable accessory", 3),
    ("Tool", 3),
    ("Enclosure", 4),
    ("Heatsink", 4),
    ("Fan", 4),
    ("Motor", 4),
    ("Battery", 4),
    ("Battery holder", 4),
    ("Screw", 4),
    ("Nut", 4),
    ("Washer", 4),
    ("Spacer", 4),
    ("Standoff", 4),
    ("Knob", 4),
    ("Breadboard", 4),
    ("PCB blank", 4),
    ("Thermal material", 4),
];

/// Attribute definitions: (key, label, data_type, unit_kind, identity).
/// Order stable; append only.
const ATTRIBUTES: &[(&str, &str, &str, Option<&str>, bool)] = &[
    // shared
    ("package", "Package", "text", None, true),
    ("mounting_style", "Mounting style", "choice", None, false),
    ("pinout", "Pinout", "text", None, false),
    // resistor
    (
        "resistance",
        "Resistance",
        "number_unit",
        Some("resistance"),
        true,
    ),
    (
        "tolerance",
        "Tolerance",
        "number_unit",
        Some("percent"),
        true,
    ),
    (
        "power_rating",
        "Power rating",
        "number_unit",
        Some("power"),
        true,
    ),
    (
        "temp_coefficient",
        "Temperature coefficient",
        "text",
        None,
        false,
    ),
    (
        "max_voltage",
        "Maximum voltage",
        "number_unit",
        Some("voltage"),
        false,
    ),
    ("technology", "Technology", "choice", None, false),
    ("num_elements", "Number of elements", "number", None, false),
    (
        "network_config",
        "Network configuration",
        "text",
        None,
        false,
    ),
    // capacitor
    (
        "capacitance",
        "Capacitance",
        "number_unit",
        Some("capacitance"),
        true,
    ),
    ("dielectric", "Type / dielectric", "choice", None, true),
    (
        "voltage_rating",
        "Voltage rating",
        "number_unit",
        Some("voltage"),
        true,
    ),
    ("esr", "ESR", "number_unit", Some("resistance"), false),
    ("polarized", "Polarized", "boolean", None, false),
    (
        "ripple_current",
        "Ripple current",
        "number_unit",
        Some("current"),
        false,
    ),
    ("rated_lifetime", "Rated lifetime", "text", None, false),
    // inductor
    (
        "inductance",
        "Inductance",
        "number_unit",
        Some("inductance"),
        true,
    ),
    (
        "current_rating",
        "Current rating",
        "number_unit",
        Some("current"),
        true,
    ),
    // semiconductors
    ("channel_type", "Channel type", "choice", None, true),
    (
        "vds_max",
        "Max drain-source voltage",
        "number_unit",
        Some("voltage"),
        true,
    ),
    (
        "id_continuous",
        "Continuous drain current",
        "number_unit",
        Some("current"),
        false,
    ),
    (
        "vgs_threshold",
        "Gate threshold range",
        "range",
        Some("voltage"),
        false,
    ),
    (
        "rds_on",
        "RDS(on)",
        "number_unit",
        Some("resistance"),
        false,
    ),
    (
        "gate_charge",
        "Gate charge",
        "number_unit",
        Some("charge"),
        false,
    ),
    ("logic_level", "Logic-level gate", "boolean", None, false),
    (
        "forward_voltage",
        "Forward voltage",
        "number_unit",
        Some("voltage"),
        false,
    ),
    (
        "forward_current",
        "Forward current",
        "number_unit",
        Some("current"),
        false,
    ),
    (
        "reverse_voltage",
        "Reverse voltage",
        "number_unit",
        Some("voltage"),
        true,
    ),
    ("led_color", "Color", "choice", None, true),
    // op amp / comparator
    ("channel_count", "Channel count", "number", None, true),
    (
        "supply_min",
        "Minimum supply voltage",
        "number_unit",
        Some("voltage"),
        false,
    ),
    (
        "supply_max",
        "Maximum supply voltage",
        "number_unit",
        Some("voltage"),
        false,
    ),
    (
        "rail_to_rail_input",
        "Rail-to-rail input",
        "boolean",
        None,
        false,
    ),
    (
        "rail_to_rail_output",
        "Rail-to-rail output",
        "boolean",
        None,
        false,
    ),
    (
        "gbw",
        "Gain-bandwidth product",
        "number_unit",
        Some("frequency"),
        false,
    ),
    ("slew_rate", "Slew rate", "text", None, false),
    (
        "input_offset",
        "Input offset voltage",
        "number_unit",
        Some("voltage"),
        false,
    ),
    // regulator
    ("regulator_type", "Regulator type", "choice", None, true),
    (
        "output_voltage",
        "Output voltage",
        "number_unit",
        Some("voltage"),
        true,
    ),
    (
        "output_current",
        "Output current",
        "number_unit",
        Some("current"),
        false,
    ),
    (
        "dropout",
        "Dropout voltage",
        "number_unit",
        Some("voltage"),
        false,
    ),
    // crystal
    (
        "frequency",
        "Frequency",
        "number_unit",
        Some("frequency"),
        true,
    ),
    (
        "load_capacitance",
        "Load capacitance",
        "number_unit",
        Some("capacitance"),
        false,
    ),
    // connector
    ("connector_family", "Connector family", "text", None, true),
    ("contact_count", "Contact count", "number", None, true),
    ("rows", "Rows", "number", None, false),
    ("pitch", "Pitch", "number_unit", Some("length"), true),
    ("gender", "Gender", "choice", None, true),
    ("orientation", "Orientation", "choice", None, false),
    ("termination", "Termination type", "choice", None, false),
    ("mating_pn", "Mating part number", "text", None, false),
];

/// (attribute key, choices)
const CHOICES: &[(&str, &[&str])] = &[
    (
        "mounting_style",
        &["SMD", "THT", "Panel mount", "Free hanging", "Chassis"],
    ),
    (
        "technology",
        &[
            "Thick film",
            "Thin film",
            "Metal film",
            "Carbon film",
            "Wirewound",
        ],
    ),
    (
        "dielectric",
        &[
            "C0G/NP0",
            "X7R",
            "X5R",
            "Y5V",
            "Aluminum electrolytic",
            "Tantalum",
            "Film",
            "Ceramic disc",
        ],
    ),
    ("channel_type", &["N-channel", "P-channel", "NPN", "PNP"]),
    (
        "led_color",
        &[
            "Red", "Green", "Blue", "Yellow", "Orange", "White", "Amber", "IR", "UV", "RGB",
        ],
    ),
    (
        "regulator_type",
        &[
            "Linear LDO",
            "Linear standard",
            "Buck",
            "Boost",
            "Buck-boost",
            "Charge pump",
        ],
    ),
    ("gender", &["Male", "Female", "Hermaphroditic"]),
    ("orientation", &["Vertical", "Right angle"]),
    (
        "termination",
        &["Solder", "Crimp", "Screw", "IDC", "Press-fit"],
    ),
];

/// (category name, attribute keys in display order)
const CATEGORY_LINKS: &[(&str, &[&str])] = &[
    (
        "Resistor",
        &[
            "resistance",
            "tolerance",
            "power_rating",
            "package",
            "mounting_style",
            "temp_coefficient",
            "max_voltage",
            "technology",
        ],
    ),
    (
        "Resistor network",
        &[
            "resistance",
            "tolerance",
            "num_elements",
            "network_config",
            "package",
            "mounting_style",
        ],
    ),
    (
        "Capacitor",
        &[
            "capacitance",
            "dielectric",
            "voltage_rating",
            "tolerance",
            "package",
            "mounting_style",
            "esr",
            "polarized",
            "ripple_current",
            "rated_lifetime",
        ],
    ),
    (
        "Inductor",
        &[
            "inductance",
            "current_rating",
            "tolerance",
            "package",
            "mounting_style",
        ],
    ),
    (
        "Diode",
        &[
            "reverse_voltage",
            "forward_current",
            "forward_voltage",
            "package",
            "mounting_style",
        ],
    ),
    (
        "Zener diode",
        &[
            "reverse_voltage",
            "power_rating",
            "tolerance",
            "package",
            "mounting_style",
        ],
    ),
    (
        "Schottky diode",
        &[
            "reverse_voltage",
            "forward_current",
            "forward_voltage",
            "package",
            "mounting_style",
        ],
    ),
    (
        "LED",
        &[
            "led_color",
            "forward_voltage",
            "forward_current",
            "package",
            "mounting_style",
        ],
    ),
    (
        "BJT",
        &[
            "channel_type",
            "max_voltage",
            "current_rating",
            "package",
            "mounting_style",
            "pinout",
        ],
    ),
    (
        "MOSFET",
        &[
            "channel_type",
            "vds_max",
            "id_continuous",
            "vgs_threshold",
            "rds_on",
            "gate_charge",
            "logic_level",
            "package",
            "pinout",
        ],
    ),
    (
        "Op amp",
        &[
            "channel_count",
            "supply_min",
            "supply_max",
            "rail_to_rail_input",
            "rail_to_rail_output",
            "gbw",
            "slew_rate",
            "input_offset",
            "package",
            "pinout",
        ],
    ),
    (
        "Comparator",
        &[
            "channel_count",
            "supply_min",
            "supply_max",
            "package",
            "pinout",
        ],
    ),
    (
        "Voltage regulator",
        &[
            "regulator_type",
            "output_voltage",
            "output_current",
            "supply_max",
            "dropout",
            "package",
        ],
    ),
    (
        "Crystal",
        &[
            "frequency",
            "load_capacitance",
            "tolerance",
            "package",
            "mounting_style",
        ],
    ),
    (
        "Oscillator",
        &[
            "frequency",
            "supply_min",
            "supply_max",
            "package",
            "mounting_style",
        ],
    ),
    (
        "Connector",
        &[
            "connector_family",
            "contact_count",
            "rows",
            "pitch",
            "gender",
            "orientation",
            "mounting_style",
            "termination",
            "current_rating",
            "voltage_rating",
            "mating_pn",
        ],
    ),
    (
        "Header",
        &[
            "contact_count",
            "rows",
            "pitch",
            "gender",
            "orientation",
            "mounting_style",
        ],
    ),
];

pub fn ensure_builtins(conn: &mut Connection) -> Result<SeedReport, DbError> {
    let tx = conn.transaction()?;
    let mut report = SeedReport::default();

    for (i, (name, group)) in CATEGORIES.iter().enumerate() {
        let n = tx.execute(
            "INSERT OR IGNORE INTO categories (id, name, group_name, built_in) VALUES (?1, ?2, ?3, 1)",
            rusqlite::params![det_id('C', i + 1), name, GROUPS[*group]],
        )?;
        report.categories_inserted += n;
    }
    for (i, (key, label, dtype, unit, identity)) in ATTRIBUTES.iter().enumerate() {
        let canonical = unit.map(|u| {
            inventory_core::units::UnitKind::from_sql(u)
                .expect("seed unit kind")
                .canonical_unit()
        });
        let n = tx.execute(
            "INSERT OR IGNORE INTO attribute_defs (id, key, label, data_type, unit_kind, canonical_unit, identity, built_in)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1)",
            rusqlite::params![det_id('A', i + 1), key, label, dtype, unit, canonical, identity],
        )?;
        report.attributes_inserted += n;
    }
    for (key, choices) in CHOICES {
        for (order, choice) in choices.iter().enumerate() {
            let n = tx.execute(
                "INSERT OR IGNORE INTO attribute_choices (attribute_id, value, display_order)
                 SELECT id, ?2, ?3 FROM attribute_defs WHERE key = ?1",
                rusqlite::params![key, choice, order as i64],
            )?;
            report.choices_inserted += n;
        }
    }
    for (cat_name, keys) in CATEGORY_LINKS {
        for (order, key) in keys.iter().enumerate() {
            let n = tx.execute(
                "INSERT OR IGNORE INTO category_attributes (category_id, attribute_id, display_order)
                 SELECT c.id, a.id, ?3 FROM categories c, attribute_defs a
                 WHERE c.name = ?1 AND c.built_in = 1 AND a.key = ?2",
                rusqlite::params![cat_name, key, order as i64],
            )?;
            report.links_inserted += n;
        }
    }
    tx.commit()?;
    Ok(report)
}
