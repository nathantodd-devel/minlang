//! Sizes and alignments of IR types, matching LLVM's x86-64 data layout. Used
//! to size the payload of enums whose variants carry data.

use std::collections::HashMap;

use super::ir::IrType;

/// Named struct types and their fields.
pub type NamedTypes = HashMap<String, Vec<IrType>>;

/// `(size, alignment)` in bytes.
pub fn size_align(ty: &IrType, named: &NamedTypes) -> (u64, u64) {
    match ty {
        IrType::Void | IrType::Token => (0, 1),
        IrType::Int(bits) => {
            let bytes = u64::from(bits.div_ceil(8)).next_power_of_two();
            (bytes, bytes)
        }
        IrType::Float => (4, 4),
        IrType::Double | IrType::Ptr => (8, 8),
        IrType::Array(count, element) => {
            let (size, align) = size_align(element, named);
            (size * count, align)
        }
        IrType::Struct(fields) => struct_size_align(fields, named),
        IrType::Named(name) => {
            let fields = named.get(name).map(Vec::as_slice).unwrap_or_default();
            struct_size_align(fields, named)
        }
    }
}

fn struct_size_align(fields: &[IrType], named: &NamedTypes) -> (u64, u64) {
    let mut offset: u64 = 0;
    let mut struct_align: u64 = 1;
    for field in fields {
        let (size, align) = size_align(field, named);
        offset = offset.next_multiple_of(align);
        offset += size;
        struct_align = struct_align.max(align);
    }
    (offset.next_multiple_of(struct_align), struct_align)
}

/// The type of an enum's payload: enough bytes, at enough alignment, to hold
/// any of the given variant payloads.
pub fn payload_type(variants: &[IrType], named: &NamedTypes) -> IrType {
    let (size, align) = variants
        .iter()
        .map(|variant| size_align(variant, named))
        .fold((0, 1), |(size, align), (variant_size, variant_align)| {
            (size.max(variant_size), align.max(variant_align))
        });
    let unit = IrType::Int(u32::try_from(align * 8).unwrap_or(64));
    IrType::Array(size.div_ceil(align), Box::new(unit))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(ty: &IrType) -> (u64, u64) {
        size_align(ty, &NamedTypes::new())
    }

    #[test]
    fn scalars() {
        assert_eq!(layout(&IrType::BOOL), (1, 1));
        assert_eq!(layout(&IrType::I8), (1, 1));
        assert_eq!(layout(&IrType::Int(16)), (2, 2));
        assert_eq!(layout(&IrType::I32), (4, 4));
        assert_eq!(layout(&IrType::I64), (8, 8));
        assert_eq!(layout(&IrType::Float), (4, 4));
        assert_eq!(layout(&IrType::Double), (8, 8));
        assert_eq!(layout(&IrType::Ptr), (8, 8));
    }

    #[test]
    fn structs_are_padded() {
        let ty = IrType::Struct(vec![IrType::I8, IrType::I32, IrType::I8]);
        assert_eq!(layout(&ty), (12, 4));
        let ty = IrType::Struct(vec![IrType::I8, IrType::Double]);
        assert_eq!(layout(&ty), (16, 8));
        assert_eq!(layout(&IrType::Struct(Vec::new())), (0, 1));
    }

    #[test]
    fn arrays_and_named_types() {
        let mut named = NamedTypes::new();
        named.insert("P".to_owned(), vec![IrType::Float, IrType::Float]);
        let ty = IrType::Array(3, Box::new(IrType::Named("P".to_owned())));
        assert_eq!(size_align(&ty, &named), (24, 4));
    }

    #[test]
    fn payloads_fit_every_variant() {
        let named = NamedTypes::new();
        let variants = [
            IrType::Struct(vec![IrType::Float]),
            IrType::Struct(vec![IrType::Float, IrType::Float]),
            IrType::Struct(vec![IrType::I8, IrType::Ptr]),
        ];
        assert_eq!(
            payload_type(&variants, &named),
            IrType::Array(2, Box::new(IrType::I64))
        );
        assert_eq!(
            payload_type(&[IrType::Struct(vec![IrType::I8])], &named),
            IrType::Array(1, Box::new(IrType::I8))
        );
    }
}
