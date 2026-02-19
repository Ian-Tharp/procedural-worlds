//! Structure generation — procedural placement of small buildings and landmarks
//!
//! Provides a template-based system for placing predetermined structures
//! (huts, wells, towers, etc.) into chunks during terrain generation.
//! Placement is fully deterministic given the same world seed and chunk position.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use std::sync::LazyLock;

use bevy::prelude::*;

use crate::world::{BlockType, CHUNK_SIZE};
use super::biome::{biome_at, BiomeType};
use super::TerrainConfig;
use crate::world::Chunk;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StructureType {
    Hut, Tower, Well, Ruin, Shrine, CaveEntrance, WatchPost, Campsite,
}

impl StructureType {
    pub fn all() -> &'static [StructureType] {
        &[Self::Hut, Self::Tower, Self::Well, Self::Ruin, Self::Shrine, Self::CaveEntrance, Self::WatchPost, Self::Campsite]
    }

    fn allowed_biomes(&self) -> &'static [BiomeType] {
        match self {
            Self::Hut | Self::Campsite => &[BiomeType::Plains, BiomeType::Forest],
            Self::Tower | Self::WatchPost => &[BiomeType::Mountains, BiomeType::Plains],
            Self::Well => &[BiomeType::Plains, BiomeType::Desert],
            Self::Ruin => &[BiomeType::Plains, BiomeType::Desert, BiomeType::Forest, BiomeType::Mountains, BiomeType::Tundra, BiomeType::Volcanic],
            Self::Shrine => &[BiomeType::Forest, BiomeType::Mountains],
            Self::CaveEntrance => &[BiomeType::Mountains, BiomeType::Tundra],
        }
    }
}

pub struct StructureTemplate {
    pub size: (usize, usize, usize),
    pub blocks: Vec<Option<BlockType>>,
}

impl StructureTemplate {
    fn new(sx: usize, sy: usize, sz: usize) -> Self {
        Self { size: (sx, sy, sz), blocks: vec![None; sx * sy * sz] }
    }
    fn set(&mut self, x: usize, y: usize, z: usize, block: BlockType) {
        let (sx, sy, _) = self.size;
        self.blocks[x + y * sx + z * sx * sy] = Some(block);
    }
    fn get(&self, x: usize, y: usize, z: usize) -> Option<BlockType> {
        let (sx, sy, _) = self.size;
        self.blocks[x + y * sx + z * sx * sy]
    }
    pub fn block_count(&self) -> usize {
        self.blocks.iter().filter(|b| b.is_some()).count()
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct StructurePlacement {
    pub structure_type: StructureType,
    pub offset: IVec3,
    pub rotation: u8,
}

pub fn build_templates() -> HashMap<StructureType, StructureTemplate> {
    let mut m = HashMap::new();
    m.insert(StructureType::Hut, build_hut());
    m.insert(StructureType::Well, build_well());
    m.insert(StructureType::Tower, build_tower());
    m.insert(StructureType::Ruin, build_ruin());
    m.insert(StructureType::Shrine, build_shrine());
    m.insert(StructureType::CaveEntrance, build_cave_entrance());
    m.insert(StructureType::WatchPost, build_watch_post());
    m.insert(StructureType::Campsite, build_campsite());
    m
}

fn build_hut() -> StructureTemplate {
    let (sx, sy, sz) = (5, 4, 5);
    let mut t = StructureTemplate::new(sx, sy, sz);
    for x in 0..sx { for z in 0..sz { t.set(x, 0, z, BlockType::Stone); } }
    for y in 1..=2 { for x in 0..sx { for z in 0..sz {
        let edge = x == 0 || x == sx-1 || z == 0 || z == sz-1;
        if edge { if x == 0 && z == 2 { t.set(x,y,z,BlockType::Air); } else { t.set(x,y,z,BlockType::Wood); } }
        else { t.set(x,y,z,BlockType::Air); }
    }}}
    for x in 0..sx { for z in 0..sz { t.set(x, 3, z, BlockType::Wood); } }
    t
}

fn build_well() -> StructureTemplate {
    let (sx, sy, sz) = (3, 5, 3);
    let mut t = StructureTemplate::new(sx, sy, sz);
    t.set(1, 0, 1, BlockType::Water);
    for x in 0..sx { for z in 0..sz { if !(x==1&&z==1) { t.set(x,0,z,BlockType::Stone); } } }
    for y in 1..=2 { for x in 0..sx { for z in 0..sz {
        if x==1&&z==1 { t.set(x,y,z,BlockType::Air); } else { t.set(x,y,z,BlockType::Stone); }
    }}}
    for x in 0..sx { for z in 0..sz { if x==1&&z==1 { t.set(x,3,z,BlockType::Air); } else { t.set(x,3,z,BlockType::Stone); } } }
    for x in 0..sx { for z in 0..sz { t.set(x,4,z,BlockType::Air); } }
    t
}

fn build_tower() -> StructureTemplate {
    let (sx, sy, sz) = (5, 8, 5);
    let mut t = StructureTemplate::new(sx, sy, sz);
    for y in 0..sy { for x in 0..sx { for z in 0..sz {
        let edge = x==0||x==sx-1||z==0||z==sz-1;
        if y==0||y==4 { if x==1&&z==1 { t.set(x,y,z,BlockType::Wood); } else { t.set(x,y,z,BlockType::Stone); } }
        else if y==7 { if edge { t.set(x,y,z,BlockType::Stone); } else { t.set(x,y,z,BlockType::Air); } }
        else if edge { if x==0&&z==2&&(y==1||y==2) { t.set(x,y,z,BlockType::Air); } else { t.set(x,y,z,BlockType::Stone); } }
        else if x==1&&z==1 { t.set(x,y,z,BlockType::Wood); }
        else { t.set(x,y,z,BlockType::Air); }
    }}}
    t
}

fn build_ruin() -> StructureTemplate {
    let (sx, sy, sz) = (7, 4, 7);
    let mut t = StructureTemplate::new(sx, sy, sz);
    for x in 0..sx { for z in 0..sz { t.set(x,0,z,BlockType::Stone); } }
    for y in 1..=3 { for x in 0..sx { for z in 0..sz {
        let edge = x==0||x==sx-1||z==0||z==sz-1;
        if edge { if ((x*7+z*13+y*3)%5)<2 { t.set(x,y,z,BlockType::Air); } else { t.set(x,y,z,BlockType::Stone); } }
        else if y==1&&((x+z)%4==0) { t.set(x,y,z,BlockType::Stone); }
        else { t.set(x,y,z,BlockType::Air); }
    }}}
    t
}

fn build_shrine() -> StructureTemplate {
    let mut t = StructureTemplate::new(3, 3, 3);
    for x in 0..3 { for z in 0..3 { t.set(x,0,z,BlockType::Stone); } }
    t.set(1,1,1,BlockType::Stone);
    t.set(1,2,1,BlockType::GoldOre);
    t
}

fn build_cave_entrance() -> StructureTemplate {
    let (sx, sy, sz) = (5, 5, 5);
    let mut t = StructureTemplate::new(sx, sy, sz);
    for y in 0..sy { for x in 0..sx { for z in 0..sz {
        let edge = x==0||x==sx-1||z==sz-1||y==sy-1;
        if z==0&&y<3&&x>=1&&x<=3 { t.set(x,y,z,BlockType::Air); }
        else if edge { t.set(x,y,z,BlockType::Stone); }
        else if y<3 { t.set(x,y,z,BlockType::Air); }
        else { t.set(x,y,z,BlockType::Stone); }
    }}}
    t
}

fn build_watch_post() -> StructureTemplate {
    let (sx, sy, sz) = (3, 6, 3);
    let mut t = StructureTemplate::new(sx, sy, sz);
    for y in 0..4 { t.set(1,y,1,BlockType::Wood); }
    for x in 0..sx { for z in 0..sz { t.set(x,4,z,BlockType::Wood); } }
    for x in 0..sx { for z in 0..sz {
        if x==0||x==sx-1||z==0||z==sz-1 { t.set(x,5,z,BlockType::Wood); } else { t.set(x,5,z,BlockType::Air); }
    }}
    t
}

fn build_campsite() -> StructureTemplate {
    let (sx, sy, sz) = (5, 2, 5);
    let mut t = StructureTemplate::new(sx, sy, sz);
    for &(x,z) in &[(1,1),(1,2),(1,3),(2,1),(2,3),(3,1),(3,2),(3,3)] { t.set(x,0,z,BlockType::Stone); }
    t.set(2,0,2,BlockType::Stone);
    for &(x,z) in &[(0,0),(4,0),(0,4),(4,4)] { t.set(x,0,z,BlockType::Wood); }
    for x in 0..sx { for z in 0..sz { t.set(x,1,z,BlockType::Air); } }
    t
}

fn structure_hash(cx: i32, cz: i32, seed: u32) -> f64 {
    let mut h = DefaultHasher::new();
    "structure_placement".hash(&mut h); seed.hash(&mut h); cx.hash(&mut h); cz.hash(&mut h);
    (h.finish() as f64) / (u64::MAX as f64)
}

fn structure_type_hash(cx: i32, cz: i32, seed: u32) -> u64 {
    let mut h = DefaultHasher::new();
    "structure_type".hash(&mut h); seed.hash(&mut h); cx.hash(&mut h); cz.hash(&mut h);
    h.finish()
}

fn structure_position_hash(cx: i32, cz: i32, seed: u32, axis: &str) -> u64 {
    let mut h = DefaultHasher::new();
    "structure_pos".hash(&mut h); axis.hash(&mut h); seed.hash(&mut h); cx.hash(&mut h); cz.hash(&mut h);
    h.finish()
}

pub fn generate_structures(chunk: &mut Chunk, config: &TerrainConfig) {
    let world_pos = chunk.world_position();
    let (cx, cz) = (chunk.position.x, chunk.position.z);

    if structure_hash(cx, cz, config.seed) > 0.12 { return; }

    static TEMPLATES: LazyLock<HashMap<StructureType, StructureTemplate>> = LazyLock::new(build_templates);
    let templates = &*TEMPLATES;
    let biome_noise = noise::Simplex::new(config.seed.wrapping_add(config.biome_seed_offset));
    let biome = biome_at(world_pos.x + CHUNK_SIZE as i32 / 2, world_pos.z + CHUNK_SIZE as i32 / 2, &biome_noise, config.biome_scale);

    let allowed: Vec<_> = StructureType::all().iter().copied().filter(|st| st.allowed_biomes().contains(&biome)).collect();
    if allowed.is_empty() { return; }

    let st = allowed[structure_type_hash(cx, cz, config.seed) as usize % allowed.len()];
    let template = match templates.get(&st) { Some(t) => t, None => return };
    let (sx, sy, sz) = template.size;

    let (max_x, max_z) = (CHUNK_SIZE.saturating_sub(sx), CHUNK_SIZE.saturating_sub(sz));
    if max_x == 0 || max_z == 0 { return; }

    let lx = (structure_position_hash(cx,cz,config.seed,"x") as usize % max_x).max(1);
    let lz = (structure_position_hash(cx,cz,config.seed,"z") as usize % max_z).max(1);

    let mut min_h = i32::MAX; let mut max_h = i32::MIN; let mut surf_y: Option<usize> = None;
    for &(dx,dz) in &[(0,0),(sx-1,0),(0,sz-1),(sx-1,sz-1)] {
        let (px, pz) = (lx+dx, lz+dz);
        if px >= CHUNK_SIZE || pz >= CHUNK_SIZE { return; }
        for ly in (0..CHUNK_SIZE).rev() {
            let b = chunk.get_block(px, ly, pz);
            if b != BlockType::Air && b != BlockType::Water {
                let h = world_pos.y + ly as i32;
                min_h = min_h.min(h); max_h = max_h.max(h);
                if surf_y.is_none() { surf_y = Some(ly); }
                break;
            }
        }
    }

    if min_h == i32::MAX || max_h == i32::MIN { return; }
    if max_h - min_h > 2 { return; }
    let base_y = match surf_y { Some(y) => y, None => return };
    if base_y + sy >= CHUNK_SIZE { return; }

    for tz in 0..sz { for ty in 0..sy { for tx in 0..sx {
        if let Some(block) = template.get(tx, ty, tz) {
            let (bx,by,bz) = (lx+tx, base_y+ty, lz+tz);
            if bx < CHUNK_SIZE && by < CHUNK_SIZE && bz < CHUNK_SIZE {
                chunk.set_block(bx, by, bz, block);
            }
        }
    }}}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_templates_valid() {
        let templates = build_templates();
        for st in StructureType::all() {
            let t = templates.get(st).unwrap_or_else(|| panic!("{:?} missing", st));
            assert!(t.block_count() > 0, "{:?} has zero blocks", st);
        }
    }

    #[test]
    fn test_template_dimensions() {
        for (st, t) in &build_templates() {
            let (sx, sy, sz) = t.size;
            assert!(sx <= CHUNK_SIZE && sy <= CHUNK_SIZE && sz <= CHUNK_SIZE, "{:?} too large: {}×{}×{}", st, sx, sy, sz);
        }
    }

    #[test]
    fn test_hut_has_interior() {
        let hut = build_templates().remove(&StructureType::Hut).unwrap();
        let (sx, _, sz) = hut.size;
        let air: usize = (1..=2).flat_map(|y| (1..sx-1).flat_map(move |x| (1..sz-1).map(move |z| (x,y,z))))
            .filter(|&(x,y,z)| hut.get(x,y,z) == Some(BlockType::Air)).count();
        assert!(air >= 6, "Interior air: {}", air);
    }

    #[test]
    fn test_deterministic_placement() {
        let config = TerrainConfig::default();
        let mut c1 = Chunk::new(IVec3::new(0,2,0));
        let mut c2 = Chunk::new(IVec3::new(0,2,0));
        crate::generation::generate_chunk_terrain(&mut c1, &config);
        crate::generation::generate_chunk_terrain(&mut c2, &config);
        generate_structures(&mut c1, &config);
        generate_structures(&mut c2, &config);
        for x in 0..CHUNK_SIZE { for y in 0..CHUNK_SIZE { for z in 0..CHUNK_SIZE {
            assert_eq!(c1.get_block(x,y,z), c2.get_block(x,y,z), "Mismatch at ({},{},{})", x, y, z);
        }}}
    }

    #[test]
    fn test_structure_density() {
        let seed = TerrainConfig::default().seed;
        let n: usize = (0..100).filter(|&i| structure_hash(i%10-5, i/10-5, seed) <= 0.12).count();
        assert!(n >= 5 && n <= 25, "Expected ~12, got {}", n);
    }
}
