use super::super::text::MAX_PURPOSE;
use super::*;

// --- purpose signature ------------------------------------------------

#[test]
fn purpose_signature_composes_kind_name_bases_and_public_methods() {
    let e = extract_src(
            "namespace Fixtures.Orders { public interface IReader { int Count { get; } } public class Order : IReader { public int Count { get; set; } public void SaveAsync() {} private void Hidden() {} } }",
        );
    assert_eq!(
        e.purpose.as_deref(),
        Some("interface IReader | class Order : IReader; Save")
    );
}

#[test]
fn purpose_signature_is_none_when_no_namespace_level_types() {
    let e = extract_src("// just a comment\n");
    assert!(e.purpose.is_none());
}

#[test]
fn purpose_signature_truncates_at_200_utf16_units() {
    let long_bases = (0..40)
        .map(|i| format!("IFace{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let src = format!("public class Wide : {long_bases} {{}}");
    let e = extract_src(&src);
    let p = e.purpose.expect("purpose present");
    assert_eq!(p.encode_utf16().count(), MAX_PURPOSE);
    assert!(p.ends_with("..."));
}

// --- inherits via primary_constructor_base_type ------------------------

#[test]
fn primary_constructor_base_type_records_inherits_ref() {
    let e = extract_src(
            "namespace Fixtures.Misc { public class GadgetBase { public GadgetBase(string name) {} } public record GadgetRecord(string Name) : GadgetBase(Name); }",
        );
    let inherits: Vec<&str> = e
        .refs
        .iter()
        .filter(|r| r.kind == "inherits")
        .map(|r| r.name.as_str())
        .collect();
    assert!(inherits.contains(&"GadgetBase"));
}

// --- tuple types --------------------------------------------------------

#[test]
fn tuple_return_type_records_each_element_type() {
    let e = extract_src(
            "namespace Fixtures.Misc { public class GadgetBase {} public class Gadget { public (Gadget Primary, GadgetBase Fallback) Describe() { return (this, null); } } }",
        );
    let uses_type_names: Vec<&str> = e
        .refs
        .iter()
        .filter(|r| r.kind == "uses-type")
        .map(|r| r.name.as_str())
        .collect();
    assert!(uses_type_names.contains(&"Gadget"));
    assert!(uses_type_names.contains(&"GadgetBase"));
}
