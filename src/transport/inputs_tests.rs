use super::*;

#[test]
fn transport_facts_ac_2_project_usings_follow_the_sdk_and_the_using_items() {
    let web = r#"<Project Sdk="Microsoft.NET.Sdk.Web">
  <PropertyGroup><ImplicitUsings>enable</ImplicitUsings></PropertyGroup>
  <ItemGroup>
    <Using Include="Fixture.Shared" />
    <Using Remove="System.Linq" />
    <Using Include="Fixture.Aliased" Alias="Aliased" />
    <Using Include="Fixture.Statics" Static="true" />
  </ItemGroup>
</Project>"#;
    let namespaces = project_namespaces(web);
    assert!(namespaces.contains(&"Microsoft.Extensions.DependencyInjection".to_string()));
    assert!(namespaces.contains(&"Fixture.Shared".to_string()));
    assert!(!namespaces.contains(&"System.Linq".to_string()));
    assert!(!namespaces
        .iter()
        .any(|n| n == "Fixture.Aliased" || n == "Fixture.Statics"));

    let disabled = web.replace("enable", "disable");
    assert_eq!(
        project_namespaces(&disabled),
        vec!["Fixture.Shared".to_string()]
    );
    let other_sdk = web.replace("Microsoft.NET.Sdk.Web", "Vendor.Sdk");
    assert_eq!(
        project_namespaces(&other_sdk),
        vec!["Fixture.Shared".to_string()]
    );
}

#[test]
fn transport_facts_ac_4_only_appsettings_files_are_settings_files() {
    assert!(is_settings_file("src/App/appsettings.json"));
    assert!(is_settings_file("src/App/appsettings.Development.json"));
    assert!(!is_settings_file("src/App/appsettings..json"));
    assert!(!is_settings_file("src/App/launchSettings.json"));
    assert!(!is_settings_file("src/App/appsettings.json.bak"));
}
