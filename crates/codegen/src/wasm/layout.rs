//! ADT and Record memory layout computation.

use kyokara_hir_def::item_tree::{ItemTree, TypeDefKind, TypeItemIdx};
use kyokara_hir_def::name::Name;
use kyokara_intern::Interner;
use rustc_hash::FxHashMap;

/// Precomputed memory layout for an ADT type.
#[derive(Debug, Clone)]
pub struct AdtLayout {
    /// Total allocation size in bytes (8 + max_fields * 8).
    /// The first 8 bytes: [tag: i32][pad: 4 bytes].
    pub size: u32,
    /// Variant name -> tag value (0-based).
    pub tag_map: FxHashMap<Name, u32>,
    /// Variant name -> number of fields.
    pub variant_field_counts: FxHashMap<Name, u32>,
}

/// Field offset within an ADT: tag at 0, pad at 4, fields at 8, 16, 24, ...
pub const ADT_HEADER_SIZE: u32 = 8;
pub const FIELD_SIZE: u32 = 8;

impl AdtLayout {
    /// Byte offset of the i-th field in an ADT variant.
    pub fn field_offset(field_index: u32) -> u32 {
        ADT_HEADER_SIZE + field_index * FIELD_SIZE
    }
}

/// Compute layouts for all ADT types in the item tree.
pub fn compute_adt_layouts(
    item_tree: &ItemTree,
    _interner: &Interner,
) -> FxHashMap<TypeItemIdx, AdtLayout> {
    let mut layouts = FxHashMap::default();

    for (idx, type_item) in item_tree.types.iter() {
        if let TypeDefKind::Adt { variants } = &type_item.kind {
            let mut tag_map = FxHashMap::default();
            let mut variant_field_counts = FxHashMap::default();
            let mut max_fields: u32 = 0;

            for (i, variant) in variants.iter().enumerate() {
                tag_map.insert(variant.name, i as u32);
                let field_count = variant.fields.len() as u32;
                variant_field_counts.insert(variant.name, field_count);
                max_fields = max_fields.max(field_count);
            }

            let size = ADT_HEADER_SIZE + max_fields * FIELD_SIZE;

            layouts.insert(
                idx,
                AdtLayout {
                    size,
                    tag_map,
                    variant_field_counts,
                },
            );
        }
    }

    layouts
}

/// Compute byte size for a record with `field_count` fields.
/// Records have no tag header, just packed 8-byte fields sorted by name.
pub fn record_size(field_count: u32) -> u32 {
    field_count * FIELD_SIZE
}

/// Byte offset for the i-th field in a record (fields sorted by name).
pub fn record_field_offset(field_index: u32) -> u32 {
    field_index * FIELD_SIZE
}
