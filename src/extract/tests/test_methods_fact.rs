use super::arg_count_fact::def_json_keys;
use super::*;

// --- testMethods -----------------------------------------------------------

#[test]
fn test_coverage_xunit_fact_and_theory_land_in_test_methods_serialized_last_after_bases() {
    let e = extract_src(
            "namespace App.Tests;\n\npublic class WidgetTests : TestBase\n{\n  [Fact]\n  public void ComputesTotal() { }\n\n  [Theory]\n  [InlineData(1)]\n  public void RejectsEmptyCart(int n) { }\n\n  public void Helper() { }\n}\n",
        );
    let d = find_def(&e, "App.Tests.WidgetTests").expect("WidgetTests def present");
    assert_eq!(
        d.test_methods,
        vec!["ComputesTotal", "RejectsEmptyCart"],
        "source order; the unattributed helper is a method but not a test"
    );
    assert_eq!(
        def_json_keys(d),
        vec![
            "id",
            "name",
            "namespace",
            "kind",
            "line",
            "methods",
            "bases",
            "testMethods"
        ],
        "testMethods lands LAST -- after bases, which was the final fact before this stage"
    );
}

#[test]
fn test_coverage_suffixed_qualified_targeted_and_shared_bracket_attribute_forms_all_match() {
    let e = extract_src(
            "namespace App.Tests;\n\npublic class SpellingTests\n{\n  [FactAttribute]\n  public void Suffixed() { }\n\n  [Xunit.Fact]\n  public void Qualified() { }\n\n  [method: Fact]\n  public void Targeted() { }\n\n  [Fact, Trait(\"speed\", \"fast\")]\n  public void SharesABracket() { }\n\n  [method: Xunit.FactAttribute]\n  public void EveryFormAtOnce() { }\n}\n",
        );
    let d = find_def(&e, "App.Tests.SpellingTests").expect("SpellingTests def present");
    assert_eq!(
            d.test_methods,
            vec!["Suffixed", "Qualified", "Targeted", "SharesABracket", "EveryFormAtOnce"],
            "the last dotted segment is compared with and without the Attribute suffix, per attribute node, not per bracket pair"
        );
}

#[test]
fn test_coverage_nunit_attributes_match_without_a_test_fixture_on_the_class() {
    let e = extract_src(
            "namespace App.Tests;\n\npublic class OrderServiceTests\n{\n  [Test]\n  public void Renders() { }\n\n  [TestCase(1, 2)]\n  public void Adds(int a, int b) { }\n\n  [TestCaseSource(nameof(Cases))]\n  public void Divides(int a) { }\n}\n",
        );
    let d = find_def(&e, "App.Tests.OrderServiceTests").expect("OrderServiceTests def present");
    assert_eq!(
        d.test_methods,
        vec!["Renders", "Adds", "Divides"],
        "NUnit makes the class attribute optional, so requiring one would drop real tests"
    );
}

#[test]
fn test_coverage_lifecycle_data_source_and_class_container_attributes_never_mark_a_method() {
    let e = extract_src(
            "namespace App.Tests;\n\npublic class NotTests\n{\n  [SetUp]\n  public void Prepare() { }\n\n  [TearDown]\n  public void Cleanup() { }\n\n  [OneTimeSetUp]\n  public void Once() { }\n\n  [OneTimeTearDown]\n  public void Finally() { }\n\n  [TestInitialize]\n  public void Init() { }\n\n  [TestCleanup]\n  public void Done() { }\n\n  [InlineData(1)]\n  public void OnlyInline(int n) { }\n\n  [MemberData(nameof(Cases))]\n  public void OnlyMember(int n) { }\n\n  [ClassData(typeof(Cases))]\n  public void OnlyClassData(int n) { }\n\n  [DataRow(1)]\n  public void OnlyRow(int n) { }\n\n  [DynamicData(nameof(Cases))]\n  public void OnlyDynamic(int n) { }\n\n  [TestFixture]\n  public void FixtureOnAMethod() { }\n\n  [TestClass]\n  public void ClassMarkerOnAMethod() { }\n}\n",
        );
    let d = find_def(&e, "App.Tests.NotTests").expect("NotTests def present");
    assert!(d.test_methods.is_empty());
    assert_eq!(
            def_json_keys(d),
            vec!["id", "name", "namespace", "kind", "line", "methods"],
            "not one of these marks a test, so the key is absent entirely and the def keeps its pre-stage bytes"
        );
}

#[test]
fn test_coverage_mstest_methods_count_only_inside_a_test_class() {
    let source = |class_attribute: &str| {
        format!(
                "namespace App.Tests;\n\n{class_attribute}public class CartTests\n{{\n  [TestMethod]\n  public void Places() {{ }}\n\n  [DataTestMethod]\n  [DataRow(1)]\n  public void Prices(int n) {{ }}\n}}\n"
            )
    };
    let ungated = extract_src(&source(""));
    assert!(
            find_def(&ungated, "App.Tests.CartTests").expect("CartTests def present").test_methods.is_empty(),
            "MSTest does not discover a [TestMethod] whose class lacks [TestClass] -- neither does this"
        );

    let gated = extract_src(&source("[TestClass]\n"));
    assert_eq!(
        find_def(&gated, "App.Tests.CartTests")
            .expect("CartTests def present")
            .test_methods,
        vec!["Places", "Prices"]
    );
}

#[test]
fn test_coverage_a_nested_type_does_not_inherit_an_enclosing_test_class() {
    let e = extract_src(
            "namespace App.Tests;\n\n[TestClass]\npublic class OuterTests\n{\n  [TestMethod]\n  public void Outer() { }\n\n  public class Inner\n  {\n    [TestMethod]\n    public void Nested() { }\n  }\n}\n",
        );
    assert_eq!(
        find_def(&e, "App.Tests.OuterTests")
            .expect("OuterTests def present")
            .test_methods,
        vec!["Outer"]
    );
    assert!(
        find_def(&e, "App.Tests.OuterTests+Inner")
            .expect("Inner def present")
            .test_methods
            .is_empty(),
        "each type computes its own list from its OWN attribute_list"
    );
}

#[test]
fn test_coverage_an_interface_or_enum_body_never_emits_test_methods() {
    let e = extract_src(
            "namespace App.Tests;\n\npublic interface ITestContract\n{\n  [Fact]\n  void Runs();\n}\n\npublic enum Speed\n{\n  Fast,\n  Slow,\n}\n",
        );
    assert!(find_def(&e, "App.Tests.ITestContract")
        .expect("interface def present")
        .test_methods
        .is_empty());
    assert!(find_def(&e, "App.Tests.Speed")
        .expect("enum def present")
        .test_methods
        .is_empty());
    assert!(find_def(&e, "App.Tests.Speed.Fast")
        .expect("enum-member def present")
        .test_methods
        .is_empty());
}

#[test]
fn test_coverage_a_struct_and_a_record_carry_test_methods_too_and_a_local_function_never_does() {
    let e = extract_src(
            "namespace App.Tests;\n\npublic struct ValueTests\n{\n  [Fact]\n  public void Holds() { }\n}\n\npublic record RecordTests\n{\n  [Fact]\n  public void Keeps()\n  {\n    [Fact]\n    void Inner() { }\n  }\n}\n",
        );
    assert_eq!(
        find_def(&e, "App.Tests.ValueTests")
            .expect("struct def present")
            .test_methods,
        vec!["Holds"]
    );
    assert_eq!(
        find_def(&e, "App.Tests.RecordTests")
            .expect("record def present")
            .test_methods,
        vec!["Keeps"],
        "a local function is not a method_declaration at type body level"
    );
}
