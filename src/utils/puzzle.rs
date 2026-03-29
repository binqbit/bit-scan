use std::{collections::HashMap, sync::OnceLock};

const PUZZLE_ADDRESSES_CSV: &str = include_str!("../../config/puzzle_addresses.csv");

static PUZZLE_MAP: OnceLock<HashMap<u32, &'static str>> = OnceLock::new();

fn build_puzzle_map() -> HashMap<u32, &'static str> {
    let mut map = HashMap::new();
    for line in PUZZLE_ADDRESSES_CSV.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.splitn(2, ',');
        let Some(idx_str) = parts.next() else {
            continue;
        };
        let Some(addr_str) = parts.next() else {
            continue;
        };
        if let Ok(index) = idx_str.trim().parse::<u32>() {
            let address = addr_str.trim();
            if !address.is_empty() {
                map.entry(index).or_insert(address);
            }
        }
    }
    map
}

fn puzzle_map() -> &'static HashMap<u32, &'static str> {
    PUZZLE_MAP.get_or_init(build_puzzle_map)
}

pub fn warm_puzzle_cache() {
    let _ = puzzle_map();
}

pub fn puzzle_address_for(number: u32) -> Option<&'static str> {
    puzzle_map().get(&number).copied()
}

pub fn puzzle_numbers() -> Vec<(u32, &'static str)> {
    let mut entries: Vec<_> = puzzle_map()
        .iter()
        .map(|(&number, &address)| (number, address))
        .collect();
    entries.sort_unstable_by_key(|&(number, _)| number);
    entries
}

pub struct ResolvedTarget {
    pub address: String,
    pub suggested_bits: Option<u32>,
}

pub fn resolve_target(input: &str) -> Result<ResolvedTarget, String> {
    if let Ok(number) = input.parse::<u32>() {
        if let Some(address) = puzzle_address_for(number) {
            let bits = if (1..=128).contains(&number) {
                Some(number)
            } else {
                None
            };
            return Ok(ResolvedTarget {
                address: address.to_owned(),
                suggested_bits: bits,
            });
        }
        return Err(format!("Unknown puzzle number {number}"));
    }
    Ok(ResolvedTarget {
        address: input.to_owned(),
        suggested_bits: None,
    })
}
