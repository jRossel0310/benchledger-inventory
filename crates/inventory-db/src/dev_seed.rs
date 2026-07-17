//! Development-only representative dataset: parts across several built-in
//! categories, with real stock movements applied through the ledger, a
//! couple of projects, a few manufacturer variants and supplier listings,
//! some physical dimensions, a handful of low-stock thresholds, and one
//! archived part — so the desktop UI has something worth looking at while
//! Phase 3's screens are built.
//!
//! Everything here goes through the same `Database` repository and ledger
//! methods the Tauri commands use (`create_part`, `set_attribute`, `apply`,
//! `add_dimension`, `add_variant`, `add_supplier_listing`,
//! `create_project`, `set_part_archived`) — no direct SQL — so the seed can
//! never construct a state the real app couldn't also reach.
//!
//! Reachable only through the `#[cfg(debug_assertions)]` `dev_seed` Tauri
//! command (`apps/desktop/src-tauri/src/commands.rs`); this module itself
//! has no `cfg` gate (so it stays compiled and tested in every profile),
//! but nothing calls `run` outside of a debug build or this module's own
//! tests.
//!
//! Idempotent: `run` returns `Ok(0)` without writing anything when the
//! database already has any part (`list_parts` non-empty).

use std::collections::HashMap;

use inventory_core::ids::{CategoryId, PartId};
use inventory_core::ledger::LedgerOp;
use inventory_core::quantity::{Quantity, QuantityUnit};

use crate::dimensions::{DimensionDraft, DimensionGroup, DimensionSource};
use crate::parts::{ListingDraft, PartDraft, VariantDraft};
use crate::{Database, DbError, MISC_CATEGORY_ID};

/// One row of the flat seed table: enough to create a part, set its bin and
/// low-stock threshold, attach a handful of category-appropriate
/// attributes, and receive an initial quantity of stock.
struct SeedPart {
    /// Built-in category name (`seed.rs`'s `CATEGORIES`), or `"__misc__"`
    /// for the deterministic Miscellaneous category.
    category: &'static str,
    name: &'static str,
    description: &'static str,
    bin: &'static str,
    unit: QuantityUnit,
    /// Whole units received on creation via a real `LedgerOp::Receive`. 0
    /// leaves the part with no stock (e.g. a never-restocked legacy part).
    receive: i64,
    low_stock_threshold: Option<i64>,
    usage: &'static str,
    attrs: &'static [(&'static str, &'static str)],
}

const EACH: QuantityUnit = QuantityUnit::Each;

#[rustfmt::skip]
const RESISTORS: &[SeedPart] = &[
    SeedPart { category: "Resistor", name: "10R 0603 1% resistor",   description: "Thick film chip resistor", bin: "A1",  unit: EACH, receive: 500, low_stock_threshold: None,      usage: "usually_consumed", attrs: &[("resistance", "10R"),   ("tolerance", "1%"), ("power_rating", "1/4W"), ("package", "0603"), ("mounting_style", "SMD")] },
    SeedPart { category: "Resistor", name: "22R 0603 1% resistor",   description: "Thick film chip resistor", bin: "A2",  unit: EACH, receive: 500, low_stock_threshold: None,      usage: "usually_consumed", attrs: &[("resistance", "22R"),   ("tolerance", "1%"), ("power_rating", "1/4W"), ("package", "0603"), ("mounting_style", "SMD")] },
    SeedPart { category: "Resistor", name: "47R 0603 1% resistor",   description: "Thick film chip resistor", bin: "A3",  unit: EACH, receive: 500, low_stock_threshold: None,      usage: "usually_consumed", attrs: &[("resistance", "47R"),   ("tolerance", "1%"), ("power_rating", "1/4W"), ("package", "0603"), ("mounting_style", "SMD")] },
    SeedPart { category: "Resistor", name: "100R 0603 1% resistor",  description: "Thick film chip resistor", bin: "A4",  unit: EACH, receive: 500, low_stock_threshold: None,      usage: "usually_consumed", attrs: &[("resistance", "100R"),  ("tolerance", "1%"), ("power_rating", "1/4W"), ("package", "0603"), ("mounting_style", "SMD")] },
    SeedPart { category: "Resistor", name: "220R 0603 1% resistor",  description: "Thick film chip resistor", bin: "A5",  unit: EACH, receive: 500, low_stock_threshold: None,      usage: "usually_consumed", attrs: &[("resistance", "220R"),  ("tolerance", "1%"), ("power_rating", "1/4W"), ("package", "0603"), ("mounting_style", "SMD")] },
    SeedPart { category: "Resistor", name: "470R 0603 1% resistor",  description: "Thick film chip resistor", bin: "A6",  unit: EACH, receive: 500, low_stock_threshold: None,      usage: "usually_consumed", attrs: &[("resistance", "470R"),  ("tolerance", "1%"), ("power_rating", "1/4W"), ("package", "0603"), ("mounting_style", "SMD")] },
    SeedPart { category: "Resistor", name: "1k 0603 1% resistor",    description: "Thick film chip resistor", bin: "A7",  unit: EACH, receive: 500, low_stock_threshold: None,      usage: "usually_consumed", attrs: &[("resistance", "1k"),    ("tolerance", "1%"), ("power_rating", "1/4W"), ("package", "0603"), ("mounting_style", "SMD")] },
    SeedPart { category: "Resistor", name: "2k2 0603 1% resistor",   description: "Thick film chip resistor", bin: "A8",  unit: EACH, receive: 500, low_stock_threshold: None,      usage: "usually_consumed", attrs: &[("resistance", "2k2"),   ("tolerance", "1%"), ("power_rating", "1/4W"), ("package", "0603"), ("mounting_style", "SMD")] },
    SeedPart { category: "Resistor", name: "4k7 0603 1% resistor",   description: "Thick film chip resistor", bin: "A9",  unit: EACH, receive: 500, low_stock_threshold: None,      usage: "usually_consumed", attrs: &[("resistance", "4k7"),   ("tolerance", "1%"), ("power_rating", "1/4W"), ("package", "0603"), ("mounting_style", "SMD")] },
    SeedPart { category: "Resistor", name: "10k 0603 1% resistor",   description: "Thick film chip resistor", bin: "A10", unit: EACH, receive: 500, low_stock_threshold: Some(100), usage: "usually_consumed", attrs: &[("resistance", "10k"),   ("tolerance", "1%"), ("power_rating", "1/4W"), ("package", "0603"), ("mounting_style", "SMD")] },
    SeedPart { category: "Resistor", name: "22k 0603 1% resistor",   description: "Thick film chip resistor", bin: "A11", unit: EACH, receive: 500, low_stock_threshold: None,      usage: "usually_consumed", attrs: &[("resistance", "22k"),   ("tolerance", "1%"), ("power_rating", "1/4W"), ("package", "0603"), ("mounting_style", "SMD")] },
    SeedPart { category: "Resistor", name: "47k 0603 1% resistor",   description: "Thick film chip resistor", bin: "A12", unit: EACH, receive: 500, low_stock_threshold: None,      usage: "usually_consumed", attrs: &[("resistance", "47k"),   ("tolerance", "1%"), ("power_rating", "1/4W"), ("package", "0603"), ("mounting_style", "SMD")] },
];

#[rustfmt::skip]
const CAPACITORS: &[SeedPart] = &[
    SeedPart { category: "Capacitor", name: "22pF 0603 C0G capacitor",       description: "Crystal load capacitor", bin: "B1", unit: EACH, receive: 200, low_stock_threshold: None, usage: "usually_consumed", attrs: &[("capacitance", "22pF"),  ("dielectric", "C0G/NP0"), ("voltage_rating", "50V"), ("package", "0603"), ("mounting_style", "SMD")] },
    SeedPart { category: "Capacitor", name: "1nF 0603 C0G capacitor",        description: "General purpose",        bin: "B2", unit: EACH, receive: 200, low_stock_threshold: None, usage: "usually_consumed", attrs: &[("capacitance", "1nF"),   ("dielectric", "C0G/NP0"), ("voltage_rating", "50V"), ("package", "0603"), ("mounting_style", "SMD")] },
    SeedPart { category: "Capacitor", name: "100nF 0603 X7R capacitor",      description: "Decoupling capacitor",   bin: "B3", unit: EACH, receive: 500, low_stock_threshold: Some(200), usage: "usually_consumed", attrs: &[("capacitance", "100nF"), ("dielectric", "X7R"), ("voltage_rating", "25V"), ("package", "0603"), ("mounting_style", "SMD")] },
    SeedPart { category: "Capacitor", name: "1uF 0805 X5R capacitor",        description: "Bulk decoupling",        bin: "B4", unit: EACH, receive: 150, low_stock_threshold: None, usage: "usually_consumed", attrs: &[("capacitance", "1uF"),   ("dielectric", "X5R"), ("voltage_rating", "16V"), ("package", "0805"), ("mounting_style", "SMD")] },
    SeedPart { category: "Capacitor", name: "10uF 0805 X5R capacitor",       description: "Bulk decoupling",        bin: "B5", unit: EACH, receive: 100, low_stock_threshold: None, usage: "usually_consumed", attrs: &[("capacitance", "10uF"),  ("dielectric", "X5R"), ("voltage_rating", "16V"), ("package", "0805"), ("mounting_style", "SMD")] },
    SeedPart { category: "Capacitor", name: "100uF radial electrolytic capacitor", description: "Bulk filtering",   bin: "B6", unit: EACH, receive: 40,  low_stock_threshold: None, usage: "usually_consumed", attrs: &[("capacitance", "100uF"), ("dielectric", "Aluminum electrolytic"), ("voltage_rating", "25V"), ("package", "radial 5mm"), ("mounting_style", "THT")] },
];

#[rustfmt::skip]
const LEDS: &[SeedPart] = &[
    SeedPart { category: "LED", name: "5mm red LED",   description: "Standard-brightness indicator LED", bin: "C1", unit: EACH, receive: 100, low_stock_threshold: Some(20), usage: "usually_consumed", attrs: &[("led_color", "Red"),   ("forward_voltage", "2V"),   ("forward_current", "20mA"), ("package", "5mm"), ("mounting_style", "THT")] },
    SeedPart { category: "LED", name: "5mm green LED", description: "Standard-brightness indicator LED", bin: "C2", unit: EACH, receive: 100, low_stock_threshold: None,     usage: "usually_consumed", attrs: &[("led_color", "Green"), ("forward_voltage", "2.1V"), ("forward_current", "20mA"), ("package", "5mm"), ("mounting_style", "THT")] },
    SeedPart { category: "LED", name: "5mm blue LED",  description: "Standard-brightness indicator LED", bin: "C3", unit: EACH, receive: 60,  low_stock_threshold: None,     usage: "usually_consumed", attrs: &[("led_color", "Blue"),  ("forward_voltage", "3.2V"), ("forward_current", "20mA"), ("package", "5mm"), ("mounting_style", "THT")] },
    SeedPart { category: "LED", name: "5mm white LED", description: "Standard-brightness indicator LED", bin: "C4", unit: EACH, receive: 60,  low_stock_threshold: None,     usage: "usually_consumed", attrs: &[("led_color", "White"), ("forward_voltage", "3.2V"), ("forward_current", "20mA"), ("package", "5mm"), ("mounting_style", "THT")] },
];

#[rustfmt::skip]
const MOSFETS: &[SeedPart] = &[
    SeedPart { category: "MOSFET", name: "2N7000 N-channel small-signal MOSFET", description: "TO-92 logic-level switch",  bin: "D1", unit: EACH, receive: 40, low_stock_threshold: None,     usage: "usually_consumed", attrs: &[("channel_type", "N-channel"), ("vds_max", "60V"),  ("id_continuous", "200mA"), ("package", "TO-92"),  ("logic_level", "true")] },
    SeedPart { category: "MOSFET", name: "IRLZ44N N-channel power MOSFET",       description: "TO-220 logic-level switch",  bin: "D2", unit: EACH, receive: 25, low_stock_threshold: None,     usage: "usually_consumed", attrs: &[("channel_type", "N-channel"), ("vds_max", "55V"),  ("id_continuous", "47A"),  ("package", "TO-220"), ("vgs_threshold", "1V..2V"), ("logic_level", "true")] },
    SeedPart { category: "MOSFET", name: "IRF9540 P-channel power MOSFET",       description: "TO-220 power switch",        bin: "D3", unit: EACH, receive: 4,  low_stock_threshold: Some(5), usage: "usually_consumed", attrs: &[("channel_type", "P-channel"), ("vds_max", "-100V"), ("id_continuous", "23A"),  ("package", "TO-220")] },
];

#[rustfmt::skip]
const CONNECTORS: &[SeedPart] = &[
    SeedPart { category: "Connector", name: "JST-PH 2-pin receptacle", description: "2mm pitch battery/board connector", bin: "E1", unit: EACH, receive: 60, low_stock_threshold: None, usage: "usually_consumed", attrs: &[("connector_family", "JST PH"), ("contact_count", "2"), ("pitch", "2mm"), ("gender", "Female"), ("mounting_style", "THT")] },
    SeedPart { category: "Connector", name: "JST-PH 4-pin receptacle", description: "2mm pitch signal connector",         bin: "E2", unit: EACH, receive: 40, low_stock_threshold: None, usage: "usually_consumed", attrs: &[("connector_family", "JST PH"), ("contact_count", "4"), ("pitch", "2mm"), ("gender", "Female"), ("mounting_style", "THT")] },
    SeedPart { category: "Header",    name: "0.1in header, 1x4 male",     description: "Breakaway pin header",             bin: "E3", unit: EACH, receive: 50, low_stock_threshold: None, usage: "usually_consumed", attrs: &[("contact_count", "4"),  ("rows", "1"), ("pitch", "0.1in"), ("gender", "Male"), ("mounting_style", "THT")] },
    SeedPart { category: "Header",    name: "0.1in header, 2x20 male",    description: "Breakaway pin header",             bin: "E4", unit: EACH, receive: 20, low_stock_threshold: None, usage: "usually_consumed", attrs: &[("contact_count", "40"), ("rows", "2"), ("pitch", "0.1in"), ("gender", "Male"), ("mounting_style", "THT")] },
];

#[rustfmt::skip]
const CRYSTALS: &[SeedPart] = &[
    SeedPart { category: "Crystal", name: "16MHz HC-49 crystal",       description: "Microcontroller clock crystal", bin: "F1", unit: EACH, receive: 30, low_stock_threshold: None,      usage: "usually_consumed", attrs: &[("frequency", "16MHz"), ("load_capacitance", "18pF"), ("package", "HC-49/US"), ("mounting_style", "THT")] },
    SeedPart { category: "Crystal", name: "32.768kHz watch crystal",   description: "Real-time clock crystal",       bin: "F2", unit: EACH, receive: 8,  low_stock_threshold: Some(10), usage: "usually_consumed", attrs: &[("frequency", "32.768kHz"), ("package", "3215"), ("mounting_style", "SMD")] },
];

#[rustfmt::skip]
const REGULATORS: &[SeedPart] = &[
    SeedPart { category: "Voltage regulator", name: "7805 5V linear regulator",        description: "TO-220 linear regulator", bin: "G1", unit: EACH, receive: 40, low_stock_threshold: None, usage: "usually_consumed", attrs: &[("regulator_type", "Linear standard"), ("output_voltage", "5V"),   ("output_current", "1A"), ("package", "TO-220")] },
    SeedPart { category: "Voltage regulator", name: "AMS1117-3.3 3.3V LDO regulator",  description: "SOT-223 LDO regulator",   bin: "G2", unit: EACH, receive: 40, low_stock_threshold: None, usage: "usually_consumed", attrs: &[("regulator_type", "Linear LDO"),      ("output_voltage", "3.3V"), ("output_current", "1A"), ("package", "SOT-223")] },
    SeedPart { category: "Voltage regulator", name: "LM317 adjustable linear regulator", description: "TO-220 adjustable regulator", bin: "G3", unit: EACH, receive: 20, low_stock_threshold: None, usage: "usually_consumed", attrs: &[("regulator_type", "Linear standard"), ("package", "TO-220")] },
];

#[rustfmt::skip]
const MECHANICAL: &[SeedPart] = &[
    SeedPart { category: "Screw",           name: "M3x6 socket head screw",     description: "Stainless, SHCS", bin: "H1", unit: EACH, receive: 200, low_stock_threshold: None, usage: "usually_consumed",     attrs: &[] },
    SeedPart { category: "Nut",             name: "M3 hex nut",                 description: "Stainless",       bin: "H2", unit: EACH, receive: 200, low_stock_threshold: None, usage: "usually_consumed",     attrs: &[] },
    SeedPart { category: "Standoff",        name: "M3x10 nylon standoff",       description: "PCB mounting standoff", bin: "H3", unit: EACH, receive: 60, low_stock_threshold: None, usage: "usually_consumed", attrs: &[] },
    SeedPart { category: "Enclosure",       name: "ABS project enclosure",      description: "100x60x25mm hinged box", bin: "H4", unit: EACH, receive: 10, low_stock_threshold: None, usage: "usually_consumed", attrs: &[] },
    SeedPart { category: "Breadboard",      name: "Half-size solderless breadboard", description: "400 tie-point breadboard", bin: "H5", unit: EACH, receive: 5, low_stock_threshold: None, usage: "usually_checked_out", attrs: &[] },
    SeedPart { category: "Battery",         name: "9V alkaline battery",        description: "PP3 alkaline battery", bin: "H6", unit: EACH, receive: 12, low_stock_threshold: None, usage: "usually_consumed", attrs: &[] },
    SeedPart { category: "Battery holder",  name: "9V battery clip",            description: "PP3 snap connector",   bin: "H7", unit: EACH, receive: 15, low_stock_threshold: None, usage: "usually_consumed", attrs: &[] },
    SeedPart { category: "Heatsink",        name: "TO-220 clip-on heatsink",    description: "Small extruded heatsink", bin: "H8", unit: EACH, receive: 25, low_stock_threshold: None, usage: "usually_consumed", attrs: &[] },
];

#[rustfmt::skip]
const CONTINUOUS: &[SeedPart] = &[
    SeedPart { category: "Wire",     name: "22AWG stranded hookup wire, red", description: "PVC-insulated stranded wire", bin: "W1", unit: QuantityUnit::Meter, receive: 25, low_stock_threshold: Some(5), usage: "usually_consumed", attrs: &[] },
    SeedPart { category: "__misc__", name: "3mm heat-shrink tubing, black",   description: "2:1 polyolefin heat-shrink",  bin: "W2", unit: QuantityUnit::Foot,  receive: 10, low_stock_threshold: None,    usage: "usually_consumed", attrs: &[] },
];

fn category_lookup(categories: &[crate::categories::CategoryRecord], name: &str) -> CategoryId {
    if name == "__misc__" {
        return CategoryId::from_string(MISC_CATEGORY_ID.to_string())
            .expect("MISC_CATEGORY_ID is a valid ULID");
    }
    categories
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("dev_seed: built-in category '{name}' not found — did seed.rs's built-in list change?"))
        .id
        .clone()
}

/// Create every part in `rows`, set its attributes, and receive its initial
/// stock. Returns the created parts' ids keyed by display name so later
/// steps (reservations, checkouts, variants, archival) can find them.
fn create_seed_parts(
    db: &mut Database,
    categories: &[crate::categories::CategoryRecord],
    rows: &[SeedPart],
    by_name: &mut HashMap<&'static str, PartId>,
) -> Result<u32, DbError> {
    let mut created = 0u32;
    for row in rows {
        let draft = PartDraft {
            display_name: row.name.to_string(),
            category_id: category_lookup(categories, row.category),
            description: row.description.to_string(),
            bin_label: Some(row.bin.to_string()),
            usage_behavior: row.usage.to_string(),
            quantity_unit: row.unit,
            low_stock_threshold: row
                .low_stock_threshold
                .map(Quantity::from_whole)
                .transpose()
                .expect("seed low_stock_threshold is a small non-negative whole number"),
            public_notes: String::new(),
            private_notes: String::new(),
        };
        let part = db.create_part(&draft)?;
        for (key, raw) in row.attrs {
            db.set_attribute(&part.id, key, raw)?;
        }
        if row.receive > 0 {
            db.apply(&LedgerOp::Receive {
                part_id: part.id.clone(),
                quantity: Quantity::from_whole(row.receive)
                    .expect("seed receive quantity is a small non-negative whole number"),
                note: "initial stock".to_string(),
            })?;
        }
        created += 1;
        by_name.insert(row.name, part.id);
    }
    Ok(created)
}

/// Populate a representative dataset for the desktop UI to render. No-ops
/// (returns `Ok(0)`) if the database already has any part, so it is safe to
/// call on every dev launch.
pub fn run(db: &mut Database) -> Result<u32, DbError> {
    if !db.list_parts(true)?.is_empty() {
        return Ok(0);
    }

    let categories = db.list_categories()?;
    let mut by_name: HashMap<&'static str, PartId> = HashMap::new();
    let mut created = 0u32;

    for group in [
        RESISTORS, CAPACITORS, LEDS, MOSFETS, CONNECTORS, CRYSTALS, REGULATORS, MECHANICAL,
        CONTINUOUS,
    ] {
        created += create_seed_parts(db, &categories, group, &mut by_name)?;
    }

    // A DIP microcontroller and a reusable development board — neither
    // fits the flat table well (the board gets a variant, a listing, and
    // dimensions), so they're created directly.
    let mcu = db.create_part(&PartDraft {
        display_name: "ATmega328P-PU".to_string(),
        category_id: category_lookup(&categories, "Microcontroller"),
        description: "8-bit AVR microcontroller, DIP-28".to_string(),
        bin_label: Some("M1".to_string()),
        usage_behavior: "usually_consumed".to_string(),
        quantity_unit: EACH,
        low_stock_threshold: None,
        public_notes: String::new(),
        private_notes: String::new(),
    })?;
    db.set_attribute(&mcu.id, "package", "DIP-28")?;
    db.apply(&LedgerOp::Receive {
        part_id: mcu.id.clone(),
        quantity: Quantity::from_whole(10).unwrap(),
        note: "initial stock".to_string(),
    })?;
    created += 1;
    by_name.insert("ATmega328P-PU", mcu.id);

    let dev_board = db.create_part(&PartDraft {
        display_name: "Uno-style development board".to_string(),
        category_id: category_lookup(&categories, "Development board"),
        description: "ATmega328P development board, USB-B, 5V/3.3V rails".to_string(),
        bin_label: Some("M2".to_string()),
        usage_behavior: "usually_checked_out".to_string(),
        quantity_unit: EACH,
        low_stock_threshold: Some(Quantity::from_whole(1).unwrap()),
        public_notes: String::new(),
        private_notes: String::new(),
    })?;
    db.apply(&LedgerOp::Receive {
        part_id: dev_board.id.clone(),
        quantity: Quantity::from_whole(3).unwrap(),
        note: "initial stock".to_string(),
    })?;
    created += 1;
    let dev_board_variant = db.add_variant(
        &dev_board.id,
        &VariantDraft {
            manufacturer: "Generic".to_string(),
            mpn: "UNO-R3-CH340".to_string(),
            description: "ATmega328P board, CH340 USB-serial".to_string(),
            package: None,
            datasheet_url: None,
            product_url: None,
            lifecycle: Some("active".to_string()),
            notes: String::new(),
        },
    )?;
    db.set_preferred_variant(&dev_board.id, &dev_board_variant.id)?;
    db.add_supplier_listing(
        &dev_board_variant.id,
        &ListingDraft {
            supplier: "DigiKey".to_string(),
            supplier_sku: "1050-1024-ND".to_string(),
            product_url: None,
            packaging: None,
            typical_order: Some(Quantity::from_whole(1).unwrap()),
            last_unit_price_micros: Some(9_990_000),
            currency: Some("USD".to_string()),
            last_purchase_date: Some("2026-06-01".to_string()),
        },
    )?;
    for (name, raw_value) in [("Length", "68.6 mm"), ("Width", "53.4 mm")] {
        db.add_dimension(
            &dev_board.id,
            &DimensionDraft {
                group: DimensionGroup::Overall,
                name: name.to_string(),
                raw_value: raw_value.to_string(),
                source: DimensionSource::Manufacturer,
                notes: String::new(),
                measured_date: None,
            },
        )?;
    }
    by_name.insert("Uno-style development board", dev_board.id);

    // Dimensions for the enclosure created above.
    let enclosure_id = by_name["ABS project enclosure"].clone();
    for (name, raw_value) in [
        ("Length", "100 mm"),
        ("Width", "60 mm"),
        ("Height", "25 mm"),
    ] {
        db.add_dimension(
            &enclosure_id,
            &DimensionDraft {
                group: DimensionGroup::Overall,
                name: name.to_string(),
                raw_value: raw_value.to_string(),
                source: DimensionSource::Manufacturer,
                notes: String::new(),
                measured_date: None,
            },
        )?;
    }

    // A manufacturer variant + supplier listing on a common passive too —
    // most parts in a real library never get one, but plenty do.
    let resistor_10k = by_name["10k 0603 1% resistor"].clone();
    let resistor_variant = db.add_variant(
        &resistor_10k,
        &VariantDraft {
            manufacturer: "Yageo".to_string(),
            mpn: "RC0603FR-0710KL".to_string(),
            description: "10k 0603 1% thick film resistor".to_string(),
            package: Some("0603".to_string()),
            datasheet_url: None,
            product_url: None,
            lifecycle: Some("active".to_string()),
            notes: String::new(),
        },
    )?;
    db.set_preferred_variant(&resistor_10k, &resistor_variant.id)?;
    db.add_supplier_listing(
        &resistor_variant.id,
        &ListingDraft {
            supplier: "DigiKey".to_string(),
            supplier_sku: "311-10.0KLRCT-ND".to_string(),
            product_url: None,
            packaging: Some("Cut Tape".to_string()),
            typical_order: Some(Quantity::from_whole(100).unwrap()),
            last_unit_price_micros: Some(20_400),
            currency: Some("USD".to_string()),
            last_purchase_date: Some("2026-05-12".to_string()),
        },
    )?;

    // A couple of projects, with reservations and checkouts against them —
    // exercises the ledger states beyond plain "available" stock.
    let blinky = db.create_project("Blinky Board")?;
    let psu_rebuild = db.create_project("Bench PSU Rebuild")?;

    let reserve = |db: &mut Database,
                   name: &'static str,
                   quantity: i64,
                   project: &inventory_core::ids::ProjectId|
     -> Result<(), DbError> {
        db.apply(&LedgerOp::Reserve {
            part_id: by_name[name].clone(),
            quantity: Quantity::from_whole(quantity).unwrap(),
            project_id: project.clone(),
        })?;
        Ok(())
    };
    let check_out = |db: &mut Database,
                     name: &'static str,
                     quantity: i64,
                     project: &inventory_core::ids::ProjectId|
     -> Result<(), DbError> {
        db.apply(&LedgerOp::CheckOut {
            part_id: by_name[name].clone(),
            quantity: Quantity::from_whole(quantity).unwrap(),
            project_id: project.clone(),
        })?;
        Ok(())
    };

    reserve(db, "10k 0603 1% resistor", 50, &blinky)?;
    reserve(db, "5mm red LED", 10, &blinky)?;
    check_out(db, "5mm red LED", 5, &blinky)?;
    check_out(db, "Uno-style development board", 1, &blinky)?;
    reserve(db, "7805 5V linear regulator", 4, &psu_rebuild)?;
    check_out(db, "TO-220 clip-on heatsink", 2, &psu_rebuild)?;
    reserve(db, "M3x6 socket head screw", 20, &psu_rebuild)?;

    // One archived part — no longer stocked, kept for reference.
    db.set_part_archived(&by_name["100uF radial electrolytic capacitor"], true)?;

    // A grouped receive — mirrors a multi-line supplier order landing at
    // once (the shape Phase 5's import will produce via `apply_group`), so
    // the History screen (Phase 3 Task 9) has a real group to expand and
    // reverse against seeded data rather than only single ungrouped rows.
    db.apply_group(
        "receive_batch",
        "DigiKey order DK-2026-0611 arrived",
        &[
            LedgerOp::Receive {
                part_id: by_name["100nF 0603 X7R capacitor"].clone(),
                quantity: Quantity::from_whole(200).unwrap(),
                note: "restock".to_string(),
            },
            LedgerOp::Receive {
                part_id: by_name["1uF 0805 X5R capacitor"].clone(),
                quantity: Quantity::from_whole(100).unwrap(),
                note: "restock".to_string(),
            },
        ],
    )?;

    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `TempDir` must outlive `Database` (whose `rusqlite::Connection`
    /// holds an open handle into it) and drops in reverse declaration
    /// order relative to it, so callers destructure both and keep them in
    /// scope together — returning only the `Database` would drop the
    /// tempdir (and try to remove the still-open sqlite file) as soon as
    /// this function returns.
    fn open_test_db() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let db =
            Database::open_and_migrate(&dir.path().join("dev.sqlite"), &dir.path().join("backups"))
                .unwrap();
        (dir, db)
    }

    #[test]
    fn seeds_a_representative_dataset() {
        let (_dir, mut db) = open_test_db();
        let created = run(&mut db).unwrap();
        assert!(created >= 40, "expected at least 40 parts, got {created}");

        let parts = db.list_parts(true).unwrap();
        assert_eq!(parts.len() as u32, created);

        // At least one archived part.
        assert!(parts.iter().any(|p| p.archived));
        // At least one part with a low-stock threshold set.
        assert!(parts.iter().any(|p| p.low_stock_threshold.is_some()));
        // Both continuous units are represented.
        assert!(parts.iter().any(|p| p.quantity_unit == QuantityUnit::Meter));
        assert!(parts.iter().any(|p| p.quantity_unit == QuantityUnit::Foot));

        // Some part has stock movement beyond plain "available".
        let any_reserved_or_checked_out = parts.iter().any(|p| {
            let stock = db.get_stock(&p.id).unwrap();
            stock.reserved.as_milli() > 0 || stock.checked_out.as_milli() > 0
        });
        assert!(any_reserved_or_checked_out);
    }

    #[test]
    fn is_idempotent_when_parts_already_exist() {
        let (_dir, mut db) = open_test_db();
        let first = run(&mut db).unwrap();
        assert!(first > 0);
        let second = run(&mut db).unwrap();
        assert_eq!(second, 0);
        // No duplicate rows: the count didn't change on the second call.
        let parts = db.list_parts(true).unwrap();
        assert_eq!(parts.len() as u32, first);
    }

    #[test]
    fn every_part_has_stock_row_and_valid_category() {
        let (_dir, mut db) = open_test_db();
        run(&mut db).unwrap();
        let categories: std::collections::HashSet<_> = db
            .list_categories()
            .unwrap()
            .into_iter()
            .map(|c| c.id)
            .collect();
        for part in db.list_parts(true).unwrap() {
            assert!(categories.contains(&part.category_id));
            // get_stock succeeds for every created part (a part_stock row exists).
            db.get_stock(&part.id).unwrap();
        }
    }
}
