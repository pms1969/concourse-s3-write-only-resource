use crate::{S3WriteOnly, Source};
use concourse_resource::Resource;

#[cfg(test)]
#[test]
fn check_one() {
    // Set up the source.

    use crate::Version;

    let source = Source {
        ..Default::default()
    };

    let out = S3WriteOnly::resource_in(
        Some(source),
        Version {
            path: "/prefix/path/thing".into(),
        },
        None,
        "",
    )
    .unwrap();

    // And in theory, we should get back an empty vector of versions.
    assert_eq!(&out.version.path, "/prefix/path/thing");
    assert!(out.metadata.is_some());
    assert_eq!(&out.metadata.unwrap().path, "/prefix/path/thing");
}
