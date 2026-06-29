use crate::parser::bvh::WallHit;

/// Surface type indices — must match what parser assigns
pub const SURFACE_CONCRETE: u8 = 0;
pub const SURFACE_METAL: u8 = 1;
pub const SURFACE_SOLIDMETAL: u8 = 2;
pub const SURFACE_WOOD: u8 = 3;
pub const SURFACE_WOOD_PANEL: u8 = 4;
pub const SURFACE_WOOD_DENSE: u8 = 5;
pub const SURFACE_GLASS: u8 = 6;
pub const SURFACE_ROCK: u8 = 7;
pub const SURFACE_DIRT: u8 = 8;
pub const SURFACE_SAND: u8 = 9;
pub const SURFACE_CHAINLINK: u8 = 10;
pub const SURFACE_METALVENT: u8 = 11;
pub const SURFACE_METALPANEL: u8 = 12;
pub const SURFACE_METALVEHICLE: u8 = 13;
pub const SURFACE_TILE: u8 = 14;
pub const SURFACE_CARPET: u8 = 15;
pub const SURFACE_DEFAULT: u8 = 16;
pub const SURFACE_GRATE: u8 = 17;
pub const SURFACE_PLASTIC: u8 = 18;

/// Penetration modifier per surface type.
/// Higher = easier to penetrate. 0.0 = impenetrable.
/// Based on CS2 surface properties and CSGO source leak values.
pub fn surface_penetration_modifier(surface_index: u8) -> f32 {
    match surface_index {
        SURFACE_CONCRETE => 0.5,
        SURFACE_METAL => 0.5,
        SURFACE_SOLIDMETAL => 0.1, // very hard to penetrate
        SURFACE_WOOD => 1.0,
        SURFACE_WOOD_PANEL => 1.0,
        SURFACE_WOOD_DENSE => 0.6,
        SURFACE_GLASS => 1.5,
        SURFACE_ROCK => 0.4,
        SURFACE_DIRT => 0.5,
        SURFACE_SAND => 0.5,
        SURFACE_CHAINLINK => 2.0, // very easy
        SURFACE_METALVENT => 1.2,
        SURFACE_METALPANEL => 0.6,
        SURFACE_METALVEHICLE => 0.4,
        SURFACE_TILE => 0.5,
        SURFACE_CARPET => 0.8,
        SURFACE_GRATE => 1.5,
        SURFACE_PLASTIC => 1.0,
        SURFACE_DEFAULT | _ => 0.5,
    }
}

/// Damage modifier per surface type (how much damage is lost per unit thickness).
pub fn surface_damage_modifier(surface_index: u8) -> f32 {
    match surface_index {
        SURFACE_CONCRETE => 0.7,
        SURFACE_METAL => 0.7,
        SURFACE_SOLIDMETAL => 0.9,
        SURFACE_WOOD => 0.4,
        SURFACE_WOOD_PANEL => 0.3,
        SURFACE_WOOD_DENSE => 0.6,
        SURFACE_GLASS => 0.2,
        SURFACE_ROCK => 0.8,
        SURFACE_DIRT => 0.5,
        SURFACE_SAND => 0.5,
        SURFACE_CHAINLINK => 0.1,
        SURFACE_METALVENT => 0.3,
        SURFACE_METALPANEL => 0.5,
        SURFACE_METALVEHICLE => 0.7,
        SURFACE_TILE => 0.6,
        SURFACE_CARPET => 0.3,
        SURFACE_GRATE => 0.1,
        SURFACE_PLASTIC => 0.3,
        SURFACE_DEFAULT | _ => 0.5,
    }
}

/// Weapon penetration power from items_game.txt
pub fn weapon_penetration_power(weapon_name: &str) -> f32 {
    match weapon_name {
        // Snipers: 2.5
        "awp" | "g3sg1" | "scar20" | "ssg08" => 2.5,
        // Rifles + heavy pistols + LMGs: 2.0
        "ak47" | "aug" | "famas" | "galilar" | "m4a1" | "m4a1_silencer" | "m4a1_silencer_off"
        | "sg556" | "deagle" | "revolver" | "m249" | "negev" => 2.0,
        // Pistols + SMGs + Shotguns: 1.0
        "glock" | "hkp2000" | "usp_silencer" | "usp_silencer_off" | "p250" | "cz75a" | "tec9"
        | "fiveseven" | "elite" | "bizon" | "mac10" | "mp5sd" | "mp7" | "mp9" | "p90"
        | "ump45" | "mag7" | "nova" | "sawedoff" | "xm1014" => 1.0,
        // Taser/utility: 0
        "taser" => 0.0,
        // Unknown: assume 1.0
        _ => 1.0,
    }
}

/// Weapon base damage values from CS2
pub fn weapon_base_damage(weapon_name: &str) -> f32 {
    match weapon_name {
        "awp" => 115.0,
        "g3sg1" | "scar20" => 80.0,
        "ssg08" => 88.0,
        "ak47" => 36.0,
        "m4a1" | "m4a1_silencer" => 33.0,
        "aug" => 28.0,
        "sg556" => 30.0,
        "famas" => 26.0,
        "galilar" => 30.0,
        "deagle" => 63.0,
        "revolver" => 86.0,
        "m249" => 32.0,
        "negev" => 35.0,
        "glock" => 30.0,
        "hkp2000" | "usp_silencer" => 35.0,
        "p250" => 38.0,
        "cz75a" | "tec9" | "fiveseven" => 33.0,
        "elite" => 36.0,
        "bizon" => 27.0,
        "mac10" => 29.0,
        "mp5sd" => 27.0,
        "mp7" => 29.0,
        "mp9" => 26.0,
        "p90" => 26.0,
        "ump45" => 35.0,
        "mag7" => 30.0,
        "nova" => 26.0,
        "sawedoff" => 32.0,
        "xm1014" => 20.0,
        _ => 30.0,
    }
}

/// Result of penetration calculation
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PenetrationResult {
    pub can_penetrate: bool,
    pub remaining_damage_ratio: f32, // 0.0 - 1.0
    pub total_thickness: f32,
    pub wall_count: usize,
}

/// Calculate if a bullet can penetrate through the given walls.
/// Returns penetration result with remaining damage ratio.
pub fn can_penetrate_walls(
    walls: &[WallHit],
    weapon_name: &str,
    base_damage: f32,
    min_damage: f32,
) -> PenetrationResult {
    let weapon_pen = weapon_penetration_power(weapon_name);

    if weapon_pen <= 0.0 || walls.is_empty() {
        return PenetrationResult {
            can_penetrate: walls.is_empty(),
            remaining_damage_ratio: if walls.is_empty() { 1.0 } else { 0.0 },
            total_thickness: 0.0,
            wall_count: walls.len(),
        };
    }

    let mut remaining_damage = base_damage;
    let mut total_thickness = 0.0;

    for wall in walls {
        let pen_mod = surface_penetration_modifier(wall.surface_index);
        let dmg_mod = surface_damage_modifier(wall.surface_index);

        // Maximum thickness this weapon can penetrate through this surface
        let max_thickness = weapon_pen * pen_mod * 32.0;

        if wall.thickness >= max_thickness {
            return PenetrationResult {
                can_penetrate: false,
                remaining_damage_ratio: 0.0,
                total_thickness: total_thickness + wall.thickness,
                wall_count: walls.len(),
            };
        }

        // Damage loss through this wall
        let damage_loss = (wall.thickness / max_thickness) * dmg_mod * remaining_damage;
        remaining_damage -= damage_loss;
        total_thickness += wall.thickness;

        if remaining_damage < min_damage {
            return PenetrationResult {
                can_penetrate: false,
                remaining_damage_ratio: remaining_damage / base_damage,
                total_thickness,
                wall_count: walls.len(),
            };
        }
    }

    PenetrationResult {
        can_penetrate: true,
        remaining_damage_ratio: remaining_damage / base_damage,
        total_thickness,
        wall_count: walls.len(),
    }
}

/// Map surface property name (from vmdl) to surface index
pub fn surface_name_to_index(name: &str) -> u8 {
    let lower = name.to_lowercase();
    if lower.starts_with("concrete") || lower == "asphalt" || lower == "porcelain" {
        SURFACE_CONCRETE
    } else if lower == "solidmetal" {
        SURFACE_SOLIDMETAL
    } else if lower.starts_with("metal_sand") || lower.starts_with("metalvehicle") || lower == "metal_vehiclesoundoverride" {
        SURFACE_METALVEHICLE
    } else if lower == "metalvent" {
        SURFACE_METALVENT
    } else if lower == "metalpanel" {
        SURFACE_METALPANEL
    } else if lower.starts_with("metalgrate") || lower == "grate" {
        SURFACE_GRATE
    } else if lower.starts_with("metal") {
        SURFACE_METAL
    } else if lower == "wood_panel" {
        SURFACE_WOOD_PANEL
    } else if lower == "wood_dense" {
        SURFACE_WOOD_DENSE
    } else if lower.starts_with("wood") {
        SURFACE_WOOD
    } else if lower.starts_with("glass") {
        SURFACE_GLASS
    } else if lower.starts_with("rock") {
        SURFACE_ROCK
    } else if lower.starts_with("dirt") || lower == "mud" {
        SURFACE_DIRT
    } else if lower.starts_with("sand") {
        SURFACE_SAND
    } else if lower.starts_with("chainlink") {
        SURFACE_CHAINLINK
    } else if lower.starts_with("tile") {
        SURFACE_TILE
    } else if lower.starts_with("carpet") {
        SURFACE_CARPET
    } else if lower.starts_with("plastic") || lower == "rubber" || lower == "rubbertire" {
        SURFACE_PLASTIC
    } else {
        SURFACE_DEFAULT
    }
}
