use crate::resources::LocalResources;
use std::path::PathBuf;

#[test]
fn resource_roots_are_explicit_and_local() {
    let resources = LocalResources::new().with_skill_root("project/skills");
    assert_eq!(resources.skill_roots(), &[PathBuf::from("project/skills")]);
}

#[test]
fn workspace_resources_are_derived_without_network_configuration() {
    let resources = LocalResources::from_workspace("project");
    assert_eq!(
        resources.skill_roots(),
        &[
            PathBuf::from("project/skills"),
            PathBuf::from("project/.iota/skills"),
        ]
    );
}
